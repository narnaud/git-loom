# Spec 012: Absorb

## Overview

`git loom absorb` automatically distributes working tree changes into the
commits that last touched the affected lines. It uses blame to determine the
correct target for each file, then amends those commits — all in a single
operation.

## Why Absorb?

Working with stacked branches and integration workflows frequently produces
this situation: you notice small fixes or tweaks that belong in earlier
commits, but you must manually figure out which commit owns each file:

- "I fixed a typo in auth.rs — which commit introduced that line?"
- "I modified three files — they each belong in different commits"
- "I want to amend, but not into HEAD — into the commit that created those lines"

`git loom fold <file> <commit>` handles each case individually, but requires
the user to identify the target commit for every file. `absorb` automates
this: it blames the modified lines, groups files by their target commit, and
amends them all at once.

This is inspired by `hg absorb` (Mercurial), `git-absorb` (standalone tool),
and `jj absorb` (Jujutsu).

## CLI

```bash
git-loom absorb [--dry-run] [files...]
```

**Options:**

- `--dry-run` / `-n`: Show what would be absorbed without making changes.
  Prints the plan and exits.

**Arguments:**

- `[files...]`: Optional list of file paths to restrict absorption to.
  If omitted, all tracked files with uncommitted changes are analyzed.

## Algorithm

For each file with uncommitted changes:

1. Parse the unified diff (`git diff HEAD -- <file>`) to find which original
   lines are modified or deleted. Original lines are the `-` lines in the
   diff — the lines that existed in HEAD and are being changed.

2. Blame the file at HEAD to determine which commit last touched each
   original line.

3. If all modified/deleted original lines trace to the **same commit** and
   that commit is **in scope** (between merge-base and HEAD), the file is
   assigned to that commit.

4. Otherwise, the file is **skipped** with an explanation.

After analysis, all assigned files are staged and committed as fixup commits,
then a single Weave-based rebase folds each fixup into its target.

### In-Scope Commits

A commit is "in scope" if it is a non-merge commit between the upstream
merge-base and HEAD. Commits before the merge-base (from upstream history)
are out of scope — they cannot be amended via rebase.

### Per-File Assignment

In the current version, absorption works at the file level: if all hunks
in a file trace to the same commit, the entire file is absorbed. If hunks
trace to different commits, the entire file is skipped.

This handles the vast majority of real-world cases (a file's working-tree
changes usually relate to a single earlier commit). Per-hunk splitting may
be added in a future version.

## What Happens

### Successful Absorption

For each assigned file, its working tree changes are amended into the commit
that originally introduced those lines. The operation is atomic: all files
are absorbed in a single rebase.

**What changes:**

- Target commits absorb the file changes (new hashes)
- All descendant commits get new hashes
- Absorbed changes are removed from the working tree (they are now part of
  their target commits)

**What stays the same:**

- All commit messages
- Commit topology (branch structure, merge order)
- Skipped files remain as uncommitted changes in the working tree
- Other branches not in the ancestry chain

### Skipped Files

Files are skipped (left as uncommitted changes) for any of these reasons:

| Reason | Description |
|--------|-------------|
| New file | File does not exist in HEAD (no blame possible) |
| Binary file | Binary files cannot be blamed at line level |
| Multiple sources | Modified lines trace to different commits |
| Out of scope | Modified lines trace to commits before the merge-base |
| Pure addition | Diff contains only additions (no modified/deleted lines from HEAD) |

### Dry-Run Mode

With `--dry-run`, the command prints the analysis plan without creating any
commits or running any rebase. The working tree is left unchanged.

## Output Format

The command prints one line per analyzed file, then a summary:

```
  src/auth.rs -> abc1234 "Add login form" (feature-auth)
  src/utils.rs -> def5678 "Add helper functions"
  src/new.rs -- skipped (new file)
  src/mixed.rs -- skipped (lines from multiple commits)

Absorbed 2 file(s) into 2 commit(s)
```

For dry-run:

```
  src/auth.rs -> abc1234 "Add login form" (feature-auth)
  src/utils.rs -> def5678 "Add helper functions"

Dry run: would absorb 2 file(s) into 2 commit(s)
```

When a file's target commit belongs to a feature branch, the branch name is
shown in parentheses.

## Prerequisites

- Git 2.38 or later
- Must be in a git repository with a working tree (not bare)
- Must have upstream tracking configured (needed to determine merge-base)
- At least one commit in scope (between merge-base and HEAD)
- At least one tracked file with uncommitted changes

## Error Cases

- **No changes**: `"Nothing to absorb — make some changes to tracked files first"`
- **No in-scope commits**: `"No commits in scope — nothing to absorb into"`
- **All files skipped**: `"No files could be absorbed"` (prints skip reasons
  before the error)
