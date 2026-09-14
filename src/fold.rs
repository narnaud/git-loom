use anyhow::{Context, Result, bail};
use git2::{Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::diff;
use crate::core::graph;
use crate::core::msg;
use crate::core::repo::{self, Target, TargetKind};
use crate::core::staging;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::{self, EmptiedRefs, RebaseOutcome, Weave};
use crate::git;
use crate::tui::hunk_selector::FileEntry;

#[derive(Serialize, Deserialize)]
#[serde(tag = "op")]
enum FoldVariant {
    FilesIntoCommit {
        original_commit_hash: String,
        files_count: usize,
        saved_staged: String,
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
}

/// Temporary branch used to track a commit's new OID through a rebase.
const TRACK_BRANCH: &str = "_loom-track";
const COMMAND: &str = "fold";

/// Fold source(s) into a target.
///
/// Forms:
/// - File(s) + Commit → amend files into the commit
/// - Commit + Commit  → fixup source into target (source disappears)
/// - Commit(s) + Branch → move the commit(s) to the branch, oldest-first
///
/// With `--create` (`-c`): create a new branch and move the source commit(s)
/// into it. The name must not be taken.
pub fn run(create: bool, patch: bool, args: Vec<String>, theme: &graph::Theme) -> Result<()> {
    if args.is_empty() {
        bail!(
            "At least one argument required\n\
             Usage: git-loom fold [<source>...] <target>"
        );
    }

    let repo = repo::open_repo()?;

    if create {
        return run_create(&repo, &args);
    }

    if patch {
        return run_patch_fold(&repo, &args, theme);
    }

    if args.len() == 1 {
        return run_staged(&repo, &args[0]);
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
            fold_files_into_commit(&repo, &files, &commit, false)
        }
        FoldOp::CommitIntoCommit { source, target } => {
            fold_commit_into_commit(&repo, &source, &target)
        }
        FoldOp::CommitsToBranch { commits, branch } => {
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
        FoldOp::CommitToUnstaged { commit } => fold_commit_to_unstaged(&repo, &commit),
        FoldOp::CommitFileToUnstaged { commit, path } => {
            fold_commit_file_to_unstaged(&repo, &commit, &path)
        }
        FoldOp::CommitFileToCommit {
            source_commit,
            path,
            target_commit,
        } => fold_commit_file_to_commit(&repo, &source_commit, &path, &target_commit),
    }
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

/// Re-stage what the abort brought back as unstaged.
///
/// `git rebase --abort` replays its autostash into the working tree, so the
/// content returns but the staging mostly does not. The snapshot is a
/// HEAD-to-index diff, so the index is first reset back to HEAD; the working
/// tree is untouched by that, and a patch that still will not apply is saved
/// aside.
fn restage_after_abort(workdir: &Path, staged: &str) {
    if staged.is_empty() {
        return;
    }
    let restored =
        git::reset_mixed(workdir, "HEAD").and_then(|()| git::apply_cached_patch(workdir, staged));
    if let Err(e) = restored {
        msg::warn(&format!("could not re-stage your staged changes: {e}"));
        save_or_warn(workdir, "unrestored-staged", staged, true);
    }
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

    // The rebase autostashes, and the abort below replays that into the
    // working tree only — staged changes would come back unstaged.
    let saved_staged = git::diff_cached(workdir)?;

    let parked = match move_commits_to_branch(repo, commit_hashes, branch_name) {
        Ok((RebaseOutcome::Completed, parked)) => parked,
        Ok((RebaseOutcome::Stopped | RebaseOutcome::Paused, _)) => {
            let err = git::abort_after_failure(workdir);
            // Only once the rebase is really gone: while it is still on disk
            // HEAD sits detached mid-pick, so resetting the index there would
            // clobber it, and the branch may be the checked-out ref.
            if git::rebase_is_in_progress(repo.path()) {
                // The abort failed too, so the index is not ours to touch.
                // Park the staging rather than let it go with the error.
                save_or_warn(workdir, "unrestored-staged", &saved_staged, true);
            } else {
                restage_after_abort(workdir, &saved_staged);
                if created {
                    let _ = git::branch_delete(workdir, branch_name);
                }
            }
            return Err(err);
        }
        Err(e) => {
            if created {
                let _ = git::branch_delete(workdir, branch_name);
            }
            return Err(e);
        }
    };

    let new_hash = git::rev_parse(workdir, branch_name)?;
    let mut message = if created {
        format!(
            "Created branch `{}` and moved {} commit(s) to it (now `{}`)",
            branch_name,
            commit_hashes.len(),
            git::short_hash(&new_hash)
        )
    } else {
        format!(
            "Moved {} commit(s) to branch `{}` (now `{}`)",
            commit_hashes.len(),
            branch_name,
            git::short_hash(&new_hash)
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

/// Fold interactively-selected hunks into a target commit, or move/uncommit
/// hunks from a source commit.
///
/// Forms:
/// - `fold -p [<files>...] <commit>` — pick working-tree hunks, fold into commit
/// - `fold -p <commit1> <commit2>` — pick hunks from commit1 to move into commit2
/// - `fold -p <commit> zz` — pick hunks from commit to uncommit to working tree
fn run_patch_fold(repo: &Repository, args: &[String], theme: &graph::Theme) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;

    let (target_arg, source_args) = args.split_last().expect("args is non-empty");

    // Detect commit-source forms: fold -p <commit> <commit|zz>
    if source_args.len() == 1 {
        let source_arg = &source_args[0];
        if let Ok(Target::Commit(source_hash)) =
            repo::resolve_arg(repo, source_arg, &[TargetKind::Commit])
        {
            if target_arg == "zz" {
                return run_patch_fold_commit_to_unstaged(repo, workdir, &source_hash, theme);
            }
            if let Ok(Target::Commit(target_hash)) =
                repo::resolve_arg(repo, target_arg, &[TargetKind::Commit])
            {
                return run_patch_fold_commit_to_commit(
                    repo,
                    workdir,
                    &source_hash,
                    &target_hash,
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

    let resolved = repo::resolve_arg(repo, target_arg, &[TargetKind::Commit])?;
    let commit_hash = match resolved {
        Target::Commit(hash) => hash,
        _ => unreachable!(),
    };

    let confirmed = staging::run_hunk_picker(repo, workdir, source_args, theme)?;
    if !confirmed {
        bail!("Cancelled");
    }

    let staged = repo::get_staged_files(repo)?;
    if staged.is_empty() {
        bail!("Nothing to commit");
    }
    fold_files_into_commit(repo, &staged, &commit_hash, true)
}

/// Build a unified diff patch from the selected text hunks across all files.
fn build_selected_patch(selections: &[FileEntry]) -> String {
    let mut patch = String::new();
    for file in selections {
        if file.binary {
            continue;
        }
        let selected: Vec<_> = file
            .hunks
            .iter()
            .filter(|h| h.selected)
            .map(|h| &h.hunk)
            .collect();
        if !selected.is_empty() {
            patch.push_str(&diff::build_hunk_patch(&file.path, &selected));
        }
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
    git::commit_amend_no_edit(workdir)
}

/// A submodule a `-p` selection picked, carried by the commit's own diff.
struct PickedGitlink {
    path: String,
    /// The commit's whole-file diff for it, which carries the 160000 mode that
    /// [`diff::build_hunk_patch`] cannot write into a hunk patch.
    diff: String,
    /// Whether the commit removes the entry; see [`keep_submodule_removal`].
    removed: bool,
}

/// The submodules a `-p` selection picked, with the diffs that can move them.
fn picked_gitlinks(
    workdir: &Path,
    commit: &str,
    selections: &[FileEntry],
) -> Result<Vec<PickedGitlink>> {
    let gitlinks = git::commit_gitlinks(workdir, commit)?;
    let mut picked = Vec::new();
    for file in selections {
        if let Some(&removed) = gitlinks.get(&file.path)
            && file.hunks.iter().any(|h| h.selected)
        {
            picked.push(PickedGitlink {
                path: file.path.clone(),
                diff: git::diff_commit_file(workdir, commit, &file.path)?,
                removed,
            });
        }
    }
    Ok(picked)
}

/// At a rebase edit pause: apply (or reverse-apply) patch, stage affected files, amend.
///
/// Picked submodules go to the index instead of being staged by path: `git add`
/// on one stages whatever its checkout currently holds, which is not what the
/// commit this selection came from recorded.
fn apply_and_amend(
    workdir: &Path,
    selections: &[FileEntry],
    patch: &str,
    gitlinks: &[PickedGitlink],
    reverse: bool,
) -> Result<()> {
    if !patch.is_empty() {
        if reverse {
            git::apply_patch_reverse(workdir, patch)?;
        } else {
            git::apply_patch(workdir, patch)?;
        }
    }
    for gitlink in gitlinks {
        if reverse {
            git::apply_cached_patch_reverse(workdir, &gitlink.diff)?;
        } else {
            git::apply_cached_patch(workdir, &gitlink.diff)?;
        }
    }
    for file in selections {
        if file.hunks.iter().any(|h| h.selected) && !gitlinks.iter().any(|g| g.path == file.path) {
            git::stage_path(workdir, &file.path)?;
        }
    }
    git::commit_amend_no_edit(workdir)
}

/// Pick hunks from `source_hash` to move into `target_hash`.
///
/// Selected hunks are removed from source and added to target via a two-phase
/// edit+continue rebase. Requires source to be newer than target.
fn run_patch_fold_commit_to_commit(
    repo: &Repository,
    workdir: &Path,
    source_hash: &str,
    target_hash: &str,
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

    let selections = staging::run_commit_hunk_picker(workdir, source_hash, &[], theme)?
        .ok_or_else(|| anyhow::anyhow!("Cancelled"))?;

    if !selections
        .iter()
        .any(|f| f.hunks.iter().any(|h| h.selected))
    {
        bail!("No hunks selected");
    }

    let gitlinks = picked_gitlinks(workdir, source_hash, &selections)?;

    let selected_patch = build_selected_patch(&selections);
    if selected_patch.is_empty() && gitlinks.is_empty() {
        bail!("No text hunks selected — binary and deleted files are not supported with -p");
    }

    let saved_head = repo::head_oid(repo)?.to_string();
    let saved_refs = repo::snapshot_branch_refs(repo)?;
    // Snapshot for `rollback_fold`.
    let saved_worktree = WorktreeSnapshot::take(workdir)?;

    // Unstage pre-existing staged changes so the amends below leave them out.
    let saved_staged = staging::save_and_unstage_staged(repo, workdir)?;

    // Phase 1: edit source, remove selected hunks.
    let mut graph = Weave::from_repo(repo)?;
    graph.edit_commit(source_oid);
    let todo = graph.to_todo();
    git::branch_force_create(workdir, TRACK_BRANCH, target_hash)?;

    if let Err(e) =
        weave::run_rebase_expecting_edit(workdir, Some(&graph.base_oid.to_string()), &todo)
    {
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        let _ = git::restore_staged_patch(workdir, &saved_staged);
        return Err(e);
    }

    if let Err(e) = apply_and_amend(workdir, &selections, &selected_patch, &gitlinks, true) {
        return Err(git::rebase_abort_then_cleanup(workdir, e, || {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            let _ = git::restore_staged_patch(workdir, &saved_staged);
        }));
    }

    let new_source_hash = git::rev_parse(workdir, "HEAD")?;

    if let Err(e) = git::continue_rebase_expecting_edit(workdir) {
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        let _ = git::restore_staged_patch(workdir, &saved_staged);
        return Err(e);
    }

    // Phase 2: edit target (new OID tracked via TRACK_BRANCH), add selected hunks.
    let phase2_target_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
    let _ = git::branch_delete(workdir, TRACK_BRANCH);
    let phase2_target_oid = git2::Oid::from_str(&phase2_target_hash)?;

    // Re-open repo after phase 1 rebase (OIDs changed)
    let repo2 = Repository::open(workdir)?;
    let mut graph2 = Weave::from_repo(&repo2)?;
    graph2.edit_commit(phase2_target_oid);
    let todo2 = graph2.to_todo();

    // Phase 1 is already committed, so undoing phase 2 means resetting over a
    // working tree its rebase has restored. The snapshot predates
    // `save_and_unstage_staged`, so it puts `saved_staged` back along with it.
    let rollback = || rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);

    if let Err(e) =
        weave::run_rebase_expecting_edit(workdir, Some(&graph2.base_oid.to_string()), &todo2)
    {
        rollback();
        return Err(e);
    }

    if let Err(e) = apply_and_amend(workdir, &selections, &selected_patch, &gitlinks, false) {
        return Err(git::rebase_abort_then_cleanup(workdir, e, rollback));
    }

    let new_target_hash = git::rev_parse(workdir, "HEAD")?;

    if let Err(e) = git::continue_rebase_expecting_edit(workdir) {
        rollback();
        return Err(e);
    }

    git::restore_staged_patch(workdir, &saved_staged)?;

    msg::success(&format!(
        "Moved hunk(s) from `{}` (now `{}`) into `{}` (now `{}`)",
        git::short_hash(source_hash),
        git::short_hash(&new_source_hash),
        git::short_hash(target_hash),
        git::short_hash(&new_target_hash)
    ));

    Ok(())
}

/// Pick hunks from `commit_hash` to uncommit back into the working tree.
///
/// Selected hunks are removed from the commit and left unstaged.
fn run_patch_fold_commit_to_unstaged(
    repo: &Repository,
    workdir: &Path,
    commit_hash: &str,
    theme: &graph::Theme,
) -> Result<()> {
    let selections = staging::run_commit_hunk_picker(workdir, commit_hash, &[], theme)?
        .ok_or_else(|| anyhow::anyhow!("Cancelled"))?;

    if !selections
        .iter()
        .any(|f| f.hunks.iter().any(|h| h.selected))
    {
        bail!("No hunks selected");
    }

    let gitlinks = picked_gitlinks(workdir, commit_hash, &selections)?;

    let selected_patch = build_selected_patch(&selections);
    if selected_patch.is_empty() && gitlinks.is_empty() {
        bail!("No text hunks selected — binary and deleted files are not supported with -p");
    }

    let head_oid = repo::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    // Snapshot for `rollback_fold`.
    let saved_worktree = WorktreeSnapshot::take(workdir)?;

    // Unstage pre-existing staged changes so the amend below leaves them out.
    let saved_staged = staging::save_and_unstage_staged(repo, workdir)?;

    let new_hash;

    if is_head {
        let pre_amend_hash = head_oid.to_string();
        // Same rollback as below: a failure part-way through leaves the hunks
        // reverse-applied in the working tree, and `saved_staged` unstaged.
        if let Err(e) = apply_and_amend(workdir, &selections, &selected_patch, &gitlinks, true) {
            rollback_fold(workdir, &pre_amend_hash, None, &saved_worktree);
            return Err(e).context("Failed to remove hunks from the commit, operation rolled back");
        }
        new_hash = git::rev_parse(workdir, "HEAD")?;
        if !selected_patch.is_empty()
            && let Err(e) = git::apply_patch_to_worktree(workdir, &selected_patch)
        {
            // The snapshot predates `save_and_unstage_staged`, so the rollback
            // puts `saved_staged` back along with the rest.
            rollback_fold(workdir, &pre_amend_hash, None, &saved_worktree);
            return Err(e)
                .context("Failed to restore hunks to working directory, operation rolled back");
        }
    } else {
        let saved_head = head_oid.to_string();
        let saved_refs = repo::snapshot_branch_refs(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(target_oid);
        let todo = graph.to_todo();
        if let Err(e) =
            weave::run_rebase_expecting_edit(workdir, Some(&graph.base_oid.to_string()), &todo)
        {
            let _ = git::restore_staged_patch(workdir, &saved_staged);
            return Err(e);
        }

        if let Err(e) = apply_and_amend(workdir, &selections, &selected_patch, &gitlinks, true) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                let _ = git::restore_staged_patch(workdir, &saved_staged);
            }));
        }

        new_hash = git::rev_parse(workdir, "HEAD")?;
        if let Err(e) = git::continue_rebase_expecting_edit(workdir) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            }));
        }

        if !selected_patch.is_empty()
            && let Err(e) = git::apply_patch_to_worktree(workdir, &selected_patch)
        {
            rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            return Err(e)
                .context("Failed to apply changes to working directory, operation rolled back");
        }
    }

    git::restore_staged_patch(workdir, &saved_staged)?;

    let mut staged: Vec<String> = gitlinks
        .iter()
        .filter(|g| g.removed && keep_submodule_removal(workdir, &g.path))
        .map(|g| g.path.clone())
        .collect();
    staged.sort();

    let mut message = format!(
        "Uncommitted hunk(s) from `{}` (now `{}`) to working directory",
        git::short_hash(commit_hash),
        git::short_hash(&new_hash)
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
fn run_staged(repo: &Repository, target_arg: &str) -> Result<()> {
    let resolved = repo::resolve_arg(repo, target_arg, &[TargetKind::Commit])?;
    let commit_hash = match resolved {
        Target::Commit(hash) => hash,
        _ => unreachable!(),
    };

    let staged = repo::get_staged_files(repo)?;
    if staged.is_empty() {
        bail!("Nothing to commit");
    }
    fold_files_into_commit(repo, &staged, &commit_hash, true)
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

    // Unstage pre-existing staged files outside the target list, so they do
    // not end up in this commit/amend.
    let saved_staged = staging::save_and_unstage_other_staged(repo, workdir, &file_refs)?;

    let new_hash;

    if is_head {
        if !skip_staging {
            git::stage_files(workdir, &file_refs)?;
        }
        if let Err(e) = git::commit_amend_no_edit(workdir) {
            if !skip_staging {
                let _ = git::unstage_files(workdir, &file_refs);
            }
            let _ = git::restore_staged_patch(workdir, &saved_staged);
            return Err(e);
        }
        git::restore_staged_patch(workdir, &saved_staged)?;
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
        if let Err(e) = git::commit(workdir, &message) {
            if !skip_staging {
                let _ = git::unstage_files(workdir, &file_refs);
            }
            let _ = git::restore_staged_patch(workdir, &saved_staged);
            return Err(e);
        }

        // From here the repository carries a commit the user never asked for,
        // and their other staged files live only in `saved_staged`.
        let git_dir = repo.path().to_path_buf();
        match squash_fixup_into_commit(
            &git_dir,
            workdir,
            target_oid,
            head_oid,
            files,
            &saved_staged,
        ) {
            // The rebase is over, so the finishing steps run outside the
            // rollback: undoing a rewrite that succeeded would leave the
            // integration branch behind its own feature branches.
            Ok(FixupOutcome::Rebased) => {
                transaction::delete(&git_dir)?;
                git::restore_staged_patch(workdir, &saved_staged)?;
                new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
                let _ = git::branch_delete(workdir, TRACK_BRANCH);
            }
            // `loom continue` and `loom abort` own the rest, through the state
            // file the rebase left behind.
            Ok(FixupOutcome::Paused) => return Ok(()),
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
                    let _ = git::restore_staged_patch(workdir, &saved_staged);
                    let _ = git::branch_delete(workdir, TRACK_BRANCH);
                    let _ = transaction::delete(&git_dir);
                }));
            }
        }
    }

    msg::success(&format!(
        "Folded {} file(s) into `{}` (now `{}`)",
        files.len(),
        git::short_hash(commit_hash),
        git::short_hash(&new_hash)
    ));

    Ok(())
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
    git::branch_force_create(workdir, TRACK_BRANCH, &commit_hash)?;
    graph.track_commit(target_oid, TRACK_BRANCH);

    let fold_ctx = serde_json::to_value(FoldVariant::FilesIntoCommit {
        original_commit_hash: commit_hash,
        files_count: files.len(),
        saved_staged: saved_staged.to_string(),
    })?;
    // saved_staged is stored in both rollback (for `loom abort`) and context
    // (for `loom continue` → after_continue). Both paths are required.
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
    };
    transaction::save(git_dir, &loom_state)?;

    let todo = graph.to_todo();
    match weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)? {
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
    git::branch_force_create(workdir, TRACK_BRANCH, target_hash)?;
    graph.track_commit(target_oid, TRACK_BRANCH);

    let git_dir = repo.path().to_path_buf();
    let fold_ctx = serde_json::to_value(FoldVariant::CommitIntoCommit {
        source_hash: source_hash.to_string(),
        target_hash: target_hash.to_string(),
    })?;
    let loom_state = LoomState {
        command: COMMAND.to_string(),
        rollback: Rollback {
            delete_branches: vec![TRACK_BRANCH.to_string()],
            ..Default::default()
        },
        context: fold_ctx,
    };
    transaction::save(&git_dir, &loom_state)?;

    let todo = graph.to_todo();
    match weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)? {
        RebaseOutcome::Completed => {
            transaction::delete(&git_dir)?;
            let new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            msg::success(&format!(
                "Folded `{}` into `{}` (now `{}`)",
                git::short_hash(source_hash),
                git::short_hash(target_hash),
                git::short_hash(&new_hash)
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
            // `git rebase --abort` replays its autostash into the working tree
            // only, so without this an abort hands the staged half back
            // unstaged while reporting the original state restored.
            saved_staged_patch: git::diff_cached(workdir)?,
            ..Default::default()
        },
        context: ctx,
    };
    transaction::save(&git_dir, &state)?;

    let todo = graph.to_todo();
    let outcome = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)
        .map_err(|e| transaction::discard_state_after(workdir, &git_dir, e))?;
    match outcome {
        RebaseOutcome::Completed => {
            transaction::delete(&git_dir)?;
            let new_hash = git::rev_parse(workdir, branch_name)?;
            report_moved(commit_hash, branch_name, &new_hash, &parked);
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
fn report_moved(commit_hash: &str, branch_name: &str, new_hash: &str, parked: &[String]) {
    let mut message = format!(
        "Moved `{}` to branch `{}` (now `{}`)",
        git::short_hash(commit_hash),
        branch_name,
        git::short_hash(new_hash)
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
/// resulting history order should pass them oldest-first. Returns the rebase
/// outcome and the branches the move left empty — callers build and save
/// their own `LoomState`.
pub fn move_commits_to_branch(
    repo: &Repository,
    commit_hashes: &[String],
    branch_name: &str,
) -> Result<(RebaseOutcome, Vec<String>)> {
    let workdir = repo::require_workdir(repo, COMMAND)?;
    let (graph, parked) = plan_move(repo, commit_hashes, branch_name)?;
    let todo = graph.to_todo();
    let outcome = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;
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
    let is_woven =
        graph.has_branch_section(branch_name) || graph.inner_branch_section(branch_name).is_some();
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

/// Write a patch that could not be applied under the git dir, so the user can
/// still get at it. Returns where it landed, if it could be written at all.
///
/// This is the only copy left of that work, so it never writes over an earlier
/// save: each file is created exclusively and the counter climbs until a free
/// name turns up — a guarantee a clock reading cannot give. The git dir is
/// asked for, never assumed: in a linked worktree or submodule `.git` is a
/// file, and a hardcoded `.git/loom` would fail to be created in exactly the
/// case this holds the last copy of the user's work.
fn save_patch_aside(workdir: &Path, name: &str, patch: &str) -> Result<PathBuf> {
    let dir = git::git_path(workdir, "loom")?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create '{}'", dir.display()))?;

    for attempt in 0..1000 {
        let path = dir.join(format!("{name}-{attempt}.patch"));
        match std::fs::File::create_new(&path) {
            Ok(mut file) => {
                return file
                    .write_all(patch.as_bytes())
                    .map(|()| path.clone())
                    .with_context(|| format!("Failed to write '{}'", path.display()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("Failed to create '{}'", path.display()));
            }
        }
    }
    bail!(
        "'{}' already holds 1000 saved {name} patches",
        dir.display()
    )
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
        save_or_warn(workdir, "unrestored", &saved_worktree.worktree, false);
        save_or_warn(workdir, "unrestored-staged", &saved_worktree.staged, true);
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
        save_or_warn(workdir, "unrestored-staged", &saved_worktree.staged, true);
    }
    if !saved_worktree.worktree.is_empty()
        && let Err(e) = git::apply_patch(workdir, &saved_worktree.worktree)
    {
        msg::warn(&format!("could not restore your uncommitted changes: {e}"));
        save_or_warn(workdir, "unrestored", &saved_worktree.worktree, false);
    }
}

/// Park a patch the rollback could not replay, and say where it went and how to
/// replay it by hand — or, if even that fails, that the autostash commit is now
/// the only copy.
///
/// `cached` tells the two halves apart: the staged snapshot is a HEAD → index
/// diff, so replaying it into the working tree instead would apply it twice
/// over.
fn save_or_warn(workdir: &Path, name: &str, patch: &str, cached: bool) {
    if patch.is_empty() {
        return;
    }
    let flag = if cached { " --cached" } else { "" };
    match save_patch_aside(workdir, name, patch) {
        Ok(path) => msg::warn(&format!(
            "those changes are saved as a patch — replay them with `git apply{flag} {}`",
            path.display()
        )),
        Err(e) => msg::warn(&format!(
            "the patch of those changes could not be saved either ({e}) — the autostash \
             commit that `git fsck --lost-found` lists is the last copy"
        )),
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
fn fold_commit_file_to_unstaged(repo: &Repository, commit_hash: &str, path: &str) -> Result<()> {
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
        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, true) {
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
        graph.edit_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase_expecting_edit(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, true) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {}));
        }

        new_hash = git::rev_parse(workdir, "HEAD")?;

        git::continue_rebase_expecting_edit(workdir)?;

        if !gitlink && let Err(e) = git::apply_patch_to_worktree(workdir, &file_diff) {
            rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            return Err(e).context("Failed to uncommit file, operation rolled back");
        }
    }

    let mut staged_removal = false;
    if gitlinks.get(path).copied().unwrap_or(false) {
        staged_removal = keep_submodule_removal(workdir, path);
    }

    msg::success(&format!(
        "Uncommitted `{}` from `{}` (now `{}`) {}",
        path,
        git::short_hash(commit_hash),
        git::short_hash(&new_hash),
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
        graph.edit_commit(source_oid);
        let todo = graph.to_todo();
        git::branch_force_create(workdir, TRACK_BRANCH, target_hash)?;

        if let Err(e) =
            weave::run_rebase_expecting_edit(workdir, Some(&graph.base_oid.to_string()), &todo)
        {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            return Err(e);
        }

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, true) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                let _ = git::branch_delete(workdir, TRACK_BRANCH);
            }));
        }

        // Capture source's new hash before continue moves HEAD;
        // it will be tracked through phase 2 via a temp branch.
        let phase1_source_hash = git::rev_parse(workdir, "HEAD")?;

        if let Err(e) = git::continue_rebase_expecting_edit(workdir) {
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            return Err(e);
        }

        // Phase 2: resolve the target's new OID via the temp branch.
        let phase2_target_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
        let phase2_target_oid = git2::Oid::from_str(&phase2_target_hash)?;

        // Re-open repo after phase 1 rebase (OIDs changed)
        let repo2 = Repository::open(workdir)?;
        let mut graph2 = Weave::from_repo(&repo2)?;
        graph2.edit_commit(phase2_target_oid);

        // Track source through phase 2 — it will be rewritten when the
        // graph is replayed from base_oid.
        let phase1_source_oid = git2::Oid::from_str(&phase1_source_hash)?;
        git::branch_force_create(workdir, TRACK_BRANCH, &phase1_source_hash)?;
        graph2.track_commit(phase1_source_oid, TRACK_BRANCH);

        let todo2 = graph2.to_todo();

        if let Err(e) =
            weave::run_rebase_expecting_edit(workdir, Some(&graph2.base_oid.to_string()), &todo2)
        {
            rollback();
            return Err(e);
        }

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, false) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, rollback));
        }

        new_target_hash = git::rev_parse(workdir, "HEAD")?;

        if let Err(e) = git::continue_rebase_expecting_edit(workdir) {
            rollback();
            return Err(e);
        }

        new_source_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
        let _ = git::branch_delete(workdir, TRACK_BRANCH);
    } else {
        // Source is older than target: single rebase with two edit pauses.
        // Source is picked first (older), target second (newer). Removing
        // the file from source before target is replayed avoids conflicts.
        let saved_head = repo::head_oid(repo)?.to_string();
        let saved_refs = repo::snapshot_branch_refs(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(source_oid);
        graph.edit_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase_expecting_edit(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, true) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {}));
        }

        new_source_hash = git::rev_parse(workdir, "HEAD")?;

        git::continue_rebase_expecting_edit(workdir)?;

        if let Err(e) = apply_and_amend_path(workdir, &file_diff, path, gitlink, false) {
            return Err(git::rebase_abort_then_cleanup(workdir, e, || {
                rollback_fold(workdir, &saved_head, Some(&saved_refs), &saved_worktree);
            }));
        }

        new_target_hash = git::rev_parse(workdir, "HEAD")?;

        git::continue_rebase_expecting_edit(workdir)?;
    }

    msg::success(&format!(
        "Moved `{}` from `{}` (now `{}`) to `{}` (now `{}`)",
        path,
        git::short_hash(source_hash),
        git::short_hash(&new_source_hash),
        git::short_hash(target_hash),
        git::short_hash(&new_target_hash)
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
            rollback: Rollback::default(),
            context: fold_ctx,
        };
        transaction::save(&git_dir, &loom_state)?;

        let todo = graph.to_todo();
        let outcome = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)
            .map_err(|e| transaction::discard_state_after(workdir, &git_dir, e))?;
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
pub fn after_continue(workdir: &Path, context: &serde_json::Value) -> Result<()> {
    let variant: FoldVariant =
        serde_json::from_value(context.clone()).context("Failed to parse fold resume context")?;

    match variant {
        FoldVariant::FilesIntoCommit {
            original_commit_hash,
            files_count,
            saved_staged,
        } => {
            let new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            git::restore_staged_patch(workdir, &saved_staged)?;
            msg::success(&format!(
                "Folded {} file(s) into `{}` (now `{}`)",
                files_count,
                git::short_hash(&original_commit_hash),
                git::short_hash(&new_hash)
            ));
        }
        FoldVariant::CommitIntoCommit {
            source_hash,
            target_hash,
        } => {
            let new_hash = git::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git::branch_delete(workdir, TRACK_BRANCH);
            msg::success(&format!(
                "Folded `{}` into `{}` (now `{}`)",
                git::short_hash(&source_hash),
                git::short_hash(&target_hash),
                git::short_hash(&new_hash)
            ));
        }
        FoldVariant::CommitToBranch {
            commit_hash,
            branch_name,
            parked,
        } => {
            let new_hash = git::rev_parse(workdir, &branch_name)?;
            report_moved(&commit_hash, &branch_name, &new_hash, &parked);
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
                match save_patch_aside(workdir, "unapplied", &diff) {
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
            let staged = keep_submodule_removals(workdir, &commit_hash);
            report_uncommitted(&commit_hash, &emptied, &staged);
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "fold_test.rs"]
mod tests;
