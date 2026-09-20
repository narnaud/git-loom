use anyhow::{Context, Result, bail};
use git2::{Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::core::agent_mode;
use crate::core::diff;
use crate::core::graph;
use crate::core::hunk_select::{self, HunkArgs, Picker};
use crate::core::msg;
use crate::core::repo::{self, Target, TargetKind};
use crate::core::staging;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::{self, EmptiedRefs, Position, RebaseOutcome, Weave};
use crate::git;
use crate::tui::hunk_selector::FileEntry;

#[derive(Serialize, Deserialize)]
#[serde(tag = "op")]
enum FoldVariant {
    FilesIntoCommit {
        original_commit_hash: String,
        files_count: usize,
    },
    CommitIntoCommit {
        source_hash: String,
        target_hash: String,
    },
    CommitToBranch {
        commit_hash: String,
        branch_name: String,
        /// Branches the commit was the only commit of, now parked at their base.
        #[serde(default)]
        parked: Vec<String>,
    },
    CommitToUnstaged {
        commit_hash: String,
        diff: String,
        /// Branches the commit was the only commit of, now parked at their base.
        #[serde(default)]
        emptied: Vec<String>,
    },
    CommitRelative {
        commit_hash: String,
        target_hash: String,
        above: bool,
        /// Branches the commit was the only commit of, now parked at their base.
        #[serde(default)]
        parked: Vec<String>,
    },
}

/// Temporary branch used to track a commit's new OID through a rebase.
const TRACK_BRANCH: &str = "_loom-track";
const COMMAND: &str = "fold";

/// `--above <commit>` / `--below <commit>`: the commit the sources land next to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    Above(String),
    Below(String),
}

/// Fold source(s) into a target.
///
/// Forms:
/// - File(s) + Commit → amend files into the commit
/// - Commit + Commit  → fixup source into target (source disappears)
/// - Commit(s) + Branch → move the commit(s) to the branch, oldest-first
/// - Commit(s) + `--above`/`--below <commit>` → move next to that commit
///
/// With `--create` (`-c`): create a new branch and move the source commit(s)
/// into it. The name must not be taken.
pub fn run(
    create: bool,
    patch: bool,
    anchor: Option<Anchor>,
    hunks: HunkArgs,
    args: Vec<String>,
    git_args: Vec<String>,
    theme: &graph::Theme,
) -> Result<()> {
    if args.is_empty() {
        bail!(
            "At least one argument required\n\
             Usage: git-loom fold [<source>...] <target>"
        );
    }

    let git_opts: Vec<&str> = git_args.iter().map(String::as_str).collect();

    let repo = repo::open_repo()?;

    if let Some(anchor) = anchor {
        no_git_args(&git_opts, "moving commits next to another")?;
        return run_relative(&repo, &args, anchor);
    }

    if create {
        no_git_args(&git_opts, "moving commits to a new branch")?;
        return run_create(&repo, &args);
    }

    if patch {
        return run_patch_fold(&repo, &args, &hunks, &git_opts, theme);
    }

    if args.len() == 1 {
        return run_staged(&repo, &args[0], &git_opts);
    }

    // Last argument is the target, everything else is a source
    let (source_args, target_arg) = args.split_at(args.len() - 1);
    let target_arg = &target_arg[0];

    // If any source is "zz", expand to all changed files (zz takes precedence)
    let source_args = if source_args.iter().any(|s| s == "zz") {
        let files = collect_changed_files(&repo)?;
        if files.is_empty() {
            bail!("No changes to fold — working tree is clean");
        }
        files
    } else {
        source_args.to_vec()
    };

    let resolved_sources: Vec<Target> = source_args
        .iter()
        .map(|s| {
            repo::resolve_arg(
                &repo,
                s,
                &[
                    TargetKind::Commit,
                    TargetKind::CommitFile,
                    TargetKind::File,
                    TargetKind::Unstaged,
                ],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let resolved_target = repo::resolve_arg(
        &repo,
        target_arg,
        &[
            TargetKind::Branch,
            TargetKind::Commit,
            TargetKind::CommitFile,
            TargetKind::File,
            TargetKind::Unstaged,
        ],
    )?;

    match classify(&resolved_sources, &resolved_target)? {
        FoldOp::FilesIntoCommit { files, commit } => {
            fold_files_into_commit(&repo, &files, &commit, false, &git_opts)
        }
        FoldOp::CommitIntoCommit { source, target } => {
            no_git_args(&git_opts, "folding a commit into another")?;
            fold_commit_into_commit(&repo, &source, &target)
        }
        FoldOp::CommitsToBranch { commits, branch } => {
            no_git_args(&git_opts, "moving commits to a branch")?;
            // Order and de-duplicate first: the same commit named twice is one
            // commit, and it should keep the resumable single-commit path
            // rather than be treated as a stack because of a repeated argument.
            let info = repo::gather_commit_graph(&repo)?;
            let commits = commits_to_move(&repo, commits, weave::base_oid(&repo, &info)?)?;
            if commits.len() == 1 {
                fold_commit_to_branch(&repo, &commits[0], &branch)
            } else {
                let workdir = repo::require_workdir(&repo, COMMAND)?;
                move_commits_and_report(workdir, &repo, &commits, &branch, None)
            }
        }
        FoldOp::CommitToUnstaged { commit } => {
            no_git_args(&git_opts, "uncommitting a commit")?;
            fold_commit_to_unstaged(&repo, &commit)
        }
        FoldOp::CommitFileToUnstaged { commit, path } => {
            fold_commit_file_to_unstaged(&repo, &commit, &path, &git_opts)
        }
        FoldOp::CommitFileToCommit {
            source_commit,
            path,
            target_commit,
        } => fold_commit_file_to_commit(&repo, &source_commit, &path, &target_commit, &git_opts),
    }
}

/// A fold that only rebases runs no `git commit`, so a forwarded argument has
/// nothing to reach (Spec 021). `what` names the operation.
fn no_git_args(git_opts: &[&str], what: &str) -> Result<()> {
    if git_opts.is_empty() {
        return Ok(());
    }
    bail!("{what} runs no `git commit`, so it takes no arguments after `--`");
}

/// Create a new branch and move the source commit(s) into it.
///
/// `args` must be `[<commit>..., <new-branch-name>]` — one or more commits
/// followed by the new branch name. The branch is created at the weave base,
/// then the commits are moved to it using the same Weave machinery as the
/// normal commit-to-branch fold. The name must be free: moving onto a branch
/// that already exists is `loom fold <commit>... <branch>`.
fn run_create(repo: &Repository, args: &[String]) -> Result<()> {
    if args.len() < 2 {
        bail!(
            "fold --create requires at least one commit and one new branch name\n\
             Usage: loom fold -c <commit>... <new-branch>"
        );
    }

    let (source_args, branch_name) = args.split_at(args.len() - 1);
    let branch_name = &branch_name[0];
    let workdir = repo::require_workdir(repo, COMMAND)?;

    let mut commit_hashes = Vec::new();
    for source_arg in source_args {
        let source = repo::resolve_arg(repo, source_arg, &[TargetKind::Commit])?;
        match source {
            Target::Commit(hash) => commit_hashes.push(hash),
            _ => unreachable!(),
        }
    }

    git::branch_validate_name(workdir, branch_name)?;

    // `-c` creates: moving onto a branch that is already there is what a
    // plain fold does, and silently accepting it here turns a mistyped name
    // into commits landing in somebody else's branch.
    if repo
        .find_branch(branch_name, git2::BranchType::Local)
        .is_ok()
    {
        bail!(
            "Branch `{}` already exists\n\
             Use `loom fold <commit>... {}` to move commits onto it",
            branch_name,
            branch_name
        );
    }

    // The branch is created at the weave base so it has no commits of its own;
    // move_commits_to_branch adds a section for it. Deliberately without a
    // context: loom prints only the outermost message, which would hide whether
    // the failure was a detached HEAD or no upstream.
    let info = repo::gather_commit_graph(repo)?;
    // The weave base, which the merge-base only sometimes is. Both the branch
    // and the move scope have to use it: plan_move measures a section-less
    // branch against the weave base, and refuses the one it was just handed if
    // it was created anywhere else.
    let base_oid = weave::base_oid(repo, &info)?;

    // Ordering walks the graph, so it comes after the checks a ref lookup
    // settles. Oldest-first, so the commits land in history order.
    let commit_hashes = commits_to_move(repo, commit_hashes, base_oid)?;

    move_commits_and_report(
        workdir,
        repo,
        &commit_hashes,
        branch_name,
        Some(&base_oid.to_string()),
    )
}

/// The commits a move should relocate: de-duplicated, refused if they sit at
/// or below `base`, and ordered oldest-first.
///
/// Ancestors come before their descendants, and commits on unrelated lines
/// come in committer order. One topological walk, because pairwise ancestry
/// with a time tiebreak is not a total order and sorts descendants first.
fn commits_to_move(repo: &Repository, hashes: Vec<String>, base: git2::Oid) -> Result<Vec<String>> {
    let mut oids: Vec<git2::Oid> = Vec::new();
    for h in &hashes {
        let oid = git2::Oid::from_str(h)?;
        if !oids.contains(&oid) {
            oids.push(oid);
        }
    }

    // The base bounds the walk as well as the scope. A sorted walk
    // materializes everything reachable before it yields its first commit, so
    // an unbounded one would cost the whole repository to order a few oids.
    let ordered = ordered_topologically(repo, &oids, base)?;
    if let Some(missing) = oids.iter().find(|o| !ordered.contains(o)) {
        // The base is all that hides a commit pushed onto the walk, so this
        // one sits at or below it.
        bail!(
            "Commit `{}` is not in the integration scope\n\
             Only commits above the integration base can be moved",
            git::short_hash(&missing.to_string())
        );
    }

    Ok(ordered.iter().map(|o| o.to_string()).collect())
}

/// Order `oids` oldest-first by one topological walk, stopping at `boundary`.
///
/// Any oid the boundary hid is missing from the result.
fn ordered_topologically(
    repo: &Repository,
    oids: &[git2::Oid],
    boundary: git2::Oid,
) -> Result<Vec<git2::Oid>> {
    // TIME puts unrelated commits in committer order within the topological
    // constraint. Commits sharing a second are left to push order, so push in
    // a canonical one.
    let mut by_time: Vec<(i64, git2::Oid)> = oids
        .iter()
        .map(|oid| Ok((repo.find_commit(*oid)?.time().seconds(), *oid)))
        .collect::<Result<Vec<_>>>()?;
    by_time.sort_unstable();

    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
    for (_, oid) in &by_time {
        walk.push(*oid)?;
    }
    walk.hide(boundary)?;

    // The walk yields descendants before ancestors, so collect and reverse.
    let mut wanted: Vec<git2::Oid> = oids.to_vec();
    let mut newest_first = Vec::with_capacity(oids.len());
    for step in walk {
        let oid = step?;
        if let Some(i) = wanted.iter().position(|w| *w == oid) {
            wanted.swap_remove(i);
            newest_first.push(oid);
            if wanted.is_empty() {
                break;
            }
        }
    }

    newest_first.reverse();
    Ok(newest_first)
}

/// Move `commit_hashes` to `branch_name` and report the result.
///
/// When `base_hash` is `Some`, the branch is created at that base first (and
/// deleted again on failure). On conflict the rebase is aborted — this path is
/// not resumable.
fn move_commits_and_report(
    workdir: &Path,
    repo: &Repository,
    commit_hashes: &[String],
    branch_name: &str,
    base_hash: Option<&str>,
) -> Result<()> {
    let created = base_hash.is_some();
    if let Some(base) = base_hash {
        git::branch_create(workdir, branch_name, base)?;
    }

    // Restored whichever way the rebase ends (Spec 014).
    let saved_staged = git::diff_cached(workdir)?;

    let parked = match move_commits_to_branch(repo, commit_hashes, branch_name) {
        Ok((RebaseOutcome::Completed, parked)) => {
            git::restore_staged_after_rebase(workdir, &saved_staged);
            parked
        }
        Ok((RebaseOutcome::Stopped | RebaseOutcome::Paused, _)) => {
            let err = abort_and_restage(workdir, &saved_staged);
            // The branch may be the checked-out ref while a failed abort
            // leaves the rebase on disk.
            if created && !git::rebase_is_in_progress(repo.path()) {
                let _ = git::branch_delete(workdir, branch_name);
            }
            return Err(err);
        }
        Err(e) => {
            // The refusal aborts a rebase that has already autostashed, and
            // that replay comes back unstaged — same as the branch above. The
            // helper leaves a pre-flight refusal's index alone on its own.
            git::restore_or_park_after_abort(workdir, &saved_staged, &e);
            // Same guard as the branch above: the branch may be the checked-out
            // ref while a failed abort leaves the rebase on disk.
            if created && !git::rebase_is_in_progress(repo.path()) {
                let _ = git::branch_delete(workdir, branch_name);
            }
            return Err(e);
        }
    };

    let new_hash = git::rev_parse(workdir, branch_name)?;
    let mut message = if created {
        format!(
            "Created branch `{}` and moved {} commit(s) to it (now {})",
            branch_name,
            commit_hashes.len(),
            repo::describe_commit(workdir, &new_hash)
        )
    } else {
        format!(
            "Moved {} commit(s) to branch `{}` (now {})",
            commit_hashes.len(),
            branch_name,
            repo::describe_commit(workdir, &new_hash)
        )
    };
    if !parked.is_empty() {
        message.push_str(&format!(
            "\n{} now empty, at the base",
            weave::describe_branches(&parked)
        ));
    }
    msg::success(&message);

    Ok(())
}

/// Abort a stopped non-resumable rebase and put the staged changes back.
///
/// `git rebase --abort` replays the autostash into the working tree only.
/// Only once the rebase is really gone is the index ours to touch: while it
/// is still on disk HEAD sits detached mid-pick, so the staging is parked in
/// a patch file instead of going with the error.
fn abort_and_restage(workdir: &Path, saved_staged: &str) -> anyhow::Error {
    let err = git::abort_after_failure(workdir);
    git::restore_or_park_after_abort(workdir, saved_staged, &err);
    err
}

/// `fold <commit>... --above|--below <commit>`: move commits next to another.
fn run_relative(repo: &Repository, args: &[String], anchor: Anchor) -> Result<()> {
    let (position, target_arg) = match &anchor {
        Anchor::Above(target) => (Position::Above, target),
        Anchor::Below(target) => (Position::Below, target),
    };

    let mut commit_hashes = Vec::new();
    for arg in args {
        match repo::resolve_arg(repo, arg, &[TargetKind::Commit])? {
            Target::Commit(hash) => commit_hashes.push(hash),
            _ => unreachable!(),
        }
    }
    let target_hash = match repo::resolve_arg(repo, target_arg, &[TargetKind::Commit])? {
        Target::Commit(hash) => hash,
        _ => unreachable!(),
    };

    let info = repo::gather_commit_graph(repo)?;
    let commit_hashes = commits_to_move(repo, commit_hashes, weave::base_oid(repo, &info)?)?;

    if commit_hashes.len() == 1 {
        fold_commit_relative(repo, &commit_hashes[0], &target_hash, position)
    } else {
        move_commits_relative_and_report(repo, &commit_hashes, &target_hash, position)
    }
}

/// Build the weave with `commit_hashes` moved next to `target_hash`.
fn plan_relative(
    repo: &Repository,
    commit_hashes: &[String],
    target_hash: &str,
    position: Position,
) -> Result<(Weave, Vec<String>)> {
    let mut graph = Weave::from_repo(repo)?;
    let oids = commit_hashes
        .iter()
        .map(|h| git2::Oid::from_str(h))
        .collect::<Result<Vec<_>, _>>()?;
    let parked = graph.move_commits_relative(&oids, git2::Oid::from_str(target_hash)?, position)?;
    Ok((graph, parked))
}

/// Point `_loom-track` at `oid` and have the todo keep it there.
///
/// The branch must exist before the rebase *and* carry an `update-ref` line, so
/// a commit outside the graph is refused rather than left with a ref that never
/// moves — the caller would read its starting hash back as the result.
fn track_through_rebase(graph: &mut Weave, workdir: &Path, oid: git2::Oid) -> Result<()> {
    git::branch_force_create(workdir, TRACK_BRANCH, &oid.to_string())?;
    if !graph.track_commit(oid, TRACK_BRANCH) {
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        return Err(weave::not_in_the_weave(oid));
    }
    Ok(())
}

/// Resumable single-commit relative move; the moved commit is tracked
/// through `_loom-track` so the result can name its new hash.
fn fold_commit_relative(
    repo: &Repository,
    commit_hash: &str,
    target_hash: &str,
    position: Position,
) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;
    let git_dir = repo.path().to_path_buf();

    let (mut graph, parked) = plan_relative(
        repo,
        std::slice::from_ref(&commit_hash.to_string()),
        target_hash,
        position,
    )?;
    track_through_rebase(&mut graph, workdir, git2::Oid::from_str(commit_hash)?)?;

    let ctx = serde_json::to_value(FoldVariant::CommitRelative {
        commit_hash: commit_hash.to_string(),
        target_hash: target_hash.to_string(),
        above: position == Position::Above,
        parked: parked.clone(),
    })?;
    let state = LoomState {
        command: COMMAND.to_string(),
        rollback: Rollback {
            delete_branches: vec![TRACK_BRANCH.to_string()],
            // Restored whichever way the rebase ends (Spec 014).
            saved_staged_patch: git::diff_cached(workdir)?,
            ..Default::default()
        },
        context: ctx,
        // `_loom-track` follows the moved commit; the target goes in too,
        // because a move relative to a commit that vanished is meaningless.
        protect: vec![commit_hash.to_string(), target_hash.to_string()],
    };
    transaction::save(&git_dir, &state)?;

    let todo = graph.to_todo();
    // Not `discard_state_after`: it leaves the temp branch, so a rebase
    // refused before it starts (a branch checked out elsewhere) would leave
    // `_loom-track` behind as a branch `status` then lists.
    let base = graph.base_oid.to_string();
    let outcome = weave::run_rebase_protecting(workdir, Some(&base), &todo, &state.protect)
        .map_err(|e| transaction::roll_back_failed_rebase(workdir, &git_dir, &state, e))?;
    match outcome {
        RebaseOutcome::Completed => {
            git::restore_staged_after_rebase(workdir, &state.rollback.saved_staged_patch);
            transaction::delete(&git_dir)?;
            let new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            report_moved_relative(
                workdir,
                commit_hash,
                position,
                target_hash,
                &new_hash,
                &parked,
            );
        }
        RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some(COMMAND));
        }
        RebaseOutcome::Stopped => {
            transaction::warn_paused(workdir, COMMAND);
        }
    }

    Ok(())
}

