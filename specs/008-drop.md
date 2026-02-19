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
owned by a branch, the operation automatically delegates to the branch-drop
path. This ensures the merge topology is properly cleaned up and the branch
ref is deleted, rather than leaving an empty branch section in the integration
topology.

**Steps:**

1. Resolve target as a commit.
2. Check if the commit is the sole commit on a branch (via branch ownership
   and owned-commit count). If so, delegate to branch drop.
3. Otherwise, use interactive rebase (with `--autostash`) with a sequence
   editor that removes the `pick <hash>` line from the todo.

**What changes:**

- Target commit is removed from history
- All descendant commits get new hashes (same content/messages)
- Branch refs are updated via `--update-refs`

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
ref is simply deleted with `git branch -D`. No rebase is needed, and the
working tree does not need to be clean.

#### Woven branch (merged into integration via merge commit)

A branch is "woven" when its tip is NOT on the first-parent line from HEAD
to the merge-base — meaning it was merged into the integration branch via a
merge commit and lives on a side branch.

**Steps:**

1. Resolve target as a branch name.
2. Verify the branch is in the integration range (between merge-base and HEAD).
   Error: `"Branch '<name>' is not in the integration range. Use 'git branch -d
   <name>' to delete it directly."`
3. Use interactive rebase (with `--autostash`) with the `DropBranch` action,
   which removes:
   - The `reset` line that opens the branch section
   - All `pick` lines in the section (the branch's commits)
   - The `label <branch>` line
   - The `update-ref refs/heads/<branch>` line
   - The `merge ... <branch>` line (the merge commit)
4. Delete the branch ref with `git branch -D`.

#### Non-woven branch (on the first-parent line)

A branch is "non-woven" when its tip IS on the first-parent line — meaning
it was fast-forward merged or its commits sit directly on the integration
line without merge topology.

**Steps:**

1. Same validation as woven branch.
2. Determine all commits owned by the branch (from tip to the next branch
   boundary or merge-base).
3. Use interactive rebase with individual `Drop` actions for each owned commit.
4. Delete the branch ref.

#### Co-located woven branch (shares tip with another branch)

Two or more branches are "co-located" when they point to the same tip commit.
When dropping a co-located woven branch, the commits must be preserved for
the surviving sibling branch.

**Steps:**

1. Same validation as woven branch.
2. Detect that another branch shares the same tip.
3. Use interactive rebase with the `ReassignBranch` action, which:
   - Renames `label <drop-branch>` to `label <keep-branch>`
   - Removes `update-ref refs/heads/<drop-branch>`
   - Renames `merge ... <drop-branch>` to `merge ... <keep-branch>`
4. Delete the branch ref with `git branch -D`.

The surviving branch inherits the section and merge topology. If multiple
co-located branches exist, the first one found (by branch order) becomes
the new section owner.

#### Co-located non-woven branch (shares tip, on first-parent line)

When dropping a co-located non-woven branch, the commits are shared with
the sibling branch. `find_owned_commits()` correctly returns zero owned
commits (because the sibling's tip is hidden in the revwalk), so no rebase
is needed.

**Steps:**

1. Same validation as non-woven branch.
2. `find_owned_commits()` returns an empty set (sibling branch hides the
   shared commits).
3. Skip the rebase entirely.
4. Delete the branch ref with `git branch -D`.

**What changes (woven and non-woven, non-co-located):**

- All branch commits are removed from history
- Merge commit is removed (woven case)
- Branch ref is deleted
- Remaining commits get new hashes
- Other branch refs are updated via `--update-refs`

**What changes (co-located):**

- Branch ref is deleted
- Merge topology is reassigned to sibling branch (woven case)
- No commits are removed (sibling branch still needs them)

**What stays the same:**

- Commits on other branches
- Other branch refs
- Integration line commits (e.g., commits made directly on integration)

## Target Resolution

The `<target>` is interpreted using the shared `resolve_target()` function
(see Spec 002):

1. **Local branch names** — exact match resolves to a branch (drops the branch)
2. **Git references** — full/partial hashes, `HEAD`, etc. resolve to commits
3. **Short IDs** — branch short IDs resolve to branches, commit short IDs to
   commits

File targets are rejected with: `"Cannot drop a file. Use 'git restore' to
discard file changes."`

## Prerequisites

- Git 2.38 or later (for `--update-refs` during rebases)
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

## Architecture

### Module: `drop.rs`

The drop command dispatches based on the resolved target type:

```
drop::run(target)
    ↓
git::resolve_target(repo, target) → Target enum
    ↓
match Target:
    Commit(hash) → drop_commit(repo, hash)
        ↓
        check if only commit on a branch → delegate to drop_branch
        otherwise → Rebase with Drop action
    Branch(name) → drop_branch(repo, name)
        ↓
        at merge-base → just delete ref
        co-located woven → Rebase with ReassignBranch action + delete ref
        woven → Rebase with DropBranch action + delete ref
        co-located non-woven → just delete ref (no rebase, 0 owned commits)
        non-woven → Rebase with multiple Drop actions + delete ref
    File(_) → error
```

**Key integration points:**

- **`git::resolve_target()`** — Shared resolution logic (Spec 002)
- **`git::gather_repo_info()`** — Branch discovery and ownership analysis
- **`git::require_workdir()`** — Working directory validation
- **`git::head_oid()`** — HEAD resolution
- **`git::rebase_target_for_commit()`** — Rebase target determination
- **`branch::is_on_first_parent_line()`** — Woven vs non-woven detection
- **`git_rebase` module** — Interactive rebase for commit/branch removal
- **`git_branch::delete()`** — Branch ref deletion
- **Self-as-sequence-editor** (Spec 004) — Non-interactive rebase control

### Branch Ownership

To determine which branch owns a given commit (used for the "last commit on
branch" auto-delete feature), `find_branch_owning_commit_from_info()` walks
from each branch tip along parent links using the pre-gathered `RepoInfo`.
The walk stops at another branch's tip or the edge of the commit range.

### Owned Commits

`find_owned_commits()` uses a git revwalk from the branch tip to the
merge-base, hiding other branch tips that are ancestors **or co-located**
(sharing the same tip). The function takes the dropping branch's name so
it can correctly identify co-located siblings — branches with the same
`tip_oid` but a different name are hidden in the revwalk. This produces the
set of commits uniquely owned by the branch, excluding merge commits. For
co-located branches, this set is empty (all commits are shared).

### Sequence Editor Extensions

The `internal-sequence-edit` command (Spec 004) supports three actions for drop:

- **`Drop { short_hash }`**: removes the `pick <hash>` line from the rebase
  todo, causing git to skip that commit
- **`DropBranch { branch_name }`**: removes the entire branch section (reset,
  picks, label, update-ref) and the corresponding merge line from the rebase
  todo
- **`ReassignBranch { drop_branch, keep_branch }`**: renames the section's
  label and merge line from the dropped branch to the surviving co-located
  branch, removes the dropped branch's `update-ref`, and preserves all commits

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

Woven branches use `DropBranch` (removes the entire section atomically) while
non-woven branches use individual `Drop` actions. This was chosen because:

- Woven branches have a well-defined section structure in the rebase todo
  (reset/picks/label/update-ref/merge) that can be removed as a unit
- Non-woven branches have their commits inline on the integration line,
  so individual drop is the natural approach
- Using `DropBranch` for non-woven branches would fail since there is no
  branch section to find

### Branch Must Be in Integration Range

Dropping a branch that is not in the integration range (not between
merge-base and HEAD) is rejected with a helpful error suggesting
`git branch -d` instead. This was chosen because:

- `git loom drop` operates on the integration topology — branches outside
  the range are not part of the integration workflow
- Trying to rebase-drop commits from an unreachable branch would silently
  do nothing, which is confusing
- The error message guides users to the right tool

### Autostash Over Clean Working Tree Requirement

All rebase-based drop operations use `--autostash` to transparently stash and
restore uncommitted changes. This reduces friction — users don't need to
manually stash before dropping. Dropping a branch at the merge-base (which
only deletes the ref, no rebase needed) works regardless of working tree state.

### Co-Located Branches Preserve Shared Commits

When dropping a branch that shares its tip with another branch (co-located),
the commits are preserved for the surviving branch. This was chosen because:

- The commits are shared — removing them would break the sibling branch
- For the non-woven case, `find_owned_commits()` naturally returns zero
  commits when the sibling's tip is hidden, so no rebase is needed
- For the woven case, `ReassignBranch` renames the section to the sibling
  instead of removing it, keeping the merge topology intact
- The surviving branch transparently inherits the section — no manual
  intervention required

### Rebase from Merge-Base

Both woven and non-woven branch drops rebase from the merge-base (not from
the commit being dropped). This ensures the entire integration topology is
replayed correctly, and `--update-refs` can update all affected branch refs.
