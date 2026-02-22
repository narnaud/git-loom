# Spec 007: Fold

## Overview

`git loom fold` is a polymorphic command that combines a source into a target.
The action performed depends on the types of the arguments, detected
automatically via the shared resolution system (short IDs, filenames, commit
hashes, branch names).

## Why Fold?

Working with stacked branches and integration workflows requires frequent
small structural operations:

- "This file change should have been in that commit"
- "This fixup commit should be folded into the original"
- "This commit belongs on a different branch"

Each of these is doable with raw git, but requires different multi-step
incantations (interactive rebase with fixup, cherry-pick + rebase to remove,
etc.). `fold` unifies them under one verb: **fold source into target**.

## CLI

```bash
git-loom fold <source>... <target>
```

**Arguments:**

- `<source>...`: One or more sources to fold into the target. Sources can be
  filenames, commit hashes, partial hashes, short IDs, or branch names.
- `<target>`: The target to fold into. Can be a commit (hash, partial hash,
  short ID) or a branch name (full name or short ID).

The last argument is always the target. All preceding arguments are sources.
There must be at least two arguments total (one source + one target).

## Type Dispatch

The command resolves each argument's type, then dispatches based on the
combination:

| Source(s) | Target | Action | Multi-source? |
|-----------|--------|--------|---------------|
| File(s) | Commit | Amend: stage files into the commit | Yes |
| Commit | Commit | Fixup: absorb source into target | No |
| Commit | Branch | Move: relocate commit to the branch | No |
| Commit | Unstaged (`zz`) | Uncommit: remove commit, put changes in working directory | No |

**Invalid combinations** produce an error:

- File + Branch: `"Cannot fold files into a branch. Target a specific commit."`
- Branch + anything: `"Cannot fold a branch. Use 'git loom branch' for branch operations."`
- Unstaged + anything: `"Cannot fold unstaged changes. Stage files first, or use 'git loom fold <file> <commit>' to amend specific files."`
- File + Unstaged: `"Cannot fold files into unstaged — files are already in the working directory."`
- Mixed files and commits as sources: `"Cannot mix file and commit sources."`
- Multiple commit sources: `"Only one commit source is allowed."`

## What Happens

### Case 1: File(s) + Commit (Amend)

Folds file changes into an existing commit, effectively amending that commit
to include the current working tree changes for the specified files.

**Behavior:**

- The specified files must have changes (staged or unstaged).
  Error if a file has no changes: `"File '<path>' has no changes to fold."`
- Works for any commit in history, including HEAD.
- Uncommitted changes in other files are preserved automatically.

**What changes:**

- The target commit absorbs the file changes (new hash)
- All descendant commits get new hashes (same content/messages)

### Case 2: Commit + Commit (Fixup)

Folds the source commit into the target commit. The source commit's changes
are absorbed into the target, and the source commit disappears from history.
The target commit keeps its original message.

**Behavior:**

- The source must be a descendant of the target (source is newer).
  Error if not: `"Source commit must be newer than target commit."`
- The operation is atomic: either it completes fully or the repository is
  left unchanged.
- Uncommitted changes are preserved automatically.

**What changes:**

- Target commit absorbs source's changes (new hash)
- Source commit is removed from history
- All commits after the target get new hashes

**What stays the same:**

- Target commit's message
- Commit topology (minus the removed commit)
- Other branches not in the ancestry chain

### Case 3: Commit + Branch (Move)

Moves a commit from its current position to the tip of the target branch.
The commit is removed from its source branch and relocated in a single rebase
operation.

**Behavior:**

- The commit is removed from its source branch and appended to the target
  branch's tip in a single atomic operation.
- Both source and target branch refs are updated automatically.
- If the target branch shares its tip with other co-located branches, only
  the target branch advances; co-located branches remain unaffected.
- Uncommitted changes are preserved automatically.

**Conflict handling:**

- If the operation encounters conflicts, it stops and lets the user resolve
  them with standard git tools.

**What changes:**

- The commit moves to the target branch's section (new hash)
- The commit is removed from its original branch
- Affected commits in both branches get new hashes
- Branch refs are updated automatically

### Case 4: Commit + Unstaged (`zz`) (Uncommit)

Uncommits a commit, removing it from history and placing its changes in the
working directory as unstaged modifications. The target is specified using
`zz`, the reserved short ID for the unstaged working directory.

**Behavior:**

- **HEAD commit**: Performs a mixed reset (`git reset HEAD~1`), which moves
  HEAD back one commit and leaves the commit's changes as unstaged
  modifications.
