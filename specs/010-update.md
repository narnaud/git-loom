# Spec 010: Update

## Overview

`git loom update` fetches the latest upstream changes (including tags and pruning
deleted remote branches) then rebases the current integration branch onto the
upstream. Submodules are updated if any are configured. On merge conflict, the
error is reported so the user can resolve it manually.

## Why Update?

After initializing an integration branch with `git loom init`, the upstream
remote will continue to receive new commits. The update command brings those
changes into the local integration branch in a single step:

- **Fetch** all upstream changes, including tags, and prune deleted remote branches
- **Rebase** local commits onto the updated upstream
- **Submodule sync** automatically when the project uses submodules

## CLI

```bash
git-loom update [--yes]
```

**Arguments:**

- `--yes` / `-y`: Skip the confirmation prompt when removing branches with gone upstreams.

**Behavior:**

- Validates the current branch is an integration branch (has upstream tracking)
- Fetches all upstream changes (tags, pruning deleted remote branches)
- Rebases local commits onto the updated upstream
- Updates submodules if any are configured
- On conflict, reports the error and lets the user resolve manually
- After a successful update, proposes to remove local branches whose remote
  tracking branch was deleted (upstream is gone)

## What Happens

1. **Validation**:
   - HEAD must be on a branch (not detached)
   - The current branch must have an upstream tracking ref
   - Must not be a bare repository
2. **Fetch**: All upstream changes are fetched, including tags. Moved tags
   are force-updated. Deleted remote branches are pruned locally.
3. **Rebase**: Local commits are replayed on top of the fetched upstream
   changes. Uncommitted working tree changes are preserved automatically.
   Merge topology is preserved and branch refs are kept in sync
   (`--update-refs`), so woven feature branches survive the rebase intact.
   Before rebasing, loom inspects each woven branch section:
   - If **all** commits in the section are now reachable from the new
     upstream (the branch was fully merged upstream), the entire section
     and its merge commit are removed. The integration branch fast-forwards
     cleanly to the new upstream tip.
   - If **some** commits in the section are reachable from the new upstream
     (partial integration), those commits are dropped from the section and
     the remaining commits are rebased onto the new upstream. A fresh merge
     commit is created (the original merge SHA is no longer valid once the
     branch content changes).
4. **Submodule update** (conditional): If the project uses submodules, they
   are initialized and updated recursively.
5. **Gone upstream cleanup**: Any local branches whose configured upstream
   tracking branch no longer exists (pruned by `--prune`) are listed and the
   user is prompted once to remove them. Use `--yes` to skip the prompt.

## Conflict Handling

When the rebase encounters a merge conflict:

1. The spinner stops with an error indicator
2. The state is saved to `.git/loom/state.json`
3. git-loom reports that the operation is paused and exits successfully

The user then resolves the conflict using standard git tools. Afterward:

- `loom continue` — continues the rebase, runs submodule update, reports the
  updated upstream, and proposes removal of gone-upstream branches
- `loom abort` — aborts the rebase and restores the original branch state

While the operation is paused, most other loom commands are blocked. Only
`loom show`, `loom trace`, `loom continue`, and `loom abort` are permitted.

## Prerequisites

- Must be in a git repository with a working tree (not bare)
- Current branch must have an upstream tracking ref (use `git-loom init` first)
- Git 2.38 or later (checked globally at startup)
- Network access to the remote (for `git fetch`)

## Examples

### Update with no local changes

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# > Rebased onto upstream
```

### Update with local commits (rebased on top)

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# > Rebased onto upstream
```

Local commits are replayed on top of the fetched upstream changes.

### Update with dirty working tree (autostashed)

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# > Rebased onto upstream
```

Uncommitted changes are automatically stashed before the rebase and restored
after.

### Update with submodules

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# > Rebased onto upstream
# > Updating submodules...
# > Updated submodules
```

### Already up to date

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# > Rebased onto upstream
```

### Error: merge conflict

```bash
git-loom update
# > Fetching latest changes...
# > Fetched latest changes
# > Rebasing onto upstream...
# x Rebase failed
# error: git rebase --autostash origin/main failed:
# CONFLICT (content): Merge conflict in file.txt
# ...
```

### Error: not on an integration branch

```bash
git checkout some-branch-without-upstream
git-loom update
# error: Branch 'some-branch-without-upstream' has no upstream tracking branch.
# Run 'git-loom init' to set up an integration branch.
```

### Error: detached HEAD

```bash
git checkout --detach HEAD
git-loom update
# error: HEAD is detached. Please switch to an integration branch.
```

## Design Decisions

### Full Upstream Synchronization

The fetch step synchronizes all upstream state, not just branch commits:

- **Tags** are force-updated so moved tags (e.g., release tags re-pointed
  after a hotfix) reflect the remote state
- **Deleted remote branches** are pruned locally, keeping the remote-tracking
  state clean

This provides a complete sync rather than a minimal one.

### Automatic Working Tree Preservation

Uncommitted changes are automatically preserved during the rebase. Users
don't need to manually stash before updating.

### No Arguments

The update command takes no arguments because it always operates on the current
branch's upstream. There is no ambiguity about what to update.
