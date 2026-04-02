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
- "This file doesn't belong in this commit"
- "This file should be in a different commit"

Each of these is doable with raw git, but requires different multi-step
incantations (interactive rebase with fixup, cherry-pick + rebase to remove,
etc.). `fold` unifies them under one verb: **fold source into target**.

## CLI

```bash
git-loom fold <target>
git-loom fold <source>... <target>
git-loom fold --create <commit> <new-branch>
git-loom fold -p [<files>...] <commit>
git-loom fold -p <commit1> <commit2>
git-loom fold -p <commit> zz
```

**Arguments:**

- `<target>`: The target to fold into. Can be a commit (hash, partial hash,
  short ID) or a branch name (full name or short ID).
- `<source>...`: One or more sources to fold into the target. Sources can be
  filenames, commit hashes, partial hashes, short IDs, or branch names.

When only a target is provided (single argument), the currently staged files
are folded into the target commit. If nothing is staged, an error is returned:
`"Nothing to commit"`.

When two or more arguments are provided, the last argument is the target and
all preceding arguments are sources.

**Flags:**

- `--create` / `-c`: Create a new branch from the source commit and move it
  there. The target must be a branch name that does not yet exist. Requires
  exactly one commit source. The branch is created at the upstream merge-base
  and the commit is moved into it — whether the commit was a loose commit on
  the integration line or already on an existing branch.
- `-p` / `--patch`: Hunk-level fold mode. Opens an interactive hunk picker
  instead of operating at the file level. Has three forms (see Patch Mode
  below).

## Type Dispatch

The command resolves each argument's type, then dispatches based on the
combination:

| Source(s) | Target | Action | Multi-source? |
|-----------|--------|--------|---------------|
| *(staged)* | Commit | Amend staged: fold currently staged files into the commit | No |
| File(s) | Commit | Amend: stage files into the commit | Yes |
| Unstaged (`zz`) | Commit | Amend all: stage all changed files into the commit | No |
| Commit | Commit | Fixup: absorb source into target | No |
| Commit | Branch | Move: relocate commit to the branch | No |
| Commit | Unstaged (`zz`) | Uncommit: remove commit, put changes in working directory | No |
| CommitFile | Unstaged (`zz`) | Uncommit file: remove one file from a commit to working directory | No |
| CommitFile | Commit | Move file: move one file's changes from one commit to another | No |
| Commit | New branch (`-c`) | Create: make a new branch and move the commit into it | No |

CommitFile sources use the `commit_sid:index` format shown by `git loom status -f`
(e.g. `fa:0` for the first file in commit `fa`).

**Invalid combinations** produce an error:

- Single-arg with nothing staged: `"Nothing to commit"`
- Single-arg with non-commit target: `"Target must be a commit when folding staged files"`
- File + Branch: `"Cannot fold files into a branch. Target a specific commit."`
- Branch + anything: `"Cannot fold a branch. Use 'git loom branch' for branch operations."`
- Unstaged (`zz`) + non-Commit target: `"Cannot fold files into unstaged — files are already in the working directory."` / `"Cannot fold files into a branch. Target a specific commit."`
- Unstaged (`zz`) with clean working tree: `"No changes to fold — working tree is clean"`
- Mixed files and commits as sources: `"Cannot mix file and commit sources."`
- Multiple commit sources: `"Only one commit source is allowed."`
- CommitFile + Branch: `"Cannot fold a commit file into a branch. Target a specific commit or use 'zz' to uncommit."`

## What Happens

### Case 0: Staged Files + Commit (Single-Argument)

Folds currently staged files into an existing commit. This is the single-argument
form: `git-loom fold <target>`. Only files in the git index (staged) are folded;
unstaged changes to the same files are preserved.

**Behavior:**

- The git index must have at least one staged change.
  Error if nothing staged: `"Nothing to commit"`
- The target must resolve to a commit.
  Error if not: `"Target must be a commit when folding staged files"`
- Works for any commit in history, including HEAD.
- Unstaged changes (including to the same files) are preserved automatically.

**What changes:**

- The target commit absorbs the staged file changes (new hash)
- All descendant commits get new hashes (same content/messages)

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

### Case 1b: Unstaged (`zz`) + Commit (Amend All)

Shorthand for folding all working tree changes into a commit. Equivalent to
listing every changed file individually. If `zz` appears alongside individual
file arguments, `zz` takes precedence and all changed files are folded.

**Behavior:**

- The working tree must have at least one changed file (staged or unstaged).
  Error if clean: `"No changes to fold — working tree is clean"`
- Works for any commit in history, including HEAD.

**What changes:**

- Same as Case 1 — the target commit absorbs all file changes.

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
- If the target branch has no section in the Weave graph (e.g. it sits at
  the merge-base with no commits of its own), a branch section and merge
  entry are created automatically before moving the commit.
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

