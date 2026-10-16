# merge

Weave an existing branch into the integration branch.

To take a branch back out, use [`unmerge`](unmerge.md). To create a new branch, use [`branch`](branch.md) or [`commit -b`](commit.md).

## Usage

```
git loom merge [<branch>] [--all]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<branch>` | A local branch, or a remote one such as `origin/feature-x`. A picker opens if omitted. |

### Options

| Option | Description |
|--------|-------------|
| `-a, --all` | Also list remote branches that have no local branch |

## What It Does

Merges the branch so it gets its own section in `git loom status`. Merging a remote branch first creates a local branch tracking it.

A branch can be woven into only one integration branch at a time. `merge` refuses a branch already woven here or into another [worktree](worktree.md)'s integration branch, as well as a branch with nothing to merge. Staged changes must be committed or unstaged first.

## Examples

### Merge a branch

```bash
git loom merge feature-auth
# ✓ Merged `feature-auth` into integration branch
```

### Pick one, remote branches included

```bash
git loom merge --all
# ? Select branch to merge ›
#   feature-auth
#   origin/feature-logging
```

## Conflicts

If the merge conflicts, it pauses:

```bash
git loom merge feature-auth
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the merge
#   › loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Merged `feature-auth` into integration branch
```

See [`continue`](continue.md) and [`abort`](abort.md).
