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
  - `saved_head`: HEAD OID before the operation started
  - `saved_refs`: Snapshot of all branch ref OIDs before the operation
  - `delete_branches`: Branch names created during this operation (to delete on abort)
  - `saved_staged_patch`: Staged diff saved aside during the operation
  - `saved_worktree_patch`: Full working-tree diff saved before the rebase
- `context`: Command-specific resume data (serialized as JSON)

The `.git/loom/` directory is created lazily when state is first saved.

## Why a Rebase Stops

A conflict is the usual reason a rebase stops, but not the only one: an
untracked file in the way of a picked commit, a stale `index.lock`, a
permission or disk error all leave the rebase paused with a clean index.

Loom therefore checks for unmerged paths before describing the pause. With
none, the message says the rebase stopped part-way and points at `loom trace`
instead of telling the user to resolve conflicts that do not exist. This
applies both to the pause message of a resumable command and to the error an
out-of-scope command reports after aborting.

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

   If a step here fails, the state file stays as well, for the same reason: the
   rollback is half-applied, and it is the only record of what is left to undo.
   The message says so rather than blaming git — the abort already succeeded.
4. Deletes the state file.
5. Reports success.

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

After continue: runs submodule update (if applicable), reports the upstream
commit info, and proposes removing gone-upstream branches.

### `commit` context

```json
{
  "branch_name": "<target feature branch name>",
  "saved_staged_patch": "<patch content>"
}
```

After continue: restores pre-existing staged changes, prints the success
message with the new commit hash on the target branch.

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

After continue: prints the drop success message.

### `reword` context

```json
{
  "display": "<short hash of the commit before the rebase>",
  "new_display": "<short hash the commit now has>"
}
```

After continue: prints the reword success message.

A `reword` conflicts only in its final phase. The rebase pauses at the target
with an `edit`, the amend happens, and `git rebase --continue` then replays
everything above — including any merge commit, whose parents have changed and
which therefore has to be remerged from scratch. A merge that was originally
resolved by hand conflicts again at that point, because a merge commit records
its result tree, never the resolution that produced it.

Aborting needs no `rollback` fields: the amend is made inside the rebase, so
`git rebase --abort` discards it along with everything else.

### `fold` context

```json
{
  "fold_op": "<FoldOp discriminator>",
  ... (operation-specific fields)
}
```

After continue: prints the fold success message.

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

### Rollback Restores Pre-Existing State

The rollback on `loom abort` restores all branch refs and staged/worktree
patches to their state before the operation started. For `commit`, the
working-tree changes are preserved via mixed reset (the commit content returns
to the working directory as unstaged changes).
