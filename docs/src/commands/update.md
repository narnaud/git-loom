# update

Pull-rebase the integration branch onto the latest upstream and update submodules.

**Alias:** `up`

## Usage

```
git loom update [-y]
```

### Options

| Option | Description |
|--------|-------------|
| `-y, --yes` | Skip confirmation prompt when removing branches that are fully merged upstream or have a gone upstream |

### Configuration

| Config | Description |
|--------|-------------|
| `loom.pruneGoneBranches` | When `true`, always remove fully merged and gone-upstream branches without prompting (same as `--yes`). Set with `git config loom.pruneGoneBranches true`. |

## What It Does

### Fetch

Runs `git fetch --tags --force --prune` against the tracked remote. Force-updates moved tags and prunes deleted remote branches from local tracking refs.

In a fork workflow, where `loom push` sends feature branches to another remote (see [push](push.md)), that remote is fetched too with `git fetch --prune <remote>` — otherwise its tracking refs go stale and branches deleted on the fork are never seen as gone. Tags are only fetched from the tracked remote. If the fork is unreachable, loom warns and continues with the update.

### Upstream Commit Filtering

Before rebasing, loom scans every feature-branch commit against the new upstream and drops any that are already present. Two strategies are applied:

1. **Direct merge** — if the upstream is a descendant of the commit's OID, the commit was merged directly.
2. **Cherry-pick** — if the commit's patch-ID matches a new upstream commit, it was cherry-picked.

If an entire branch empties out after filtering, its section and merge entry are removed from the rebase todo. This also covers a stacked branch whose commits all landed upstream while the branch built on top of it did not. Such fully merged branches are offered for removal after the rebase (see Branch Cleanup below).

### Rebase

Replays local commits onto the updated upstream using a topology-aware weave model — ensuring new upstream commits land on the base line, not inside feature branch sections. Uncommitted working tree changes are automatically stashed and restored, staged ones as staged.

If the current branch has no weave topology (a plain tracked branch), loom falls back to a standard `git rebase --autostash --update-refs --rebase-merges`.

### Submodule Update

If `.gitmodules` exists, runs `git submodule update --init --recursive`.

### Branch Cleanup

Lists local branches that are fully merged upstream (every commit was filtered out before the rebase) and local branches whose upstream tracking ref was pruned in the fetch step, then prompts once to remove them, naming each branch and why it goes. Pass `-y` to skip the prompt; the branches are then listed as warnings. Branches are force-deleted one by one; each success message shows the tip the branch had, so it can be revived. A branch that cannot be deleted (checked out in another worktree, for instance) is skipped with a warning rather than aborting the cleanup.

## Examples

### Standard update

```bash
git loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

### Cherry-picked commits auto-dropped

```bash
# feature-a had commits F1, F2, F3 — upstream cherry-picked F1 and F2
git loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
# F1 and F2 are silently dropped; F3 remains on feature-a
```

### With submodules

```bash
git loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated submodules
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

### Branch merged upstream

```bash
git loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
#   › `feature-x`: fully merged upstream
# ? Remove local branch? [y/N]
```

### Gone upstream branches

```bash
git loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
#   › `feature-x`: upstream gone
#   › `feature-y`: upstream gone
# ? Remove 2 local branches? [y/N]
```

### Skip the removal prompt

```bash
git loom update -y
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
# ! 2 local branches with a gone upstream:
#   › old-feature
#   › closed-pr
# ✓ Removed branch `old-feature`
# ✓ Removed branch `closed-pr`
```

The same behavior can be made permanent with `git config loom.pruneGoneBranches true`.

### Gone branch that cannot be deleted

```bash
git loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
#   › `work-in-progress`: upstream gone
# ? Remove local branch? [y/N] y
# ! Skipped branch `work-in-progress` — could not delete it (run `loom trace` for the git error)
```

## Conflicts

If the rebase encounters a conflict, loom saves state and pauses:

```bash
git loom update
# ✓ Fetched latest changes
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the update
#   loom abort      to cancel and restore original state
```

After resolving:

```bash
git add <resolved-files> && git loom continue
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

Or cancel:

```bash
git loom abort
# ✓ Aborted `loom update` and restored original state
```

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Must be in a git repository with a working tree
- Current branch must have upstream tracking configured (use [`init`](init.md) first)
- Network access to the remote
