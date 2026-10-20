use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::{BranchType, Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::core::remote;
use crate::core::repo;

use crate::core::agent_mode;
use crate::core::msg;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::{self, Command, CommitEntry, Weave};
use crate::git::{self, RebaseOutcome};

#[derive(Serialize, Deserialize)]
struct UpdateContext {
    branch_name: String,
    upstream_name: String,
    skip_confirm: bool,
    /// Branches whose every commit was already upstream before the rebase.
    #[serde(default)]
    merged_branches: Vec<String>,
    /// The branch the rebase replaces with its remote version (Spec 010).
    #[serde(default)]
    updated_branch: Option<UpdatedBranch>,
}

/// A woven branch taking its remote version, as the report names it.
#[derive(Serialize, Deserialize, Clone)]
struct UpdatedBranch {
    name: String,
    /// The remote branch, as `origin/feature`.
    remote: String,
    /// Its tip, which the report names: the rebase gives the branch a new one.
    tip: String,
    /// The branch's own tip before, the way back the report names.
    old_tip: String,
}

/// The branch `loom update <branch>` names, resolved before the fetch.
struct Target {
    name: String,
    /// The remote branch, as `origin/feature`, and its full ref name.
    remote: String,
    remote_ref: String,
    /// Where it is fetched from: the remote, and the branch there, as
    /// `refs/heads/feature`.
    remote_name: String,
    merge_ref: String,
}

/// What taking the remote version did to the graph.
enum Taken {
    Replaced(UpdatedBranch),
    /// The remote is at the local tip already.
    Unchanged,
    /// The new upstream has every remote commit: the section is gone and the
    /// branch is fully merged.
    AllUpstream,
}

/// Update the integration branch by fetching and rebasing from upstream.
pub fn run(branch: Option<String>, skip_confirm: bool) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "update")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();

    let head = repo.head().context("Failed to get HEAD reference")?;
    if !head.is_branch() {
        bail!("HEAD is detached\nSwitch to an integration branch");
    }

    let branch_name = head
        .shorthand()
        .context("Could not determine current branch name")?
        .to_string();

    let local_branch = repo.find_branch(&branch_name, BranchType::Local)?;
    let upstream = local_branch.upstream().with_context(|| {
        format!(
            "Branch `{}` has no upstream tracking branch\n\
             Run `loom init` to set up an integration branch",
            branch_name
        )
    })?;
    let upstream_name = upstream
        .name()?
        .context("Upstream branch name is not valid UTF-8")?
        .to_string();

    let head_refname = head.name().context("HEAD ref name is not valid UTF-8")?;
    let (upstream_remote, upstream_branch) = repo::upstream_remote_and_branch(&repo, head_refname)?;

    // Resolved before the fetch, so a wrong name costs nothing.
    let target = branch
        .as_deref()
        .map(|arg| resolve_target(&repo, arg))
        .transpose()?;

    // Fetch with tags, force-update, and prune deleted remote branches. The
    // remote is named explicitly so `--tags` makes the tag refspec explicit and
    // `--prune` only drops tags missing from *this* remote: under
    // `fetch.all = true` a plain `git fetch` would also hit other (typically
    // tagless) remotes and wipe every tag on each run. The spinner covers a slow
    // fetch; git's own summary is printed afterwards so the user can see whether
    // anything was pulled, with `--no-progress` keeping out the transfer noise.
    let spinner = msg::spinner();
    spinner.start("Fetching latest changes...");

    let result = git::run_git_combined(
        &workdir,
        &[
            "fetch",
            "--no-progress",
            "--tags",
            "--force",
            "--prune",
            &upstream_remote,
        ],
    );

    match result {
        Ok(summary) => {
            spinner.stop("Fetched latest changes");
            if !summary.is_empty() {
                msg::human_line(&summary);
            }
        }
        Err(e) => {
            spinner.error("Fetch failed");
            // Under the TUI git may not prompt (Spec 020), so missing
            // credentials are the failure to point at.
            if crate::core::ui::active() {
                return Err(e.context(
                    "Fetch failed\n\
                     If the remote needs credentials, run `loom update` in a terminal, \
                     or set up a credential helper or ssh-agent",
                ));
            }
            return Err(e);
        }
    }

    let push_remote = fetch_push_remote(&repo, &workdir, &upstream_remote, &upstream_branch);
    if let Some(target) = &target
        && target.remote_name != upstream_remote
        && push_remote.as_ref() != Some(&target.remote_name)
    {
        fetch_target(&workdir, target)?;
    }

    let repo = git2::Repository::discover(&workdir)?;

    // Rebase onto upstream using the weave model. Plain
    // `git rebase --rebase-merges` preserves merge topology literally, which can
    // place new upstream commits inside a feature branch instead of on the base
    // line; the weave's todo `reset onto` for every branch section, so branches
    // land on the new upstream tip.
    let (todo, merged_branches, updated_branch) = match Weave::from_repo(&repo) {
        Ok(mut graph) => {
            let new_upstream_oid = repo
                .revparse_single(&upstream_name)
                .context("Failed to resolve upstream ref")?
                .id();

            // The named branch takes its remote version first, so it goes
            // through the filtering and the rebase like its own.
            let mut updated = None;
            let mut taken_merged = Vec::new();
            if let Some(target) = &target {
                match take_remote_version(&repo, &mut graph, target, new_upstream_oid)? {
                    Taken::Replaced(info) => updated = Some(info),
                    Taken::Unchanged => msg::success(&format!(
                        "Branch `{}` is up to date with `{}`",
                        target.name, target.remote
                    )),
                    Taken::AllUpstream => taken_merged.push(target.name.clone()),
                }
            }

            // Drop branch-section commits already in the new upstream, so their
            // content is not replayed onto a base that already has it.
            let filtered_out = graph.filter_upstream_commits(&repo, &workdir, new_upstream_oid)?;
            // A remote version the upstream already has in full empties the
            // section: the ref stays where it was, and the branch is merged.
            if updated
                .as_ref()
                .is_some_and(|b| filtered_out.contains(&b.name))
            {
                updated = None;
            }

            // Fully merged branches: every commit was filtered out (merged
            // or cherry-picked), or the tip sits below the merge base with
            // the upstream. The weave only knows tips above the merge base,
            // so ancestry has to find the latter.
            let mut merged = find_branches_merged_upstream(
                &repo,
                &branch_name,
                repo::head_oid(&repo)?,
                graph.base_oid,
                new_upstream_oid,
            )?;
            // The updated branch's old tip may be upstream; its new commits are not.
            merged.retain(|name| updated.as_ref().is_none_or(|b| b.name != *name));
            merged.extend(filtered_out);
            merged.extend(taken_merged);
            // A woven branch started from a local `main` holds its tip, so both
            // tests can find it; it stays loom's to keep (Spec 010).
            merged.retain(|name| *name != upstream_branch);
            merged.sort();
            merged.dedup();
            (Some(graph.to_todo()), merged, updated)
        }
        // Git's own rebase still updates (an octopus merge, Spec 010). The
        // cause line only: the builder's remedy is for a refused rewrite.
        Err(e) => {
            let cause = e.to_string();
            let cause = cause.lines().next().unwrap_or_default();
            if let Some(target) = &target {
                bail!("Cannot update `{}`: {cause}", target.name);
            }
            msg::warn(&format!(
                "{cause}\nUpdating with git's own rebase instead: branches already \
                 merged upstream are not offered for removal"
            ));
            (None, Vec::new(), None)
        }
    };

    // Save rollback state before the rebase
    let ctx = UpdateContext {
        branch_name: branch_name.clone(),
        upstream_name: upstream_name.clone(),
        skip_confirm,
        merged_branches,
        updated_branch,
    };
    let state = LoomState {
        command: "update".to_string(),
        rollback: Rollback {
            // Restored whichever way the rebase ends (Spec 014).
            saved_staged_patch: git::diff_cached(&workdir)?,
            ..Default::default()
        },
        context: serde_json::to_value(&ctx)?,
        protect: Vec::new(),
        targets: Vec::new(),
        skipped_fixups: Vec::new(),
    };
    transaction::save(&git_dir, &state)?;

    let spinner = msg::spinner();
    spinner.start("Rebasing onto upstream...");

    let outcome = match &todo {
        Some(todo) => crate::core::weave::run_rebase(&workdir, Some(&upstream_name), todo),
        None => git::rebase(&git_dir, &workdir, &upstream_name),
    };

    match outcome {
        Ok(RebaseOutcome::Completed) => {
            spinner.stop("Rebased onto upstream");
            git::restore_staged_after_rebase(&workdir, &state.rollback.saved_staged_patch);
            transaction::delete(&git_dir)?;
            let repo2 = git2::Repository::discover(&workdir)?;
            post_update(&workdir, &repo2, &ctx)?;
        }
        Ok(RebaseOutcome::Stopped) => {
            spinner.error("Rebase paused");
            transaction::warn_paused(&workdir, "update");
        }
        Ok(RebaseOutcome::Paused) => {
            spinner.error("Rebase paused");
            transaction::warn_paused_at_edit(Some("update"));
        }
        Err(e) => {
            spinner.error("Rebase failed");
            return Err(transaction::discard_state_after(
                &workdir,
                &git_dir,
                &state.rollback.saved_staged_patch,
                e,
            ));
        }
    }

    Ok(())
}

