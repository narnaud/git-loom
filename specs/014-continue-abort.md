# Spec 014: Continue and Abort (Conflict Recovery)

## Overview

When a loom command encounters a rebase conflict, it pauses the operation and
saves recovery state to disk. The user resolves the conflict with standard git
tools, then runs `loom continue` to finish or `loom abort` to cancel.

This replaces the previous behavior of auto-aborting on conflict and forcing
the user to re-run the command from scratch.

## In-Scope Commands

The following commands support resumable conflict handling:

- `update`
- `commit`
- `absorb`
- `drop commit` (not `drop branch`)
- `reword` (commit reword; branch rename never rebases)
- Simple single-rebase `fold` paths:
  - `fold_files_into_commit` non-HEAD path
  - `fold_commit_into_commit`
  - `fold_commit_to_branch`
  - `fold_commit_to_unstaged` non-HEAD path (single rebase boundary only)

## Out-of-Scope Commands

The following commands retain the old hard-fail behavior: if they encounter a
conflict they abort immediately and leave the repository in its original state:

- `drop branch`
- `split`
- `fold` edit-and-continue paths
- `fold` multi-phase paths

## Saved State

When an in-scope command hits a conflict, it saves a state file:

```
.git/loom/state.json
```

The state file contains:

- `command`: The name of the interrupted command (e.g., `"update"`, `"commit"`)
- `rollback`: Saved references and patches for abort recovery:
  - `reset_mixed_to` / `reset_hard_to`: OID to reset to, when git's own abort
    does not go back far enough
  - `delete_branches`: Branch names created during this operation (to delete on abort)
  - `saved_staged_patch`: Index to restore, as a HEAD-to-index diff. Restored
    whichever way the rebase ends, not only on abort — see *Staging survives a
    rebase that completed* below
  - `saved_worktree_patch`: Full working-tree diff saved before the rebase
- `protect`: Commits the resumed rebase MUST NOT drop as empty (Spec 004). A
  `run_rebase_protecting` caller that can pause MUST record them here, or
  `loom continue` replays without the protection the command started with.
- `targets`: Protected commits the operation lands on (a fold or absorb
  target); the refusal does not offer `loom drop` for them.
- `context`: Command-specific resume data (serialized as JSON)

The `.git/loom/` directory is created lazily when state is first saved.

## Why a Rebase Stops

A conflict is the usual reason a rebase stops, but not the only one: an
untracked file in the way of a picked commit, a stale `index.lock`, a
permission or disk error all leave the rebase paused with a clean index.

A clean index can also mean the opposite of a breakdown: `rerere` replays a
recorded resolution, and with `rerere.autoUpdate` it stages the result, so the
rebase stops on a conflict that is already resolved.

Loom carries a rebase past that stop itself and reports each one: a conflict
already resolved leaves nothing to ask about. It follows the user's setting and
stages nothing itself: without `rerere.autoUpdate` the replay reaches the
working tree only, and the stop is a conflict like any other. Nor does it carry
a resolution that leaves HEAD's tree unchanged: `--continue` would then drop the
commit silently, even under `--empty=stop`, past the protection of Spec 004.
Loom keeps the stops it has continued past, each one the `AUTO_MERGE` id
together with the step it belongs to — two steps can conflict into the same
tree — so a step failing for another reason — a hook turning the commit down —
ends the carry instead of repeating for ever, and is not reported as carried. A
merge outside a rebase is not carried at all.

Loom describes whatever pause is left from what it finds:

| State | Message |
|-------|---------|
| Unmerged paths | Conflicts detected — resolve them |
| Clean index, new `AUTO_MERGE` | `rerere` resolved the conflicts — review the result |
| Clean index, unchanged or no `AUTO_MERGE` | The operation stopped part-way — run `loom trace` |

