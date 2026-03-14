# update

Pull-rebase the integration branch and update submodules.

## Usage

```
git-loom update [--yes]
```

**Options:**

- `--yes` / `-y` — skip the confirmation prompt when removing branches with a gone upstream

## What It Does

1. **Fetch** — fetches all upstream changes, including tags (force-updated) and pruning deleted remote branches
2. **Rebase** — replays local commits on top of the updated upstream
3. **Submodule update** (if applicable) — initializes and updates submodules recursively
4. **Gone upstream cleanup** — lists local branches whose remote tracking branch was deleted, and offers to remove them

Uncommitted working tree changes are automatically preserved during the rebase.

## Examples

### Standard update

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# > Rebased onto upstream
```

### With gone upstream branches

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# > Rebased onto upstream
# > Updated branch `integration` with `origin/main` (abc1234 Some commit)
# ! 2 local branches with a gone upstream:
#   · feature-x
#   · feature-y
# ? Remove them? [y/N]
```

### With submodules

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# > Rebased onto upstream
# > Updating submodules...
# > Updated submodules
```

### Merge conflict

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# x Rebase paused due to conflicts
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the update
#   loom abort      to cancel and restore original state
```

Resolve conflicts in your editor, stage the resolutions, then continue:

```bash
git add <resolved-files>
git-loom continue
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest commit)
```

Or cancel the update:

```bash
git-loom abort
# ✓ Aborted `loom update` and restored original state
```

See [`continue`](continue.md) and [`abort`](abort.md) for details.

### Error: not on an integration branch

```bash
git-loom update
# error: Branch has no upstream tracking branch.
# Run 'git-loom init' to set up an integration branch.
```

## Prerequisites

- Must be on a branch with upstream tracking configured
- Network access to the remote
