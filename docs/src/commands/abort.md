# abort

Cancel a paused loom operation and restore the repository to its original state.
Without a saved state file there is nothing to roll back, so only the rebase or
merge git has in progress is canceled.

## Usage

```
git loom abort
```

## When to Use It

When a loom operation is paused due to a conflict and you decide you don't want
to complete it, `loom abort` cancels the operation and rolls back all changes
made so far:

```bash
git loom commit -b feature-auth -m "add auth" zz
# ! Conflicts detected...

git loom abort
# ✓ Aborted `loom commit` and restored original state
```

## What It Does

1. Loads the saved state from `.git/loom/state.json`
2. Aborts the active rebase (if one is in progress)
3. Applies rollback:
   - Hard-resets HEAD to the pre-operation state
   - Restores all branch refs to their pre-operation positions
   - Deletes any branches that were created during the operation
   - Re-applies pre-existing staged changes (if any were saved aside)
   - Re-applies working-tree changes (if any were saved)
4. Deletes the state file
5. Reports success

### Special case: `commit`

After aborting a `commit`, the committed content is returned to the **working
tree as unstaged changes** (via `git reset --mixed`) rather than being
discarded. Your work is preserved; the commit is simply undone.

## When the Abort Itself Fails

`git rebase --abort` (or `git merge --abort`) can fail, typically because
another git process holds `.git/index.lock`. Loom then reports the failure and
keeps the state file instead of rolling back on top of a rebase that is still
running — rerun `loom abort` once the repository is free.

A reset or a staged-patch restore in the rollback that follows behaves the same
way: the state file stays, because it is the only record of what is left to
undo, and the message names the half-applied rollback rather than git, which has
finished its own abort by then. A temp branch that will not delete, or a
working-tree patch that will not re-apply, only warns — the rollback carries on
and the state file goes.

For that same reason, never delete `.git/loom/state.json` to get unstuck. Once
it is gone, `loom abort` can only run git's own abort: a temp branch, a
pre-rebase commit, or a saved staged patch would all be stranded. If loom
reports the file as corrupted, move it aside rather than removing it.

## Without a Saved State File

If git has a rebase or merge in progress that no loom state file describes — a
command that failed before it could save state, or one you started with raw
git — `loom abort` cancels it anyway:

```bash
git loom abort
# ✓ Canceled the rebase git had in progress (no loom state to roll back)
```

A merge is canceled the same way, with `git merge --abort`.

There is no loom state here, so there is nothing to roll back beyond what git
itself undoes: if a loom command had already created a commit or a temp branch,
or set staged changes aside, before its rebase started, none of that is undone.

## Error: No Operation in Progress

With neither a state file nor a rebase or merge in progress:

```bash
git loom abort
# error: No loom operation is in progress
```

## See Also

- [`continue`](continue.md) — resume instead of canceling
