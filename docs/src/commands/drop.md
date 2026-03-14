# drop

Drop a commit, branch, file, or all local changes.

## Usage

```
git-loom drop [-y] <target>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | Commit hash, branch name, file short ID, or short ID |

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

### When Target is a File

Behavior depends on the file's status:

- **Tracked file with modifications** — `git restore --staged --worktree <path>`. Prompt: `"Discard changes to '<path>'?"`. Output: `"Restored '<path>'"`.
- **Staged new file** (`A` in index) — `git rm --force <path>`. Prompt: `"Delete '<path>'?"`. Output: `"Deleted '<path>'"`.
- **Untracked file** (`??`) — deleted from disk. Prompt: `"Delete '<path>'?"`. Output: `"Deleted '<path>'"`.

A confirmation prompt is shown first (skippable with `-y`).

### When Target is `zz` (all local changes)

Discards everything in the working tree and index:

1. `git restore --staged --worktree .` — reverts all tracked modifications
2. `git clean -fd` — deletes all untracked files and directories

If there are no local changes, the command errors with `"No local changes to discard"`.

## Target Resolution

1. **Branch names** — exact match resolves to a branch (drops the branch)
2. **Git references** — full/partial hashes resolve to commits
3. **Short IDs** — branch short IDs resolve to branches, commit short IDs to commits, file short IDs to files
4. **`zz`** — always resolves to all local changes

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

### Drop a file (discard changes)

```bash
git-loom drop ma
# Discard changes to `src/main.rs`? (y/n)
# Restored `src/main.rs`
```

### Drop a new or untracked file

```bash
git-loom drop nf
# Delete `new_feature.rs`? (y/n)
# Deleted `new_feature.rs`
```

### Drop all local changes

```bash
git-loom drop zz
# Discard all local changes? (y/n)
# Discarded all local changes
```

### Drop a co-located branch

```bash
git-loom drop feature-a
# Removes feature-a ref, reassigns section to sibling branch
# Commits preserved for the surviving branch
```

## Conflicts

**Dropping a commit** supports conflict recovery. If the rebase hits a conflict,
the operation is paused:

```bash
git-loom drop ab
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the drop
#   loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git-loom continue
# ✓ Dropped commit `ab`
```

**Dropping a branch** does not support pause/resume — if a conflict occurs it
aborts immediately and leaves the repository in its original state.

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Must be in a git repository with a working tree
- For branch drops: the branch must be in the integration range
- All operations are atomic and automatically preserve uncommitted changes