- **Non-HEAD commit**: Captures the commit's diff, drops the commit from
  history via Weave rebase, then applies the diff to the working directory.
- Uncommitted changes in other files are preserved automatically.

**What changes:**

- The target commit is removed from history
- The commit's changes appear in the working directory as unstaged modifications
- All descendant commits get new hashes (for non-HEAD case)

**What stays the same:**

- Other commits' content and messages
- Other branches not in the ancestry chain
- Existing uncommitted changes in the working directory

## Target Resolution

Arguments are resolved using the shared resolution strategy (see Spec 002)
with an additional filesystem fallback:

1. **Standard resolution** is tried first — handles branch names, git
   references (hashes, `HEAD`, etc.), and short IDs.
2. **Filesystem fallback** — if standard resolution fails, the argument is
   checked as a filesystem path. If the path exists and has uncommitted
   changes, it resolves as a file target.

This means arguments can be:

- **Filenames**: paths to files with changes in the working tree
- **Commit hashes**: full or partial git hashes
- **Branch names**: local branch names
- **Short IDs**: the compact IDs shown by `git-loom status`
- **Git references**: `HEAD`, `HEAD~2`, etc.

The command distinguishes sources from the target purely by position (last
argument is target).

## Prerequisites

- Git 2.38 or later
- Must be in a git repository with a working tree
- For short ID arguments: must have upstream tracking configured

## Examples

### Amend a file into a commit

```bash
git-loom status
# Shows: │●  ab  72f9d3 Fix login bug
# Working tree has changes to src/auth.rs

git-loom fold src/auth.rs ab
# Stages src/auth.rs and amends it into commit ab
```

### Amend multiple files into HEAD

```bash
git-loom fold src/main.rs src/lib.rs HEAD
# Stages both files and amends them into the HEAD commit
```

### Fixup a commit into an earlier one

```bash
git-loom status
# Shows:
# │●  c1  aaa111 Add feature X
# │●  c2  bbb222 Fix typo in feature X   ← this should be part of c1

git-loom fold c2 c1
# c2's changes are absorbed into c1, c2 disappears
```

### Move a commit to another branch

```bash
git-loom status
# Shows commit d0 on the current branch, and branch feature-b exists

git-loom fold d0 feature-b
# Commit d0 is moved to the tip of feature-b and removed from current branch
```

### Uncommit a commit to the working directory

```bash
git-loom status
# Shows: │●  ab  72f9d3 Fix login bug

git-loom fold ab zz
# Removes the commit and puts its changes in the working directory
```

### Using full git hashes

```bash
git-loom fold abc123d def456a
# Fixup commit abc123d into commit def456a
```

### Using filenames

```bash
git-loom fold README.md HEAD
# Amend README.md changes into HEAD
```

## Design Decisions

### Single Verb, Multiple Actions

Rather than separate commands (`amend`, `fixup`, `move`), fold uses type
dispatch to choose the action. This was chosen because:

- **Conceptual unity**: all three operations are "put X into Y"
- **Discoverability**: one command to learn instead of three
- **Consistency**: same argument syntax regardless of operation
- **Simplicity**: the short ID system already knows the types

### Source Before Target

The argument order `<source> <target>` was chosen because:

- It reads naturally: "fold X into Y"
- It matches common CLI conventions (e.g., `cp source dest`, `mv source dest`)
- The target is always singular (one commit or one branch)
- Multiple sources (files) naturally extend to the left

### Fixup Over Squash

For commit+commit, fold uses fixup semantics (keep target's message) rather
than squash (concatenate messages). This was chosen because:

- The typical use case is "this fix belongs in that commit" — the target's
  message is already correct
- If the user wants to change the message, they can `fold` then `reword`
- Simpler mental model: source disappears, target stays as-is

### True Move for Commit+Branch

The commit+branch case performs a true move (remove from source, add to target)
rather than a copy. This was chosen because:

- "Fold commit into branch" implies the commit becomes part of that branch
- Leaving a copy behind would be surprising and create duplicate work
- If users want a copy, they can use `git cherry-pick` directly

### Atomic Operations

All fold operations are atomic: either they complete fully or the repository
is left in its original state. The user is never left in a partially-applied
state that requires manual recovery.

### Automatic Working Tree Preservation

All operations automatically preserve uncommitted changes in the working tree.
Users don't need to manually stash before folding.

### Move Handles Co-Located Branches

When moving a commit to a branch that shares its tip with other co-located
branches, only the target branch advances to include the moved commit.
Co-located branches remain unaffected, pointing at their original tip.