/// Non-resumable relative move of several commits: on conflict the rebase
/// is aborted and the staged changes restored.
fn move_commits_relative_and_report(
    repo: &Repository,
    commit_hashes: &[String],
    target_hash: &str,
    position: Position,
) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;
    let (graph, parked) = plan_relative(repo, commit_hashes, target_hash, position)?;

    let saved_staged = git::diff_cached(workdir)?;
    let todo = graph.to_todo();
    // The count has to be true: a moved commit dropped as empty is one the user
    // is told moved and cannot find. The target goes in because a move relative
    // to a commit that vanished is meaningless.
    let protect: Vec<String> = commit_hashes
        .iter()
        .cloned()
        .chain([target_hash.to_string()])
        .collect();
    let base = graph.base_oid.to_string();
    let outcome = weave::run_rebase_protecting(workdir, Some(&base), &todo, &protect)
        // The refusal aborts a rebase that has already autostashed, and that
        // replay comes back unstaged.
        .inspect_err(|e| git::restore_or_park_after_abort(workdir, &saved_staged, e))?;
    match outcome {
        RebaseOutcome::Completed => git::restore_staged_after_rebase(workdir, &saved_staged),
        RebaseOutcome::Stopped | RebaseOutcome::Paused => {
            return Err(abort_and_restage(workdir, &saved_staged));
        }
    }

    let mut message = format!(
        "Moved {} commit(s) {} `{}`",
        commit_hashes.len(),
        position.as_str(),
        git::short_hash(target_hash)
    );
    if !parked.is_empty() {
        message.push_str(&format!(
            "\n{} now empty, at the base",
            weave::describe_branches(&parked)
        ));
    }
    msg::success(&message);
    Ok(())
}

fn report_moved_relative(
    workdir: &Path,
    commit_hash: &str,
    position: Position,
    target_hash: &str,
    new_hash: &str,
    parked: &[String],
) {
    let mut message = format!(
        "Moved `{}` {} `{}` (now {})",
        git::short_hash(commit_hash),
        position.as_str(),
        git::short_hash(target_hash),
        repo::describe_commit(workdir, new_hash)
    );
    if !parked.is_empty() {
        message.push_str(&format!(
            "\n{} now empty, at the base",
            weave::describe_branches(parked)
        ));
    }
    msg::success(&message);
}

/// `fold -p` moves text hunks, and submodules and deletions whole. A binary
/// file has nothing to move (Spec 007), so `--hunks` refuses its id — but the
/// TUI still offers it, and `fold` then leaves it behind with a warning.
fn fold_picker(hunks: &HunkArgs, command: &str, target_hash: Option<&str>) -> Picker {
    Picker {
        hunks: hunks.clone(),
        command: command.to_string(),
        whole_files: false,
        target_hash: target_hash.map(str::to_string),
    }
}

