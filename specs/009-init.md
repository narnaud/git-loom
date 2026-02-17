# Spec 009: Init

## Overview

`git loom init` creates a new integration branch that tracks a remote upstream.
It is the entry point for starting a git-loom workflow: set up a branch that
combines multiple feature branches, all tracked against a shared upstream.

## Why Init?

Starting a git-loom integration workflow requires:

- Creating a new local branch
- Setting up upstream tracking to a remote branch (e.g., `origin/main`)
- Switching to the new branch

`git-loom init` does all three in one step, with smart defaults:

- **Auto-detection**: Reads the upstream from the current branch, so you don't
  have to type `origin/main` manually
- **Sensible default name**: Uses `loom` if no name is provided
- **Prompt when ambiguous**: If multiple remotes or default branches exist,
  asks the user to choose

## CLI

```bash
git-loom init [name]
```

**Arguments:**

- `[name]`: Branch name (optional; defaults to `"loom"`)

**Behavior:**

- With `name`: creates the integration branch with that name
- Without `name`: creates a branch named `"loom"`
- The branch is created at the upstream tip and tracks it
- HEAD is switched to the new branch

## What Happens

1. **Name resolution**: Use the provided name or default to `"loom"`
2. **Validation**: Name is trimmed, checked for emptiness, validated via
   `git check-ref-format`, and checked for duplicates
3. **Upstream detection**: The upstream tracking ref is determined:
   - If the current branch has an upstream (e.g., `main` tracks `origin/main`),
     use that upstream
   - Otherwise, scan remotes for common default branches (`main`, `master`,
     `develop`)
   - If exactly one candidate is found, use it automatically
   - If multiple candidates exist, prompt the user to choose
   - If no candidates are found, error with guidance to add a remote
4. **Creation**: `git switch -c <name> --track <upstream>` creates the branch,
   sets up tracking, and switches to it in one operation

## Upstream Detection

### Strategy

The upstream is resolved in priority order:

1. **Current branch's upstream** — If you're on `main` tracking `origin/main`,
   the new integration branch will also track `origin/main`. This is the most
   common case.

2. **Remote scan** — If the current branch has no upstream (e.g., a detached
   HEAD or a branch without tracking), git-loom scans all remotes for branches
   named `main`, `master`, or `develop`.

3. **Interactive prompt** — If multiple candidates are found (e.g., both
   `origin/main` and `upstream/main`), the user is prompted to select one.

4. **Error** — If no remote tracking branches are found at all, an error
   message guides the user to set up a remote.

## Prerequisites

- Must be in a git repository with a working tree (not bare)
- At least one remote with a fetchable branch must be configured
- Git 2.38 or later (checked globally at startup)

## Name Validation

Branch names are validated in three steps:

1. **Empty check**: Rejects empty or whitespace-only names
2. **Git format check**: Uses `git check-ref-format --branch` to validate
   against git's naming rules
3. **Duplicate check**: Rejects names that match existing local branches

## Examples

### Initialize with defaults

```bash
git-loom init
# Initialized integration branch 'loom' tracking origin/main
```

### Initialize with a custom name

```bash
git-loom init my-integration
# Initialized integration branch 'my-integration' tracking origin/main
```

### Initialize when current branch has no upstream

```bash
git checkout --detach HEAD
git-loom init
# (scans remotes, finds origin/main)
# Initialized integration branch 'loom' tracking origin/main
```

### Error: no remotes configured

```bash
git-loom init
# error: No remote tracking branches found.
# Set up a remote with: git remote add origin <url>
```

### Error: branch already exists

```bash
git-loom init loom
# (first time succeeds)
git-loom init loom
# error: Branch 'loom' already exists
```

## Architecture

### Module: `init.rs`

The init command is a thin orchestration layer:

```
init::run(name)
    |
    v
name resolution (default to "loom", trim, empty check)
    |
    v
git_branch::validate_name(name)
    |
    v
duplicate check (repo.find_branch)
    |
    v
detect_upstream(repo)
    |
    v
match upstream detection:
    Current branch has upstream -> use it
    One candidate found         -> use it
    Multiple candidates         -> prompt user
    No candidates               -> error
    |
    v
git_branch::switch_create_tracking(workdir, name, upstream)
    |
    v
print success message
```

### Module: `git_commands/git_branch.rs`

New function added:

- **`switch_create_tracking(workdir, name, upstream)`** — Wraps
  `git switch -c <name> --track <upstream>`

## Design Decisions

### Default Name: "loom"

The default name `"loom"` was chosen because:

- It's short and memorable
- It clearly associates the branch with the git-loom tool
- It avoids conflicts with common branch names like `main`, `master`, `develop`

### Auto-Detection Over Explicit Arguments

Rather than requiring `git-loom init --track origin/main`, the command
auto-detects the upstream. This reduces friction in the common case (where
the user is on `main` tracking `origin/main`) while still handling edge
cases through prompting.

### Single Git Command

Using `git switch -c <name> --track <upstream>` handles branch creation,
tracking setup, and checkout in one atomic operation. This avoids partial
states where the branch exists but isn't checked out or doesn't have
tracking configured.
