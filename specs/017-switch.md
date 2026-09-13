# Spec 017: Switch

This specification is normative.

## Overview

`git loom switch` lets you check out any branch — local or remote — for
quick inspection or testing, without weaving it into the integration branch.
Remote-only branches (those that exist on the remote but have no local
counterpart) detach HEAD at the remote ref rather than creating a tracking
branch. The command refuses to run when the working tree has staged or
unstaged changes to tracked files.

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

## Required Behavior

### When the Target is a Local Branch

HEAD moves to the named local branch. The branch pointer is not changed.

HEAD becomes attached to the named local branch. No branch ref or history is
rewritten, and nothing is woven or unwoven. Git still rejects worktree files
that conflict with the target. Success: `✓ Switched to <branch-name>`.

### When the Target is a Remote-Only Branch

A remote-only branch is a remote-tracking ref (e.g. `origin/feature-x`)
with no local branch of the same short name. HEAD is detached at the
remote ref's commit. No local tracking branch is created.

HEAD is detached at the remote ref's OID. No local branch is created; refs,
history, and integration topology are unchanged. Success:
`✓ Detached HEAD at <remote/branch-name>`.

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

## Example

```bash
git-loom switch origin/colleague-work
git-loom switch integration
```

The first command detaches without creating a local branch; the second
reattaches to the local `integration` branch.
