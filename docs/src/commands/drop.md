# drop

Drop a commit or an entire branch from history.

## Usage

```
git-loom drop [-y] <target>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | Commit hash, branch name, or short ID |

### Options

| Option | Description |
|--------|-------------|
| `-y, --yes` | Skip confirmation prompt |

## What It Does

### When Target is a Commit

Removes the commit from history. All descendant commits are replayed to maintain a consistent history.

If the commit is the **only** commit on a branch, the entire branch is dropped automatically (commits removed, merge topology unwoven, branch ref deleted).

### When Target is a Branch

Removes the entire branch in a single operation:

- All commits owned by the branch are removed
- The merge topology is unwoven (if the branch was woven)
- The branch ref is deleted

**Co-located branches** (sharing the same tip commit with another branch): only the branch ref is deleted. Commits are preserved for the surviving sibling branch, and the merge topology is reassigned.

## Target Resolution

1. **Branch names** — exact match resolves to a branch (drops the branch)
2. **Git references** — full/partial hashes resolve to commits
3. **Short IDs** — branch short IDs resolve to branches, commit short IDs to commits

File targets are rejected.

## Examples

### Drop a commit by short ID

```bash
git-loom drop ab
# Removes the commit from history
```

### Drop a commit by hash

```bash
git-loom drop abc123d
# Removes the commit from history
```

### Drop a branch

```bash
git-loom drop feature-a
# Removes all commits, unweaves merge topology, deletes branch ref
```

### Drop a branch by short ID

```bash
git-loom drop fa
# Same as above, using the short ID
```

### Drop a co-located branch

```bash
git-loom drop feature-a
# Removes feature-a ref, reassigns section to sibling branch
# Commits preserved for the surviving branch
```

## Prerequisites

- Must be in a git repository with a working tree
- For branch drops: the branch must be in the integration range
- All operations are atomic and automatically preserve uncommitted changes