`AUTO_MERGE` is the ref git keeps while a conflicted merge is unfinished — a
conflicted pick during a rebase included — and drops once the resolution is
committed, so it describes the stop at hand and not an earlier one. Loom reads
it with `git rev-parse`, never as a file under the git dir: it is a ref, and the
reftable backend writes no file of that name. Only the `ort` merge strategy
writes it; under another strategy loom falls back to the generic message.

"New" means the id changed. `loom continue` therefore reads the id before
running git's `--continue` and compares it with the one left behind: an
unchanged id is the conflict the user was already on, so nothing resolved it for
them and the step failed for another reason — a hook rejecting the commit, say.
Without that check, a `git merge --continue` a hook turned down would be
reported as `rerere` having done the work.

The same distinction applies to the error an out-of-scope command reports after
aborting: a stop `rerere` resolved is still a conflict.

## Exit 0 Is Not "Finished"

`git rebase --continue` exits 0 both when the rebase is over and when it merely
advances to the next `edit`/`break` step, so the outcome of a rebase has three
states, not two: finished, paused at a step the todo asked for, and stopped
part-way.

Which of the last two counts as success depends on the caller, so the two are
never merged into one outcome:

- A command that put the `edit` steps in the todo itself (`split`, the
  edit-and-continue and multi-phase `fold` paths) drives them one by one, so
  reaching the next pause is the expected result.
- A command whose todo has no such step has nowhere to stop on purpose. A pause
  there means the rebase is still mid-flight, and treating it as finished would
  run the command's post-rebase work on a detached HEAD.
- `loom continue` reports the pause, names the command it belongs to, and leaves
  the state file in place.

## Paused Operation Lifecycle

```
loom <command>
  → rebase starts
  → conflict encountered
  → state saved to .git/loom/state.json
  → spinner shows error, user sees conflict guidance
  → process exits successfully (exit code 0)

user resolves conflicts:
  git add <resolved files>

loom continue
  → loads state
  → runs git rebase --continue (if rebase still active)
    → if another conflict: stays paused, state preserved
    → if completed: runs post-rebase work for the interrupted command
  → deletes state on success

--- OR ---

loom abort
  → loads state
  → aborts active rebase (if any)
  → applies rollback (restore refs, staged patch, worktree patch)
  → deletes state
  → reports success
```

## Allowed Commands While Paused

When `.git/loom/state.json` exists, most commands are blocked with an error
naming the interrupted command and instructing the user to run `loom continue`
or `loom abort`.

**Allowed while paused:**

- `show`
- `trace`
- `continue`
- `abort`

**Blocked while paused:**

- `status`
- `update`
- `commit`
- `absorb`
- `drop`
- `fold`
- `branch`
- `push`
- `init`
- `reword`
- `split`

**Always exempt (never checked):**

- `Completions` — does not interact with the repo
- `InternalWriteTodo` — runs as a git subprocess during rebase

Error message format, when git still has a rebase or merge in progress:

```
A `loom <command>` is paused due to conflicts.
Resolve them, then run `loom continue` to resume, or `loom abort` to cancel.
```

When no rebase or merge is in progress, the user most likely finished it with
raw git commands, so the message points at the bookkeeping `loom continue` has
left to do:

```
A `loom <command>` is paused, but no rebase is in progress.
If you finished it yourself, run `loom continue` to wrap up and clear the state.
Run `loom abort` to discard it instead.
```

## `loom continue`

```bash
loom continue
```

1. Loads `.git/loom/state.json`. If the file does not exist, falls back to the
   stateless path below.
2. If a rebase is still in progress (`MERGE_HEAD` or `rebase-merge/` exists):
   - Runs `git rebase --continue`.
   - If `--continue` encounters another conflict: stays paused, keeps the state
     file, reports that the operation is still paused, exits successfully.
   - If it halts on a commit in `protect` or `targets` that replayed empty,
     that is not a pause and not the user's to resolve: the rebase is aborted,
     the rollback is applied, the state file goes, and the refusal is reported as an error.
     Where the rollback takes that commit out of reach, the message MUST NOT
     offer `loom drop` for it.
   - If `--continue` succeeds: moves to dispatch.
