# Spec 017: Switch

## Overview

`git loom switch` lets you check out any branch — local or remote — for
quick inspection or testing, without weaving it into the integration branch.
Remote-only branches (those that exist on the remote but have no local
counterpart) detach HEAD at the remote ref rather than creating a tracking
branch. The command refuses to run when the working tree has staged or
unstaged changes to tracked files.

## Why Switch?

While working inside an integration branch, you sometimes need to briefly
inspect or test another branch — a colleague's remote branch, an upstream
fix, or a local feature branch — without permanently merging it into your
integration topology.

Raw git requires extra steps, especially for remote branches:

```bash
# To inspect a remote branch with no local counterpart:
git fetch origin
git switch -c colleague-work --track origin/colleague-work
# ... look around ...
git switch integration
git branch -D colleague-work   # manual cleanup
```

`git-loom switch` condenses this to a single command and leaves no local
branch behind when testing remote-only refs:

```bash
git-loom switch origin/colleague-work
# Detaches HEAD at the remote ref — no local branch created
```

For local branches, it is a one-step alternative to `git switch` that
integrates with loom's short ID system.

## CLI

```bash
git-loom switch [<branch>]
git-loom sw     [<branch>]       # alias
```

**Arguments:**

- `<branch>` *(optional)*: branch to switch to. Accepted forms:
  - Local branch name (e.g. `feature-x`)
  - Remote branch name (e.g. `origin/feature-x`)
  - Loom short ID for a woven branch (e.g. `fx`) — best-effort; see
    [Target Resolution](#target-resolution)
  - If omitted, shows an interactive picker listing all local branches and
    all remote-only branches

## What Happens

### When the Target is a Local Branch

HEAD moves to the named local branch. The branch pointer is not changed.

**What changes:**

- HEAD now points to the local branch (attached, not detached)

**What stays the same:**

- All branch refs and commit history
- The integration branch topology (the switch does not weave or unweave anything)
- The working tree (git refuses to switch if files conflict with the target)

Success message: `✓ Switched to <branch-name>`

### When the Target is a Remote-Only Branch

A remote-only branch is a remote-tracking ref (e.g. `origin/feature-x`)
with no local branch of the same short name. HEAD is detached at the
remote ref's commit. No local tracking branch is created.

**What changes:**

- HEAD is detached at the remote ref's OID

**What stays the same:**

- All local branch refs (no new branch is created)
- The integration branch topology
- The working tree

Success message: `✓ Detached HEAD at <remote/branch-name>`

To return to normal branch mode, run `git-loom switch <branch>` or
`git switch <branch>`.

### Interactive Picker (no argument)

When no branch name is provided, an interactive menu is shown with:

1. All local branches, except the current branch
2. All remote-only branches (remote-tracking refs that have no local
   counterpart, excluding `<remote>/HEAD` pointers)

Selecting a local branch switches as described above. Selecting a
remote-only branch detaches HEAD as described above.

If there are no branches to show (e.g. the repo has only the current
branch and no remotes), the command errors with
`"No branches available to switch to"`.

## Target Resolution

When a `<branch>` argument is supplied, resolution is attempted in this order:

1. **Local branch name** — exact match against local branches
2. **Remote branch name** — exact match against remote-tracking refs
   (e.g. `origin/feature-x`)
3. **Loom short ID** — best-effort lookup via the woven-branch graph
   (see Spec 002). This only succeeds when loom is on an integration
   branch with upstream tracking configured. If it fails (e.g. HEAD is
   detached or no upstream is set), it is silently skipped.

Short IDs resolve only to **local** branches (those woven into the
integration branch visible in `git-loom status`). To switch to a
remote-only branch, use its full remote-tracking name (e.g.
`origin/feature-x`).

If none of the above match, the command errors with
`"Branch '<name>' not found"`.

## Conflict Recovery

`switch` never performs a rebase, so it has no conflict recovery. There is
no `.git/loom/state.json` written. `switch` is blocked (like most commands)
when a loom operation is already paused — run `loom continue` or
`loom abort` first.

## Prerequisites

- Must be run inside a git repository with a working directory (not a bare
  repository).
- The working tree must be clean: no staged changes and no unstaged
  modifications to tracked files. Untracked (new) files are allowed.
  If dirty, the command errors with:
  ```
  Working tree has uncommitted changes.
  Stash or commit your changes before switching branches.
  ```
- `switch` is blocked while a loom operation is paused (state file exists).

## Examples

### Switch to a local feature branch

```
git-loom status
# ●  (upstream)
# │╮─ fx [feature-x]
# │●   a1b2 Add widget

git-loom switch feature-x
# ✓ Switched to `feature-x`
# HEAD is now on feature-x
```

### Switch using a short ID

```
git-loom status
# │╮─ fx [feature-x]

git-loom switch fx
# ✓ Switched to `feature-x`
```

### Inspect a remote-only branch (no local counterpart)

```
git fetch origin

git-loom switch origin/colleague-work
# ✓ Detached HEAD at `origin/colleague-work`
# No local branch is created.

# ... test, review ...

git-loom switch integration
# ✓ Switched to `integration`
```

### Switch back from detached HEAD

```
git-loom switch integration
# ✓ Switched to `integration`
```

## Design Decisions

### Detach HEAD for Remote-Only Branches Instead of Creating a Tracking Branch

When the target is a remote-only branch, loom detaches HEAD at the remote
ref rather than creating a local tracking branch (`git switch -c name
--track origin/name`).

Creating a local branch would require the user to clean it up manually
after testing, and it implies ongoing ownership (pushes, tracking status)
that is not intended for a temporary inspection. Detaching HEAD makes the
temporary intent explicit: you are looking at a commit, not claiming a
branch.

### Clean Working Tree Required

The command refuses to run when tracked files have staged or unstaged
changes. This prevents a silent loss of context: after switching and
switching back, locally-staged work might appear to be in a different
state relative to the new HEAD. Requiring a clean tree also matches the
mental model that `switch` is for observation, not for carrying work
across branches.

Untracked files are permitted because git itself allows switching with
untracked files as long as they do not conflict with the target branch.

### Short IDs Are Best-Effort

Short ID resolution requires loading the integration branch graph
(`gather_repo_info`), which in turn requires being on a branch with
upstream tracking configured. If the resolution fails for any reason
(detached HEAD, no upstream, not an integration branch), it is silently
skipped and the command falls through to a "not found" error.

This keeps `switch` usable in any repository state while still supporting
the convenient short-ID workflow when the full loom context is available.

### Remote-Only Branches Always Shown in Picker

The interactive picker always includes remote-only branches without
requiring a flag (contrast with `branch merge`, which requires `--all`).
For `switch`, the primary motivation is testing remote branches, so
hiding them by default would undermine the command's purpose.
