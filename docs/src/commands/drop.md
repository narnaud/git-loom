# drop

Drop a commit, a branch, or local changes.

**Alias:** `rm`

## Usage

```
git loom drop [-y] <target>...
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>...` | A commit, a branch, files or directories (short IDs, names, hashes or paths), or `zz` for all local changes. Several files can be dropped at once. |

### Options

| Option | Description |
|--------|-------------|
| `-y, --yes` | Skip the confirmation prompt |

## What It Does

Every drop asks for confirmation first, listing what it will remove; `-y` skips the question.

### Commit

Removes the commit from history. A branch whose only commit it was is kept, empty, at the base, ready for a new `git loom commit -b`:

```bash
git loom drop pop
# Drop commit `4783c1b` Add login form, leaving branch `feature-a` empty? (y/N)
# ✓ Dropped commit `4783c1b`
#   › branch feature-a now empty, at the base
```

### Branch

Removes the branch and the commits it owns, and takes it out of the integration branch:

```bash
git loom drop feature-a
# Drop branch `feature-a` and its 3 commits? (y/N)
# ✓ Dropped branch `feature-a` and its 3 commits
```

An empty branch is deleted without asking. When the commits are shared, only the branch goes:

- **Co-located branches** (same tip): the commits stay with the other branch.
- **Stacked branches**: dropping the lower branch keeps its commits on the branch above. Dropping the upper one keeps the lower branch.

```bash
git loom drop feat1
# Drop branch `feat1`, keeping its commits on `feat2`? (y/N)
# ✓ Dropped branch `feat1`, its commits stay on `feat2`
```

### Files and directories

A modified file is restored to its committed version; a new or untracked file is deleted. A directory gets both: its changes are discarded and its untracked files deleted.

```bash
git loom drop ma untracked.txt
#   › restore `src/main.rs`
#   › delete `untracked.txt`
# Discard all selected changes and delete all selected files? (y/N)
# ✓ Restored `src/main.rs`
# ✓ Deleted `untracked.txt`
```

### `zz`

Discards all local changes, staged or not, and deletes untracked files. Ignored files are kept.

## Target Resolution

1. **Paths** of changed files or directories
2. **Branch names**
3. **Git revisions**: hashes, `HEAD`, …
4. **Short IDs** of branches, commits and files
5. **`zz`**

## Conflicts

Dropping a commit pauses on a conflict:

```bash
git loom drop pop
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the drop
#   › loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Dropped commit `4783c1b`
```

Dropping a branch rolls back on a conflict, leaving everything as it was. See [`continue`](continue.md) and [`abort`](abort.md).