3. If no rebase is in progress: assumes the user already ran `git rebase
   --continue` manually and moves to dispatch.
4. Dispatches to the command-specific `after_continue` handler.
5. Deletes the state file only after dispatch succeeds.

## `loom abort`

```bash
loom abort
```

1. Loads `.git/loom/state.json`. If the file does not exist, falls back to the
   stateless path below.
2. Aborts the active rebase or merge if one is in progress. If the abort itself
   fails (a concurrent git process, a stale `index.lock`), the command stops
   there: the rollback is not applied on top of a live rebase, and the state
   file stays so the user can retry once the repository is free.
3. Applies the rollback the command saved:
   - `reset --mixed` to `reset_mixed_to` (if non-empty)
   - `reset --hard` to `reset_hard_to` (if non-empty)
   - Deletes branches listed in `delete_branches`
   - Re-applies `saved_staged_patch` (if non-empty)
   - Re-applies `saved_worktree_patch` (if non-empty)

   Only a reset can fail the abort. When one does the state file stays as well,
   for the same reason: the rollback is half-applied, and it is the only record
   of what is left to undo. The message says so rather than blaming git — the
   abort already succeeded.

   The other steps never fail it. A branch that will not delete is left behind,
   and a patch that will not re-apply is parked as a file and named, so the
   rollback reports success and the state file goes with it — which is why the
   patch has to be parked rather than warned about.
4. Deletes the state file.
5. Reports success.

A command whose own rebase fails undoes itself the same way and removes its
state, so nothing later reports a paused operation. The exception is a rebase
still in progress — a failed abort, or the refusal above declining to reset over
uncommitted work: the rollback would make that worse, so both stay for
`loom abort`.

A refusal that never started the rebase is the other exception, and only in
part. Nothing was autostashed, so a command that moved nothing before it takes
back only the refs it made: the index and working tree are the user's, and
replaying a saved patch over them would double their staging. One that did move
something first MUST apply the whole rollback — its commits are already in
history and the state file recording the undo is deleted here. A recorded
`reset_mixed_to` or `reset_hard_to` is what tells the two apart. A command that
unstages before its rebase without moving HEAD MUST restore that itself
(Specs 006 and 007).

The same rule holds outside `loom abort`. Wherever a command aborts its own
rebase and then cleans up after itself — deleting a temp branch, resetting
refs, restoring a saved patch, removing the state file — the cleanup is skipped
only when a rebase is *still running* after a failed abort, since that is the
case where it would make the mess worse. It runs when the abort succeeded, and
equally when there was no rebase to abort: a command that failed before
starting one (the worktree check, for instance) has nothing in progress, and
skipping the cleanup there would strand exactly what it was about to remove.

The error reported in every case keeps the command's own failure visible. Only
one message reaches the user, so replacing the cause with a hint about the
abort would hide why the command failed at all.

### The state file is never deleted to recover

A corrupted or unreadable state file is not fixed by deleting it. It is the only
record of what `loom abort` would undo, and without it an abort runs
`git rebase --abort` and nothing else — a temp branch, a pre-rebase commit and a
saved staged patch all stay behind with no way back. The error tells the user to
move it aside instead, so the contents survive.

The file is written through a temp file that is renamed into place, so a process
killed mid-write leaves either the old state or the new one, never a truncated
one. The temp file has a random name and is removed when it goes out of scope,
so two loom processes saving at once cannot overwrite each other's and a crash
between the two steps leaves nothing behind. This protects against a killed
process, not a power cut, which would additionally need the rename itself made
durable with an fsync on the containing directory.

## Stateless Continue and Abort

Git can be left with a rebase or merge in progress that no state file describes:
an out-of-scope command whose own abort failed (a stale `index.lock`, a
concurrent git process), a loom process killed before it could save state, or a
rebase the user started with raw git.

In that case:

- `loom continue` runs `git rebase --continue` (or `git merge --continue`) and
  reports `"Completed the <rebase|merge> git had in progress (no loom state, so
  nothing else was done)"`, pausing again if it stops at another conflict or an
  `edit` step.
