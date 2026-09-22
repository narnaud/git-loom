use std::path::Path;

use anyhow::Result;

/// Outcome of a rebase operation.
#[derive(Debug, PartialEq, Eq)]
pub enum RebaseOutcome {
    /// The rebase finished and git left no state behind.
    Completed,
    /// git exited successfully but the rebase is still in progress: it stopped
    /// at an `edit` or `break` step.
    Paused,
    /// The rebase stopped part-way and git left its state on disk. A conflict
    /// is the usual reason, but not the only one — see [`abort_after_failure`].
    Stopped,
}

/// Continue an in-progress rebase.
///
/// Returns `Completed` if the rebase finished, `Paused` if it advanced to the
/// next `edit`/`break` step, or `Stopped` if it stopped again. A failure that
/// leaves no rebase in progress is an error. Does NOT abort — the caller is
/// responsible.
pub fn continue_rebase(workdir: &Path) -> Result<RebaseOutcome> {
    // Resolve this before continuing: failing afterwards would report an error
    // for a rebase that already moved on, and the caller would keep its state
    // file for a step that is done.
    let git_dir = super::absolute_git_dir(workdir)?;

    rebase_outcome(&git_dir, super::run_git(workdir, &["rebase", "--continue"]))
}

/// Classify how a `git rebase …` command ended, from whether it succeeded and
/// whether git left its state behind (`rebase-merge/` or `rebase-apply/`).
///
/// Exit 0 does not mean the rebase is over: git also exits 0 when it stops at
/// an `edit` or `break` step (`Paused`). A failure that left state behind is
/// `Stopped`, whatever stopped it; any other failure (bad args, missing ref)
/// is returned as `Err`.
pub fn rebase_outcome(git_dir: &Path, result: Result<()>) -> Result<RebaseOutcome> {
    let in_progress = rebase_is_in_progress(git_dir);
    match result {
        Ok(()) if in_progress => Ok(RebaseOutcome::Paused),
        Ok(()) => Ok(RebaseOutcome::Completed),
        Err(_) if in_progress => Ok(RebaseOutcome::Stopped),
        Err(e) => Err(e),
    }
}

/// Run a plain `git rebase` onto `upstream`; see [`rebase_outcome`] for the
/// result.
pub fn rebase(git_dir: &Path, workdir: &Path, upstream: &str) -> Result<RebaseOutcome> {
    rebase_outcome(
        git_dir,
        super::run_git(
            workdir,
            &[
                "rebase",
                "--autostash",
                "--update-refs",
                "--rebase-merges",
                upstream,
            ],
        ),
    )
}

/// Rebase commits between `upstream` and HEAD onto `newbase`
/// (`git rebase --onto <newbase> <upstream> --update-refs`), keeping branch
/// refs in the range up to date.
#[cfg(test)]
pub fn rebase_onto(workdir: &Path, newbase: &str, upstream: &str) -> Result<()> {
    super::run_git(
        workdir,
        &[
            "rebase",
            "--onto",
            newbase,
            upstream,
            "--autostash",
            "--update-refs",
        ],
    )
}

/// What a rebase replay preserves: the author and the message, never the hash.
///
/// Not unique — a repeated `wip` by one author in the same second collides —
/// so it backstops [`skip_empty_stops`] rather than carrying the guarantee.
fn replay_identity(workdir: &Path, rev: &str) -> Result<String> {
    super::run_git_stdout(
        workdir,
        &["show", "-s", "--format=%an%x00%ae%x00%at%x00%B", rev],
    )
}

/// Verify a paused rebase stopped on the replay of `expect_stop`, aborting the
/// rebase if it did not (Spec 004).
///
/// A backstop behind [`skip_empty_stops`], for a stop landing elsewhere for any
/// other reason, before the caller amends or resets whatever HEAD happens to be.
pub fn verify_paused_at(workdir: &Path, expect_stop: &str) -> Result<()> {
    match stopped_on_the_replay(workdir, expect_stop) {
        Ok(true) => Ok(()),
        // A git that cannot answer is no more a license to rewrite than a
        // mismatch is, so both exits abort the rebase they leave running.
        Ok(false) => {
            // Built first: the abort below moves HEAD back, and this names the
            // commit the rebase stopped on.
            let cause = mismatch_error(workdir, expect_stop);
            Err(rebase_abort_then_cleanup(workdir, cause, || {}))
        }
        Err(e) => Err(rebase_abort_then_cleanup(workdir, e, || {})),
    }
}

