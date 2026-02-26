# reword

Reword a commit message or rename a branch.

## Usage

```
git-loom reword <target> [-m <message>]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | Commit hash, branch name, or short ID |

### Options

| Option | Description |
|--------|-------------|
| `-m, --message <message>` | New commit message or branch name. Opens editor/prompt if omitted. |

## What It Does

### When Target is a Commit

Changes the commit message using git's native interactive rebase. All descendant commits are replayed to update their hashes.

- Works on any commit in history, including the root commit
- With `-m`: applies the new message non-interactively
- Without `-m`: opens the git editor with the current message

**What changes:** target commit gets a new message and hash; all descendant commits get new hashes.

**What stays the same:** commit content (files, diffs), topology, and branches outside the ancestry chain.

### When Target is a Branch

Renames the branch using `git branch -m`.

- With `-m`: renames non-interactively
- Without `-m`: interactive prompt showing current name as placeholder

## Target Resolution

The target is resolved in this order:

1. **Branch names** — exact match resolves to a branch (for renaming)
2. **Git references** — full/partial hashes, `HEAD`, etc. resolve to commits
3. **Short IDs** — branch short IDs resolve to branches, commit short IDs to commits

To reword the commit at a branch tip, use its commit hash or commit short ID (not the branch name, which would trigger a rename).

## Examples

### Reword a commit with editor

```bash
git-loom reword ab
# Opens editor with current message
```

### Reword a commit directly

```bash
git-loom reword ab -m "Fix authentication bug in login flow"
```

### Rename a branch interactively

```bash
git-loom reword feature-a
# ? New branch name › feature-a
# User types: feature-authentication
```

### Rename a branch directly

```bash
git-loom reword fa -m feature-authentication
```

## Prerequisites

- Any git repository for commit rewording
- For short IDs: must be on a branch with upstream tracking configured
