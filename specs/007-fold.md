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

**Invalid combinations** produce an error:

- File + Branch: `"Cannot fold files into a branch. Target a specific commit."`
- Branch + anything: `"Cannot fold a branch. Use 'git loom branch' for branch operations."`
- Mixed files and commits as sources: `"Cannot mix file and commit sources."`
- Multiple commit sources: `"Only one commit source is allowed."`

## What Happens

### Case 1: File(s) + Commit (Amend)

Folds file changes into an existing commit, effectively amending that commit
to include the current working tree changes for the specified files.

**Steps:**

1. Resolve all sources as files, resolve target as a commit.
2. Validate that the specified files have changes (staged or unstaged).
   Error if a file has no changes: `"File '<path>' has no changes to fold."`
3. Stage the specified files (`git add <files>`).
4. If the target is HEAD: `git commit --amend --no-edit`.
5. If the target is not HEAD: create a temporary commit with the staged
   changes, then use the fixup mechanism (same as Case 2) to fold the
   temporary commit into the target. This avoids stash/unstash complexity
   during interactive rebase and reuses the fixup infrastructure.

**Dirty working tree:**

- For non-HEAD targets: uncommitted changes are automatically stashed before
  the rebase and restored after (`--autostash`).

**What changes:**

- The target commit absorbs the file changes (new hash)
- All descendant commits get new hashes (same content/messages)

### Case 2: Commit + Commit (Fixup)

Folds the source commit into the target commit. The source commit's changes
are absorbed into the target, and the source commit disappears from history.
The target commit keeps its original message.

**Steps:**

1. Resolve source as a commit, resolve target as a commit.
2. Validate that the source is a descendant of the target (source is newer).
   Error if not: `"Source commit must be newer than target commit."`
3. Build a `Weave` from the repo, call `fixup_commit(source_oid, target_oid)`.
   This removes the source commit, changes its command to `fixup`, and inserts
   it immediately after the target commit.
4. Execute via `run_rebase()` (with `--autostash`). The rebase replays the
   history: the target absorbs the source's changes, and the source disappears.

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

In git-loom's integration branch topology, all feature branch commits are
reachable from HEAD. This means a single interactive rebase can move a commit
line from one branch's section to another, and `--update-refs` keeps all
branch refs correct automatically.

**Steps:**

1. Resolve source as a commit, resolve target as a branch.
2. Build a `Weave` from the repo, call `move_commit(source_oid, branch_name)`.
   This removes the commit from its current position and appends it to the
   target branch section. If the target branch is co-located, the section is
   split (see Spec 004).
3. Execute via `run_rebase()` (with `--autostash`). `--update-refs` ensures
   both the source and target branch refs update to reflect the new topology.

**Conflict handling:**

- If the rebase encounters conflicts, stop and let the user resolve.
  Message: `"Rebase conflict while moving commit. Resolve and run
  'git rebase --continue'."`

**What changes:**

- The commit moves to the target branch's section (new hash)
- The commit is removed from its original branch
- Affected commits in both branches get new hashes
- Branch refs are updated automatically via `--update-refs`

## Target Resolution

Arguments are resolved using a fold-specific wrapper around the shared
`resolve_target()` function (see Spec 002):

1. **`resolve_target()`** is tried first — handles branch names, git
   references (hashes, `HEAD`, etc.), and short IDs.
2. **Filesystem fallback** — if `resolve_target()` fails, the argument is
   checked as a filesystem path. If the path exists and has uncommitted
   changes, it resolves as `Target::File`.

This means arguments can be:

- **Filenames**: paths to files with changes in the working tree
- **Commit hashes**: full or partial git hashes
- **Branch names**: local branch names
- **Short IDs**: the compact IDs shown by `git-loom status`
- **Git references**: `HEAD`, `HEAD~2`, etc.

The command distinguishes sources from the target purely by position (last
argument is target).

## Prerequisites

- Git 2.38 or later (for `--update-refs` during rebases)
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

## Architecture

### Module: `fold.rs`

The fold command is an orchestration layer that dispatches to the appropriate
operation based on resolved argument types:

```
fold::run(args)
    ↓
split: sources = args[..n-1], target = args[n-1]
    ↓
resolve each arg via resolve_fold_arg()
    (resolve_target() first, filesystem path fallback)
    ↓
classify: (source_types, target_type)
    ↓
match:
    (File(s), Commit) →
        if HEAD: stage + amend_no_edit
        if not HEAD: stage + temp commit + Fixup rebase
    (Commit, Commit)  → Fixup rebase
    (Commit, Branch)  → Move rebase
    _                 → error (invalid combination)
```

**Key integration points:**

- **`git::resolve_target()`** - Shared resolution logic (Spec 002)
- **Weave graph model** - Topology mutation and rebase execution (Spec 004)
- **`git_commit` module** - `stage_files()`, `amend_no_edit()`, `commit()`
- **Automatic abort on failure** - Atomic operations via `run_rebase()`

### Weave Mutations

Fold uses two Weave mutations (Spec 004):

- **`fixup_commit(source_oid, target_oid)`**: Remove the source commit,
  change its command to `Fixup`, and insert it immediately after the target.
  Used for commit-into-commit and file-into-non-HEAD-commit operations.
- **`move_commit(oid, to_branch)`**: Remove the commit from its current
  location and append it to the target branch section. Handles co-located
  branches by splitting the section when needed. Used for commit-to-branch
  operations.

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

### Single Rebase for Move

The commit+branch move uses a single interactive rebase to relocate the commit
line within the integration branch topology, rather than cherry-pick + drop.
This was chosen because:

- **Atomic**: one operation instead of two — no half-done state if something fails
- **Consistent**: uses the same rebase infrastructure as all other operations
- **Correct**: `--update-refs` automatically updates all affected branch refs
- **Simple**: no branch switching needed — everything happens from HEAD

### Temp Commit for Non-HEAD File Fold

When folding files into a non-HEAD commit, the implementation creates a
temporary commit with the staged files, then uses the fixup mechanism to
fold it into the target. This was chosen over the alternative of stopping
the rebase at the target commit and amending in-place because:

- **Avoids stash issues**: `--autostash` would stash the staged files before
  the rebase stops, requiring fragile manual stash pop during a checkpoint
- **Reuses infrastructure**: the fixup mechanism is already built and tested
  for commit+commit; non-HEAD file fold is just a special case
- **Simpler**: single rebase operation instead of a three-step edit/amend/continue

### Move Handles Co-Located Branches

When moving a commit to a branch that shares a section with other co-located
branches, the Weave's `move_commit()` method splits the section. The original
section keeps its existing commits and the remaining branch names. A new
stacked section is created for the target branch, containing only the moved
commit, with its `reset_target` pointing to the original section. This ensures:

- The target branch ref advances to include the moved commit
- Co-located branches (sharing the same tip) stay pointing at the original
  section's tip, unaffected by the move
- The integration line's merge entry is updated to reference the outermost
  (stacked) section

### Autostash Over Clean Working Tree Requirements

All rebase-based operations (commit+commit, commit+branch, and non-HEAD
file+commit) use `--autostash` to transparently stash and restore uncommitted
changes. This reduces friction — users don't need to manually stash before
folding. File+commit targeting HEAD uses `git commit --amend` directly, which
handles dirty trees natively.