/// Whether the paused rebase stopped on `expect_stop`, by the name git itself
/// recorded for the stop.
///
/// `stopped-sha` holds the original commit, which is exactly what `expect_stop`
/// names, so it tells two commits apart where identity cannot: a commit already
/// cherry-picked upstream shares its author, date and message with its
/// duplicate. Identity remains the fallback for a pause git recorded no
/// `stopped-sha` for.
fn stopped_on_the_replay(workdir: &Path, expect_stop: &str) -> Result<bool> {
    if let Some(sha) = stopped_sha(&super::absolute_git_dir(workdir)?) {
        return Ok(shas_match(&sha, expect_stop));
    }
    Ok(replay_identity(workdir, "HEAD")? == replay_identity(workdir, expect_stop)?)
}

/// Names the commit the rebase stopped on by subject: its hash is a replay that
/// the abort is about to make unreachable, so it would tell the user nothing.
fn mismatch_error(workdir: &Path, expect_stop: &str) -> anyhow::Error {
    let subject = super::run_git_stdout(workdir, &["show", "-s", "--format=%s", "HEAD"])
        .unwrap_or_default()
        .trim()
        .to_string();
    let stopped_on = match subject.as_str() {
        "" => "another commit".to_string(),
        subject => format!("`{subject}`"),
    };
    anyhow::anyhow!(
        "Commit `{}` was not replayed — the rebase stopped on {stopped_on} instead\n\
         Nothing was rewritten",
        super::short_hash(expect_stop)
    )
}

/// Whether the worktree or index carries changes that `git rebase --skip`, a
/// hard reset, would throw away. Untracked files survive a skip.
///
/// `--ignore-submodules=dirty` because a submodule's own worktree is not the
/// superproject's to lose: autostash never stashes it, so counting it would
/// leave this permanently true and deadlock every empty stop. A gitlink the
/// index does move is still reported — that one a reset would discard.
///
/// A git that cannot answer counts as changed: not knowing is never a license
/// to reset.
fn has_local_changes(workdir: &Path) -> bool {
    super::run_git_stdout(
        workdir,
        &[
            "status",
            "--porcelain",
            "--untracked-files=no",
            "--ignore-submodules=dirty",
        ],
    )
    .map_or(true, |out| !out.trim().is_empty())
}

/// Whether two commit SHAs name the same commit, either one abbreviated.
///
/// The abbreviated side is always a todo's own hash, which `Weave::to_todo`
/// and `build_and_run_linear_edit` both take from `short_id()` — the shortest
/// prefix unambiguous in the repository — so a shared prefix is the same
/// commit. (`git::short_hash`'s fixed 7 is for display and never reaches here.)
/// Sliced as bytes, which cannot land mid-character.
fn shas_match(a: &str, b: &str) -> bool {
    let shortest = a.len().min(b.len());
    shortest > 0 && a.as_bytes()[..shortest] == b.as_bytes()[..shortest]
}

