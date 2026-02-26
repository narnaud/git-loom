# update

Pull-rebase the integration branch and update submodules.

## Usage

```
git-loom update
```

No arguments.

## What It Does

1. **Fetch** — fetches all upstream changes, including tags (force-updated) and pruning deleted remote branches
2. **Rebase** — replays local commits on top of the updated upstream
3. **Submodule update** (if applicable) — initializes and updates submodules recursively

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
# x Rebase failed
# error: CONFLICT (content): Merge conflict in file.txt
```

Resolve conflicts with standard git commands (`git rebase --continue`, etc.).

### Error: not on an integration branch

```bash
git-loom update
# error: Branch has no upstream tracking branch.
# Run 'git-loom init' to set up an integration branch.
```

## Prerequisites

- Must be on a branch with upstream tracking configured
- Network access to the remote
