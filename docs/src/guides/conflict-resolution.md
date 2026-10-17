# Resolving Conflicts

When a loom command rewrites history and two changes touch the same lines, git can't combine them on its own. Most commands then **pause**, so you can resolve the conflict and carry on. Nothing is lost either way.

## 1. The Pause

```bash
git loom commit -b feature-auth -m "add auth middleware" zz
# ✓ Created branch `feature-auth` at `a1b2c3d`
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the commit
#   › loom abort      to cancel and restore original state
```

Until you continue or abort, most loom commands are refused; `show`, `diff`, `trace` and `tui` still work.

## 2. Find the Conflicts

`git status` lists them under *Unmerged paths*, and `git loom tui` marks them `!!`. `git diff` shows the conflicts inline.

## 3. Resolve Each File

Git marks both versions in the file:

```
<<<<<<< HEAD
// existing middleware code
=======
// your new auth middleware
>>>>>>> 1a2b3c4 (add auth middleware)
```

Edit the file to keep what you want, and remove the `<<<<<<<`, `=======` and `>>>>>>>` lines. Most editors have buttons for this, or run `git mergetool`.

## 4. Mark Them Resolved

Stage each resolved file with plain git:

```bash
git add src/middleware.rs
```

Use `git rm` for a file the resolution deletes.

## 5. Continue or Abort

```bash
git loom continue
# ✓ Created commit `mqt` (b2c3d4e) on branch `feature-auth`
```

Loom finishes the command as if nothing had happened, staged changes included. If a later commit conflicts too, it pauses again: repeat steps 2 to 5.

To give up instead:

```bash
git loom abort
# ✓ Aborted `loom commit` and restored original state
```

Everything goes back to how it was before the command: history, branches, staged and unstaged changes.

You can also resolve with the [TUI](../commands/tui.md#conflicts), which opens on a paused operation.

## Commands That Don't Pause

`split`, `branch`, `unmerge`, dropping a branch, and some `fold` forms don't pause: on a conflict they roll back and leave everything as it was. See [continue](../commands/continue.md#which-commands-pause) for the full list.

## When Things Go Off Script

- **You finished the rebase with git**: if you ran `git rebase --continue` to the end, `git loom continue` still completes the loom command.
- **Your resolution left a commit with no changes**: git drops that commit, and the command still reports success as if it had survived; `git reflog` still has it. A change you were folding or absorbing is not lost this way: it still lands, or comes back to your working tree, and loom says so — as long as you resolved with `git checkout HEAD -- <file>` and `git add` rather than `git reset --hard` or a commit of your own.
- **Changes that can't be put back**: if git couldn't replay its stash, your changes wait in `git stash list`. If your staged changes no longer apply, loom saves them as a patch and prints the `git apply` command that restores them.
- **Loom says an operation is paused but no rebase is in progress**: run `git loom continue` if you finished it yourself, or `git loom abort` to discard it. Never delete the paused-operation state by hand: it is what abort uses to restore your changes.

See also: [`continue`](../commands/continue.md) · [`abort`](../commands/abort.md)