/// Fold interactively-selected hunks into a target commit, or move/uncommit
/// hunks from a source commit.
///
/// Forms:
/// - `fold -p [<files>...] <commit>` — pick working-tree hunks, fold into commit
/// - `fold -p <commit1> <commit2>` — pick hunks from commit1 to move into commit2
/// - `fold -p <commit> zz` — pick hunks from commit to uncommit to working tree
fn run_patch_fold(
    repo: &Repository,
    args: &[String],
    hunks: &HunkArgs,
    git_opts: &[&str],
    theme: &graph::Theme,
) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;

    let (target_arg, source_args) = args.split_last().expect("args is non-empty");

    // Detect commit-source forms: fold -p <commit> <commit|zz>
    if source_args.len() == 1 {
        let source_arg = &source_args[0];
        if let Ok(Target::Commit(source_hash)) =
            repo::resolve_arg(repo, source_arg, &[TargetKind::Commit])
        {
            if target_arg == "zz" {
                let picker = fold_picker(
                    hunks,
                    &format!("loom fold -p {} zz", hunk_select::quoted(source_arg)),
                    None,
                );
                return run_patch_fold_commit_to_unstaged(
                    repo,
                    workdir,
                    &source_hash,
                    &picker,
                    git_opts,
                    theme,
                );
            }
            if let Ok(Target::Commit(target_hash)) =
                repo::resolve_arg(repo, target_arg, &[TargetKind::Commit])
            {
                let picker = fold_picker(
                    hunks,
                    &format!(
                        "loom fold -p {} {}",
                        hunk_select::quoted(source_arg),
                        hunk_select::quoted(target_arg)
                    ),
                    Some(&target_hash),
                );
                return run_patch_fold_commit_to_commit(
                    repo,
                    workdir,
                    &source_hash,
                    &target_hash,
                    target_arg,
                    &picker,
                    git_opts,
                    theme,
                );
            }
        }
    }

    // Working-tree hunk fold: validate sources are files/zz (not commits/branches).
    for arg in source_args {
        if arg == "zz" {
            continue;
        }
        match repo::resolve_arg(
            repo,
            arg,
            &[TargetKind::File, TargetKind::Commit, TargetKind::Branch],
        ) {
            Ok(Target::Commit(_)) | Ok(Target::Branch(_)) => bail!(
                "fold -p does not support commit or branch sources\n\
                 Use file paths, short IDs, or 'zz' to filter the hunk picker"
            ),
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }

    // Working-tree hunks have no id listing: staged and unstaged entries for the
    // same file share the numbering and it shifts as soon as anything is staged.
    if !hunks.is_empty() {
        bail!(
            "--hunks only applies to a commit source\n\
             Use `loom fold -p <commit> <target>`, or pass explicit files"
        );
    }
    if agent_mode::enabled() {
        bail!(
            "--patch over working-tree changes is interactive and unavailable in agent mode\n\
             Pass explicit files instead"
        );
    }

    let resolved = repo::resolve_arg(repo, target_arg, &[TargetKind::Commit])?;
    let commit_hash = match resolved {
        Target::Commit(hash) => hash,
        _ => unreachable!(),
    };

    // Resolved once, before the picker stages: a short ID names what is changed
    // now, and staging changes that.
    let filter = staging::filter_paths(repo, source_args)?;

    let staged = match staging::run_hunk_picker(repo, workdir, filter.as_deref(), theme)? {
        Some(paths) => paths,
        None => return Err(msg::cancelled()),
    };
    if staged.is_empty() {
        bail!("No hunks selected");
    }
    fold_files_into_commit(repo, &staged, &commit_hash, true, git_opts)
}

/// The selected entries of this file that travel in the hunk patch. A
/// whole-file label never does: a submodule or a deletion moves as the commit's
/// own whole-file diff instead (`picked_whole_files`).
fn selected_hunks(file: &FileEntry) -> impl Iterator<Item = &diff::DiffHunk> {
    file.hunks
        .iter()
        .filter(|h| h.selected && h.hunk.is_text())
        .map(|h| &h.hunk)
}

/// Build a unified diff patch from the selected text hunks across all files.
fn build_selected_patch(selections: &[FileEntry]) -> String {
    let mut patch = String::new();
    for file in selections {
        let selected: Vec<_> = selected_hunks(file).collect();
        patch.push_str(&diff::build_hunk_patch(&file.path, &selected));
    }
    patch
}

/// At a rebase edit pause: apply (or reverse-apply) a single-file patch, stage the path, amend.
///
/// A `gitlink` patch goes to the index instead and its path is never staged
/// (Spec 007). The whole-file diff carries the 160000 mode, so `--cached`
/// recreates or drops the entry as a submodule.
fn apply_and_amend_path(
    workdir: &Path,
    patch: &str,
    path: &str,
    gitlink: bool,
    reverse: bool,
    git_opts: &[&str],
) -> Result<()> {
    match (gitlink, reverse) {
        (true, true) => git::apply_cached_patch_reverse(workdir, patch)?,
        (true, false) => git::apply_cached_patch(workdir, patch)?,
        (false, true) => {
            git::apply_patch_reverse(workdir, patch)?;
            git::stage_path(workdir, path)?;
        }
        (false, false) => {
            git::apply_patch(workdir, patch)?;
            git::stage_path(workdir, path)?;
        }
    }
    git::commit_amend_no_edit(workdir, git_opts)
}

/// How a whole-file pick has to be applied.
enum WholeFileKind {
    /// A submodule: index only, so the 160000 entry never reaches the working
    /// tree. `removed` says whether the commit drops it; see
    /// [`keep_submodule_removal`].
    Gitlink { removed: bool },
    /// A file the commit deletes: working tree and index, like a hunk.
    Deletion,
}

/// A file a `-p` selection picked whole, carried by the commit's own diff.
struct PickedWholeFile {
    path: String,
    /// The commit's whole-file diff for it, carrying what
    /// [`diff::build_hunk_patch`] cannot write into a hunk patch: the 160000
    /// mode of a submodule, or the `deleted file mode` of a deletion.
    diff: String,
    kind: WholeFileKind,
}

/// The files a `-p` selection picked whole, with the diffs that can move them.
///
/// `selections` must be `collect_commit_hunks`' entries for `commit`: it is
/// what puts the commit's own name-status in `index_status`, where `'D'` names
/// a deletion rather than a staged one.
fn picked_whole_files(
    workdir: &Path,
    commit: &str,
    selections: &[FileEntry],
) -> Result<Vec<PickedWholeFile>> {
    let gitlinks = git::commit_gitlinks(workdir, commit)?;
    let mut picked = Vec::new();
    for file in selections {
        if !file.hunks.iter().any(|h| h.selected) {
            continue;
        }
        let kind = if let Some(&removed) = gitlinks.get(&file.path) {
            WholeFileKind::Gitlink { removed }
        } else if file.index_status == 'D' {
            WholeFileKind::Deletion
        } else {
            continue;
        };
        picked.push(PickedWholeFile {
            path: file.path.clone(),
            diff: git::diff_commit_file(workdir, commit, &file.path)?,
            kind,
        });
    }
    Ok(picked)
}

/// At a rebase edit pause: apply (or reverse-apply) patch, stage affected files, amend.
///
/// A picked submodule goes to the index instead of being staged by path:
/// `git add` on one stages whatever its checkout currently holds, which is not
/// what the commit this selection came from recorded. A picked deletion goes to
/// both at once: reversed it writes the file back, forward it removes it, and
/// `--index` stages either way without `git add`'s ignore rules.
fn apply_and_amend(
    workdir: &Path,
    selections: &[FileEntry],
    patch: &str,
    whole_files: &[PickedWholeFile],
    reverse: bool,
    git_opts: &[&str],
) -> Result<()> {
    if !patch.is_empty() {
        if reverse {
            git::apply_patch_reverse(workdir, patch)?;
        } else {
            git::apply_patch(workdir, patch)?;
        }
    }
    for whole in whole_files {
        match whole.kind {
            WholeFileKind::Gitlink { .. } => {
                if reverse {
                    git::apply_cached_patch_reverse(workdir, &whole.diff)?;
                } else {
                    git::apply_cached_patch(workdir, &whole.diff)?;
                }
            }
            WholeFileKind::Deletion => {
                if reverse {
                    git::apply_patch_with_index_reverse(workdir, &whole.diff)?;
                } else {
                    git::apply_patch_with_index(workdir, &whole.diff)?;
                }
            }
        }
    }
    for file in selections {
        if selected_hunks(file).next().is_some() && !is_whole_file(whole_files, &file.path) {
            git::stage_path(workdir, &file.path)?;
        }
    }
    git::commit_amend_no_edit(workdir, git_opts)
}

fn is_whole_file(whole_files: &[PickedWholeFile], path: &str) -> bool {
    whole_files.iter().any(|w| w.path == path)
}

/// Put the selection back in the working tree, unstaged.
///
/// A picked deletion is not in `patch` — it travels as its own whole-file diff
/// — so it is removed from disk here, over the index entry the amend restored,
/// which is what an unstaged deletion is.
fn restore_to_worktree(workdir: &Path, patch: &str, whole_files: &[PickedWholeFile]) -> Result<()> {
    if !patch.is_empty() {
        git::apply_patch_to_worktree(workdir, patch)?;
    }
    for whole in whole_files {
        if matches!(whole.kind, WholeFileKind::Deletion) {
            git::apply_patch_to_worktree(workdir, &whole.diff)?;
        }
    }
    Ok(())
}

/// The picked files `-p` will leave behind: every entry selected for them is a
/// whole-file label, and no whole-file diff carries them either.
fn unmovable_picks<'a>(
    selections: &'a [FileEntry],
    whole_files: &[PickedWholeFile],
) -> Vec<&'a str> {
    selections
        .iter()
        .filter(|f| {
            f.hunks.iter().any(|h| h.selected)
                && selected_hunks(f).next().is_none()
                && !is_whole_file(whole_files, &f.path)
        })
        .map(|f| f.path.as_str())
        .collect()
}

/// Says which files stayed put and how to move one whole instead.
///
/// Names where the id comes from rather than building one: a `CommitFile` id
/// resolves only through the short-ID allocator, so neither the hash nor the
/// revision the caller typed would work in its place.
fn unmovable_warning(left: &[&str], destination: &str) -> String {
    format!(
        "Left behind, no hunk to move: {}\n\
         To move one whole, take its `<commit>:<index>` id from `loom status -f` \
         and run `loom fold <id> {destination}`",
        left.join(", ")
    )
}

/// The patch `fold -p` will apply, refusing a selection with no hunk in it and
/// naming any picked file left behind rather than dropping it in silence.
fn build_movable_patch(
    selections: &[FileEntry],
    whole_files: &[PickedWholeFile],
    destination: &str,
) -> Result<String> {
    let patch = build_selected_patch(selections);
    if patch.is_empty() && whole_files.is_empty() {
        bail!("No text hunks selected — binary files are not supported with -p");
    }
    let left = unmovable_picks(selections, whole_files);
    if !left.is_empty() {
        msg::warn(&unmovable_warning(&left, destination));
    }
    Ok(patch)
}

/// Pick hunks from `source_hash` to move into `target_hash`.
///
/// Selected hunks are removed from source and added to target via a two-phase
/// edit+continue rebase. Requires source to be newer than target.
#[allow(clippy::too_many_arguments)]
fn run_patch_fold_commit_to_commit(
    repo: &Repository,
    workdir: &Path,
    source_hash: &str,
    target_hash: &str,
    target_arg: &str,
    picker: &Picker,
    git_opts: &[&str],
    theme: &graph::Theme,
) -> Result<()> {
    let source_oid = git2::Oid::from_str(source_hash)?;
    let target_oid = git2::Oid::from_str(target_hash)?;

    if source_oid == target_oid {
        bail!("Source and target are the same commit");
    }
    if !repo.graph_descendant_of(source_oid, target_oid)? {
        bail!("Source commit must be newer than target commit");
    }

    let selections = staging::run_commit_hunk_picker(workdir, source_hash, &[], picker, theme)?
        .ok_or_else(msg::cancelled)?;

    let (new_source_hash, new_target_hash) = fold_selected_hunks_to_commit(
        repo,
        workdir,
        source_hash,
        target_hash,
        target_arg,
        &selections,
        git_opts,
    )?;

    msg::success(&format!(
        "Moved hunk(s) from `{}` (now {}) into `{}` (now {})",
        git::short_hash(source_hash),
        repo::describe_commit(workdir, &new_source_hash),
        git::short_hash(target_hash),
        repo::describe_commit(workdir, &new_target_hash)
    ));

    Ok(())
}