/// Fetch and prune the push remote when it differs from the integration
/// branch's remote; returns it once fetched.
///
/// In a fork workflow feature branches live on the push remote, which the
/// fetch above never touches: its remote-tracking refs would stay stale and
/// branches deleted on the fork would never show up as gone.
///
/// Only branches are pruned: tags come from the integration remote, which is
/// the authoritative one. `--no-prune-tags` says so explicitly — a user-level
/// `fetch.pruneTags = true` would delete every tag missing from the fork, and
/// the next update's `--tags` fetch would re-add them all.
fn fetch_push_remote(
    repo: &git2::Repository,
    workdir: &Path,
    upstream_remote: &str,
    upstream_branch: &str,
) -> Option<String> {
    let remote = remote::fork_push_remote(repo, workdir, upstream_remote, upstream_branch)?;

    let spinner = msg::spinner();
    spinner.start(&format!("Fetching `{}`...", remote));

    match git::run_git_combined(
        workdir,
        &[
            "fetch",
            "--no-progress",
            "--prune",
            "--no-prune-tags",
            &remote,
        ],
    ) {
        Ok(summary) => {
            spinner.stop(&format!("Fetched `{}`", remote));
            if !summary.is_empty() {
                msg::human_line(&summary);
            }
            Some(remote)
        }
        // An unreachable fork must not stop the update: the integration branch
        // can still be rebased onto its own upstream.
        Err(_) => {
            spinner.error(&format!(
                "Could not fetch `{}` — branches deleted there are not detected",
                remote
            ));
            None
        }
    }
}

