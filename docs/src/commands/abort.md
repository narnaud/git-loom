# abort

Cancel a paused loom operation and put everything back as it was.

## Usage

```
git loom abort
```

## What It Does

Cancels the paused operation and restores the state from before the command: history, branches, staged changes and working-tree changes. Branches the command created are deleted.

```bash
git loom commit -b feature-auth -m "add auth" zz
# ! Conflicts detected...

git loom abort
# ✓ Aborted `loom commit` and restored original state
```

After aborting a `commit`, the changes you were committing are back as they were: staged ones staged, the rest unstaged.

After aborting a [`merge`](merge.md), the merge is undone and your uncommitted changes are back as they were, staging included. Anything you changed while it was paused is dropped.

If the abort can't complete — another git process is holding the repository, for example — loom says so and keeps the paused operation, so you can run `loom abort` again.

> [!WARNING]
> Never delete the paused-operation state under `.git/loom/` to get unstuck: it is what `loom abort` uses to restore your changes. If loom reports it as corrupted, move it aside and keep it — anything the command had set aside can then only be recovered from it by hand — and run `loom abort` if git still has a rebase in progress.

## Without a Paused Loom Operation

If git has a rebase or merge in progress that loom didn't start, `loom abort` cancels it:

```bash
git loom abort
# ✓ Canceled the rebase git had in progress (no loom state to roll back)
```

With nothing in progress at all, it fails with `No loom operation is in progress`.

See also [`continue`](continue.md).
