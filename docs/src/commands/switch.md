# switch

Check out any branch for testing without weaving it into the integration branch.

## Usage

```
git-loom switch [<branch>]
git-loom sw     [<branch>]
```

If no branch is given, an interactive picker lists all local branches and all remote-only branches.

### Arguments

| Argument | Description |
|----------|-------------|
| `<branch>` | Branch to switch to: local branch name, remote-tracking name (e.g. `origin/feature-x`), or short ID of a woven branch. Optional — omit to pick interactively. |

## What It Does

### When Target is a Local Branch

HEAD moves to the named local branch (attached, not detached). No branch refs or commit history are changed.

### When Target is a Remote-Only Branch

A remote-only branch is a remote-tracking ref (e.g. `origin/colleague-work`) with no local counterpart. HEAD is detached at that ref's commit. No local tracking branch is created, so there is nothing to clean up afterward.

### Interactive Picker (no argument)

Shows all local branches except the current one, followed by all remote-only branches. Selecting a local branch switches normally; selecting a remote-only branch detaches HEAD.

## Target Resolution

1. **Local branch name** — exact match against local branches
2. **Remote branch name** — exact match against remote-tracking refs (e.g. `origin/feature-x`)
3. **Short ID** — best-effort lookup via the woven-branch graph (requires being on an integration branch with upstream tracking configured; silently skipped otherwise)

## Examples

### Switch to a local branch

```bash
git-loom switch feature-x
# ✓ Switched to `feature-x`
```

### Switch using a short ID

```bash
git-loom switch fx
# ✓ Switched to `feature-x`
```

### Inspect a remote-only branch

```bash
git-loom switch origin/colleague-work
# ✓ Detached HEAD at `origin/colleague-work`
# No local branch is created.
```

### Return to the integration branch

```bash
git-loom switch integration
# ✓ Switched to `integration`
```

## Prerequisites

- Must be in a git repository with a working tree (not bare)
- Working tree must be clean: no staged changes and no unstaged modifications to tracked files (untracked files are allowed)
- Blocked while a loom operation is paused — run [`continue`](continue.md) or [`abort`](abort.md) first
