# merge

Merge an existing branch into the integration branch, with a merge commit loom keeps as the branch's own section.

Branches are created with [`branch`](branch.md); [`unmerge`](unmerge.md) takes one back out.

## Usage

```
git loom merge [branch] [--all]
```

## Arguments

| Argument | Description |
|----------|-------------|
| `[branch]` | Branch name (optional; shows interactive picker if omitted) |

## Options

| Option | Description |
|--------|-------------|
| `-a, --all` | Also show remote branches without a local counterpart |

## What It Does

1. **Branch selection** — uses the provided name, or shows an interactive picker listing non-woven local branches
2. **Validation** — checks that the branch exists, is not already woven, into this integration branch or into the integration branch of another [worktree](worktree.md), and is not that integration branch itself: a branch lives in one integration branch at a time, since rewriting it from one would leave the other with its old commits
3. **Remote handling** — if a remote branch is selected (with `--all`), creates a local tracking branch automatically
4. **Merge** — performs a `git merge --no-ff`, so the branch keeps its own section in the integration topology

## Examples

### Merge a specific branch

```bash
git loom merge feature-auth
# ✓ Merged `feature-auth` into integration branch
```

### Interactive picker

```bash
git loom merge
# ? Select branch to merge ›
#   feature-auth
#   feature-logging
# ✓ Merged `feature-auth` into integration branch
```

### Include remote branches

```bash
git loom merge --all
# ? Select branch to merge ›
#   feature-auth
#   origin/feature-logging
```

## Conflicts

If the merge encounters a conflict, loom saves state and pauses:

```bash
git loom merge feature-auth
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the merge
#   loom abort      to cancel and restore original state
```

After resolving:

```bash
git add <resolved-files> && git loom continue
# ✓ Merged `feature-auth` into integration branch
```

See [`continue`](continue.md) and [`abort`](abort.md) for details.
