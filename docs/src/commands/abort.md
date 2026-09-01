# abort

Cancel a paused loom operation and restore the repository to its original state.

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

## Without a Saved State File

If git has a rebase or merge in progress that no loom state file describes — a
command that failed before it could save state, or a rebase you started with
raw git — `loom abort` cancels it anyway:

```bash
git loom abort
# ✓ Aborted the rebase in progress
```

## Error: No Operation in Progress

With neither a state file nor a rebase or merge in progress:

```bash
git loom abort
# error: No loom operation is in progress
```

## See Also

- [`continue`](continue.md) — resume instead of cancelling