/// Fetch `target`'s remote branch when no fetch before covered its remote (a
/// third one, or a push remote whose fetch failed): its tracking ref may be
/// stale, and taking it would bring back an old version. Tags come from the
/// integration remote only.
fn fetch_target(workdir: &Path, target: &Target) -> Result<()> {
    let spinner = msg::spinner();
    spinner.start(&format!("Fetching `{}`...", target.remote));
    // Forced: the remote branch may have been rewritten, the case naming it is for.
    let refspec = format!("+{}:{}", target.merge_ref, target.remote_ref);
    match git::run_git_combined(
        workdir,
        &[
            "fetch",
            "--no-progress",
            "--no-tags",
            &target.remote_name,
            &refspec,
        ],
    ) {
        Ok(summary) => {
            spinner.stop(&format!("Fetched `{}`", target.remote));
            if !summary.is_empty() {
                msg::human_line(&summary);
            }
            Ok(())
        }
        Err(e) => {
            spinner.error("Fetch failed");
            Err(e.context(format!("Could not fetch `{}`", target.remote)))
        }
    }
}

/// Resume an `update` operation after a conflict has been resolved.
pub fn after_continue(
    workdir: &Path,
    rollback: &Rollback,
    context: &serde_json::Value,
) -> Result<()> {
    let ctx: UpdateContext =
        serde_json::from_value(context.clone()).context("Failed to parse update resume context")?;
    git::restore_staged_after_rebase(workdir, &rollback.saved_staged_patch);
    let repo = git2::Repository::discover(workdir)?;
    post_update(workdir, &repo, &ctx)
}

