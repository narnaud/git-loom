# Spec 004: Weave

## Overview

The **Weave** is the heart of git-loom's history rewriting. It provides a
structured graph model of the integration branch topology and a set of pure
mutation operations. All commands that modify history (branch, commit, drop,
fold, reword) follow the same pattern:

1. **Build** a graph from the repository state
2. **Mutate** the graph (drop a commit, move a commit, fixup, etc.)
3. **Serialize** the graph to a rebase todo
4. **Execute** a single rebase with the pre-generated todo

git-loom generates the entire rebase todo from scratch based on the graph,
rather than parsing and patching git's generated todo. This makes the process
robust and predictable.

## Data Model

The Weave graph captures the integration branch topology as three components:

### Base

The merge-base commit between HEAD and the upstream — the root of the
integration range.

### Branch Sections

Each woven branch is represented as a section containing:

- **Reset target**: Where the section forks from (the base, or another section)
- **Commits**: The branch's commits in oldest-first order
- **Label**: A canonical name for the section
- **Branch names**: All branch refs at this tip (supports co-located branches
  where multiple branches point to the same commit)

### Integration Line

The first-parent line from the base to HEAD. Each entry is either:

- **A regular commit** (with optional branch refs for non-woven branches)
- **A merge point** referencing a branch section

For existing merge commits, the original merge message is preserved. For
newly created merges, git generates a default message.

### Commit Commands

Each commit in the graph carries a command that controls its behavior during
rebase:

- **Pick**: Replay the commit as-is (default)
- **Edit**: Pause the rebase at this commit (used by reword)
- **Fixup**: Absorb this commit's changes into the previous commit (used by fold)

## Building the Graph

The graph is constructed by walking the first-parent line from HEAD to the
merge-base:

- **Merge commits** identify woven branches: the second parent is followed
  to collect the branch's commits, branch refs are matched by tip, and a
  branch section + merge entry are created.
- **Regular commits** become entries on the integration line. If a branch
  ref points at a regular commit, it's recorded as a non-woven branch.
- **Empty branches** (tip at merge-base with no commits) are skipped.

Building the graph requires an integration branch with upstream tracking.
Commands that need to operate outside this context (e.g., reword on a
non-integration branch) fall back to a simpler linear approach.

## Mutations

All mutations are pure operations on the in-memory graph. They do not touch
the repository until the graph is serialized and executed.

### Drop Commit

Remove a commit from the graph. If it was the last commit in a branch
section, the section and its merge entry are also removed.

### Drop Branch

Remove an entire branch section and its merge entry.

### Move Commit

Move a commit to the tip of a target branch section. The commit is removed
from its current location and appended to the target.

**Co-located branch handling:** When the target branch shares a section with
other branches, the section is split. The original section keeps the remaining
branches and existing commits. A new stacked section is created for the target
branch containing only the moved commit. This ensures the moved commit appears
only on the target branch, not on all co-located branches.

### Fixup Commit

Remove a commit from its current location and insert it immediately after a
target commit with a fixup command. During rebase, the source's changes are
absorbed into the target.

### Edit Commit

Mark a commit so the rebase pauses there, allowing the user to amend it.

### Add Branch Section

Add a new branch section to the graph. Used when creating merge topology for
a branch that doesn't have a section yet (e.g., a newly created empty branch).

### Add Merge

Add a merge entry on the integration line, referencing a branch section.

### Weave Branch

Convert a non-woven branch (commits on the integration line) into a woven
branch. The commits are moved into a new branch section and a merge entry
is added.

### Reassign Branch

Reassign a branch section from one branch to another. Used when dropping a
co-located woven branch — the surviving branch inherits the section and
merge topology.

## Serialization

The graph is serialized to a git rebase todo file. The output follows git's
`--rebase-merges` format:

- Branch sections are emitted first, in dependency order
- Each section forks from its reset target and ends with a label
- The integration line follows, with merge entries referencing branch sections
- Existing merges preserve their original message
- New merges use git's default message
- Non-woven branch refs are tracked via update-ref directives

## Execution

The serialized todo is executed as a single native git interactive rebase.
Key behaviors:

- The full integration range (merge-base to HEAD) is replayed
- Merge topology is preserved and created via `--rebase-merges`
- All branch refs are kept up to date automatically
- Uncommitted working tree changes are preserved
- Empty commits are preserved
- On failure, the rebase is automatically aborted, leaving the repository
  in its original state

## Integration with Commands

| Command | Mutations used |
|---------|---------------|
| `branch` (Spec 005) | Weave branch |
| `commit` (Spec 006) | Add branch section + merge (empty branch), move commit |
| `drop` (Spec 008) | Drop commit, drop branch, reassign branch |
| `fold` (Spec 007) | Fixup commit, move commit |
| `reword` (Spec 003) | Edit commit |

Commands that don't modify history (`status`, `init`, `update`, `reword`
for branch rename) do not use the Weave.

## Design Decisions

### Generate Todo From Scratch

Rather than parsing git's generated todo file and applying text-level edits,
git-loom generates the entire todo from the commit graph. This eliminates
dependence on git's exact output format and makes operations composable —
multiple mutations can be applied to the graph before a single serialization.

### Always Rebase from Merge-Base

All operations scope the rebase from the merge-base commit, replaying the
full integration history. The trade-off is slightly slower for large branches,
but dramatically simpler — one graph covers the entire topology.

### Co-Located Branch Splitting

When moving a commit to a co-located branch (one that shares a section with
other branches), the section is split into a stacked topology. This ensures
the moved commit appears only on the target branch, not on all co-located
branches.

### Atomic Operations

If the rebase fails, it is automatically aborted, leaving the repository in
its original state. Either the operation succeeds completely or nothing changes.

### Fallback for Non-Integration Repos

`reword` is the only command that can operate outside an integration branch
context. When the full graph cannot be built, `reword` falls back to a
simpler linear approach that doesn't require the Weave data model.
