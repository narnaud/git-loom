# Spec 008: Drop

## Overview

`git loom drop` removes a commit or an entire branch from history. When
dropping a branch, it removes all the branch's commits, unweaves the merge
topology (if the branch was woven), and deletes the branch ref — all in a
single operation.

## Why Drop?

Removing commits or branches from an integration branch is a multi-step
process in raw git:

- Dropping a commit requires interactive rebase with manual `drop` editing
- Removing a woven branch requires understanding the merge topology, running
  an interactive rebase to remove the branch section and merge commit, then
  deleting the branch ref
- Getting it wrong can leave the repository in an inconsistent state

`git-loom drop` provides a single command that handles all cases:

- Direct: target a commit or branch by hash, name, or short ID
- Safe: uses native git rebase under the hood with automatic abort on failure
- Complete: for branches, handles topology cleanup and ref deletion atomically

## CLI

```bash
git-loom drop <target>
```

**Arguments:**

- `<target>`: A commit hash (full or partial), branch name, or short ID

**Behavior:**

- If `<target>` resolves to a commit: removes the commit from history
- If `<target>` resolves to a branch: removes all commits, unweaves merge
  topology, and deletes the branch ref
- If `<target>` resolves to a file: returns an error

## What Happens

### When Target is a Commit

The commit is removed from history via interactive rebase. All descendant
commits are replayed to maintain a consistent history.

**Special case — last commit on a branch:** If the commit is the only commit
owned by a branch, the operation automatically drops the entire branch instead.
This ensures the merge topology is properly cleaned up and the branch ref is
deleted, rather than leaving an empty branch section.

**What changes:**

- Target commit is removed from history
- All descendant commits get new hashes (same content/messages)
- Branch refs are updated automatically

**What stays the same:**

- All other commits' content and messages
- Commit topology (minus the removed commit)
- Branch refs not in the ancestry chain

### When Target is a Branch

The entire branch is removed: all commits owned by the branch are dropped,
the merge topology is unwoven, and the branch ref is deleted.

Five sub-cases are handled:

#### Branch at merge-base (no commits)

If the branch tip equals the merge-base, it has no owned commits. The branch
ref is simply deleted. No rebase is needed, and the working tree does not
need to be clean.

#### Woven branch (merged into integration via merge commit)

A branch is "woven" when its tip is NOT on the first-parent line from HEAD
to the merge-base — meaning it was merged into the integration branch via a
merge commit and lives on a side branch.

The branch section and its merge entry are removed from the integration
topology, and the branch ref is deleted. All of this happens in a single
atomic operation.

The branch must be in the integration range (between merge-base and HEAD).
Branches outside this range are rejected with: `"Branch '<name>' is not in
the integration range. Use 'git branch -d <name>' to delete it directly."`

#### Non-woven branch (on the first-parent line)

A branch is "non-woven" when its tip IS on the first-parent line — meaning
it was fast-forward merged or its commits sit directly on the integration
line without merge topology.

All commits owned by the branch are removed from history, and the branch
ref is deleted.

#### Co-located woven branch (shares tip with another branch)

Two or more branches are "co-located" when they point to the same tip commit.
When dropping a co-located woven branch, the commits are preserved for
the surviving sibling branch.

The section and merge topology are reassigned to the surviving branch.
The dropped branch ref is deleted, but no commits are removed. If multiple
co-located branches exist, the first one found (by branch order) becomes
the new section owner.

#### Co-located non-woven branch (shares tip, on first-parent line)

When dropping a co-located non-woven branch, the commits are shared with
the sibling branch. No commits are removed — only the branch ref is deleted.

**What changes (woven and non-woven, non-co-located):**

- All branch commits are removed from history
- Merge commit is removed (woven case)
- Branch ref is deleted
- Remaining commits get new hashes
- Other branch refs are updated automatically

**What changes (co-located):**