- `loom abort` runs `git rebase --abort` (or `git merge --abort`) and reports
  `"Canceled the <rebase|merge> git had in progress (no loom state to roll
  back)"`. If the abort itself fails, the error says so instead of claiming
  success.

Both say that git's own step is all that ran. There is no state file, so
`loom continue` finishes no command off — no saved patch re-staged, no temp
branch removed, no per-command success line such as `✓ Updated branch …` — and
`loom abort` undoes nothing beyond git's abort: a commit or temp branch an
earlier loom command created before its rebase started stays behind. A pause at
an `edit` step offers `loom abort` as "to cancel it (no loom state to roll
back)" for the same reason.

With neither a state file nor an in-progress rebase or merge, both still error
with `"No loom operation is in progress"`.

Other commands are blocked while such a rebase is in progress, with a message
naming the step it stopped at:

```
A git rebase is in progress (stopped at step 32/35), but no loom operation is recorded.
Resolve any conflicts and run `loom continue` to finish it, or `loom abort` to cancel it.
```

This replaces the bare `HEAD is detached` that `loom status` would otherwise
report.

## Double-Conflict Behavior

If `loom continue` encounters another conflict after running `git rebase
--continue`, the state file is kept and the rebase remains paused. The user
resolves the new conflict and runs `loom continue` again. This can repeat as
many times as needed until the rebase completes.

## Missing or Corrupted State

- **`loom continue` / `loom abort` with no state file**: See "Stateless Continue
  and Abort" — they act on an in-progress rebase or merge, and error with
  `"No loom operation is in progress"` when there is none.
- **Corrupted state file**: Both commands error with a parse failure message
  that names the file and tells the user to move it aside — never to delete it
  — and run `loom abort`. The state file is written to a temp file and renamed,
  so a killed process leaves either the old state or the new one, never a
  truncated file.

## Command-Specific Resume Context

Each resumable command stores its own context in the `context` field. The
context is opaque JSON. The command discriminator (`command` field) determines
which `after_continue` handler is invoked during dispatch.

### `update` context

```json
{
  "branch_name": "<local branch name>",
  "upstream_name": "<upstream tracking branch name>",
  "skip_confirm": false
}
```

After continue: restores pre-existing staged changes, runs submodule update (if
applicable), reports the upstream commit info, and proposes removing
gone-upstream branches.

### `commit` context

```json
{
  "branch_name": "<target feature branch name>",
  "saved_staged": "<patch content>"
}
```

After continue: restores the staged work set aside for this commit, prints the
success message with the new commit hash on the target branch. `saved_staged`
is absent in a state file written before the field existed, where the rollback
holds that same patch.

The rollback's `saved_staged_patch` is a different snapshot: the whole index as
the user left it, which the mixed reset restores when the commit is undone.

### `absorb` context

```json
{}
```

After continue: restores pre-existing staged changes and re-applies skipped
worktree changes (from `saved_worktree_patch`), prints the absorb success
message.

### `drop` context

```json
{
  "commit_hash": "<short hash of dropped commit>"
}
```

After continue: restores pre-existing staged changes, prints the drop success
message.

### `reword` context

```json
{
  "display": "<short hash of the commit before the rebase>",
  "new_hash": "<hash the commit now has>"
}
```

After continue: restores pre-existing staged changes, prints the reword success
message.

A `reword` conflicts only in its final phase. The rebase pauses at the target
with an `edit`, the amend happens, and `git rebase --continue` then replays
everything above — including any merge commit, whose parents have changed and
which therefore has to be remerged from scratch. A merge that was originally
resolved by hand conflicts again at that point, because a merge commit records
its result tree, never the resolution that produced it.

Aborting needs no `rollback` field other than `saved_staged_patch`: the amend is
made inside the rebase, so `git rebase --abort` discards it along with
everything else.

### `fold` context