/// The rest of [`run_patch_fold_commit_to_commit`], once the hunks are picked;
/// split off so tests can supply `selections` without the picker. Returns the
/// hashes the source and the target ended up with.
///
/// `target_arg` is what the user typed for the target, which the left-behind
/// warning echoes: a short ID or a branch name still resolves once this rebase
/// has rewritten the hash, unlike the hash itself.
fn fold_selected_hunks_to_commit(
    repo: &Repository,
    workdir: &Path,
    source_hash: &str,
    target_hash: &str,
    target_arg: &str,
    selections: &[FileEntry],
    git_opts: &[&str],
) -> Result<(String, String)> {
    let source_oid = git2::Oid::from_str(source_hash)?;

    if !selections
        .iter()
        .any(|f| f.hunks.iter().any(|h| h.selected))
    {
        bail!("No hunks selected");
    }

    let whole_files = picked_whole_files(workdir, source_hash, selections)?;

    let selected_patch = build_movable_patch(selections, &whole_files, target_arg)?;

    let saved_head = repo::head_oid(repo)?.to_string();
    let saved_refs = repo::snapshot_branch_refs(repo)?;
    // Snapshot for `rollback_fold`.
    let saved_worktree = WorktreeSnapshot::take(workdir)?;

    // Unstage pre-existing staged changes so the amends below leave them out.
    let staged = staging::save_and_unstage_staged(repo, workdir)?;

    // Phase 1: edit source, remove selected hunks.
    let mut graph = Weave::from_repo(repo)?;
    let _ = graph.edit_commit(source_oid);
    let todo = graph.to_todo();
    git::branch_force_create(workdir, TRACK_BRANCH, target_hash)?;

    if let Err(e) = weave::run_rebase_expecting_edit(
        workdir,
        Some(&graph.base_oid.to_string()),
        &todo,
        source_oid,
        &[target_hash],
    ) {
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        return Err(e);
    }

    if let Err(e) = apply_and_amend(
        workdir,
        selections,
        &selected_patch,
        &whole_files,
        true,
        git_opts,
    ) {
        return Err(git::rebase_abort_then_cleanup(workdir, e, || {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
        }));
    }

    let phase1_source_hash = git::rev_parse(workdir, "HEAD").map_err(|e| {
        // Still inside the paused rebase, so the abort comes first.
        git::rebase_abort_then_cleanup(workdir, e, || {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
        })
    })?;

    // The source replays during this continue; dropping it as empty would
    // leave the hash carried into phase 2 naming someone else's commit.
    let protect = [phase1_source_hash.clone()];
    if let Err(e) =
        git::continue_rebase_expecting_edit(workdir, git::AfterStop::nothing().protecting(&protect))
    {
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        return Err(e);
    }

    // Phase 1 is already committed, so undoing anything from here means
    // resetting over a working tree its rebase has restored. The snapshot
    // predates the unstaging, so a rollback puts the set-aside work back with
    // the rest — but only once it runs, and a failed abort skips it, so the
    // handover is inside the closure rather than before it.
    let rollback = || {
        staged.handed_over();
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
    };

    // Phase 2: edit target (new OID tracked via TRACK_BRANCH), add selected
    // hunks. Everything here is phase 1's to undo, so one rollback covers it.
    let plan_phase2 = || -> Result<(git2::Oid, Weave)> {
        let phase2_target_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        let phase2_target_oid = git2::Oid::from_str(&phase2_target_hash)?;

        // Re-open repo after phase 1 rebase (OIDs changed)
        let repo2 = Repository::open(workdir)?;
        let mut graph2 = Weave::from_repo(&repo2)?;
        let _ = graph2.edit_commit(phase2_target_oid);

        // Phase 2 replays the source too, on a rewritten target, so its phase 1
        // hash is stale by the end — track it to report the one that survives.
        let phase1_source_oid = git2::Oid::from_str(&phase1_source_hash)?;
        track_through_rebase(&mut graph2, workdir, phase1_source_oid)?;
        Ok((phase2_target_oid, graph2))
    };
    let (phase2_target_oid, graph2) = plan_phase2().inspect_err(|_| rollback())?;

    let todo2 = graph2.to_todo();

    if let Err(e) = weave::run_rebase_expecting_edit(
        workdir,
        Some(&graph2.base_oid.to_string()),
        &todo2,
        phase2_target_oid,
        &[],
    ) {
        rollback();
        return Err(e);
    }

    if let Err(e) = apply_and_amend(
        workdir,
        selections,
        &selected_patch,
        &whole_files,
        false,
        git_opts,
    ) {
        return Err(git::rebase_abort_then_cleanup(workdir, e, rollback));
    }

    // Still inside the paused rebase, so the abort comes first: `rollback_fold`
    // resets hard, and doing that under a live rebase makes the mess worse.
    let new_target_hash = match git::rev_parse(workdir, "HEAD") {
        Ok(hash) => hash,
        Err(e) => return Err(git::rebase_abort_then_cleanup(workdir, e, rollback)),
    };

    // The source replays again here, tracked by TRACK_BRANCH: dropping it as
    // empty would slide that ref down onto the commit below.
    if let Err(e) =
        git::continue_rebase_expecting_edit(workdir, git::AfterStop::nothing().protecting(&protect))
    {
        rollback();
        return Err(e);
    }

    // Both phases are committed by now, so this reads the result rather than
    // undoing it — but the index is still the one `save_and_unstage_staged`
    // emptied, and it goes back whichever way this ends.
    let tracked = git::rev_parse(workdir, TRACK_BRANCH);
    let _ = git::branch_delete(workdir, TRACK_BRANCH);
    staged.restore();
    let new_source_hash = tracked?;

    Ok((new_source_hash, new_target_hash))
}

/// Pick hunks from `commit_hash` to uncommit back into the working tree.
///
/// Selected hunks are removed from the commit and left unstaged.
fn run_patch_fold_commit_to_unstaged(
    repo: &Repository,
    workdir: &Path,
    commit_hash: &str,
    picker: &Picker,
    git_opts: &[&str],
    theme: &graph::Theme,
) -> Result<()> {
    let selections = staging::run_commit_hunk_picker(workdir, commit_hash, &[], picker, theme)?
        .ok_or_else(msg::cancelled)?;

    if !selections
        .iter()
        .any(|f| f.hunks.iter().any(|h| h.selected))
    {
        bail!("No hunks selected");
    }

    let whole_files = picked_whole_files(workdir, commit_hash, &selections)?;

    let selected_patch = build_movable_patch(&selections, &whole_files, "zz")?;

    let head_oid = repo::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    // Snapshot for `rollback_fold`.
    let saved_worktree = WorktreeSnapshot::take(workdir)?;

    // Unstage pre-existing staged changes so the amend below leaves them out.
    // Held armed throughout, so every exit below puts it back; only the
    // `rollback_fold` paths hand it over, to the snapshot above, which predates
    // the unstaging.
    let staged_aside = staging::save_and_unstage_staged(repo, workdir)?;

    let new_hash;

    if is_head {
        let pre_amend_hash = head_oid.to_string();
        // Same rollback as below: a failure part-way through leaves the hunks
        // reverse-applied in the working tree, and the set-aside work unstaged.
        if let Err(e) = apply_and_amend(
            workdir,
            &selections,
            &selected_patch,
            &whole_files,
            true,
            git_opts,
        ) {
            staged_aside.handed_over();
            rollback_fold(workdir, &pre_amend_hash, None, &saved_worktree);
            return Err(e).context("Failed to remove hunks from the commit, operation rolled back");
        }
        new_hash = git::rev_parse(workdir, "HEAD")?;
        if let Err(e) = restore_to_worktree(workdir, &selected_patch, &whole_files) {
            staged_aside.handed_over();
            rollback_fold(workdir, &pre_amend_hash, None, &saved_worktree);
            return Err(e)
                .context("Failed to restore hunks to working directory, operation rolled back");
        }
    } else {
        let saved_head = head_oid.to_string();
        let saved_refs = repo::snapshot_branch_refs(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        let _ = graph.edit_commit(target_oid);
        let todo = graph.to_todo();
        weave::run_rebase_expecting_edit(
            workdir,
            Some(&graph.base_oid.to_string()),
            &todo,
            target_oid,
            &[],
        )?;

        if let Err(e) = apply_and_amend(
            workdir,
            &selections,
            &selected_patch,
            &whole_files,
            true,
            git_opts,
        ) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {}));
        }

        new_hash = git::rev_parse(workdir, "HEAD").map_err(|e| {
            // Still inside the paused rebase, so the abort comes first.
            git::rebase_abort_then_cleanup(workdir, e, || {})
        })?;
        if let Err(e) = git::continue_rebase_expecting_edit(workdir, git::AfterStop::nothing()) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                staged_aside.handed_over();
                rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            }));
        }

        if let Err(e) = restore_to_worktree(workdir, &selected_patch, &whole_files) {
            staged_aside.handed_over();
            rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            return Err(e)
                .context("Failed to apply changes to working directory, operation rolled back");
        }
    }

    staged_aside.restore();

    let mut staged: Vec<String> = whole_files
        .iter()
        .filter(|w| matches!(w.kind, WholeFileKind::Gitlink { removed: true }))
        .filter(|w| keep_submodule_removal(workdir, &w.path))
        .map(|w| w.path.clone())
        .collect();
    staged.sort();

    let mut message = format!(
        "Uncommitted hunk(s) from `{}` (now {}) to working directory",
        git::short_hash(commit_hash),
        repo::describe_commit(workdir, &new_hash)
    );
    if !staged.is_empty() {
        let names: Vec<String> = staged.iter().map(|p| format!("`{p}`")).collect();
        message.push_str(&format!(
            "\nThe removal of {} is staged — the checkout is still on disk",
            names.join(", ")
        ));
    }
    msg::success(&message);

    Ok(())
}

/// Fold currently staged files into a target commit.
///
/// Single-argument form: `loom fold <target>`. The target must resolve to a
/// commit. If nothing is staged, bails with the same message as `loom commit`.
fn run_staged(repo: &Repository, target_arg: &str, git_opts: &[&str]) -> Result<()> {
    let resolved = repo::resolve_arg(repo, target_arg, &[TargetKind::Commit])?;
    let commit_hash = match resolved {
        Target::Commit(hash) => hash,
        _ => unreachable!(),
    };

    let staged = repo::get_staged_files(repo)?;
    if staged.is_empty() {
        bail!("Nothing to commit");
    }
    fold_files_into_commit(repo, &staged, &commit_hash, true, git_opts)
}

#[derive(Debug)]
enum FoldOp {
    FilesIntoCommit {
        files: Vec<String>,
        commit: String,
    },
    CommitIntoCommit {
        source: String,
        target: String,
    },
    CommitsToBranch {
        commits: Vec<String>,
        branch: String,
    },
    CommitToUnstaged {
        commit: String,
    },
    /// Uncommit a single file from a commit to the working directory.
    CommitFileToUnstaged {
        commit: String,
        path: String,
    },
    /// Move a file's changes from one commit to another.
    CommitFileToCommit {
        source_commit: String,
        path: String,
        target_commit: String,
    },
}

