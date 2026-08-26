# Resolving Conflicts

When a loom operation rewrites history and two commits touch the same lines,
git can't merge them automatically. Instead of aborting, loom **pauses** the
operation and lets you fix the conflict before continuing.

## What a Paused Operation Looks Like

```bash
$ git loom commit -b feature-auth -m "add auth middleware" zz
✓ Created branch `feature-auth` at `a1b2c3d`
! Conflicts detected — resolve them with git, then run:
  loom continue   to complete the commit
  loom abort      to cancel and restore original state
```

The process exits with code 0. Your work is safe — loom saved the operation
state to `.git/loom/state.json` and left the rebase paused at the conflicting
commit.

## Step 1: Find the Conflicts

```bash
$ git status
You are currently rebasing branch 'integration' on 'a1b2c3d'.
  (fix conflicts and then run "git rebase --continue")

Unmerged paths:
  (use "git add <file>..." to mark resolution)
        both modified:   src/middleware.rs
```

The conflicting files are listed under *Unmerged paths*. You can also run
`git diff` to see the conflict markers inline.

## Step 2: Resolve Each File

Open each conflicting file in your editor. Git inserts conflict markers to
show both versions:

```
<<<<<<< HEAD
// existing middleware code
=======
// your new auth middleware
>>>>>>> feature-auth
```

Edit the file to keep what you want — either one side, the other, or a
combination of both — and remove the `<<<<<<<`, `=======`, and `>>>>>>>`
markers entirely.

> [!TIP]
> Most editors have built-in conflict resolution UI. In VS Code, click
> *Accept Current*, *Accept Incoming*, or *Accept Both* above each conflict
> block. For a dedicated mergetool, run `git mergetool`.
> See the [git documentation on resolving conflicts](https://git-scm.com/docs/git-merge#_how_conflicts_are_presented)
> for more detail.

## Step 3: Mark Files as Resolved

Once a file is clean (no more conflict markers), stage it:

```bash
$ git add src/middleware.rs
```

For a file that should be deleted entirely as the resolution, use:

```bash
$ git rm src/middleware.rs
```

Repeat for every conflicting file. When `git status` shows no more unmerged
paths, you're ready to continue.

## Step 4: Continue or Abort

**To finish the operation:**

```bash
$ git loom continue
✓ Created commit `b2c3d4e` on branch `feature-auth`
```

Loom runs `git rebase --continue` internally, completes the interrupted
command's post-rebase work (restoring staged patches, printing the success
message), and removes the saved state.

**To cancel and go back to where you started:**

```bash
$ git loom abort
✓ Aborted `loom commit` and restored original state
```

Abort rolls back all branch refs, removes any branches created during the
operation, and restores any staged changes that were saved aside. For `commit`,
the content you were committing comes back as unstaged working-tree changes so
nothing is lost.

## Multiple Conflicts

If your branch has several commits that conflict, each `loom continue` may
pause again at the next one. Repeat the resolve → `git add` → `loom continue`
cycle until the operation completes:

```bash
$ git loom update
! Conflicts detected...

$ git add src/api.rs && git loom continue
! Conflicts remain — resolve them and run `loom continue` again

$ git add src/models.rs && git loom continue
✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

## If You Finished the Rebase Yourself

That's fine. If you resolved the conflicts with raw git commands and ran
`git rebase --continue` to the end, loom notices the rebase is no longer
active and skips straight to the post-rebase work when you run
`loom continue`: worktree syncs, submodule updates, branch cleanup, and
finally removing the state file.

The blocked-command error tells you which case you are in:

```
✗ A `loom update` is paused, but no rebase is in progress.
  › If you finished it yourself, run `loom continue` to wrap up and clear the state.
  › Run `loom abort` to discard it instead.
```

## If the State File is Stale

If loom blocks you with a "paused operation" error but you know no operation
is actually in progress (e.g., after a crash or force-reset), run
`loom continue` to finish up, or `loom abort` to discard the operation.

As a last resort you can delete the state file by hand:

```bash
rm .git/loom/state.json
```

In a linked worktree it lives under that worktree's git directory instead:
`.git/worktrees/<name>/loom/state.json`.

> [!WARNING]
> Only do this if you are certain no loom operation is paused. If a rebase is
> still in progress, run `loom abort` instead — that also aborts the rebase
> and restores your branch refs.

## See Also

- [`continue`](../commands/continue.md) — reference for `loom continue`
- [`abort`](../commands/abort.md) — reference for `loom abort`
- [Git documentation: Basic Merge Conflicts](https://git-scm.com/book/en/v2/Git-Branching-Basic-Branching-and-Merging#_basic_merge_conflicts)