```json
{
  "fold_op": "<FoldOp discriminator>",
  ... (operation-specific fields)
}
```

After continue: restores pre-existing staged changes, prints the fold success
message.

## Design Decisions

### State File in `.git/loom/`

Storing state under `.git/loom/` (rather than, e.g., the work tree root or
XDG config) keeps it:

- Scoped to the specific repository
- Out of the work tree (not visible to `git status`)
- Co-located with git's own recovery state (e.g., `rebase-merge/`)

### Exit Code 0 on Conflict Pause

When the operation is paused due to a conflict, loom exits with code 0. This
prevents CI pipelines or shell scripts from seeing a spurious failure while the
user is in the middle of conflict resolution. The conflict is surfaced visually
via the spinner error indicator and guidance message.

### State Deleted Only After Success

The state file is deleted only after `after_continue` succeeds. If
`after_continue` itself fails (e.g., a patch restore error), the state file
remains and the user can retry `loom continue`.

The exception is a refusal that already ended the rebase — a protected commit
that replayed empty. There is nothing left to continue, so the undo is finished
on the spot and the state goes with it; kept, it would report a paused
operation to every later command, including the one the refusal suggests.

### Staging Survives a Rebase That Completed

`git rebase --autostash` replays its stash into the working tree only, so a
staged *modification* comes back unstaged — on a rebase that completed as much
as on one `git rebase --abort` undid. (A staged *new* file keeps its index
entry either way.)

A command that saves `saved_staged_patch` MUST therefore restore it on every
exit: the `RebaseOutcome::Completed` arm, its `after_continue` handler, and the
abort. On that arm the restore MUST precede `transaction::delete`, so a delete
that fails still leaves the index as the user had it; the exception is an arm
whose own work reads the index to undo itself, which fold's uncommit does, and
there the restore follows that work. The restore applies three-way, never
resetting the index first: the patch is a HEAD-to-index diff, and a plain apply
would refuse the whole of it over either a staged *new* file whose entry
survived the autostash or a hunk whose context the rebase rewrote. Resetting to
make it apply does not help and can lose work: on the success path HEAD has
moved, so the reset lands the index on the *new* HEAD while the patch is
against the old one — and it has already dropped what the autostash preserved,
which the failing apply then cannot put back.

The restore is best-effort and MUST NOT fail its caller: it runs after that
command's own rewrite has landed, so an error here would report a rewrite that
succeeded as a failure — and one caller deletes the branch it just wove. What
it cannot replay it parks as a patch file and names, rather than dropping: where
the staged side differed from the working tree, a clean autostash replay takes
the stash with it and that patch is what is left.

A command that unstages before its rebase, and rewrites only files the patch
does not name, may restore with a plain apply instead — its index is at HEAD
and the patch's context is untouched. So may one restoring after
`git rebase --abort`, which puts HEAD back where the patch was taken: there a
reset makes the index exactly what the patch expects, at the cost of dropping
whatever the abort left staged outside it.

Two exceptions: a refusal that never started the rebase autostashed nothing, so
unless its rollback records a reset target that index is the user's, and an
abort that failed leaves the rebase on disk — neither index is loom's to touch.
An unmerged index is left alone for the same reason: the autostash replay
conflicted, so those stages are the user's to resolve and git kept the stash.

### Rollback Restores Pre-Existing State

The rollback on `loom abort` restores all branch refs and staged/worktree
patches to their state before the operation started. For `commit`, the
working-tree changes are preserved via mixed reset (the commit content returns
to the working directory as unstaged changes).

A saved patch that will not re-apply is parked as a file and named, not dropped.
It matters most on the abort path, where a reset may have taken the working tree
with it and the state file holding the patch is deleted as soon as the abort
reports success — but the rule holds wherever loom puts a saved patch back.

Where no `reset_*` field puts the index back at HEAD first, the staged patch
goes on three-way over whatever the abort left, rather than over a reset. The
result is the saved index in the ordinary case and never less than it: anything
the abort left staged that the patch does not name survives instead of being
discarded.
