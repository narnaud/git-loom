# continue

Resume a paused loom operation once its conflicts are resolved.

**Alias:** `cont`

## Usage

```
git loom continue
```

## What It Does

When a command hits a conflict, it pauses:

```
! Conflicts detected — resolve them with git, then run:
  › loom continue   to complete the commit
  › loom abort      to cancel and restore original state
```

Resolve each conflicting file, stage it with `git add`, then run `git loom continue`. Loom resumes the rewrite and finishes the command, printing the success line it would have printed. If another commit conflicts, it pauses again; repeat until done:

```bash
git add src/auth.rs && git loom continue
# ! Conflicts remain — resolve them and run `loom continue` again

git add src/shared.rs && git loom continue
# ✓ Created commit `mqt` (a1b2c3d) on branch `feature-auth`
```

If you already finished the rebase yourself with `git rebase --continue`, `loom continue` still completes the loom command.

Continuing a [`merge`](merge.md) puts your set-aside uncommitted changes back over it; if they conflict, it pauses again for you to resolve them.

To give up instead, run [`abort`](abort.md). The [conflict guide](../guides/conflict-resolution.md) walks through a full resolution.

## Which Commands Pause

| Pause on a conflict | Roll back on a conflict |
|---------------------|-------------------------|
| `update` | `split` |
| `commit` | `branch` |
| `absorb` | `unmerge` |
| `swap` | `drop` of a branch |
| `merge` | `fold` moving several commits, `-c`, a commit file (`mqt:0`), or a commit's hunks (`-p`) |
| `reword` of a commit | |
| `drop` of a commit | |
| `fold` amending (`-p` included) or fixing up a commit, moving one commit, uncommitting a commit | |

A command that rolls back leaves the repository as it was before you ran it.

## While Paused

Other loom commands are refused until you continue or abort. These still work: `show`, `diff`, `trace`, `tui`, `worktree list`, `worktree path` and `agent install`. Stage resolutions with plain `git add`.

## Without a Paused Loom Operation

If git has a rebase or merge in progress that loom didn't start, `loom continue` continues it:

```bash
git loom continue
# ✓ Completed the rebase git had in progress (no loom state, so nothing else was done)
```

With nothing in progress at all, it fails with `No loom operation is in progress`.