fn classify(sources: &[Target], target: &Target) -> Result<FoldOp> {
    for source in sources {
        if matches!(source, Target::Branch(_)) {
            bail!("Cannot fold a branch\nUse `git loom branch` for branch operations");
        }
    }

    if matches!(target, Target::CommitFile { .. }) {
        bail!("Target must be a commit, branch, or unstaged (zz), not a commit file");
    }

    let has_files = sources.iter().any(|s| matches!(s, Target::File(_)));
    let has_commits = sources.iter().any(|s| matches!(s, Target::Commit(_)));
    let has_commit_files = sources
        .iter()
        .any(|s| matches!(s, Target::CommitFile { .. }));

    if [has_files, has_commits, has_commit_files]
        .iter()
        .filter(|&&x| x)
        .count()
        > 1
    {
        bail!("Cannot mix different source types (files, commits, commit files)");
    }

    if has_commit_files {
        if sources.len() > 1 {
            bail!("Only one commit file source is allowed");
        }
        let (commit, path) = match &sources[0] {
            Target::CommitFile { commit, path } => (commit.clone(), path.clone()),
            _ => unreachable!(),
        };

        return match target {
            Target::Unstaged => Ok(FoldOp::CommitFileToUnstaged { commit, path }),
            Target::Commit(hash) => Ok(FoldOp::CommitFileToCommit {
                source_commit: commit,
                path,
                target_commit: hash.clone(),
            }),
            Target::Branch(_) => {
                bail!(
                    "Cannot fold a commit file into a branch\n\
                     Target a specific commit or use `zz` to uncommit"
                )
            }
            Target::File(_) => bail!("Target must be a commit or unstaged (zz), not a file"),
            Target::CommitFile { .. } => unreachable!(),
        };
    }

    if matches!(target, Target::Unstaged) {
        if has_files {
            bail!("Cannot fold files into unstaged — files are already in the working directory");
        }

        if sources.len() > 1 {
            bail!("Only one commit source is allowed");
        }

        let source_hash = match &sources[0] {
            Target::Commit(hash) => hash.clone(),
            _ => unreachable!(),
        };

        return Ok(FoldOp::CommitToUnstaged {
            commit: source_hash,
        });
    }

    if has_files {
        // File(s) + target
        let files: Vec<String> = sources
            .iter()
            .map(|s| match s {
                Target::File(path) => path.clone(),
                _ => unreachable!(),
            })
            .collect();

        match target {
            Target::Commit(hash) => Ok(FoldOp::FilesIntoCommit {
                files,
                commit: hash.clone(),
            }),
            Target::Branch(_) => {
                bail!("Cannot fold files into a branch\nTarget a specific commit")
            }
            Target::File(_) => bail!("Target must be a commit or branch, not a file"),
            _ => unreachable!(),
        }
    } else {
        // Commit(s) + target
        let hashes: Vec<String> = sources
            .iter()
            .map(|s| match s {
                Target::Commit(hash) => hash.clone(),
                _ => unreachable!(),
            })
            .collect();

        match target {
            Target::Commit(hash) => {
                // A fixup absorbs one commit into another; several sources
                // have no single meaning here.
                if hashes.len() > 1 {
                    bail!("Only one commit source is allowed");
                }
                Ok(FoldOp::CommitIntoCommit {
                    source: hashes[0].clone(),
                    target: hash.clone(),
                })
            }
            Target::Branch(name) => Ok(FoldOp::CommitsToBranch {
                commits: hashes,
                branch: name.clone(),
            }),
            Target::File(_) => bail!("Target must be a commit or branch, not a file"),
            _ => unreachable!(),
        }
    }
}

/// Collect all file paths with staged or unstaged changes.
fn collect_changed_files(repo: &Repository) -> Result<Vec<String>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut opts))?;
    let mut paths = Vec::new();
    for entry in statuses.iter() {
        if let Ok(path) = entry.path() {
            paths.push(path.to_string());
        }
    }
    Ok(paths)
}

/// Fold file changes into a commit (Case 1: File(s) + Commit).
///
/// When `skip_staging` is true the caller has already staged exactly the right
/// content (e.g. from a hunk picker), so the file-level `git add` is skipped.
fn fold_files_into_commit(
    repo: &Repository,
    files: &[String],
    commit_hash: &str,
    skip_staging: bool,
    git_opts: &[&str],
) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;

    if !skip_staging {
        for file in files {
            if !repo::path_has_changes(repo, file)? {
                bail!("File '{}' has no changes to fold", file);
            }
        }
    }

    let head_oid = repo::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    // Ask before touching anything: the path below commits a `fixup!` first,
    // so a target the weave cannot rewrite would leave that commit behind and
    // report failure. This graph is not the one that drives the rebase — that
    // one has to be built again afterwards, to see the fixup commit.
    if !is_head {
        Weave::from_repo(repo)?.require_commit(target_oid)?;
    }

    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();

    // What a rollback has to take back out of the index, as opposed to what the
    // user staged themselves.
    let staged_by_loom: &[&str] = if skip_staging { &[] } else { &file_refs };

    // Unstage pre-existing staged files outside the target list, so they do
    // not end up in this commit/amend.
    let staged = staging::save_and_unstage_other_staged(repo, workdir, &file_refs)?;

    let new_hash;

    if is_head {
        if !skip_staging {
            git::stage_files(workdir, &file_refs)?;
        }
        if let Err(e) = git::commit_amend_no_edit(workdir, git_opts) {
            // An amend that got as far as replacing HEAD and then failed leaves
            // it on a commit the user never asked for, so this takes HEAD back
            // too.
            undo_commit_attempt(workdir, head_oid, staged_by_loom, staged);
            return Err(e);
        }
        staged.restore();
        new_hash = git::rev_parse(workdir, "HEAD")?;
    } else {
        // Create a fixup commit on HEAD with only the changed files, then
        // use the weave machinery to squash it into the target commit.
        // This avoids fragile file-restoration that can fail on Windows
        // (os error 5) when files are locked by editors or indexers.
        let target_commit = repo.find_commit(target_oid)?;
        let subject = target_commit.summary().ok().flatten().unwrap_or("fixup");
        let message = format!("fixup! {}", subject);

        if !skip_staging {
            git::stage_files(workdir, &file_refs)?;
        }
        if let Err(e) = git::commit_captured(workdir, &message, git_opts) {
            undo_commit_attempt(workdir, head_oid, staged_by_loom, staged);
            return Err(e);
        }

        // Data safety: a forwarded argument git takes but loom does not know
        // can leave no commit behind, or amend HEAD in place. The squash below
        // would then feed the user's own HEAD commit into the target and lose
        // it, so check what git actually did before anything is rewritten.
        if !committed_onto(workdir, head_oid) {
            undo_commit_attempt(workdir, head_oid, staged_by_loom, staged);
            let blame = if git_opts.is_empty() {
                ""
            } else {
                "\nAn argument after `--` stopped it from committing"
            };
            bail!("`git commit` left no new commit on HEAD, so nothing was folded{blame}");
        }
        // The parent check alone accepts a child that holds nothing: `--only`
        // with no pathspec commits none of the index, and `--allow-empty` lets
        // the result through. The squash would then rewrite the target with
        // nothing in it and report the fold as done.
        if !git_opts.is_empty() && committed_the_same_tree(workdir, head_oid) {
            undo_commit_attempt(workdir, head_oid, staged_by_loom, staged);
            bail!(
                "`git commit` made an empty `fixup!` commit, so nothing was folded\n\
                 An argument after `--` kept the staged changes out of it"
            );
        }

        // From here the repository carries a commit the user never asked for,
        // and their other staged files live only in the guard.
        let git_dir = repo.path().to_path_buf();
        // Not in the match scrutinee: that would hold the borrow of `staged`
        // across arms that consume it.
        let outcome = squash_fixup_into_commit(
            &git_dir,
            workdir,
            target_oid,
            head_oid,
            files,
            staged.patch(),
        );
        match outcome {
            // The rebase is over, so the finishing steps run outside the
            // rollback: undoing a rewrite that succeeded would leave the
            // integration branch behind its own feature branches.
            Ok(FixupOutcome::Rebased) => {
                staged.restore();
                transaction::delete(&git_dir)?;
                new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
                let _ = git::branch_delete(workdir, TRACK_BRANCH);
            }
            // `loom continue` and `loom abort` own the rest, through the state
            // file the rebase left behind — the patch is in it.
            Ok(FixupOutcome::Paused) => {
                staged.handed_over();
                return Ok(());
            }
            Err(e) => {
                return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                    // Take the commit back first: the saved patch was made
                    // against the HEAD below it.
                    let _ = git::reset_soft(workdir, &head_oid.to_string());
                    if !skip_staging {
                        // The reset staged what loom staged itself; the user
                        // had these files modified, not staged.
                        let _ = git::unstage_files(workdir, &file_refs);
                    }
                    let _ = git::branch_delete(workdir, TRACK_BRANCH);
                    let _ = transaction::delete(&git_dir);
                }));
            }
        }
    }

    msg::success(&format!(
        "Folded {} file(s) into `{}` (now {})",
        files.len(),
        git::short_hash(commit_hash),
        repo::describe_commit(workdir, &new_hash)
    ));

    Ok(())
}

/// Whether HEAD is now a commit made on top of `parent`, which is what a `git
/// commit` that ran leaves behind. False for a root HEAD, and for a `git
/// commit` that committed nothing or amended `parent` away.
fn committed_onto(workdir: &Path, parent: git2::Oid) -> bool {
    git::rev_parse(workdir, "HEAD^").is_ok_and(|first| first == parent.to_string())
}

/// Whether the commit git just made holds the same tree as `parent`, which
/// [`committed_onto`] accepts because it reads the parent alone. A git that
/// cannot answer says yes, so the caller takes the commit back rather than
/// rewriting history on top of it.
fn committed_the_same_tree(workdir: &Path, parent: git2::Oid) -> bool {
    match (
        git::rev_parse(workdir, "HEAD^{tree}"),
        git::rev_parse(workdir, &format!("{parent}^{{tree}}")),
    ) {
        (Ok(now), Ok(before)) => now == before,
        _ => true,
    }
}

/// Take back a `git commit` that did not do what fold asked: whatever it did to
/// HEAD goes first, then loom's own staging, then the user's staged patch.
/// `staged_by_loom` is empty when the caller did not stage anything itself.
fn undo_commit_attempt(
    workdir: &Path,
    head_oid: git2::Oid,
    staged_by_loom: &[&str],
    staged: staging::StagedAside<'_>,
) {
    // A `--amend` moved HEAD instead of adding to it; the reset puts the
    // commit back and leaves what it held staged, for the two steps below. A
    // hook that simply refused leaves HEAD where it was, and there is nothing
    // to take back — an unreadable HEAD counts as moved, so the reset runs.
    let head_moved = git::rev_parse(workdir, "HEAD").ok() != Some(head_oid.to_string());
    if head_moved && let Err(e) = git::reset_soft(workdir, &head_oid.to_string()) {
        // The commit git replaced is reachable by hash only, so name it.
        msg::warn(&format!(
            "could not put your commit back on HEAD: {e}\n\
             `git reset --soft {head_oid}` restores it"
        ));
        // Restoring the index over the wrong HEAD would only make it worse.
        git::save_or_warn(
            workdir,
            "unrestored-staged",
            &staged.release(),
            git::Replay::Cached,
        );
        return;
    }
    if !staged_by_loom.is_empty() {
        let _ = git::unstage_files(workdir, staged_by_loom);
    }
    staged.restore();
}

