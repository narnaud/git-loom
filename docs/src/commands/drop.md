# drop

Drop a commit, branch, file, or all local changes.

## Usage

```
git loom drop [-y] <target>...
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>...` | Commit hash, branch name, file, short ID, or `zz`; several files at once |

### Options

| Option | Description |
|--------|-------------|
| `-y, --yes` | Skip confirmation prompt |

## What It Does

### When Target is a Commit

Removes the commit from history. All descendant commits are replayed to maintain a consistent history.

Dropping a commit never deletes a branch. If the commit was the **only** commit of a branch, that branch survives, empty, at the base it built on — unwoven, and ready for `git loom commit -b <branch>` once the change is reworked. This holds for a branch that owns the commit, several branches at the same sole commit, and a stacked branch whose only commit it is; the prompt and the result name them.

```bash
git loom drop osy
# Drop commit `4783c1b` Add login form, leaving branch `feature-a` empty? (y/n)
# ✓ Dropped commit `4783c1b`
#   › branch feature-a now empty, at the base
```

To remove the branch as well, drop the branch itself — that takes its commits with it in one command:

```bash
git loom drop feature-a
```

### When Target is a Branch

Removes the entire branch in a single operation:

- Commits the branch owns are removed; a commit the integration line also carries stays
- The merge topology is unwoven (if the branch was woven)
- The branch ref is deleted

The result says how many commits went with the branch, so a `-y` drop reports
its size too:

```bash
git loom drop feature-a
# Drop branch `feature-a` and its 3 commits? (y/n)
# ✓ Dropped branch `feature-a` and its 3 commits
```

An **empty branch** (tip at the merge-base) is deleted without a prompt — it has
no commit to lose:

```bash
git loom drop feature-b
# ✓ Dropped empty branch `feature-b`
```

**Co-located branches** (sharing the same tip commit with another branch): only the branch ref is deleted. Commits are preserved for the surviving sibling branch, and the merge topology is reassigned.

**Stacked branches**: dropping the inner branch of a woven stack only deletes its ref, since the outer branch already carries its commits. Dropping the outer branch removes its own commits and keeps the inner branch. A stack that is not woven sits on the integration line, so dropping its inner branch removes that branch's commits like any other non-woven branch.

```bash
git loom drop feat1
# Drop branch `feat1`, keeping its commits on `feat2`? (y/n)
# ✓ Dropped branch `feat1`, its commits stay on `feat2`
```

### When Target is a File

Behavior depends on the file's status:

- **Tracked file with modifications** — `git restore --staged --worktree <path>`. Prompt: `"Discard changes to '<path>'?"`. Output: `"Restored '<path>'"`.
- **Staged new file** (`A` in index) — `git rm --force <path>`. Prompt: `"Delete '<path>'?"`. Output: `"Deleted '<path>'"`.
- **Untracked file** (`??`) — deleted from disk. Prompt: `"Delete '<path>'?"`. Output: `"Deleted '<path>'"`.

A confirmation prompt is shown first (skippable with `-y`).

Several files can be dropped in one command. They share a single prompt that lists what gets restored and what gets deleted; anything other than files cannot be combined:

```bash
git loom drop ma untracked.txt
#   › restore `src/main.rs`
#   › delete `untracked.txt`
# Discard all selected changes and delete all selected files? (y/N)
# ✓ Restored `src/main.rs`
# ✓ Deleted `untracked.txt`
```

### When Target is `zz` (all local changes)

Discards everything in the working tree and index, after a prompt that lists every path it will restore or delete:

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
git loom drop osy
# Removes the commit from history
```

### Drop a commit by hash

```bash
git loom drop abc123d
# Removes the commit from history
```

### Drop a branch

```bash
git loom drop feature-a
# Removes all commits, unweaves merge topology, deletes branch ref
```

### Drop a branch by short ID

```bash
git loom drop fa
# Same as above, using the short ID
```

### Drop a file (discard changes)

```bash
git loom drop ma
# Discard changes to `src/main.rs`? (y/n)
# Restored `src/main.rs`
```

### Drop a new or untracked file

```bash
git loom drop nf
# Delete `new_feature.rs`? (y/n)
# Deleted `new_feature.rs`
```

### Drop all local changes

```bash
git loom drop zz
# Discard all local changes? (y/n)
# Discarded all local changes
```

### Drop a co-located branch

```bash
git loom drop feature-a
# Drop branch `feature-a`, keeping its commits on `feature-b`? (y/n)
# ✓ Dropped branch `feature-a`, its commits stay on `feature-b`
```

## Conflicts

**Dropping a commit** supports conflict recovery. If the rebase hits a conflict,
the operation is paused:

```bash
git loom drop osy
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the drop
#   loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Dropped commit `4783c1b`
```

**Dropping a branch** does not support pause/resume — if a conflict occurs it
aborts immediately and leaves the repository in its original state.

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Must be in a git repository with a working tree
- For branch drops: the branch must be in the integration range
- All operations are atomic and automatically preserve uncommitted changes