- Branch ref is deleted
- Merge topology is reassigned to sibling branch (woven case)
- No commits are removed (sibling branch still needs them)

**What stays the same:**

- Commits on other branches
- Other branch refs
- Integration line commits (e.g., commits made directly on integration)

## Target Resolution

The `<target>` is interpreted using the shared resolution strategy
(see Spec 002):

1. **Local branch names** — exact match resolves to a branch (drops the branch)
2. **Git references** — full/partial hashes, `HEAD`, etc. resolve to commits
3. **Short IDs** — branch short IDs resolve to branches, commit short IDs to
   commits

File targets are rejected with: `"Cannot drop a file. Use 'git restore' to
discard file changes."`

## Prerequisites

- Git 2.38 or later
- Must be in a git repository with a working tree (not bare)
- For branch drops: the branch must be in the integration range
- For short ID arguments: must have upstream tracking configured

## Examples

### Drop a commit by short ID

```bash
git-loom status
# Shows: │●  ab  72f9d3 Unwanted commit

git-loom drop ab
# Removes the commit from history
```

### Drop a commit by hash

```bash
git-loom drop abc123d
# Removes the commit from history
```

### Drop a woven branch

```bash
git-loom status
# Shows:
# │╭─ fa [feature-a]
# ││●  a1  ...  Add login form
# ││●  a2  ...  Add login validation
# │╰─── merge

git-loom drop feature-a
# Removes A1, A2, the merge commit, and deletes feature-a ref
```

### Drop a branch by short ID

```bash
git-loom drop fa
# Same as above, using the short ID for feature-a
```

### Drop a co-located branch (preserves sibling)

```bash
git-loom status
# Shows:
# │╭─ fa, fb [feature-a, feature-b]
# ││●  a1  ...  Shared commit
# │╰─── merge

git-loom drop feature-a
# Removes feature-a ref, reassigns section to feature-b
# Commits and merge topology are preserved for feature-b
```

### Drop the last commit on a branch (auto-deletes branch)

```bash
git-loom status
# Shows:
# │╭─ fa [feature-a]
# ││●  a1  ...  Only commit
# │╰─── merge

git-loom drop a1
# Detects this is the only commit on feature-a
# Drops the branch (commits + merge + ref) automatically
```

## Design Decisions

### Automatic Branch Cleanup

When dropping a commit that is the sole commit on a branch, the command
automatically removes the entire branch rather than leaving an empty branch
section. This was chosen because:

- An empty branch section with just a merge commit is useless
- The user's intent when dropping the last commit is clearly to remove the
  branch entirely
- Manual cleanup would require a separate `git branch -D` step

### Woven vs Non-Woven Strategy

Woven and non-woven branches are handled differently because they have
different topologies:

- Woven branches have a distinct merge topology (side branch + merge commit)
  that can be removed as a unit
- Non-woven branches have their commits inline on the integration line,
  so individual commit removal is the natural approach

### Branch Must Be in Integration Range

Dropping a branch that is not in the integration range (not between
merge-base and HEAD) is rejected with a helpful error suggesting
`git branch -d` instead. This was chosen because:

- `git loom drop` operates on the integration topology — branches outside
  the range are not part of the integration workflow
- Trying to rebase-drop commits from an unreachable branch would silently
  do nothing, which is confusing
- The error message guides users to the right tool

### Automatic Working Tree Preservation

All operations automatically preserve uncommitted changes in the working tree.
Users don't need to manually stash before dropping. Dropping a branch at the
merge-base (which only deletes the ref) works regardless of working tree state.

### Co-Located Branches Preserve Shared Commits

When dropping a branch that shares its tip with another branch (co-located),
the commits are preserved for the surviving branch. The surviving branch
transparently inherits the topology — no manual intervention required.

### Atomic Operations

All drop operations are atomic: either they complete fully or the repository
is left in its original state. The user is never left in a partially-applied
state that requires manual recovery.
