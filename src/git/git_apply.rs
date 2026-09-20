use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::core::msg;
use crate::trace as loom_trace;

/// Apply a patch passed on stdin (`git apply`).
pub fn apply_patch(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &[])
}

/// Apply a patch on stdin in reverse (`git apply --reverse`).
pub fn apply_patch_reverse(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--reverse"])
}

/// Apply a patch to the working tree, falling back to a three-way merge.
///
/// A plain `git apply` fails as soon as a later commit changed a line inside a
/// hunk's context, even though the change itself still fits — the usual case
/// when a commit's own diff is replayed onto a history where that commit is
/// gone but its successors remain. `--3way` merges through the blob ids in the
/// diff header instead, so only a real overlap fails.
///
/// On failure the working tree is put back as it was, best effort: `--3way`
/// writes the files it could merge even when it conflicts on another.
///
/// The patch must carry `index` lines with blob ids (`git diff --full-index`)
/// for the three-way to be possible at all; without them git refuses the
/// fallback and only the plain apply's result stands.
pub fn apply_patch_to_worktree(workdir: &Path, patch: &str) -> Result<()> {
    let Err(plain) = apply_patch(workdir, patch) else {
        return Ok(());
    };

    // What the apply changes has to be undone if it fails, and a patch's own
    // paths do not spell that out — `--numstat` names only a rename's new side.
    // The working tree does: whatever newly differs from the index, the apply
    // wrote. Files the user had already changed cannot be among them — `--3way`
    // refuses a patch that touches one.
    let Ok(before) = super::worktree_dirty_paths(workdir) else {
        return Err(plain);
    };
    // The outer error is the fallback failing to even start, which says more
    // than the plain apply did; the inner one is git refusing the patch.
    if apply_patch_three_way(workdir, patch)?.is_err() {
        let written: Vec<String> = super::worktree_dirty_paths(workdir)
            .unwrap_or_default()
            .difference(&before)
            .cloned()
            .collect();
        // Both attempts are in the trace with their own stderr. The two errors
        // carry the same message, so hand back the one the caller asked for.
        restore_from_index(workdir, &written);
        return Err(plain);
    }
    Ok(())
}

/// Run the three-way apply against a throwaway copy of the index.
///
/// `--3way` implies `--index`, but these changes belong in the working tree
/// alone. A scratch index keeps the real one — staged work, intent-to-add
/// entries, the stat cache — untouched, and leaves no conflicted entries to
/// clean up. The outer `Result` is the setup failing, the inner one is git
/// refusing the patch.
///
/// `rerere` is off because this merge is loom replaying a patch, not a conflict
/// the user ever resolved: recording it, or silently replaying an earlier
/// resolution over it, would both be wrong. `git apply --3way` does consult it.
fn apply_patch_three_way(workdir: &Path, patch: &str) -> Result<Result<()>> {
    with_scratch_index(workdir, "apply", |scratch| {
        run_apply(
            workdir,
            patch,
            &["-c", "rerere.enabled=false"],
            &["--3way"],
            Some(scratch),
        )
    })
}

/// Run `apply` against a throwaway copy of the index, whatever index git would
/// use, and remove the copy afterwards.
///
/// The outer `Result` is the copy failing, the inner one is git refusing the
/// patch.
fn with_scratch_index(
    workdir: &Path,
    name: &str,
    apply: impl FnOnce(&Path) -> Result<()>,
) -> Result<Result<()>> {
    // `--git-path` resolves `GIT_INDEX_FILE` when one is set, so the copy is of
    // the same index the apply would otherwise have written.
    let index = super::git_path(workdir, "index")?;
    // Named per process: two looms in one repo must not share a scratch index.
    // One loom is single-threaded here, so the pid is enough to tell them apart.
    let scratch = index.with_file_name(format!("loom-{name}-index-{}", std::process::id()));
    std::fs::copy(&index, &scratch).with_context(|| {
        format!(
            "Failed to copy '{}' to '{}'",
            index.display(),
            scratch.display()
        )
    })?;

    let _cleanup = ScratchIndex(&scratch);
    Ok(apply(&scratch))
}

/// Removes the scratch index however the closure leaves the stack, a panic
/// inside it included — otherwise it stays in the git dir for good. The lock
/// goes too: git writes `<index>.lock` beside it and only removes it on a run
/// that ends cleanly.
struct ScratchIndex<'a>(&'a Path);

impl Drop for ScratchIndex<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
        let _ = std::fs::remove_file(format!("{}.lock", self.0.display()));
    }
}