/// Post-rebase work: submodule update, upstream reporting, gone-branch cleanup.
fn post_update(workdir: &Path, repo: &git2::Repository, ctx: &UpdateContext) -> Result<()> {
    if workdir.join(".gitmodules").exists() {
        let spinner = msg::spinner();
        spinner.start("Updating submodules...");

        let result = git::run_git(workdir, &["submodule", "update", "--init", "--recursive"]);

        match result {
            Ok(()) => {
                spinner.stop("Updated submodules");
            }
            Err(e) => {
                spinner.error("Submodule update failed");
                return Err(e);
            }
        }
    }

    msg::success(&format!(
        "Updated branch `{}` with `{}`{}",
        ctx.branch_name,
        ctx.upstream_name,
        commit_blurb(repo, &ctx.upstream_name, None)
    ));
    if let Some(branch) = &ctx.updated_branch {
        msg::success(&format!(
            "Updated branch `{}` with `{}`{}",
            branch.name,
            branch.remote,
            commit_blurb(repo, &branch.tip, Some(&branch.old_tip))
        ));
    }

    // Propose removing local branches that are fully merged upstream or
    // whose remote tracking branch was pruned
    let gone = find_branches_with_gone_upstream(repo, &ctx.branch_name)?;
    // The merged list predates the rebase; a branch may be gone by now
    // (deleted by hand during a conflict pause)
    let merged: Vec<String> = ctx
        .merged_branches
        .iter()
        .filter(|name| !gone.contains(name) && repo.find_branch(name, BranchType::Local).is_ok())
        .cloned()
        .collect();
    let to_remove: Vec<&String> = merged.iter().chain(gone.iter()).collect();
    if !to_remove.is_empty() {
        let auto = ctx.skip_confirm || repo::prune_gone_branches(repo);
        // Post-mutation prompt: the pull-rebase already succeeded, so agent
        // mode must not answer `needs_input` (that would imply nothing
        // happened) — skip the optional pruning instead and say how to redo it.
        let ask = !auto && !agent_mode::enabled();
        // The question names the branches itself; listing them again first
        // would print them twice.
        if !ask {
            warn_branch_list(&merged, "fully merged upstream");
            warn_branch_list(&gone, "with a gone upstream");
        }
        let confirmed = if ask {
            // Dismissing the TUI menu is a no: the update itself has landed,
            // and a `Cancelled` would tell the TUI nothing changed.
            match msg::confirm(
                &removal_prompt(&merged, &gone),
                "re-run with: loom update -y",
            ) {
                Err(e) if e.is::<crate::core::ui::Cancelled>() => false,
                answer => answer?,
            }
        } else if !auto {
            msg::warn(
                "Skipped removing branches (agent mode)\n\
                 Re-run with `loom update -y` to remove them",
            );
            false
        } else {
            true
        };
        if confirmed {
            for name in to_remove {
                // Capture the tip before deletion so users can revive the branch.
                let short_id = repo
                    .revparse_single(name)
                    .ok()
                    .map(|obj| git::short_hash(&obj.id().to_string()).to_string());
                match git::branch_delete(workdir, name) {
                    Ok(()) => match &short_id {
                        Some(id) => {
                            msg::success(&format!("Removed branch `{}` (was {})", name, id))
                        }
                        None => msg::success(&format!("Removed branch `{}`", name)),
                    },
                    Err(_) => {
                        msg::warn(&format!(
                            "Skipped branch `{}` — could not delete it (run `loom trace` for the git error)",
                            name
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

/// ` (abc1234 subject[, was def5678])` for `spec` and the tip it replaced;
/// empty when `spec` does not resolve, since the message prints either way.
fn commit_blurb(repo: &Repository, spec: &str, was: Option<&str>) -> String {
    repo.revparse_single(spec)
        .ok()
        .and_then(|obj| obj.peel_to_commit().ok())
        .map(|commit| {
            let short_id = git::short_hash(&commit.id().to_string()).to_string();
            let was = was.map_or(String::new(), |old| {
                format!(", was {}", git::short_hash(old))
            });
            format!(" ({} {}{})", short_id, repo::commit_subject(&commit), was)
        })
        .unwrap_or_default()
}

/// `arg`, a branch name or short ID, with its remote branch: refused without
/// an upstream, or with a local one (remote `.`), which no fetch updates.
fn resolve_target(repo: &Repository, arg: &str) -> Result<Target> {
    let name = repo::resolve_arg(repo, arg, &[repo::TargetKind::Branch])?.expect_branch()?;
    let refname = format!("refs/heads/{name}");
    let text = |buf: Result<git2::Buf, git2::Error>| {
        buf.ok()
            .and_then(|buf| buf.as_str().ok().map(str::to_string))
    };
    let remote_name = text(repo.branch_upstream_remote(&refname));
    if remote_name.as_deref() == Some(".") {
        bail!("Branch `{name}` tracks a local branch, which no fetch updates");
    }
    let (Some(remote_name), Some(merge_ref), Some(remote_ref)) = (
        remote_name,
        text(repo.branch_upstream_merge(&refname)),
        text(repo.branch_upstream_name(&refname)),
    ) else {
        bail!(
            "Branch `{name}` has no upstream branch\n\
             Set one with `git branch --set-upstream-to=<remote>/<branch> {name}`"
        );
    };
    let remote = remote_ref
        .strip_prefix("refs/remotes/")
        .unwrap_or(&remote_ref)
        .to_string();
    Ok(Target {
        name,
        remote,
        remote_ref,
        remote_name,
        merge_ref,
    })
}

/// Replace `target`'s section with its remote branch's commits, those the new
/// upstream lacks, oldest first (Spec 010). Refused, before anything is
/// rewritten, for a branch the remote's commits could not stand in for.
fn take_remote_version(
    repo: &Repository,
    graph: &mut Weave,
    target: &Target,
    upstream_oid: Oid,
) -> Result<Taken> {
    let name = target.name.as_str();
    let local_tip = repo
        .find_branch(name, BranchType::Local)?
        .get()
        .target()
        .with_context(|| format!("Branch `{name}` has no commit"))?;
    let remote_tip = match repo.find_reference(&target.remote_ref) {
        Ok(reference) => reference
            .target()
            .with_context(|| format!("`{}` is not a branch", target.remote))?,
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            bail!("`{}` is gone: the remote no longer has it", target.remote)
        }
        Err(e) => return Err(e.into()),
    };
    if graph.is_inner_branch(name) {
        let why = match graph.inner_branch_keeper(name) {
            Some(keeper) => format!("branch `{keeper}` is stacked on it"),
            None => "commits of a branch deleted locally sit on it".to_string(),
        };
        bail!("Cannot update `{name}`: {why}");
    }
    let others = graph.other_refs_in_section(name).with_context(|| {
        format!(
            "Branch `{name}` is not woven into the integration branch\n\
             Update it with git: `git branch -f {name} {}`",
            target.remote
        )
    })?;
    if !others.is_empty() {
        bail!(
            "Cannot update `{name}`: it shares commits with {}",
            weave::describe_branches(&others)
        );
    }
    if remote_tip == local_tip {
        return Ok(Taken::Unchanged);
    }
    let (commits, has_merge) = commits_not_in(repo, remote_tip, upstream_oid)?;
    if has_merge {
        bail!(
            "Cannot update `{name}`: `{}` carries a merge commit",
            target.remote
        );
    }
    if commits.is_empty() {
        if !graph.drop_branch(name) {
            bail!("Branch `{name}` is not woven into the integration branch");
        }
        return Ok(Taken::AllUpstream);
    }
    if !graph.replace_branch_commits(name, commits) {
        bail!("Cannot update `{name}`: its section carries another ref");
    }
    Ok(Taken::Replaced(UpdatedBranch {
        name: name.to_string(),
        remote: target.remote.clone(),
        tip: remote_tip.to_string(),
        old_tip: local_tip.to_string(),
    }))
}

/// Non-merge commits of `tip` not in `upstream`, oldest first, as a section's
/// picks, and whether a merge was among them.
fn commits_not_in(repo: &Repository, tip: Oid, upstream: Oid) -> Result<(Vec<CommitEntry>, bool)> {
    let mut walk = repo.revwalk()?;
    walk.push(tip)?;
    walk.hide(upstream)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
    let mut commits = Vec::new();
    let mut has_merge = false;
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        if commit.parent_count() > 1 {
            has_merge = true;
        } else {
            commits.push(CommitEntry {
                oid: commit.id(),
                message: repo::commit_subject(&commit),
                command: Command::Pick,
                update_refs: Vec::new(),
            });
        }
    }
    Ok((commits, has_merge))
}

/// The cleanup question, naming every branch and why it goes: under the TUI
/// it is a menu, away from the log any warning would land in (Spec 020). One
/// branch per detail line, since the menu's help box does not wrap.
fn removal_prompt(merged: &[String], gone: &[String]) -> String {
    let mut prompt = match merged.len() + gone.len() {
        1 => "Remove local branch?".to_string(),
        count => format!("Remove {} local branches?", count),
    };
    let reasons = merged
        .iter()
        .map(|name| (name, "fully merged upstream"))
        .chain(gone.iter().map(|name| (name, "upstream gone")));
    for (name, why) in reasons {
        prompt.push_str(&format!("\n`{}`: {}", name, why));
    }
    prompt
}

/// Warn with the branch names listed one per line. Silent for an empty list.
fn warn_branch_list(names: &[String], what: &str) {
    if names.is_empty() {
        return;
    }
    let mut warn_msg = format!(
        "{} local {} {}:",
        names.len(),
        if names.len() == 1 {
            "branch"
        } else {
            "branches"
        },
        what
    );
    for name in names {
        warn_msg.push('\n');
        warn_msg.push_str(name);
    }
    msg::warn(&warn_msg);
}

/// Find local branches woven into the integration branch whose tip the new
/// upstream already contains.
///
/// Only branches reachable from `head_oid` and above `base_oid` (the
/// integration base) count. Below that point lies upstream history, where a
/// branch like a local `main` is not loom's to remove.
fn find_branches_merged_upstream(
    repo: &git2::Repository,
    current_branch: &str,
    head_oid: git2::Oid,
    base_oid: git2::Oid,
    upstream_oid: git2::Oid,
) -> Result<Vec<String>> {
    let contains = |tip, oid| repo::contains(repo, tip, oid);

    let mut merged = Vec::new();
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let Some(name) = branch.name()? else {
            continue;
        };
        if name == current_branch {
            continue;
        }
        let Some(tip) = branch.get().target() else {
            continue;
        };
        if !contains(base_oid, tip)? && contains(upstream_oid, tip)? && contains(head_oid, tip)? {
            merged.push(name.to_string());
        }
    }
    Ok(merged)
}

/// Find local branches whose upstream a fetch pruned (`repo::resolve_upstream`).
fn find_branches_with_gone_upstream(
    repo: &git2::Repository,
    current_branch: &str,
) -> Result<Vec<String>> {
    let mut gone = Vec::new();
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let Some(name) = branch.name()? else {
            continue;
        };
        if name != current_branch
            && matches!(
                repo::resolve_upstream(repo, &branch),
                Some(repo::Upstream::Gone)
            )
        {
            gone.push(name.to_string());
        }
    }
    Ok(gone)
}

#[cfg(test)]
#[path = "update_test.rs"]
mod tests;