/// How far [`squash_fixup_into_commit`] got.
enum FixupOutcome {
    /// The rebase finished and the caller can finish off.
    Rebased,
    /// The rebase paused or stopped, leaving the state file in charge.
    Paused,
}

/// Squash the `fixup!` commit sitting on HEAD into `target_oid`.
///
/// Everything here happens before the rebase completes, so the caller can roll
/// back any error — see [`fold_files_into_commit`]. Nothing that must survive a
/// finished rebase belongs in here.
fn squash_fixup_into_commit(
    git_dir: &Path,
    workdir: &Path,
    target_oid: git2::Oid,
    head_oid: git2::Oid,
    files: &[String],
    saved_staged: &str,
) -> Result<FixupOutcome> {
    let commit_hash = target_oid.to_string();
    let fixup_hash = git::rev_parse(workdir, "HEAD")?;
    let fixup_oid = git2::Oid::from_str(&fixup_hash)?;

    // Re-open repo after creating the fixup commit (OIDs changed)
    let repo2 = Repository::open(workdir)?;
    let mut graph = Weave::from_repo(&repo2)?;
    graph.fixup_commit(fixup_oid, target_oid)?;

    // Track target commit through the rebase via a temp branch.
    // The branch must exist before the rebase AND have an update-ref
    // line in the todo so git keeps it in sync.
    track_through_rebase(&mut graph, workdir, target_oid)?;

    let fold_ctx = serde_json::to_value(FoldVariant::FilesIntoCommit {
        original_commit_hash: commit_hash.clone(),
        files_count: files.len(),
    })?;
    let loom_state = LoomState {
        command: COMMAND.to_string(),
        rollback: Rollback {
            // `git rebase --abort` restores HEAD to the fixup commit, not past
            // it, so the undo has to go one step further back. Mixed, so the
            // folded change comes back as a working-tree change.
            reset_mixed_to: head_oid.to_string(),
            saved_staged_patch: saved_staged.to_string(),
            delete_branches: vec![TRACK_BRANCH.to_string()],
            ..Default::default()
        },
        context: fold_ctx,
        protect: vec![commit_hash],
    };
    transaction::save(git_dir, &loom_state)?;

    let todo = graph.to_todo();
    let base = graph.base_oid.to_string();
    match weave::run_rebase_protecting(workdir, Some(&base), &todo, &loom_state.protect)? {
        RebaseOutcome::Completed => Ok(FixupOutcome::Rebased),
        RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some(COMMAND));
            Ok(FixupOutcome::Paused)
        }
        RebaseOutcome::Stopped => {
            transaction::warn_paused(workdir, COMMAND);
            Ok(FixupOutcome::Paused)
        }
    }
}

/// Fold a commit into another commit (Case 2: Commit + Commit → Fixup).
fn fold_commit_into_commit(repo: &Repository, source_hash: &str, target_hash: &str) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;

    let source_oid = git2::Oid::from_str(source_hash)?;
    let target_oid = git2::Oid::from_str(target_hash)?;

    if source_oid == target_oid {
        bail!("Source and target are the same commit");
    }

    if !repo.graph_descendant_of(source_oid, target_oid)? {
        bail!("Source commit must be newer than target commit");
    }

    let mut graph = Weave::from_repo(repo)?;
    graph.fixup_commit(source_oid, target_oid)?;

    // Track target commit through the rebase via a temp branch.
    track_through_rebase(&mut graph, workdir, target_oid)?;

    let git_dir = repo.path().to_path_buf();
    let fold_ctx = serde_json::to_value(FoldVariant::CommitIntoCommit {
        source_hash: source_hash.to_string(),
        target_hash: target_hash.to_string(),
    })?;
    let loom_state = LoomState {
        command: COMMAND.to_string(),
        rollback: Rollback {
            delete_branches: vec![TRACK_BRANCH.to_string()],
            // Restored whichever way the rebase ends (Spec 014).
            saved_staged_patch: git::diff_cached(workdir)?,
            ..Default::default()
        },
        context: fold_ctx,
        protect: vec![target_hash.to_string()],
    };
    transaction::save(&git_dir, &loom_state)?;

    let todo = graph.to_todo();
    let base = graph.base_oid.to_string();
    let outcome = weave::run_rebase_protecting(workdir, Some(&base), &todo, &loom_state.protect)
        .map_err(|e| transaction::roll_back_failed_rebase(workdir, &git_dir, &loom_state, e))?;
    match outcome {
        RebaseOutcome::Completed => {
            git::restore_staged_after_rebase(workdir, &loom_state.rollback.saved_staged_patch);
            transaction::delete(&git_dir)?;
            let new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            msg::success(&format!(
                "Folded `{}` into `{}` (now {})",
                git::short_hash(source_hash),
                git::short_hash(target_hash),
                repo::describe_commit(workdir, &new_hash)
            ));
        }
        RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some(COMMAND));
        }
        RebaseOutcome::Stopped => {
            transaction::warn_paused(workdir, COMMAND);
        }
    }

    Ok(())
}

/// Move a commit to a branch (Case 3: Commit + Branch → Move).
fn fold_commit_to_branch(repo: &Repository, commit_hash: &str, branch_name: &str) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;
    let git_dir = repo.path().to_path_buf();

    // Plan before saving the state, so the parked branches reach it and
    // `loom continue` can name them too.
    let (graph, parked) = plan_move(
        repo,
        std::slice::from_ref(&commit_hash.to_string()),
        branch_name,
    )?;

    let ctx = serde_json::to_value(FoldVariant::CommitToBranch {
        commit_hash: commit_hash.to_string(),
        branch_name: branch_name.to_string(),
        parked: parked.clone(),
    })?;
    let state = LoomState {
        command: COMMAND.to_string(),
        rollback: Rollback {
            // Restored whichever way the rebase ends (Spec 014).
            saved_staged_patch: git::diff_cached(workdir)?,
            ..Default::default()
        },
        context: ctx,
        // `branch_name` is read back for the success message, so it has to
        // still name the moved commit rather than the tip it landed on.
        protect: vec![commit_hash.to_string()],
    };
    transaction::save(&git_dir, &state)?;

    let todo = graph.to_todo();
    let base = graph.base_oid.to_string();
    let outcome = weave::run_rebase_protecting(workdir, Some(&base), &todo, &state.protect)
        .map_err(|e| transaction::roll_back_failed_rebase(workdir, &git_dir, &state, e))?;
    match outcome {
        RebaseOutcome::Completed => {
            git::restore_staged_after_rebase(workdir, &state.rollback.saved_staged_patch);
            transaction::delete(&git_dir)?;
            let new_hash = git::rev_parse(workdir, branch_name)?;
            report_moved(workdir, commit_hash, branch_name, &new_hash, &parked);
        }
        RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some(COMMAND));
        }
        RebaseOutcome::Stopped => {
            transaction::warn_paused(workdir, COMMAND);
        }
    }

    Ok(())
}

/// Success message for a move, naming the branches it left empty.
fn report_moved(
    workdir: &Path,
    commit_hash: &str,
    branch_name: &str,
    new_hash: &str,
    parked: &[String],
) {
    let mut message = format!(
        "Moved `{}` to branch `{}` (now {})",
        git::short_hash(commit_hash),
        branch_name,
        repo::describe_commit(workdir, new_hash)
    );
    if !parked.is_empty() {
        message.push_str(&format!(
            "\n{} now empty, at the base",
            weave::describe_branches(parked)
        ));
    }
    msg::success(&message);
}

/// Move one or more commits to the tip of a branch using Weave.
///
/// Commits are appended in the order given, so callers that care about the
/// resulting history order should pass them oldest-first, as full object names
/// (see [`weave::run_rebase_protecting`]). Returns the rebase outcome and the
/// branches the move left empty — callers build and save their own `LoomState`.
pub fn move_commits_to_branch(
    repo: &Repository,
    commit_hashes: &[String],
    branch_name: &str,
) -> Result<(RebaseOutcome, Vec<String>)> {
    let workdir = repo::require_workdir(repo, COMMAND)?;
    let (graph, parked) = plan_move(repo, commit_hashes, branch_name)?;
    let todo = graph.to_todo();
    // Callers name the result by reading `branch_name` back, and report how
    // many commits moved: a replay dropped as empty would make both wrong.
    let base = graph.base_oid.to_string();
    let outcome = weave::run_rebase_protecting(workdir, Some(&base), &todo, commit_hashes)?;
    Ok((outcome, parked))
}

/// Build the weave with `commit_hashes` moved to the tip of `branch_name`.
///
/// Returns the graph and the branches the moved commits were the only commit
/// of, now parked at the base they built on.
fn plan_move(
    repo: &Repository,
    commit_hashes: &[String],
    branch_name: &str,
) -> Result<(Weave, Vec<String>)> {
    let mut graph = Weave::from_repo(repo)?;

    // A target branch that is neither a section nor an inner (stacked) ref
    // gets a section created for it. That happens when the branch sits at the
    // merge-base with no commits of its own: never woven, or a previous rebase
    // dropped its degenerate merge. Same pattern as commit.rs.
    let is_woven = graph.has_branch_section(branch_name) || graph.is_inner_branch(branch_name);
    if !is_woven {
        // Only a branch at the merge-base (empty) or not yet existing can get a
        // synthetic section; a diverged one is out of scope.
        if let Ok(branch) = repo.find_branch(branch_name, git2::BranchType::Local) {
            let branch_oid = branch
                .get()
                .peel_to_commit()
                .map(|c| c.id())
                .unwrap_or(git2::Oid::ZERO_SHA1);
            if branch_oid != graph.base_oid {
                bail!(
                    "Branch '{}' exists but is not part of the current integration scope.\n\
                     Use `loom branch merge {}` to weave it first.",
                    branch_name,
                    branch_name
                );
            }
        }
        graph.add_branch_section(
            branch_name.to_string(),
            vec![branch_name.to_string()],
            vec![],
            "onto".to_string(),
        );
        graph.add_merge(branch_name.to_string(), None, None);
    }

    let mut parked = Vec::new();
    for commit_hash in commit_hashes {
        let commit_oid = git2::Oid::from_str(commit_hash)?;
        parked.extend(graph.move_commit(commit_oid, branch_name)?);
    }
    // The target branch is not "left empty" — it just got the commit.
    parked.retain(|name| name != branch_name);

    Ok((graph, parked))
}

/// What the user had uncommitted before an operation started, as the two
/// patches it takes to put it back.
///
/// `worktree` is HEAD → working tree and `staged` is HEAD → index. Neither
/// contains the other — a change staged then undone in the working tree is in
/// `staged` alone — which is why both are always taken. A rollback replays
/// `staged` into the index first and `worktree` over the files after, the same
/// two patches [`Rollback`] carries, so a rollback and an abort restore alike.
/// Both hold binary files inline, which is what it costs to restore them.
struct WorktreeSnapshot {
    worktree: String,
    staged: String,
}

impl WorktreeSnapshot {
    fn take(workdir: &Path) -> Result<Self> {
        Ok(Self {
            worktree: git::diff_head(workdir)?,
            staged: git::diff_cached(workdir)?,
        })
    }
}

