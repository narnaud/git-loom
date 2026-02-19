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

- **Fetch** with `--tags --force --prune` to pull all refs, force-update moved
  tags, and remove stale remote-tracking branches
- **Rebase** with `--autostash` onto the upstream tracking branch
- **Submodule sync** automatically when the project uses submodules
- **Visual feedback** via spinners during potentially slow network operations

## CLI

```bash
git-loom update
```

**Arguments:** None.

**Behavior:**

- Validates the current branch is an integration branch (has upstream tracking)
- Runs `git fetch --tags --force --prune`
- Runs `git rebase --autostash <upstream>`
- If `.gitmodules` exists, runs `git submodule update --init --recursive`
- On conflict, reports the error and lets the user resolve manually

## What Happens

1. **Repository discovery**: Find the git repository from the current directory
2. **Validation**:
   - HEAD must be on a branch (not detached)
   - The current branch must have an upstream tracking ref
   - Must not be a bare repository
3. **Fetch**: `git fetch --tags --force --prune`
   - `--tags` fetches all tags from the remote
   - `--force` updates local tags even when the remote tag has been moved
     (e.g., release tags that are force-pushed)
   - `--prune` removes local remote-tracking refs for branches that have been
     deleted on the remote, keeping the local state clean
4. **Rebase**: `git rebase --autostash <upstream>`
   - Replays local commits on top of the fetched upstream changes
   - `--autostash` stashes dirty working tree changes before rebase and
     restores them after
5. **Submodule update** (conditional): If `.gitmodules` exists in the working
   directory, run `git submodule update --init --recursive`

## Conflict Handling

When the rebase encounters a merge conflict:

1. The spinner stops with an error indicator
2. The git error output (stderr) is displayed
3. git-loom exits with a non-zero status

The user then resolves the conflict using standard git commands:

```bash
# Fix conflicts in files
git add <resolved-files>
git rebase --continue

# Or abort the rebase
git rebase --abort
```

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

## Architecture

### Module: `update.rs`

The update command is a straightforward orchestration layer:

```
update::run()
    |
    v
Repository::discover(cwd)
    |
    v
validate HEAD is on a branch
    |
    v
validate branch has upstream tracking ref
    |
    v
capture upstream ref name (e.g., "origin/main")
    |
    v
get workdir (reject bare repos)
    |
    v
[spinner] git fetch --tags --force --prune
    |
    v
[spinner] git rebase --autostash <upstream>
    |
    v
if .gitmodules exists:
    [spinner] git submodule update --init --recursive
    |
    v
done
```

### Dependencies

- **`git_commands::run_git`** for executing git shell commands
- **`cliclack::spinner`** for visual progress feedback
- **`git2`** for repository discovery and upstream validation

## Design Decisions

### Explicit Fetch + Rebase Over `git pull`

The update command uses an explicit `git fetch` followed by `git rebase` rather
than `git pull --rebase`. This separation allows the fetch step to include
`--tags --force --prune` flags that `git pull` does not support in a single
invocation. The two-step approach is functionally equivalent to `git pull
--rebase` but with richer fetch behavior.

### `--tags --force` for Tag Synchronization

Tags are fetched with `--force` so that tags which have been moved on the remote
(e.g., release tags that get re-pointed after a hotfix) are updated locally.
Without `--force`, git would skip tags whose names already exist locally, leaving
stale tag refs.

### `--prune` for Clean Remote-Tracking State

The `--prune` flag removes local remote-tracking refs (e.g., `origin/feature-x`)
for branches that have been deleted on the remote. This keeps the local
repository's remote-tracking state clean and avoids confusion when listing
branches.

### Plain `--rebase` (not `--rebase=merges`)

The update uses a plain rebase rather than `--rebase=merges`. This keeps
the rebase operation simple and predictable.

### `--autostash` Over Clean Tree Requirement

Unlike other git-loom commands that require a clean working tree, update uses
`--autostash` to automatically stash and restore uncommitted changes. This
reduces friction since updating is a common operation that shouldn't require
the user to manually stash their work first.

### Captured Output With Spinner

Git output is captured (not streamed to the terminal) to keep the UI clean.
A cliclack spinner provides visual feedback during potentially slow network
operations. On failure, the captured stderr is included in the error message.

### Submodule Detection Via `.gitmodules`

The presence of `.gitmodules` in the working directory is used as a simple
proxy for whether submodules need updating. This avoids the overhead of
programmatic submodule enumeration and is correct for the common case.
`git submodule update` is a no-op when no submodules are configured.

### No Arguments

The update command takes no arguments because it always operates on the current
branch's upstream. There is no ambiguity about what to update.