/// The commit `--empty=stop` stopped on, as git recorded it.
fn stopped_sha(git_dir: &Path) -> Option<String> {
    let sha = std::fs::read_to_string(git_dir.join("rebase-merge").join("stopped-sha")).ok()?;
    let sha = sha.trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// Whether replaying `sha` on top of HEAD would produce nothing.
///
/// Asks the same question the sequencer did — merge the commit's own diff into
/// HEAD and see whether the tree moves — because the answer decides whether
/// `git rebase --skip`, a hard reset, runs. Comparing the files the commit
/// touches is not the same question: upstream may have taken the change plus
/// more of the same file.
///
/// Anything git cannot answer — a root commit with no `^`, a conflicting
/// merge, a git that failed to run — says no.
fn replays_empty(workdir: &Path, sha: &str) -> bool {
    let Ok(head_tree) = super::run_git_stdout(workdir, &["rev-parse", "HEAD^{tree}"]) else {
        return false;
    };
    let Ok(merged) = super::run_git_stdout(
        workdir,
        &[
            "merge-tree",
            "--write-tree",
            "--merge-base",
            &format!("{sha}^"),
            "HEAD",
            sha,
        ],
    ) else {
        return false;
    };

    merged.lines().next().map(str::trim) == Some(head_tree.trim())
}

/// Commits a replay must not drop as empty (Spec 004), split by whose they are:
/// only a commit the user named is theirs to `loom drop`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Protected<'a> {
    /// Every other protected commit: the ones loom rewrites or moves.
    pub named: &'a [String],
    /// Commits the operation lands on, such as a fold or absorb target:
    /// dropping one leaves it nothing to land on.
    pub targets: &'a [String],
}

impl<'a> Protected<'a> {
    pub fn named(named: &'a [String]) -> Self {
        Self {
            named,
            targets: &[],
        }
    }

    pub fn targeting(self, targets: &'a [String]) -> Self {
        Self { targets, ..self }
    }
}

/// Carry a rebase past every stop loom can account for on its own: a commit the
/// new history already has, and a conflict `rerere` had already resolved.
///
/// The two uncover each other — a skip can land on a replayed resolution and
/// the other way round — so they alternate until neither moves.
/// `before` identifies the stop the caller was already on, if there was one
/// (Spec 014).
pub fn carry_past_known_stops(
    workdir: &Path,
    git_dir: &Path,
    protected: Protected<'_>,
    before: Option<&StopId>,
    outcome: RebaseOutcome,
) -> Result<RebaseOutcome> {
    let mut carried = carried_set(before);
    let mut outcome = outcome;
    loop {
        outcome = skip_empty_stops(workdir, git_dir, protected, outcome)?;
        let (next, resolved) = rerere_continue_loop(workdir, git_dir, &mut carried, outcome)?;
        outcome = next;
        if resolved == 0 {
            return Ok(outcome);
        }
    }
}

/// Carry a rebase past every stop `rerere` already resolved, for a caller with
/// no empty stops to skip — its todo ran under `--empty=drop`.
pub fn continue_rerere_stops(
    workdir: &Path,
    git_dir: &Path,
    before: Option<&StopId>,
    outcome: RebaseOutcome,
) -> Result<RebaseOutcome> {
    let mut carried = carried_set(before);
    Ok(rerere_continue_loop(workdir, git_dir, &mut carried, outcome)?.0)
}

/// Which conflict stop a rebase is on: the `AUTO_MERGE` id git wrote for it and
/// the step that produced it.
///
/// The id alone does not name a stop — two steps merging the same commit into
/// the same tree write the same `AUTO_MERGE` — and neither does the step
/// number, which a step that fails to commit keeps.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct StopId {
    auto_merge: String,
    step: Option<usize>,
}

impl StopId {
    /// The `AUTO_MERGE` id alone, for callers that only ask whether git moved
    /// on to another conflict.
    pub fn auto_merge(&self) -> &str {
        &self.auto_merge
    }
}

/// The stop git is on, `None` when it is on none: a stop without an
/// `AUTO_MERGE` did not come from a conflict (Spec 014).
pub fn stop_id(workdir: &Path, git_dir: &Path) -> Option<StopId> {
    Some(StopId {
        auto_merge: auto_merge_id(workdir)?,
        step: rebase_progress(git_dir).map(|(current, _)| current),
    })
}

/// The stops a rerere continue must not count as news, seeded with the one the
/// caller was already on.
fn carried_set(before: Option<&StopId>) -> std::collections::HashSet<StopId> {
    before.cloned().into_iter().collect()
}