### Case 5: CommitFile + Unstaged (`zz`) (Uncommit File)

Removes a single file's changes from a commit and places them in the working
directory as unstaged modifications. The commit itself is preserved, minus the
file's changes. The source uses the `commit_sid:index` format shown by
`git loom status -f`.

**Behavior:**

- The file must have changes in the specified commit.
  Error if not: `"File '<path>' has no changes in commit <short_hash>"`
- **HEAD commit**: Reverse-applies the file's diff, amends HEAD to exclude
  the file, then re-applies the diff to the working directory.
- **Non-HEAD commit**: Reverse-applies the file's diff, creates a temp commit,
  fixups the temp into the target via Weave rebase, then re-applies the diff
  to the working directory.
- Uncommitted changes in other files are preserved automatically.

**What changes:**

- The target commit loses the file's changes (new hash)
- The file's changes appear in the working directory as unstaged modifications
- All descendant commits get new hashes (for non-HEAD case)

**What stays the same:**

- The target commit's message
- All other files in the target commit
- Other commits' content and messages
- Existing uncommitted changes in the working directory

### Case 6: CommitFile + Commit (Move File)

Moves a single file's changes from one commit to another. The file is removed
from the source commit and added to the target commit in a single atomic
rebase operation. The source uses the `commit_sid:index` format.

**Behavior:**

- Source and target must be different commits.
  Error if same: `"Source and target are the same commit"`
- The file must have changes in the source commit.
  Error if not: `"File '<path>' has no changes in commit <short_hash>"`
- Creates two temp commits (one reverse, one forward) and fixups both into
  their respective targets via a single Weave rebase.
- Uncommitted changes are preserved automatically.

**What changes:**

- The source commit loses the file's changes (new hash)
- The target commit gains the file's changes (new hash)
- All commits between/after the affected commits get new hashes

**What stays the same:**

- Both commits' messages
- All other files in both commits
- Commit topology
- Other branches not in the ancestry chain

## Patch Mode (`-p`)

The `-p` flag switches fold to hunk-level granularity. There are three forms,
detected by the argument types:

### Form 1: Pick working-tree hunks → fold into commit

```bash
git-loom fold -p [<files>...] <commit>
```

Opens an interactive hunk picker showing the current working-tree diff,
optionally filtered to the listed files (file paths or `zz` for all). After
the user selects hunks, the selection is staged and folded into the target
commit — identical in effect to `fold <staged files> <commit>` but at hunk
granularity.

**Error if no hunks selected:** `"No hunks selected"`.

**What changes:**

- The target commit absorbs the selected working-tree hunks (new hash).
- All descendant commits get new hashes (same content and messages).

**What stays the same:**

- Unselected working-tree changes remain unstaged.
- Other commits' content and messages.

### Form 2: Pick hunks from a commit → move into another commit

```bash
git-loom fold -p <commit1> <commit2>
```

Detected when both arguments resolve to commits. Opens the commit-diff hunk
picker for `<commit1>`. Selected hunks are removed from `<commit1>` and added
to `<commit2>` in a single two-phase edit-and-continue rebase. `<commit1>`
must be newer (a descendant of `<commit2>`).

**Constraints:**

- Source and target must be different commits.
- Source must be newer than target: `"Source commit must be newer than target commit"`.
- At least one hunk must be selected: `"No hunks selected"`.
- Binary files and deleted files are not supported: `"No text hunks selected — binary and deleted files are not supported with -p"`.

**What changes:**

- `<commit1>` loses the selected hunks (new hash).
- `<commit2>` gains the selected hunks (new hash).
- All commits between/after the affected commits get new hashes.

**What stays the same:**

- Both commits' messages.
- All other hunks in both commits.
- Commit topology.

### Form 3: Pick hunks from a commit → uncommit to working tree

```bash
git-loom fold -p <commit> zz
```

Detected when the target is `zz`. Opens the commit-diff hunk picker for
`<commit>`. Selected hunks are removed from the commit and applied to the
working directory as unstaged modifications. The commit itself remains in
history, minus the selected hunks.

**Constraints:** same as Form 2 (no binary/deleted files).

**What changes:**

- The commit loses the selected hunks (new hash).
- The selected hunks appear in the working directory as unstaged modifications.
- All descendant commits get new hashes (for non-HEAD case).

**What stays the same:**

- The commit's message.
- All unselected hunks in the commit.
- Other uncommitted changes in the working directory.

### Patch mode conflict handling

All `-p` forms use **hard-fail** conflict handling: if a conflict occurs during
the internal rebase, the operation is aborted automatically and the repository
is returned to its original state. `loom continue` / `loom abort` are not
supported for `-p` forms.

Pre-existing staged changes are always saved aside and restored regardless of
outcome.

## Target Resolution

