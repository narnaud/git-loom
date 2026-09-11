pub mod git_apply;
pub mod git_branch;
pub mod git_commit;
pub mod git_diff;
pub mod git_merge;
pub mod git_rebase;
pub mod git_worktree;

pub use git_apply::{
    apply_cached_patch, apply_cached_patch_reverse, apply_patch, apply_patch_reverse,
    apply_patch_to_worktree, restore_staged_patch,
};
pub use git_branch::{
    branch_create, branch_delete, branch_force_create, branch_rename, branch_switch,
    branch_switch_create_tracking, branch_switch_detach, branch_validate_name,
};
pub use git_commit::{
    commit, commit_amend, commit_amend_no_edit, commit_with_editor, reset_hard, reset_mixed,
    reset_soft, stage_all, stage_files, stage_path,
};
pub use git_diff::{
    diff_cached, diff_cached_file, diff_cached_file_is_binary, diff_cached_files, diff_commit,
    diff_commit_file, diff_commit_file_is_binary, diff_commit_name_status, diff_file,
    diff_file_is_binary, diff_head, diff_head_display, diff_head_file, diff_head_file_display,
    diff_head_file_is_binary, diff_head_files, diff_head_name_only, diff_range, show_commit_file,
    show_commit_patch,
};
pub use git_merge::{MergeOutcome, continue_merge, merge_abort, merge_is_in_progress, merge_no_ff};
#[cfg(test)]
pub use git_rebase::rebase_onto;
pub use git_rebase::{
    RebaseOutcome, abort_after_failure, auto_merge_id, continue_rebase,
    continue_rebase_expecting_edit, has_unmerged_paths, rebase, rebase_abort,
    rebase_abort_then_cleanup, rebase_is_in_progress, rebase_outcome, rebase_progress,
};
pub use git_worktree::ensure_not_checked_out_elsewhere;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::trace as loom_trace;

/// Config forced on every git command loom drives itself and reads back or
/// feeds patches to: [`run_git`] and friends, `git apply`, and the rebases.
///
/// A user's gitconfig shapes git's behavior and output, and loom parses that
/// output, replays it as patches, and hands git todo lists to execute. Each key
/// here is one a real gitconfig sets and loom cannot let vary. Left alone on
/// purpose: `run_git_interactive` (what the user reads is theirs to configure),
/// `git push` and `git check-ref-format` (no output loom parses).
///
/// - `commit.verbose`: git appends the diff to `COMMIT_EDITMSG`, and no editor
///   opens here to strip it back out, so a `commit-msg` hook reads the whole
///   diff as if it were the message.
/// - the four `diff.*` prefix keys: they drop or rename the `a/`…`b/` prefixes,
///   and `git apply` can no longer strip a leading path component from a patch
///   loom saved.
/// - `apply.whitespace=error` makes `git apply` reject a saved patch that adds
///   trailing whitespace; `apply.ignoreWhitespace=change` applies it somewhere
///   else.
/// - `rebase.missingCommitsCheck`: loom builds todo lists that leave commits
///   out on purpose (`drop`, moving a commit to another branch); git refuses
///   its own todo under `error` and complains under `warn`.
///
/// Color is not handled here: `color.ui` is only the default for `color.diff`
/// and friends, and an explicit `color.diff=always` beats it. The diff helpers
/// pass `--no-color` instead (see `git_diff.rs`).
pub const FORCED_CONFIG: &[&str] = &[
    "-c",
    "commit.verbose=false",
    "-c",
    "diff.noprefix=false",
    "-c",
    "diff.mnemonicPrefix=false",
    "-c",
    "diff.srcPrefix=a/",
    "-c",
    "diff.dstPrefix=b/",
    "-c",
    "apply.whitespace=nowarn",
    "-c",
    "apply.ignoreWhitespace=no",
    "-c",
    "rebase.missingCommitsCheck=ignore",
];

/// The usual reason an abort fails, wherever that is reported — rebase or merge.
pub const ABORT_FAILED_CAUSE: &str =
    "a stale `.git/index.lock` or a concurrent git process is the usual cause";

/// Minimum Git version required (--update-refs was added in 2.38).
const MIN_GIT_VERSION: (u32, u32) = (2, 38);

/// Absolute path of the git dir for `workdir`.
///
/// Always asks git: a linked worktree has a `.git` file pointing into the main
/// repository rather than a directory, and `GIT_DIR` in the environment
/// overrides both. Guessing `workdir/.git` would disagree with git in either
/// case, and loom does run under a git-set environment as the sequence editor.
pub fn absolute_git_dir(workdir: &Path) -> Result<PathBuf> {
    let out = run_git_stdout(workdir, &["rev-parse", "--absolute-git-dir"])?;
    Ok(PathBuf::from(out.trim()))
}

