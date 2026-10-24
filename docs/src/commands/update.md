# update

Fetch upstream, rebase your integration branch and all its feature branches onto it, and update submodules. With a branch name, take that branch's remote version first.

**Alias:** `up`

## Usage

```
git loom update [<branch>] [-y]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<branch>` | A woven branch to update from its remote branch, by name or short ID |

### Options

| Option | Description |
|--------|-------------|
| `-y, --yes` | Remove merged and gone branches without asking |

## What It Does

1. **Fetches** the tracked remote, tags included. In a fork setup, the remote you [push](push.md#push-remote) to is fetched too, and so is a named `<branch>`'s remote branch when it lives on yet another remote.
2. **Takes the remote version** of `<branch>`, when one is named: its commits are replaced by those of the branch it tracks, whatever the local branch had. The message names the tip it had, so you can bring it back.
3. **Drops** commits that already landed upstream, merged or cherry-picked.
4. **Rebases** every feature branch onto the new upstream, keeping the integration layout. Uncommitted changes are kept, staged ones staged.
5. **Updates submodules**, if any.
6. **Offers to remove** local branches that were fully merged upstream, or whose remote branch was deleted. Set [`loom.pruneGoneBranches`](../configuration.md#loomprunegonebranches) to always remove them without asking.

An integration branch holding an octopus merge (several branches merged at once) is still updated, by git's own rebase, with a warning: branches already merged upstream are not offered for removal, and a named branch is refused.

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

A branch you pushed and then advanced elsewhere, from another machine, a colleague's push or an update to your pull request, is refreshed by naming it:

```bash
git loom update feature-x
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
# ✓ Updated branch `feature-x` with `origin/feature-x` (9c1e2f3 Second new commit, was 4f825d2)
```

Nothing is compared: commits the local branch had and the remote lacks go with the update, and `was 4f825d2` is the way back: `git loom unmerge feature-x`, `git branch -f feature-x 4f825d2`, then `git loom merge feature-x`. A branch sharing commits with another one (stacked, or at the same tip), one whose remote holds a merge commit, one that is not woven, and one without a remote branch are refused before anything is rewritten. A branch whose remote version landed upstream whole is offered for removal instead.

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