Arguments are resolved via `resolve_arg()` with `accept = [Commit, CommitFile, File, Unstaged]` — see spec 002 for the resolution algorithm.

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

## Conflict Recovery

The following non-`-p` operations support resumable conflict handling (`loom
continue` / `loom abort`):

| Operation | `LoomState.context` |
|-----------|---------------------|
| Files into commit | `op: "FilesIntoCommit"` — original commit hash, file count, saved staged patch |
| Commit into commit (fixup) | `op: "CommitIntoCommit"` — source and target hashes |
| Commit to branch (move) | `op: "CommitToBranch"` — commit hash, branch name |
| Commit to unstaged (uncommit) | `op: "CommitToUnstaged"` — commit hash, captured diff |

When `loom continue` is called after conflict resolution, `after_continue`
reads the saved context, cleans up the tracking branch (`_loom-track`), and
prints the success message. If the `CommitToUnstaged` diff cannot be
re-applied (because conflict resolution changed the surrounding context), the
diff is saved to `.git/loom/unapplied.patch` for manual recovery.

All `-p` (patch mode) operations use **hard-fail**: no `LoomState` is saved and
`loom continue` / `loom abort` are not available. An auto-abort restores the
repository on any conflict.

## Prerequisites

- Git 2.38 or later
- Must be in a git repository with a working tree
- For short ID arguments: must have upstream tracking configured

## Examples

### Fold staged files into a commit

```bash
git add src/auth.rs
git-loom fold ab
# Folds the staged changes in src/auth.rs into commit ab
```

### Amend a file into a commit

```bash
git-loom status
# Shows: │●  ab  72f9d3 Fix login bug
# Working tree has changes to src/auth.rs

git-loom fold src/auth.rs ab
# Stages src/auth.rs and amends it into commit ab
```

### Amend all working tree changes into a commit

```bash
git-loom status
# Shows: │●  ab  72f9d3 Fix login bug
# Working tree has changes to src/auth.rs, src/main.rs

git-loom fold zz ab
# Stages all changed files and amends them into commit ab
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

### Uncommit a file from a commit

```bash
git-loom status -f
# Shows:
# │●  ab  72f9d3 Fix login bug
#      ab:0 M  src/auth.rs
#      ab:1 M  src/main.rs

git-loom fold ab:1 zz
# Removes src/main.rs changes from commit ab, puts them in the working directory
```

### Move a file between commits

```bash
git-loom status -f
# Shows:
# │●  c1  aaa111 Add feature X
#      c1:0 M  src/feature.rs
# │●  c2  bbb222 Add feature Y
#      c2:0 M  src/feature.rs
#      c2:1 A  src/other.rs    ← this should be in c1

git-loom fold c2:1 c1
# Moves src/other.rs from c2 to c1
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

### Fold selected working-tree hunks into a commit

```bash
git-loom status
# │●  ab  72f9d3 Fix login bug
# Working tree has changes to src/auth.rs

git-loom fold -p ab
# → hunk picker opens showing all working-tree changes
# User selects specific hunks → they are staged and folded into ab
```

### Move selected hunks from one commit to another

```bash
git-loom status
# │●  c1  aaa111 Add feature X
# │●  c2  bbb222 Add feature Y (contains a hunk that belongs in c1)

git-loom fold -p c2 c1
# → commit-diff picker opens for c2
# User selects the misplaced hunk → it is removed from c2 and added to c1
# ✓ Moved hunk(s) from `bbb222` (now `ccc333`) into `aaa111` (now `ddd444`)
```

### Uncommit selected hunks to working tree

```bash
git-loom status
# │●  ab  72f9d3 Refactor auth (too many changes mixed in)

git-loom fold -p ab zz
# → commit-diff picker opens for ab
# User selects hunks to extract → they are removed from ab and appear unstaged
# ✓ Uncommitted hunk(s) from `72f9d3` (now `8a3b2c`) to working directory
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

### Patch Mode as a Flag, Not Separate Forms

Hunk-level folding is exposed as `-p` on the existing `fold` command rather
than separate commands (`fold-hunks`, `fold -h`, etc.). The three `-p` dispatch
forms (working-tree→commit, commit→commit, commit→unstaged) mirror the
file-level cases and keep the same "fold X into Y" mental model at finer
granularity. Reusing `fold -p` avoids command proliferation and keeps the
help text cohesive.

### Two-Phase Rebase for Commit-to-Commit Patch Move

Moving hunks between two non-HEAD commits requires two separate edit-and-continue
rebases: phase 1 removes the selected hunks from the source commit; phase 2
adds them to the target commit. A single rebase cannot do both atomically
because the target commit's new OID is not known until phase 1 completes. The
`_loom-track` temporary branch is used to carry the target commit's pre-phase-1
OID through to phase 2.