/// Undo a failed fold: history back to `saved_head`, then the user's own
/// uncommitted changes back on top of it.
///
/// The `reset --hard` clears whatever a failed apply left behind, conflict
/// markers included, so `saved_worktree` — taken before the operation started
/// — has to be replayed afterwards, or the uncommitted work is gone for good.
/// Should the reset or either replay fail, that half of the snapshot is saved
/// where the user can still reach it, because the caller is about to report
/// the operation as rolled back.
fn rollback_fold(
    workdir: &Path,
    saved_head: &str,
    saved_refs: Option<&std::collections::HashMap<String, git2::Oid>>,
    saved_worktree: &WorktreeSnapshot,
) {
    if let Err(e) = git::reset_hard(workdir, saved_head) {
        // The tree is not where the snapshot expects it, so replaying onto it
        // would add to the mess. Hand the patches over instead.
        msg::warn(&format!(
            "could not reset back to {}: {e}\n\
             History is NOT where it was — check `loom` before replaying anything",
            git::short_hash(saved_head)
        ));
        git::save_or_warn(
            workdir,
            "unrestored",
            &saved_worktree.worktree,
            git::Replay::Worktree,
        );
        git::save_or_warn(
            workdir,
            "unrestored-staged",
            &saved_worktree.staged,
            git::Replay::Cached,
        );
        return;
    }
    if let Some(refs) = saved_refs
        && let Err(e) = repo::restore_branch_refs(workdir, refs)
    {
        msg::warn(&format!("failed to restore branch refs: {e}"));
    }
    if !saved_worktree.staged.is_empty()
        && let Err(e) = git::apply_cached_patch(workdir, &saved_worktree.staged)
    {
        msg::warn(&format!("could not re-stage your staged changes: {e}"));
        git::save_or_warn(
            workdir,
            "unrestored-staged",
            &saved_worktree.staged,
            git::Replay::Cached,
        );
    }
    if !saved_worktree.worktree.is_empty()
        && let Err(e) = git::apply_patch(workdir, &saved_worktree.worktree)
    {
        msg::warn(&format!("could not restore your uncommitted changes: {e}"));
        git::save_or_warn(
            workdir,
            "unrestored",
            &saved_worktree.worktree,
            git::Replay::Worktree,
        );
    }
}

/// Stage a submodule's removal once it is out of the commit (Spec 007): with
/// the checkout still on disk nothing would show it, and deleting a checkout
/// that may hold the user's own work is not loom's to do.
///
/// Only warns on failure. The history rewrite has already landed by the time
/// this runs, so bailing would strand the user with no rollback.
fn keep_submodule_removal(workdir: &Path, path: &str) -> bool {
    // `try_exists`, because `exists` answers an IO error with "gone" — the one
    // answer that drops the removal.
    if !workdir.join(path).try_exists().unwrap_or(true) {
        return false;
    }
    if let Err(e) = git::remove_from_index(workdir, path) {
        msg::warn(&format!(
            "could not stage the removal of submodule `{path}`: {e}\n\
             Record it with `git rm --cached {path}`"
        ));
        return false;
    }
    true
}

/// Apply [`keep_submodule_removal`] to every submodule `commit` removes, and
/// name the ones it staged, so the caller can say where they went.
fn keep_submodule_removals(workdir: &Path, commit: &str) -> Vec<String> {
    let mut staged = Vec::new();
    match git::commit_gitlinks(workdir, commit) {
        Ok(gitlinks) => {
            for (path, removed) in gitlinks {
                if removed && keep_submodule_removal(workdir, &path) {
                    staged.push(path);
                }
            }
        }
        Err(e) => msg::warn(&format!("could not read the submodules of `{commit}`: {e}")),
    }
    staged.sort();
    staged
}

/// Uncommit a single file from a commit: its changes leave the commit and land
/// in the working tree as unstaged modifications.
fn fold_commit_file_to_unstaged(
    repo: &Repository,
    commit_hash: &str,
    path: &str,
    git_opts: &[&str],
) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;

    let head_oid = repo::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    let file_diff = git::diff_commit_file(workdir, commit_hash, path)?;
    if file_diff.is_empty() {
        bail!(
            "File '{}' has no changes in commit {}",
            path,
            git::short_hash(commit_hash)
        );
    }

    // A submodule needs no replay: moving the index entry back is itself the
    // unstaged change, because nothing here ever moves the submodule checkout.
    let gitlinks = git::commit_gitlinks(workdir, commit_hash)?;
    let gitlink = gitlinks.contains_key(path);

    // Snapshot for `rollback_fold`.
    let saved_worktree = WorktreeSnapshot::take(workdir)?;

    let new_hash;

    if is_head {
        let saved_head = head_oid.to_string();
        // A failure part-way through leaves the file reverse-applied in the
        // working tree, so this rolls back like the re-apply below does.
        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, true, git_opts) {
            rollback_fold(workdir, &saved_head, None, &saved_worktree);
            return Err(e).context("Failed to uncommit file, operation rolled back");
        }
        new_hash = git::rev_parse(workdir, "HEAD")?;
        if !gitlink && let Err(e) = git::apply_patch_to_worktree(workdir, &file_diff) {
            rollback_fold(workdir, &saved_head, None, &saved_worktree);
            return Err(e).context("Failed to uncommit file, operation rolled back");
        }
    } else {
        // Non-HEAD: edit+continue pattern with save-head rollback
        let saved_head = head_oid.to_string();
        let saved_refs = repo::snapshot_branch_refs(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        let _ = graph.edit_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase_expecting_edit(
            workdir,
            Some(&graph.base_oid.to_string()),
            &todo,
            target_oid,
            &[],
        )
        .inspect_err(|e| git::restore_or_park_after_abort(workdir, &saved_worktree.staged, e))?;

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, true, git_opts) {
            // Outside the cleanup closure: a failed abort skips it, and there
            // is no `LoomState` for `loom abort` to find the patch in.
            let e = git::rebase_abort_then_cleanup(workdir, e, || {});
            git::restore_or_park_after_abort(workdir, &saved_worktree.staged, &e);
            return Err(e);
        }

        new_hash = git::rev_parse(workdir, "HEAD").map_err(|e| {
            // Still inside the paused rebase, so the abort comes first.
            let e = git::rebase_abort_then_cleanup(workdir, e, || {});
            git::restore_or_park_after_abort(workdir, &saved_worktree.staged, &e);
            e
        })?;

        git::continue_rebase_expecting_edit(workdir, git::AfterStop::nothing()).inspect_err(
            |e| git::restore_or_park_after_abort(workdir, &saved_worktree.staged, e),
        )?;

        if !gitlink && let Err(e) = git::apply_patch_to_worktree(workdir, &file_diff) {
            rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            return Err(e).context("Failed to uncommit file, operation rolled back");
        }
        // After the worktree apply, like every other uncommit path: its failure
        // recovery checks files out of the index, so it has to see the index
        // the rebase left rather than one this restore has written to.
        git::restore_staged_after_rebase(workdir, &saved_worktree.staged);
    }

    let mut staged_removal = false;
    if gitlinks.get(path).copied().unwrap_or(false) {
        staged_removal = keep_submodule_removal(workdir, path);
    }

    msg::success(&format!(
        "Uncommitted `{}` from `{}` (now {}) {}",
        path,
        git::short_hash(commit_hash),
        repo::describe_commit(workdir, &new_hash),
        if staged_removal {
            "as a staged deletion"
        } else {
            "to working directory"
        }
    ));

    Ok(())
}

