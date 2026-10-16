# update

Fetch upstream, rebase your integration branch and all its feature branches onto it, and update submodules.

**Alias:** `up`

## Usage

```
git loom update [-y]
```

### Options

| Option | Description |
|--------|-------------|
| `-y, --yes` | Remove merged and gone branches without asking |

## What It Does

1. **Fetches** the tracked remote, tags included. In a fork setup, the remote you [push](push.md#push-remote) to is fetched too.
2. **Drops** commits that already landed upstream, merged or cherry-picked.
3. **Rebases** every feature branch onto the new upstream, keeping the integration layout. Uncommitted changes are kept, staged ones staged.
4. **Updates submodules**, if any.
5. **Offers to remove** local branches that were fully merged upstream, or whose remote branch was deleted. Set [`loom.pruneGoneBranches`](../configuration.md#loomprunegonebranches) to always remove them without asking.

An integration branch holding an octopus merge (several branches merged at once) is still updated, by git's own rebase, with a warning: branches already merged upstream are not offered for removal.

## Examples

```bash
git loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
#   › `feature-x`: fully merged upstream
#   › `feature-y`: upstream gone
# ? Remove 2 local branches? [y/N] y
# ✓ Removed branch `feature-x` (was 4f825d2)
# ✓ Removed branch `feature-y` (was 711d082)
```

A removed branch's message gives its last commit, so you can bring it back with `git branch <name> <commit>`. A branch that can't be deleted, such as one checked out in another worktree, is skipped with a warning.

## Conflicts

If the rebase conflicts, the update pauses:

```bash
git loom update
# ✓ Fetched latest changes
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the update
#   › loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

See [`continue`](continue.md) and [`abort`](abort.md).

## Prerequisites

- On a branch with an upstream (see [`init`](init.md))
