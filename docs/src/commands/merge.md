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

A branch can be woven into only one integration branch at a time. `merge` refuses a branch already woven here or into another [worktree](worktree.md)'s integration branch, as well as a branch with nothing to merge. It also refuses while the working tree has unresolved conflicts, a staged submodule change, or a file removed from the index with `git rm --cached` but kept on disk, and when the branch deletes a file you changed or changes one you deleted, since your change could not be put back over it.

The merge is one transaction: your staged and unstaged changes are set aside, the branch is merged, then your changes are put back on top — staged ones staged again where possible.

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

If your uncommitted changes overlap what the merge brings in, putting them back conflicts and the merge pauses the same way. A binary file cannot show conflict markers: loom leaves your version in it, and `git checkout HEAD -- <file>` takes the merge's instead.

[`loom abort`](abort.md) at any point returns to where the merge started — the merge undone, your changes back as they were — and drops whatever was done in between.

See [`continue`](continue.md) and [`abort`](abort.md).