/// Put the working-tree files a failed apply had already written back the way
/// they were: from the index for the tracked ones, by deletion for the ones the
/// patch created. Best-effort — the caller is already reporting a failure.
fn restore_from_index(workdir: &Path, paths: &[String]) {
    // Nothing written, nothing to put back. Falling through with an empty list
    // would ask git about every path it knows instead of about none.
    if paths.is_empty() {
        return;
    }
    let refs: Vec<&str> = paths.iter().map(|p| p.as_str()).collect();
    let tracked = super::ls_files(workdir, &refs).unwrap_or_default();
    if !tracked.is_empty() {
        let tracked_refs: Vec<&str> = tracked.iter().map(|p| p.as_str()).collect();
        let _ = super::checkout_index_force(workdir, &tracked_refs);
    }
    for path in paths {
        if !tracked.contains(path) {
            let _ = std::fs::remove_file(workdir.join(path));
        }
    }
}

/// Apply a patch on stdin to the index only, not the working tree
/// (`git apply --cached`).
pub fn apply_cached_patch(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--cached"])
}

/// Reverse-apply a patch from the index only (unstage specific hunks).
///
/// Wraps `git apply --cached --reverse` with the patch passed via stdin.
pub fn apply_cached_patch_reverse(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--cached", "--reverse"])
}

/// Apply a patch to the working tree and the index at once
/// (`git apply --index`).
///
/// Not apply-then-`git add`: `git add` refuses a path an ignore rule matches,
/// even one the patch has just written back, so a file that was committed and
/// later gitignored could not be staged again.
pub fn apply_patch_with_index(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--index"])
}

/// Reverse-apply a patch from the working tree and the index
/// (`git apply --index --reverse`); see [`apply_patch_with_index`].
pub fn apply_patch_with_index_reverse(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--index", "--reverse"])
}

fn apply_patch_with_flags(workdir: &Path, patch: &str, flags: &[&str]) -> Result<()> {
    run_apply(workdir, patch, &[], flags, None)
}