/// Continue past every stop `rerere` resolved, reporting each one; returns the
/// outcome and how many stops it carried past.
///
/// A stop with an `AUTO_MERGE` came from a conflict (Spec 014), and with
/// `rerere.autoUpdate` one left with nothing unmerged is a resolution `rerere`
/// replayed and staged, so loom takes it and carries on. Without that setting
/// the user asked to review each replay, so the stop is theirs. `carried`
/// holds the stops already continued past, so a step that fails for another
/// reason — a hook turning the commit down — stays on the same stop and is
/// handed back rather than retried for ever.
fn rerere_continue_loop(
    workdir: &Path,
    git_dir: &Path,
    carried: &mut std::collections::HashSet<StopId>,
    mut outcome: RebaseOutcome,
) -> Result<(RebaseOutcome, usize)> {
    let mut resolved = 0;
    while outcome == RebaseOutcome::Stopped {
        let Some(id) = stop_id(workdir, git_dir) else {
            break;
        };
        if !carried.insert(id.clone()) {
            break;
        }
        if has_unmerged_paths(workdir) || !rerere_auto_updates(workdir) || resolves_to_head(workdir)
        {
            break;
        }
        // Read before the continue, which is about to leave that stop behind.
        let on =
            stopped_sha(git_dir).map(|sha| format!(" replaying `{}`", super::short_hash(&sha)));
        outcome = match continue_rebase(workdir) {
            Ok(next) => next,
            Err(e) => return Err(rebase_abort_then_cleanup(workdir, e, || {})),
        };
        // Still on that stop: the commit was turned down, so nothing was carried.
        if outcome == RebaseOutcome::Stopped && stop_id(workdir, git_dir).as_ref() == Some(&id) {
            break;
        }
        resolved += 1;
        crate::core::msg::warn(&format!(
            "`rerere` resolved the conflicts{} — carried on with its recorded resolution",
            on.unwrap_or_default()
        ));
    }
    Ok((outcome, resolved))
}

/// Whether the staged resolution leaves HEAD's tree as it is: git would then
/// drop the commit on `--continue`, even under `--empty=stop`, so a protected
/// commit (Spec 004) could vanish unseen. Anything git cannot answer says yes.
fn resolves_to_head(workdir: &Path) -> bool {
    match (
        super::write_tree(workdir),
        super::rev_parse(workdir, "HEAD^{tree}"),
    ) {
        (Ok(index), Ok(head)) => index == head,
        _ => true,
    }
}

/// Whether `rerere.autoUpdate` is on: then git stages each resolution it
/// replays, and the user has opted out of reviewing one before it is taken.
fn rerere_auto_updates(workdir: &Path) -> bool {
    super::run_git_stdout(workdir, &["config", "--bool", "--get", "rerere.autoUpdate"])
        .is_ok_and(|value| value.trim() == "true")
}