/// Move a file's changes from one commit to another; both are rewritten.
fn fold_commit_file_to_commit(
    repo: &Repository,
    source_hash: &str,
    path: &str,
    target_hash: &str,
    git_opts: &[&str],
) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;

    let source_oid = git2::Oid::from_str(source_hash)?;
    let target_oid = git2::Oid::from_str(target_hash)?;

    if source_oid == target_oid {
        bail!("Source and target are the same commit");
    }

    let file_diff = git::diff_commit_file(workdir, source_hash, path)?;
    if file_diff.is_empty() {
        bail!(
            "File '{}' has no changes in commit {}",
            path,
            git::short_hash(source_hash)
        );
    }

    let gitlink = git::commit_gitlinks(workdir, source_hash)?.contains_key(path);

    let source_is_newer = repo.graph_descendant_of(source_oid, target_oid)?;

    // Snapshot for `rollback_fold`: both branches below reset over a working
    // tree a rebase has put the user's uncommitted changes back into.
    let saved_worktree = WorktreeSnapshot::take(workdir)?;

    let new_source_hash;
    let new_target_hash;

    if source_is_newer {
        // Two-phase edit+continue with rollback. A source newer than its target
        // cannot be done in one rebase: adding the file to the target (picked
        // first) conflicts when the source is replayed, since the source still has
        // the file. Phase 1 removes it from the source, phase 2 adds it to the
        // target; a phase 2 failure rolls back to the pre-phase-1 state.
        let saved_head = repo::head_oid(repo)?.to_string();
        let saved_refs = repo::snapshot_branch_refs(repo)?;

        let rollback = || {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
        };

        // Phase 1: edit at source, remove file, continue.
        // Create temp branch AFTER from_repo to avoid polluting the Weave graph,
        // but before the rebase so git's --update-refs tracks the target's new OID.
        let mut graph = Weave::from_repo(repo)?;
        let _ = graph.edit_commit(source_oid);
        let todo = graph.to_todo();
        git::branch_force_create(workdir, TRACK_BRANCH, target_hash)?;

        if let Err(e) = weave::run_rebase_expecting_edit(
            workdir,
            Some(&graph.base_oid.to_string()),
            &todo,
            source_oid,
            &[target_hash],
        ) {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            git::restore_or_park_after_abort(workdir, &saved_worktree.staged, &e);
            return Err(e);
        }

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, true, git_opts) {
            // Outside the cleanup closure: a failed abort skips it, and there
            // is no `LoomState` for `loom abort` to find the patch in.
            let e = git::rebase_abort_then_cleanup(workdir, e, || {
                let _ = git::branch_delete(workdir, TRACK_BRANCH);
            });
            git::restore_or_park_after_abort(workdir, &saved_worktree.staged, &e);
            return Err(e);
        }

        // Capture source's new hash before continue moves HEAD;
        // it will be tracked through phase 2 via a temp branch.
        let phase1_source_hash = git::rev_parse(workdir, "HEAD").map_err(|e| {
            // Still inside the paused rebase, so the abort comes first.
            let e = git::rebase_abort_then_cleanup(workdir, e, || {
                let _ = git::branch_delete(workdir, TRACK_BRANCH);
            });
            git::restore_or_park_after_abort(workdir, &saved_worktree.staged, &e);
            e
        })?;

        // The source replays during this continue and again in phase 2, where
        // TRACK_BRANCH follows it; dropping it as empty would leave the hash
        // reported at the end naming someone else's commit.
        let protect = [phase1_source_hash.clone()];
        if let Err(e) = git::continue_rebase_expecting_edit(
            workdir,
            git::AfterStop::nothing().protecting(&protect),
        ) {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            git::restore_or_park_after_abort(workdir, &saved_worktree.staged, &e);
            return Err(e);
        }

        // Phase 1's rebase is over, so its autostash has already come back
        // unstaged: every exit from here on has to put the index back.
        let restage = || git::restore_staged_after_rebase(workdir, &saved_worktree.staged);

        // Phase 2: resolve the target's new OID via the temp branch.
        let phase2_target_hash =
            git::rev_parse(workdir, TRACK_BRANCH).inspect_err(|_| restage())?;
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        let phase2_target_oid =
            git2::Oid::from_str(&phase2_target_hash).inspect_err(|_| restage())?;

        // Re-open repo after phase 1 rebase (OIDs changed)
        let repo2 = Repository::open(workdir).inspect_err(|_| restage())?;
        let mut graph2 = Weave::from_repo(&repo2).inspect_err(|_| restage())?;
        let _ = graph2.edit_commit(phase2_target_oid);

        // Track source through phase 2 — it will be rewritten when the
        // graph is replayed from base_oid.
        let phase1_source_oid =
            git2::Oid::from_str(&phase1_source_hash).inspect_err(|_| restage())?;
        if let Err(e) = track_through_rebase(&mut graph2, workdir, phase1_source_oid) {
            rollback();
            return Err(e);
        }

        let todo2 = graph2.to_todo();

        if let Err(e) = weave::run_rebase_expecting_edit(
            workdir,
            Some(&graph2.base_oid.to_string()),
            &todo2,
            phase2_target_oid,
            &[],
        ) {
            rollback();
            return Err(e);
        }

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, false, git_opts) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, rollback));
        }

        new_target_hash = git::rev_parse(workdir, "HEAD").map_err(|e| {
            // Still inside the paused rebase, so the abort comes first.
            git::rebase_abort_then_cleanup(workdir, e, rollback)
        })?;

        if let Err(e) = git::continue_rebase_expecting_edit(
            workdir,
            git::AfterStop::nothing().protecting(&protect),
        ) {
            rollback();
            return Err(e);
        }

        new_source_hash = git::rev_parse(workdir, TRACK_BRANCH).inspect_err(|_| {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            restage();
        })?;
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
    } else {
        // Source is older than target: single rebase with two edit pauses.
        // Source is picked first (older), target second (newer). Removing
        // the file from source before target is replayed avoids conflicts.
        let saved_head = repo::head_oid(repo)?.to_string();
        let saved_refs = repo::snapshot_branch_refs(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        let _ = graph.edit_commit(source_oid);
        // The second `edit` is the one `run_rebase_expecting_edit` does not
        // check: it verifies the stop it is given, which is the source.
        if !graph.edit_commit(target_oid) {
            return Err(weave::not_in_the_weave(target_oid));
        }

        let todo = graph.to_todo();
        weave::run_rebase_expecting_edit(
            workdir,
            Some(&graph.base_oid.to_string()),
            &todo,
            source_oid,
            &[],
        )
        .inspect_err(|e| git::restore_or_park_after_abort(workdir, &saved_worktree.staged, e))?;

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, true, git_opts) {
            // Outside the cleanup closure: a failed abort skips it, and there
            // is no `LoomState` for `loom abort` to find the patch in.
            let e = git::rebase_abort_then_cleanup(workdir, e, || {});
            git::restore_or_park_after_abort(workdir, &saved_worktree.staged, &e);
            return Err(e);
        }

        new_source_hash = git::rev_parse(workdir, "HEAD").map_err(|e| {
            // Still inside the paused rebase, so the abort comes first.
            git::rebase_abort_then_cleanup(workdir, e, || {
                rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            })
        })?;

        // The source amend is already committed, so a continue that never
        // reached the target leaves the file removed and nowhere else: it has
        // to be rolled back, not just aborted.
        if let Err(e) = git::continue_rebase_expecting_edit(
            workdir,
            git::AfterStop::rewrite(&target_oid.to_string()),
        ) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            }));
        }

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, false, git_opts) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            }));
        }

        new_target_hash = git::rev_parse(workdir, "HEAD").map_err(|e| {
            // Still inside the paused rebase, so the abort comes first.
            git::rebase_abort_then_cleanup(workdir, e, || {
                rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            })
        })?;

        if let Err(e) = git::continue_rebase_expecting_edit(workdir, git::AfterStop::nothing()) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            }));
        }
    }

    git::restore_staged_after_rebase(workdir, &saved_worktree.staged);

    msg::success(&format!(
        "Moved `{}` from `{}` (now {}) to `{}` (now {})",
        path,
        git::short_hash(source_hash),
        repo::describe_commit(workdir, &new_source_hash),
        git::short_hash(target_hash),
        repo::describe_commit(workdir, &new_target_hash)
    ));

    Ok(())
}

/// Uncommit a commit to the working directory (Case 4: Commit + Unstaged):
/// the commit leaves history and its changes land unstaged.
fn fold_commit_to_unstaged(repo: &Repository, commit_hash: &str) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;

    let head_oid = repo::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    if is_head {
        git::reset_mixed(workdir, "HEAD~1")?;
        let staged = keep_submodule_removals(workdir, commit_hash);
        report_uncommitted(commit_hash, &[], &staged);
        return Ok(());
    } else {
        // Non-HEAD: drop the commit from the weave, then apply its diff
        let mut graph = Weave::from_repo(repo)?;
        // A branch whose only commit this is survives, parked at its base,
        // ready for the reworked change to be committed to it again.
        let Some(emptied) = graph.drop_commit(target_oid, EmptiedRefs::Park) else {
            bail!(
                "Commit `{}` is not in the local commits (upstream..HEAD)\n\
                 If history was rewritten, the SHA may be stale — run `loom` to see the current commits",
                git::short_hash(commit_hash)
            );
        };

        let diff = git::diff_commit(workdir, commit_hash)?;
        let saved_head = head_oid.to_string();
        let saved_refs = repo::snapshot_branch_refs(repo)?;
        // Snapshot for `rollback_fold`.
        let saved_worktree = WorktreeSnapshot::take(workdir)?;

        let git_dir = repo.path().to_path_buf();
        let fold_ctx = serde_json::to_value(FoldVariant::CommitToUnstaged {
            commit_hash: commit_hash.to_string(),
            diff: diff.clone(),
            emptied: emptied.clone(),
        })?;
        let loom_state = LoomState {
            command: COMMAND.to_string(),
            rollback: Rollback {
                // Restored whichever way the rebase ends (Spec 014).
                saved_staged_patch: saved_worktree.staged.clone(),
                ..Default::default()
            },
            context: fold_ctx,
            protect: Vec::new(),
        };
        transaction::save(&git_dir, &loom_state)?;

        let todo = graph.to_todo();
        let outcome = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)
            .map_err(|e| {
                transaction::discard_state_after(
                    workdir,
                    &git_dir,
                    &loom_state.rollback.saved_staged_patch,
                    e,
                )
            })?;
        let staged = match outcome {
            RebaseOutcome::Completed => {
                transaction::delete(&git_dir)?;
                if !diff.is_empty()
                    && let Err(e) = git::apply_patch_to_worktree(workdir, &diff)
                {
                    rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
                    return Err(e).context(
                        "Failed to apply changes to working directory, operation rolled back",
                    );
                }
                git::restore_staged_after_rebase(workdir, &loom_state.rollback.saved_staged_patch);
                keep_submodule_removals(workdir, commit_hash)
            }
            RebaseOutcome::Paused => {
                transaction::warn_paused_at_edit(Some(COMMAND));
                return Ok(());
            }
            RebaseOutcome::Stopped => {
                transaction::warn_paused(workdir, COMMAND);
                return Ok(());
            }
        };
        report_uncommitted(commit_hash, &emptied, &staged);
    }

    Ok(())
}

/// Success message for an uncommit, noting the branches it left empty.
fn report_uncommitted(commit_hash: &str, emptied: &[String], staged: &[String]) {
    let mut message = format!(
        "Uncommitted `{}` to working directory",
        git::short_hash(commit_hash)
    );
    if !staged.is_empty() {
        let names: Vec<String> = staged.iter().map(|p| format!("`{p}`")).collect();
        message.push_str(&format!(
            "\nThe removal of {} is staged — the checkout is still on disk",
            names.join(", ")
        ));
    }
    if !emptied.is_empty() {
        message.push_str(&format!(
            "\n{} now empty, at the base",
            weave::describe_branches(emptied)
        ));
    }
    msg::success(&message);
}

/// Resume a `fold` operation after a conflict has been resolved.
pub fn after_continue(
    workdir: &Path,
    rollback: &Rollback,
    context: &serde_json::Value,
) -> Result<()> {
    let variant: FoldVariant =
        serde_json::from_value(context.clone()).context("Failed to parse fold resume context")?;
    // Every variant that saved a patch restores it here; the rest saved none.
    // `CommitToUnstaged` is the exception and does its own below, after the
    // worktree apply: that apply's failure recovery checks files out of the
    // index, so it has to see the index the direct path leaves it, not one the
    // restore has already written to.
    if !matches!(variant, FoldVariant::CommitToUnstaged { .. }) {
        git::restore_staged_after_rebase(workdir, &rollback.saved_staged_patch);
    }

    match variant {
        FoldVariant::FilesIntoCommit {
            original_commit_hash,
            files_count,
        } => {
            let new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            msg::success(&format!(
                "Folded {} file(s) into `{}` (now {})",
                files_count,
                git::short_hash(&original_commit_hash),
                repo::describe_commit(workdir, &new_hash)
            ));
        }
        FoldVariant::CommitIntoCommit {
            source_hash,
            target_hash,
        } => {
            let new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            msg::success(&format!(
                "Folded `{}` into `{}` (now {})",
                git::short_hash(&source_hash),
                git::short_hash(&target_hash),
                repo::describe_commit(workdir, &new_hash)
            ));
        }
        FoldVariant::CommitToBranch {
            commit_hash,
            branch_name,
            parked,
        } => {
            let new_hash = git::rev_parse(workdir, &branch_name)?;
            report_moved(workdir, &commit_hash, &branch_name, &new_hash, &parked);
        }
        FoldVariant::CommitToUnstaged {
            commit_hash,
            diff,
            emptied,
        } => {
            if !diff.is_empty()
                && let Err(e) = git::apply_patch_to_worktree(workdir, &diff)
            {
                // The rebase succeeded (the commit is gone) but the diff will not
                // re-apply — usually because conflict resolution changed the surrounding
                // context. Save it to a file so the user can recover it by hand.
                let mut warning = format!("Could not re-apply changes to working directory: {e}");
                match git::save_patch_aside(workdir, "unapplied", &diff) {
                    Ok(path) => warning.push_str(&format!(
                        "\nThe diff has been saved — apply it with `git apply {}`",
                        path.display()
                    )),
                    Err(save) => warning.push_str(&format!(
                        "\nThe diff could not be saved either ({save}) — `git show {commit_hash}` \
                         still prints it, until that dangling commit is collected",
                    )),
                }
                msg::warn(&warning);
            }
            git::restore_staged_after_rebase(workdir, &rollback.saved_staged_patch);
            let staged = keep_submodule_removals(workdir, &commit_hash);
            report_uncommitted(&commit_hash, &emptied, &staged);
        }
        FoldVariant::CommitRelative {
            commit_hash,
            target_hash,
            above,
            parked,
        } => {
            let new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            let position = if above {
                Position::Above
            } else {
                Position::Below
            };
            report_moved_relative(
                workdir,
                &commit_hash,
                position,
                &target_hash,
                &new_hash,
                &parked,
            );
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "fold_test.rs"]
mod tests;