- **Rebase failure**: The repository is rolled back to its pre-absorb state.
  The user is informed and can retry or use `fold` manually.

## Examples

### Absorb a single file fix

```bash
git-loom status
# Shows:
# │●  fa  abc1234 Add login form
# │●  fb  def5678 Add dashboard
#
# Working tree: M src/login.rs

git-loom absorb
#   src/login.rs -> abc1234 "Add login form" (feature-auth)
#
# Absorbed 1 file(s) into 1 commit(s)
```

### Absorb multiple files into different commits

```bash
# Working tree has changes to auth.rs and dashboard.rs
# auth.rs was last modified by commit abc1234
# dashboard.rs was last modified by commit def5678

git-loom absorb
#   src/auth.rs -> abc1234 "Add login form"
#   src/dashboard.rs -> def5678 "Add dashboard"
#
# Absorbed 2 file(s) into 2 commit(s)
```

### Preview with dry-run

```bash
git-loom absorb --dry-run
#   src/auth.rs -> abc1234 "Add login form"
#   src/dashboard.rs -> def5678 "Add dashboard"
#
# Dry run: would absorb 2 file(s) into 2 commit(s)

# Nothing changed — safe to review the plan
```

### Restrict to specific files

```bash
git-loom absorb src/auth.rs
#   src/auth.rs -> abc1234 "Add login form"
#
# Absorbed 1 file(s) into 1 commit(s)

# src/dashboard.rs changes are still in the working tree
```

### Skip ambiguous files

```bash
# src/shared.rs has lines modified by two different commits

git-loom absorb
#   src/auth.rs -> abc1234 "Add login form"
#   src/shared.rs -- skipped (lines from multiple commits)
#
# Absorbed 1 file(s) into 1 commit(s)

# src/shared.rs changes remain in the working tree
# Use 'git loom fold src/shared.rs <commit>' to handle manually
```

### Absorb within woven feature branches

```bash
git-loom status
# Shows:
# │╭─ fa [feature-auth]
# ││●  a1  abc1234 Add login form
# ││●  a2  bbb2222 Add logout button
# │╰─── merge
# │╭─ fd [feature-dashboard]
# ││●  d1  def5678 Add dashboard
# │╰─── merge
#
# Working tree: M src/login.rs, M src/dashboard.rs

git-loom absorb
#   src/login.rs -> abc1234 "Add login form" (feature-auth)
#   src/dashboard.rs -> def5678 "Add dashboard" (feature-dashboard)
#
# Absorbed 2 file(s) into 2 commit(s)

# Branch topology is preserved — merges and branch structure unchanged
```

## Design Decisions

### Blame-Based Assignment

The algorithm uses `git blame` (via `git2::Repository::blame_file`) to trace
modified lines back to their originating commits. This was chosen over the
patch-commutation approach (used by `git-absorb`) because:

- Blame is intuitive: "which commit introduced these lines?" maps directly
  to the user's mental model
- Blame is well-understood and deterministic
- It integrates cleanly with git2's API (no external dependencies)
- The per-file simplification makes blame efficient (one blame per file)

### Per-File Granularity (v1)

Absorption operates at the file level: either all of a file's changes go to
one commit, or the file is skipped entirely. This was chosen because:

- It covers the common case (file changes usually trace to one commit)
- It avoids the complexity of constructing per-hunk patches and handling
  line-number shifts when applying them sequentially
- It is safe: no risk of partial or incorrect patch application
- Per-hunk splitting can be added later as an enhancement

### Fixup Commits + Single Rebase

Rather than using the edit+continue pattern (which would require multiple
rebase stops), absorb creates fixup commits at HEAD and uses
`Weave::fixup_commit()` to reorder them. This was chosen because:

- It requires only a single rebase execution (faster, more reliable)
- It reuses the existing Weave infrastructure with no modifications
- It handles multiple target commits naturally (one fixup commit per target)
- It follows the same pattern as `fold commit+commit`

### Automatic Working Tree Preservation

Files that cannot be absorbed remain as uncommitted changes in the working
tree. The user can then handle them manually with `fold` or address them
in a subsequent `absorb` after further commits.

### Atomic Operations

The entire absorb operation is atomic: either all fixup commits are created
and the rebase completes, or the repository is rolled back to its pre-absorb
state. The user is never left in a partially-applied state.

### Inclusive of Staged and Unstaged Changes

Absorb considers all uncommitted changes relative to HEAD, whether staged or
unstaged. This was chosen because:

- The user's intent is "put my changes where they belong" — the staging
  state is irrelevant
- Using `git diff HEAD` captures the complete picture
- After absorption, absorbed files are clean (no staged or unstaged diff)