/// Run `git apply` with `flags`, `config` as extra `-c` settings, and the index
/// at `index_file` when one is given.
fn run_apply(
    workdir: &Path,
    patch: &str,
    config: &[&str],
    flags: &[&str],
    index_file: Option<&Path>,
) -> Result<()> {
    let mut args = vec!["apply"];
    args.extend(flags);

    let start = Instant::now();
    let mut command = Command::new("git");
    command
        .current_dir(workdir)
        .args(super::FORCED_CONFIG)
        .args(config)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // No `env_remove` when there is no scratch index: an inherited
    // `GIT_INDEX_FILE` is the index every other git call loom makes reads,
    // `git rev-parse --git-path index` included, so overriding it here alone
    // would apply to a different index than the caller checked.
    if let Some(index) = index_file {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(patch.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The trace has to show what actually ran: the extra `-c` settings and the
    // redirected index change the outcome, and this is the command someone
    // reads back when a three-way went wrong.
    let mut logged: Vec<&str> = config.to_vec();
    logged.extend(&args);
    let index_note = index_file.map(|i| format!("GIT_INDEX_FILE={}", i.display()));
    if let Some(note) = &index_note {
        logged.insert(0, note);
    }
    loom_trace::log_command(
        "git",
        &logged.join(" "),
        duration_ms,
        output.status.success(),
        &stderr,
    );

    if !output.status.success() {
        bail!("git apply failed");
    }

    Ok(())
}

/// Put a saved staged patch back into the index after a rebase (Spec 014).
///
/// `--3way` rather than a plain cached apply, so this needs no reset first and
/// touches nothing it cannot restore. It merges through the blob ids in the
/// patch, which covers both ways a plain apply would refuse the whole patch:
/// a staged *new* file, whose index entry survived the autostash and is already
/// there, and a hunk whose context the rebase rewrote.
///
/// Rehearsed on a copy of the index first, because a three-way *conflict*
/// writes stages 1/2/3 into whatever index it was given and then exits
/// non-zero. Left in the real one those read as `UU` with no merge in progress
/// and — the apply being `--cached` — no markers in the files to resolve.
///
/// An unmerged index is left alone: those stages are a merge the user has to
/// finish, and writing over them would bury it. After a rebase they come from
/// an autostash replay that conflicted, and git keeps that stash behind them;
/// the guard's exits reach this having stashed nothing, so the parked patch is
/// the staged side either way.
///
/// Best-effort otherwise, and it must never fail its caller: the rebase
/// callers run it once their own rewrite has landed, one of which deletes the
/// branch it just wove on `Err`, and [`restore_loom_unstaged`] brings in the
/// guard's exits, where nothing landed and no rebase ever ran.
pub fn restore_staged_after_rebase(workdir: &Path, patch: &str) {
    if patch.is_empty() {
        return;
    }
    if super::has_unmerged_paths(workdir) {
        // Not the stash a rebase would have kept: `git stash pop --index` is
        // refused while the index is unmerged, and the guard's exits never made
        // one. The patch is the staged side, so it is handed over instead.
        msg::warn(
            "the index has unmerged paths, so your staged changes could not go back \
             — resolve them, then replay the patch below",
        );
        park(workdir, patch);
        return;
    }
    match rehearse_cached_three_way(workdir, patch) {
        Ok(Ok(())) => {
            if let Err(e) = cached_three_way(workdir, patch, None) {
                msg::warn(&format!("could not restore your staged changes: {e}"));
                // The rehearsal said this would land, so something moved the
                // index in between and it may have left stages behind.
                if super::has_unmerged_paths(workdir) {
                    msg::warn("it left unmerged entries in the index — `git reset` clears them");
                }
                park(workdir, patch);
            }
        }
        // The rehearsal kept the conflict out of the real index, which leaves
        // the patch the only copy of the staged side: a clean autostash replay
        // put the *worktree* side back and then dropped the stash. It is all or
        // nothing — one hunk that will not land parks the whole patch.
        Ok(Err(e)) => {
            // Several callers restore before deleting their state file, so a
            // failed delete can bring them back here, and a re-apply is not
            // harmless: git refuses a deletion whose index entry is already
            // gone, and refuses the whole patch with it.
            //
            // Byte equality of two `diff_cached` outputs is the only test here
            // that cannot cost data. It is exact, so a HEAD the rebase rewrote
            // reads as "not applied" and parks a patch that was already back —
            // a false alarm, which is the safe way to be wrong. Asking git
            // instead is not: every cheap probe answers "already applied" for
            // a patch that still has staged work in it. A rehearsal that
            // changes nothing does not mean the patch is in the index, because
            // git validates the whole patch before writing any of it, and a
            // reverse three-way merges clean when the index still holds the
            // preimage. Both drop the patch silently.
            if super::diff_cached(workdir).is_ok_and(|current| current == patch) {
                return;
            }
            // Not "what the rebase wrote": the guard reaches this from exits
            // that never ran one.
            msg::warn(&format!(
                "your staged changes no longer apply over what is in the index now: {e}"
            ));
            park(workdir, patch);
        }
        // The rehearsal never got as far as asking git, so nothing is known
        // about the patch itself.
        Err(e) => {
            msg::warn(&format!(
                "could not test whether your staged changes still apply: {e}"
            ));
            park(workdir, patch);
        }
    }
}

/// Put back a patch loom unstaged itself, wherever the call ended.
///
/// For the owner that cannot see the error — the guard restoring on drop — so
/// a rebase left on disk by a failed abort is recognised from the git dir
/// instead. Restoring into a live rebase's index would only be dropped again
/// by the `loom abort` that follows, so the patch goes to the user. The guard
/// emptied the index itself, so it restores after a refusal from before the
/// rebase started too: no autostash ever held that staged side.
/// [`restore_or_park_after_abort`] is the other way in and filters that case
/// out before here — its patch is autostashed work, which loom never unstaged.
pub fn restore_loom_unstaged(workdir: &Path, patch: &str) {
    // Before the git dir is asked for: a guard over nothing is the common case,
    // and it must neither warn about work that does not exist nor pay for a git
    // call per command.
    if patch.is_empty() {
        return;
    }
    if super::rebase_is_over(workdir) {
        restore_staged_after_rebase(workdir, patch);
    } else {
        msg::warn("the rebase is still on disk, so your staged changes could not be put back");
        park(workdir, patch);
    }
}

/// Put the staged patch back after a call that aborted its own rebase, or park
/// it if that abort failed and left the rebase on disk.
///
/// For the callers with no `LoomState`: nothing else will come back for this
/// patch, so it goes to the user rather than being dropped. A refusal from
/// before the rebase started never autostashed, and needs neither.
pub fn restore_or_park_after_abort(workdir: &Path, patch: &str, err: &anyhow::Error) {
    if super::rebase_never_started(err) {
        return;
    }
    restore_loom_unstaged(workdir, patch);
}

/// Hand the staged patch to the user: a clean autostash replay puts the
/// *worktree* side back and drops the stash, so where the staged side differed
/// this patch is what is left of it.
fn park(workdir: &Path, patch: &str) {
    save_or_warn(workdir, "unrestored-staged", patch, Replay::CachedThreeWay);
}

/// Apply `patch` to an index three-way, `rerere` off for the reason given on
/// [`apply_patch_three_way`].
fn cached_three_way(workdir: &Path, patch: &str, index_file: Option<&Path>) -> Result<()> {
    run_apply(
        workdir,
        patch,
        &["-c", "rerere.enabled=false"],
        &["--cached", "--3way"],
        index_file,
    )
}

/// Try the apply against a throwaway copy of the index, so a conflict leaves
/// its stages there instead of in the real one.
///
/// The copy is thrown away and the apply repeated against the real index rather
/// than renamed over it: only git's own lockfile protocol may replace `index`,
/// and a rename behind its back would race any other git touching the repo. The
/// second run is the same patch over a byte copy of the same index, so it is
/// the same merge.
fn rehearse_cached_three_way(workdir: &Path, patch: &str) -> Result<Result<()>> {
    with_scratch_index(workdir, "restage", |scratch| {
        cached_three_way(workdir, patch, Some(scratch))
    })
}

/// Re-apply a previously saved staged patch, parking it on failure.
///
/// No-ops if `patch` is empty. The primary operation has already succeeded or
/// already failed for its own reason, so this is best-effort — but the patch is
/// handed over rather than dropped: `loom abort` reaches here after a reset that
/// took the working tree, and deletes the state file holding this patch as soon
/// as it reports success.
///
/// Nothing for a caller to `?` on: doing so would report this in place of
/// whatever actually stopped the command.
pub fn restore_staged_patch(workdir: &Path, patch: &str) {
    if !patch.is_empty()
        && let Err(e) = apply_cached_patch(workdir, patch)
    {
        msg::warn(&format!(
            "could not restore pre-existing staged changes: {e}"
        ));
        save_or_warn(workdir, "unrestored-staged", patch, Replay::Cached);
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
pub fn save_patch_aside(workdir: &Path, name: &str, patch: &str) -> Result<PathBuf> {
    let dir = super::git_path(workdir, "loom")?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create '{}'", dir.display()))?;
    // This directory is usually brand new, and its own entry is no more durable
    // than the patch inside it would be, so it is synced through its parent.
    if let Some(parent) = dir.parent() {
        let _ = std::fs::File::open(parent).and_then(|d| d.sync_all());
    }

    for attempt in 0..1000 {
        let path = dir.join(format!("{name}-{attempt}.patch"));
        match std::fs::File::create_new(&path) {
            Ok(mut file) => {
                // Sync the file, then the directory naming it: a freshly
                // created entry is not durable until its directory is. Both
                // are best effort — Windows refuses to open a directory.
                let written = file
                    .write_all(patch.as_bytes())
                    .and_then(|()| file.sync_all())
                    .with_context(|| format!("Failed to save '{}'", path.display()));
                if let Err(e) = written {
                    // A corpse holding a name helps nobody, and the counter
                    // below would step over that slot for good.
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                let _ = std::fs::File::open(&dir).and_then(|d| d.sync_all());
                return Ok(path);
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

/// How to replay a parked patch by hand.
#[derive(Clone, Copy, Debug)]
pub enum Replay {
    /// Into the working tree.
    Worktree,
    /// Into the index.
    Cached,
    /// Into the index, merging: what is parked after a three-way apply was
    /// refused, where the plain `--cached` recipe would be refused the same way.
    /// Run by hand it can leave stages in the index — loom rehearses to avoid
    /// that, the user cannot, so `git reset` undoes it. The printed command
    /// turns `rerere` off: this patch already conflicted once, which is when a
    /// stale recorded resolution would be substituted into it.
    CachedThreeWay,
}

impl Replay {
    fn command(self, path: &Path) -> String {
        let path = path.display();
        match self {
            Replay::Worktree => format!("git apply {path}"),
            Replay::Cached => format!("git apply --cached {path}"),
            Replay::CachedThreeWay => {
                format!("git -c rerere.enabled=false apply --cached --3way {path}")
            }
        }
    }
}

/// Park a patch that could not be replayed, and say where it went and how to
/// replay it by hand — or, if even that fails, where the last copy still is.
///
/// The last-resort line names no copy at all, because callers do not share one:
/// a rebase leaves an autostash, an amend never made one, `loom abort` has
/// dropped the one it had, and worktree content was never a git object to find.
/// A caller that can name something better calls [`save_patch_aside`] and words
/// its own, as `fold`'s uncommit does.
///
/// [`Replay`] tells the halves apart: the staged snapshot is a HEAD → index
/// diff, so replaying it into the working tree instead would apply it twice
/// over.
pub fn save_or_warn(workdir: &Path, name: &str, patch: &str, replay: Replay) {
    if patch.is_empty() {
        return;
    }
    match save_patch_aside(workdir, name, patch) {
        Ok(path) => msg::warn(&format!(
            "those changes are saved as a patch — replay them with `{}`",
            replay.command(&path)
        )),
        Err(e) => msg::warn(&format!(
            "the patch of those changes could not be saved either ({e})"
        )),
    }
}

#[cfg(test)]
#[path = "git_apply_test.rs"]
mod tests;