/// Carry a rebase past every commit whose changes the new history already has,
/// refusing when one of them is in `protected` (Spec 004).
///
/// A skip is a hard reset, so it runs only where the repository itself says the
/// commit would add nothing, and never over a tree carrying local changes. A
/// stop this cannot account for — a conflict, a stale `index.lock`, a
/// resolution that came out empty — is handed back for the caller to report.
pub fn skip_empty_stops(
    workdir: &Path,
    git_dir: &Path,
    protected: Protected<'_>,
    mut outcome: RebaseOutcome,
) -> Result<RebaseOutcome> {
    let mut skipped_already: std::collections::HashSet<String> = std::collections::HashSet::new();

    while outcome == RebaseOutcome::Stopped && !has_unmerged_paths(workdir) {
        let Some(sha) = stopped_sha(git_dir) else {
            return Ok(outcome);
        };
        if !replays_empty(workdir, &sha) {
            return Ok(outcome);
        }

        let short = super::short_hash(&sha).to_string();
        let dirty = has_local_changes(workdir);

        // Classified before the working-tree guard, because whether the history
        // already has this commit's changes does not depend on the tree. The
        // *action* does: an abort is a hard reset, so over work the user did
        // while the rebase was paused it refuses and leaves everything alone.
        let in_list = |list: &[String]| list.iter().any(|hash| shas_match(hash, &sha));
        // Checked first: a fold target the todo marks `edit` is in both lists.
        let is_target = in_list(protected.targets);
        if is_target || in_list(protected.named) {
            let cause = anyhow::Error::new(ReplayedEmpty(sha.clone()));
            if dirty {
                // Not "run `loom abort`": that is the same hard reset, so it
                // would throw away the work this refusal just protected.
                return Err(cause.context(format!(
                    "Commit `{short}` {REPLAYS_EMPTY}\n\
                     Your uncommitted changes are in the way of the undo — commit or \
                     stash them, then run `loom abort`"
                )));
            }
            return Err(rebase_abort_then_cleanup(
                workdir,
                cause.context(format!(
                    "Commit `{short}` {REPLAYS_EMPTY}\n\
                     Nothing was rewritten. Run `loom update` if it landed upstream{}",
                    if is_target {
                        String::new()
                    } else {
                        format!(", or `loom drop {short} -y` to remove it now")
                    }
                )),
                || {},
            ));
        }

        // `--skip` is a hard reset too, so it never runs over a tree with work.
        if dirty {
            return Ok(outcome);
        }
        // A `--skip` that leaves the rebase where it was would loop here.
        if !skipped_already.insert(sha.clone()) {
            return Ok(outcome);
        }

        match rebase_outcome(git_dir, super::run_git(workdir, &["rebase", "--skip"])) {
            Ok(next) => outcome = next,
            Err(e) => return Err(rebase_abort_then_cleanup(workdir, e, || {})),
        }
        crate::core::msg::warn(&format!(
            "Dropped `{short}` — the history below it already has its change"
        ));
    }
    Ok(outcome)
}

/// The first line of the empty-replay refusal, wherever it is reported.
pub const REPLAYS_EMPTY: &str = "is redundant — the history below it already has its change";

/// Marker under the refusal [`skip_empty_stops`] returns, carrying git's
/// `stopped-sha` so a caller whose own undo removes that commit can replace a
/// hint that would then name nothing.
///
/// `main.rs` prints only the outermost message, so the text above stays what
/// the user reads. The sha is whatever git wrote, abbreviated on older
/// versions, so it is only ever handed back to git to resolve.
#[derive(Debug)]
pub(crate) struct ReplayedEmpty(String);

impl std::fmt::Display for ReplayedEmpty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "commit {} replayed empty", self.0)
    }
}

impl std::error::Error for ReplayedEmpty {}

/// The hash of the commit `err` refused over, if it is that refusal.
pub fn replayed_empty_hash(err: &anyhow::Error) -> Option<&str> {
    err.downcast_ref::<ReplayedEmpty>().map(|e| e.0.as_str())
}

/// Marker on an error raised before the rebase could start, so an undo knows
/// nothing was rewritten and leaves the index where the user left it.
#[derive(Debug)]
pub(crate) struct RebaseNotStarted;

impl std::fmt::Display for RebaseNotStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the rebase never started")
    }
}

impl std::error::Error for RebaseNotStarted {}

/// Tag `result`'s error as raised before the rebase started.
pub fn before_rebase_starts<T>(result: Result<T>) -> Result<T> {
    result.map_err(|e| {
        let shown = format!("{e}");
        anyhow::Error::new(RebaseNotStarted)
            .context(e)
            .context(shown)
    })
}

/// Whether `err` was raised before the rebase started, so no undo is owed.
pub fn rebase_never_started(err: &anyhow::Error) -> bool {
    err.downcast_ref::<RebaseNotStarted>().is_some()
}

/// Abort an in-progress rebase.
pub fn rebase_abort(workdir: &Path) -> Result<()> {
    super::run_git(workdir, &["rebase", "--abort"])
}

/// Whether no rebase is left on disk.
///
/// A git dir that cannot be resolved counts as still running, so a caller that
/// cannot tell errs toward leaving the repository alone.
pub fn rebase_is_over(workdir: &Path) -> bool {
    super::absolute_git_dir(workdir).is_ok_and(|git_dir| !rebase_is_in_progress(&git_dir))
}

