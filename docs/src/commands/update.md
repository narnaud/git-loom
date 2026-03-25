# update

Pull-rebase the integration branch onto the latest upstream and update submodules.

**Alias:** `up`

## Usage

```
git-loom update [-y]
```

### Options

| Option | Description |
|--------|-------------|
| `-y, --yes` | Skip confirmation prompt when removing branches with a gone upstream |

## What It Does

### Fetch

Runs `git fetch --tags --force --prune` against the tracked remote. Force-updates moved tags and prunes deleted remote branches from local tracking refs.

### Upstream Commit Filtering

Before rebasing, loom scans every feature-branch commit against the new upstream and drops any that are already present. Two strategies are applied:

1. **Direct merge** — if the upstream is a descendant of the commit's OID, the commit was merged directly.
2. **Cherry-pick** — if the commit's patch-ID matches a new upstream commit, it was cherry-picked.

If an entire branch section empties out after filtering, its section and merge entry are removed from the rebase todo. The branch ref is left intact for manual cleanup.

### Rebase

Replays local commits onto the updated upstream using a topology-aware weave model — ensuring new upstream commits land on the base line, not inside feature branch sections. Uncommitted working tree changes are automatically stashed and restored.

If the current branch has no weave topology (a plain tracked branch), loom falls back to a standard `git rebase --autostash --update-refs --rebase-merges`.

### Submodule Update

If `.gitmodules` exists, runs `git submodule update --init --recursive`.

### Gone Upstream Cleanup

Lists any local branches whose upstream tracking ref was pruned in the fetch step, then prompts once to remove them. Pass `-y` to skip the prompt. Each branch is deleted individually; if a branch has unmerged local commits, it is skipped with a warning rather than aborting the cleanup.

## Examples

### Standard update

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

### Cherry-picked commits auto-dropped

```bash
# feature-a had commits F1, F2, F3 — upstream cherry-picked F1 and F2
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
# F1 and F2 are silently dropped; F3 remains on feature-a
```

### With submodules

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated submodules
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

### Gone upstream branches

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
# ! 2 local branches with a gone upstream:
#   · feature-x
#   · feature-y
# ? Remove them? [y/N]
```

### Skip gone-upstream prompt

```bash
git-loom update -y
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
# ✓ Removed branch `old-feature`
# ✓ Removed branch `closed-pr`
```

### Gone branch with unmerged commits

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
# ! 1 local branch with a gone upstream:
#   work-in-progress
# Remove it? [y/N] y
# ! Skipped branch `work-in-progress` — it has unmerged local commits.
#   Use `git branch -D work-in-progress` to force-delete.
```

## Conflicts

If the rebase encounters a conflict, loom saves state and pauses:

```bash
git-loom update
# ✓ Fetched latest changes
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the update
#   loom abort      to cancel and restore original state
```

After resolving:

```bash
git add <resolved-files> && git-loom continue
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

Or cancel:

```bash
git-loom abort
# ✓ Aborted `loom update` and restored original state
```

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Must be in a git repository with a working tree
- Current branch must have upstream tracking configured (use [`init`](init.md) first)
- Network access to the remote