/// Run a git command, capture output, trace-log it, and bail on failure.
///
/// Output is piped, so an editor could never work here: `GIT_EDITOR=true`
/// keeps commands like `merge --continue` (which has no `--no-edit`) from
/// opening one and hanging. `GIT_SEQUENCE_EDITOR` falls back to it, so a
/// captured `rebase -i` must set its own sequence editor (`weave` runs its
/// own `Command` and sets both).
fn run_git_captured(workdir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let start = Instant::now();
    let output = Command::new("git")
        .current_dir(workdir)
        .args(FORCED_CONFIG)
        .args(args)
        .env("GIT_EDITOR", "true")
        .output()?;

    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let cmd = args.join(" ");
    loom_trace::log_command("git", &cmd, duration_ms, output.status.success(), &stderr);

    if !output.status.success() {
        bail!("git {} failed", args[0]);
    }

    Ok(output)
}

/// Run a git command in the given working directory.
/// On failure, returns an error with the command name; stderr is recorded
/// in the trace log via `loom_trace::log_command`.
pub fn run_git(workdir: &Path, args: &[&str]) -> Result<()> {
    run_git_captured(workdir, args).map(|_| ())
}

/// Run a git command and return its stdout as a string.
/// On failure, returns an error with the command name; stderr is recorded
/// in the trace log via `loom_trace::log_command`.
pub fn run_git_stdout(workdir: &Path, args: &[&str]) -> Result<String> {
    let output = run_git_captured(workdir, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Run a git command and return its combined stdout+stderr, trimmed.
/// Useful for commands (like `fetch`) that print their summary to stderr, so a
/// caller can show git's output after a spinner instead of streaming it live.
/// On failure, returns an error with the command name; stderr is still traced.
pub fn run_git_combined(workdir: &Path, args: &[&str]) -> Result<String> {
    let output = run_git_captured(workdir, args)?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(combined.trim().to_string())
}

/// Check that the installed Git version meets the minimum requirement.
/// Returns an error with an actionable message if the version is too old.
pub fn check_git_version() -> Result<()> {
    let output = Command::new("git").arg("--version").output()?;
    let version_str = String::from_utf8_lossy(&output.stdout);

    // Parse "git version X.Y.Z..." → (X, Y)
    let (major, minor) = parse_git_version(&version_str)
        .with_context(|| format!("Could not parse Git version from: {}", version_str.trim()))?;

    if (major, minor) < MIN_GIT_VERSION {
        bail!(
            "Git {}.{} is too old, git-loom requires Git {}.{} or later (for --update-refs)\n\
             Current version: {}",
            major,
            minor,
            MIN_GIT_VERSION.0,
            MIN_GIT_VERSION.1,
            version_str.trim()
        );
    }

    Ok(())
}

/// Parse "git version X.Y.Z..." into (major, minor).
fn parse_git_version(version_str: &str) -> Option<(u32, u32)> {
    let version_part = version_str.trim().strip_prefix("git version ")?;
    let mut parts = version_part.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Run a git command with inherited stdio (for interactive commands / pager).
/// On failure, returns an error containing the command that failed.
///
/// Note: stderr is not captured (it flows to the terminal directly),
/// so the trace log will record an empty stderr string for these calls.
pub fn run_git_interactive(workdir: &Path, args: &[&str]) -> Result<()> {
    // A pty-hosted agent must never hang inside `less` — disable the pager.
    let mut full_args: Vec<&str> = Vec::new();
    if crate::core::agent_mode::enabled() {
        full_args.extend(["-c", "core.pager=cat"]);
    }
    full_args.extend(args);

    let start = Instant::now();
    let status = Command::new("git")
        .current_dir(workdir)
        .args(&full_args)
        .status()?;

    let duration_ms = start.elapsed().as_millis();
    let cmd = args.join(" ");
    loom_trace::log_command("git", &cmd, duration_ms, status.success(), "");

    if !status.success() {
        bail!("git {} failed", args[0]);
    }

    Ok(())
}

/// Unstage specific files (remove from index without touching the working tree).
///
/// Wraps `git reset HEAD -- <files>`.
pub fn unstage_files(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["reset", "HEAD", "--"];
    args.extend(files);
    run_git(workdir, &args)
}

/// Restore tracked files in the working tree to their HEAD state.
///
/// Wraps `git checkout HEAD -- <files>`.
pub fn restore_files_to_head(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["checkout", "HEAD", "--"];
    args.extend(files);
    run_git(workdir, &args)
}

/// Restore tracked files in the working tree to their index state.
///
/// Wraps `git checkout-index -f --`.
pub fn checkout_index_force(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["checkout-index", "-f", "--"];
    args.extend(files);
    run_git(workdir, &args)
}

/// The subset of `files` that the index knows about; empty for an empty list.
///
/// Wraps `git ls-files -z -- <files>`. The paths go in as `:(literal)`
/// pathspecs: a real file named `a[12].txt` would otherwise match `a1.txt` as
/// a glob, and the caller would act on a file it never asked about. `-z`
/// because the default `core.quotePath` would otherwise escape and quote a
/// non-ASCII path into something no other git command takes.
pub fn ls_files(workdir: &Path, files: &[&str]) -> Result<Vec<String>> {
    // No pathspec means "every path in the index" to git, which is never what a
    // caller asking about a list of files wants when that list came up empty.
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let literal: Vec<String> = files.iter().map(|f| format!(":(literal){f}")).collect();
    let mut args = vec!["ls-files", "-z", "--"];
    args.extend(literal.iter().map(|f| f.as_str()));
    Ok(run_git_stdout(workdir, &args)?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect())
}

/// Every path whose working-tree content differs from the index, untracked
/// files included — but not ignored ones.
///
/// Wraps `git status --porcelain -z -uall`, keeping the entries whose
/// worktree column is set. A file that is only *staged* is deliberately left
/// out: its working-tree content still matches the index, so a caller
/// comparing two of these sets sees it the moment something writes to it.
/// `-uall` lists the files inside an untracked directory instead of collapsing
/// them into the directory itself, and `-z` stops the default `core.quotePath`
/// from escaping and quoting a non-ASCII path.
pub fn worktree_dirty_paths(workdir: &Path) -> Result<std::collections::HashSet<String>> {
    let out = run_git_stdout(workdir, &["status", "--porcelain", "-z", "-uall"])?;
    let mut paths = std::collections::HashSet::new();
    // A rename entry is `XY <new>\0<original>\0`: the second path stands alone,
    // and names a file the rename left behind, so it is never worktree-dirty.
    let mut rename_source_next = false;
    for entry in out.split('\0').filter(|e| !e.is_empty()) {
        if rename_source_next {
            rename_source_next = false;
            continue;
        }
        let Some((status, path)) = entry.split_at_checked(3) else {
            continue;
        };
        // Columns are `<index><worktree> `; a rename is recorded in either, and
        // its second path follows whether or not this entry is one we keep.
        rename_source_next = status.starts_with(['R', 'C']) || status[1..].starts_with(['R', 'C']);
        if !status[1..].starts_with(' ') {
            paths.insert(path.to_string());
        }
    }
    Ok(paths)
}

/// Resolve a path inside the git dir, e.g. `index`.
///
/// Wraps `git rev-parse --git-path <name>`, which knows where a linked
/// worktree's own git dir is and, for `index`, honors `GIT_INDEX_FILE`.
pub fn git_path(workdir: &Path, name: &str) -> Result<PathBuf> {
    let out = run_git_stdout(workdir, &["rev-parse", "--git-path", name])?;
    // Only the trailing newline: a git dir path may legitimately begin or end
    // with a space, and git never pads its own output.
    Ok(workdir.join(out.trim_end_matches('\n')))
}

/// The branch HEAD points at, without the `refs/heads/` prefix.
///
/// Errors when HEAD is detached.
pub fn current_branch(workdir: &Path) -> Result<String> {
    Ok(
        run_git_stdout(workdir, &["symbolic-ref", "--quiet", "--short", "HEAD"])?
            .trim()
            .to_string(),
    )
}

/// Resolve a git ref to its full commit hash.
///
/// Wraps `git rev-parse <ref>` and trims the output.
pub fn rev_parse(workdir: &Path, reference: &str) -> Result<String> {
    let out = run_git_stdout(workdir, &["rev-parse", reference])?;
    Ok(out.trim().to_string())
}

/// Truncate a full commit hash to a short display form (7 chars).
pub fn short_hash(hash: &str) -> &str {
    &hash[..7.min(hash.len())]
}

/// Resolve the path to the git-loom binary.
///
/// During `cargo test`, `current_exe()` returns the test harness binary in
/// `target/<profile>/deps/`, while the binary itself sits one level up in
/// `target/<profile>/` — put there by cargo because `tests/bin_is_built.rs`
/// makes the package's binaries part of the test build.
pub fn loom_exe_path() -> Result<PathBuf> {
    resolve_loom_exe(&std::env::current_exe()?)
}

/// Resolve `exe` to the real git-loom binary; see [`loom_exe_path`].
///
/// Erroring beats handing back the harness: the caller gives this to git as the
/// rebase sequence editor, and a harness rejects `--source` with
/// `Unrecognized option` and exit 101, so git aborts with "there was a problem
/// with the editor" and the caller reports nothing but `git rebase failed`.
fn resolve_loom_exe(exe: &Path) -> Result<PathBuf> {
    // A `deps` directory only means a harness under `cargo test`. An installed
    // binary that happens to sit in one is just a binary, and telling its user
    // to run `cargo build` would be nonsense.
    if !cfg!(test) {
        return Ok(exe.to_path_buf());
    }
    let Some(parent) = exe.parent() else {
        return Ok(exe.to_path_buf());
    };
    if parent.file_name().and_then(|n| n.to_str()) != Some("deps") {
        return Ok(exe.to_path_buf());
    }
    let Some(profile_dir) = parent.parent() else {
        return Ok(exe.to_path_buf());
    };

    let actual = profile_dir.join(format!("git-loom{}", std::env::consts::EXE_SUFFIX));
    if !actual.exists() {
        bail!(
            "'{}' does not exist — run `cargo build` before `cargo test`",
            actual.display()
        );
    }
    Ok(actual)
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