/// Whether a rebase is in progress: git leaves a `rebase-merge/` or
/// `rebase-apply/` directory under the git dir while one is paused.
pub fn rebase_is_in_progress(git_dir: &Path) -> bool {
    git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
}

/// Step numbers of a paused rebase as `(current, total)`, read from the
/// `msgnum`/`end` files in git's rebase state dir. `None` when no rebase is in
/// progress or the files are unreadable.
pub fn rebase_progress(git_dir: &Path) -> Option<(usize, usize)> {
    let dir = ["rebase-merge", "rebase-apply"]
        .iter()
        .map(|d| git_dir.join(d))
        .find(|d| d.exists())?;
    let read = |name: &str| -> Option<usize> {
        std::fs::read_to_string(dir.join(name))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    Some((read("msgnum")?, read("end")?))
}

/// Roll `cause` back: abort the rebase if one is running, then run `cleanup`.
///
/// The cleanup a caller wants here — deleting a temp branch, resetting refs,
/// restoring a saved patch, dropping the state file — assumes no rebase is
/// under way. So it runs in exactly two cases: the abort succeeded, or there
/// was no rebase to abort (the command failed before starting one). Only a
/// rebase that is still running after a *failed* abort skips it, because
/// resetting refs on top of that would make the mess worse.
///
/// Returns the error to report, which always keeps `cause` visible: the
/// top-level handler prints one message, so a `context` would hide the reason
/// the command failed in the first place.
pub fn rebase_abort_then_cleanup(
    workdir: &Path,
    cause: anyhow::Error,
    cleanup: impl FnOnce(),
) -> anyhow::Error {
    // Erring toward "still running" is the safe side here: a skipped cleanup
    // strands a temp branch, while cleaning up over a live rebase throws work
    // away.
    if rebase_is_over(workdir) {
        cleanup();
        return cause;
    }

    match rebase_abort(workdir) {
        Ok(()) => {
            cleanup();
            cause
        }
        Err(_) => anyhow::anyhow!(
            "{cause}\n\
             The abort failed too, so the repository is left mid-rebase.\n\
             Run `loom abort` once git is free ({})",
            super::ABORT_FAILED_CAUSE
        ),
    }
}

/// Abort a rebase that failed, and build the error to report.
///
/// Two things the caller cannot assume: the rebase may have stopped for a
/// reason other than a conflict (a stale `index.lock`, a concurrent git
/// process), and the abort itself may fail — saying "aborted" then would strand
/// the user in a half-rewritten repository.
pub fn abort_after_failure(workdir: &Path) -> anyhow::Error {
    let conflicted = has_unmerged_paths(workdir) || auto_merge_id(workdir).is_some();
    match rebase_abort(workdir) {
        Ok(()) if conflicted => anyhow::anyhow!("Rebase failed with conflicts — aborted"),
        Ok(()) => anyhow::anyhow!(
            "Rebase stopped before finishing — aborted\n\
             Run `loom trace` to see why"
        ),
        Err(_) => anyhow::anyhow!(
            "Rebase failed, and the abort failed too — the repository is left mid-rebase.\n\
             Run `loom abort` once git is free ({}).",
            super::ABORT_FAILED_CAUSE
        ),
    }
}

/// Whether the index has unmerged entries — i.e. the operation really did stop
/// on a conflict.
pub fn has_unmerged_paths(workdir: &Path) -> bool {
    !unmerged_paths(workdir).is_empty()
}

/// The paths the index holds conflict stages for. Empty when git cannot be
/// asked, so a caller that cannot tell reads it as no conflict, as the check
/// above always has.
///
/// `-z` because the default `core.quotePath` would otherwise escape and quote
/// a non-ASCII path into something no other git command takes.
pub fn unmerged_paths(workdir: &Path) -> Vec<String> {
    super::run_git_stdout(workdir, &["diff", "-z", "--name-only", "--diff-filter=U"])
        .map(|out| {
            out.split('\0')
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The id of `AUTO_MERGE`, the ref git keeps while a conflicted merge is
/// unfinished — a conflicted pick during a rebase included — and drops once the
/// resolution is committed. `Some` therefore means the stop came from a
/// conflict, resolved or not, and the id names *which* conflict, so a caller
/// that read it before continuing can tell a fresh one from the one it was on.
///
/// Asks git rather than looking for a file: `AUTO_MERGE` is a ref, and the
/// reftable backend keeps no file of that name. Only the `ort` strategy writes
/// it, so under any other this reports `None` and the caller falls back to its
/// generic message.
pub fn auto_merge_id(workdir: &Path) -> Option<String> {
    let out =
        super::run_git_stdout(workdir, &["rev-parse", "--verify", "--quiet", "AUTO_MERGE"]).ok()?;
    let id = out.trim().to_string();
    (!id.is_empty()).then_some(id)
}

/// Continue a rebase whose todo this caller filled with `edit` steps, aborting
/// automatically on conflict.
///
/// Reaching the next `edit` is the expected outcome here, so `Paused` is
/// success. Only for callers that put those steps in the todo themselves
/// (`fold`'s edit-and-continue and multi-phase paths, `split`) — a caller whose
/// todo has none would take a rebase left mid-flight for a finished one.
///
/// `after` says whether this continue has a rewrite of its own to protect.
pub fn continue_rebase_expecting_edit(workdir: &Path, after: AfterStop<'_>) -> Result<()> {
    let git_dir = super::absolute_git_dir(workdir)?;
    let mut named = after.protect.to_vec();
    named.extend(after.expect.map(str::to_string));
    let outcome = carry_past_known_stops(
        workdir,
        &git_dir,
        Protected::named(&named).targeting(after.targets),
        None,
        continue_rebase(workdir)?,
    )?;

    let Some(expect_stop) = after.expect else {
        return match outcome {
            RebaseOutcome::Completed | RebaseOutcome::Paused => Ok(()),
            RebaseOutcome::Stopped => Err(abort_after_failure(workdir)),
        };
    };

    match outcome {
        RebaseOutcome::Paused => verify_paused_at(workdir, expect_stop),
        RebaseOutcome::Completed => Err(finished_without_stopping(expect_stop)),
        RebaseOutcome::Stopped => Err(abort_after_failure(workdir)),
    }
}

/// What a caller does once a continued rebase reaches its next `edit` step.
#[derive(Debug, Default)]
pub struct AfterStop<'a> {
    expect: Option<&'a str>,
    protect: &'a [String],
    targets: &'a [String],
}

impl<'a> AfterStop<'a> {
    /// Nothing of the caller's own rides on this continue.
    pub fn nothing() -> Self {
        Self::default()
    }

    /// The caller rewrites the commit this continue stops at, so the stop is
    /// verified against it (see [`verify_paused_at`]).
    pub fn rewrite(expect_stop: &'a str) -> Self {
        Self {
            expect: Some(expect_stop),
            ..Self::default()
        }
    }

    /// Commits that must survive the replay this continue drives — one a later
    /// phase still has to find, for instance.
    pub fn protecting(self, protect: &'a [String]) -> Self {
        Self { protect, ..self }
    }

    /// The fold target this continue stops at, or another commit the
    /// operation lands on (see [`Protected`]).
    pub fn targeting(self, targets: &'a [String]) -> Self {
        Self { targets, ..self }
    }
}

/// The rebase ran to the end although its todo marked a commit for editing.
///
/// Defensive: git honors an `edit` line even for a commit it drops as empty, so
/// nothing is known to reach this. There is no rebase left to abort either way,
/// hence the report of what the repository now holds.
pub fn finished_without_stopping(expect_stop: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "The rebase finished without stopping at `{}` — history was rewritten\n\
         `git reflog` has the previous tips",
        super::short_hash(expect_stop)
    )
}

#[cfg(test)]
#[path = "git_rebase_test.rs"]
mod tests;
