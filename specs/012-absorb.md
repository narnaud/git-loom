# Spec 012: Absorb

## Overview

`git loom absorb` automatically distributes working tree changes into the
commits that last touched the affected lines. It uses blame to determine the
correct target for each hunk, then amends those commits — all in a single
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

1. Parse the unified diff (`git diff HEAD -- <file>`) into individual hunks.
   Each hunk starts at an `@@ -start,count +start,count @@` header.

2. For each hunk, extract the original (pre-image) line numbers of
   modified/deleted lines — the `-` lines in the diff.

3. Blame the file at HEAD to determine which commit last touched each
   modified line in each hunk.

4. Per-hunk assignment:
   - If all modified lines in a hunk trace to a single **in-scope** commit,
     the hunk is assigned to that commit.
   - Otherwise the hunk is **skipped** (pure addition, out of scope,
     or lines from multiple commits within the hunk).

5. Per-file result:
   - If all hunks are assigned to the **same commit**, the entire file is
     absorbed as a whole (whole-file assignment).
   - If hunks are assigned to **different commits** (or some are skipped),
     each assigned hunk is absorbed independently into its target commit.
     Skipped hunks remain in the working tree.
   - If **no hunks** can be assigned, the file is skipped entirely.

After analysis, assigned hunks are committed as fixup commits (whole-file
assignments via `git add`, per-hunk assignments via `git apply --cached`),
then a single Weave-based rebase folds each fixup into its target.

### In-Scope Commits

A commit is "in scope" if it is a non-merge commit between the upstream
merge-base and HEAD. Commits before the merge-base (from upstream history)
are out of scope — they cannot be amended via rebase.

### Per-Hunk Granularity

Absorption operates at the hunk level. When a file has hunks tracing to
different commits, each hunk is independently analyzed and absorbed into its
target commit. Only hunks that cannot be attributed (pure additions, out of
scope, ambiguous within a single hunk) are left in the working tree.

When all hunks in a file trace to the same commit, the file is absorbed as
a whole (same as the simpler file-level case).

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

### Skipped Hunks and Files

Individual hunks are skipped (left as uncommitted changes) for these reasons:

| Reason | Description |
|--------|-------------|
| Pure addition | Hunk contains only additions (no modified/deleted lines from HEAD) |
| Multiple sources | Modified lines within a single hunk trace to different commits |
| Out of scope | Modified lines trace to commits before the merge-base |

Entire files are skipped for these reasons:

| Reason | Description |
|--------|-------------|
| New file | File does not exist in HEAD (no blame possible) |
| Binary file | Binary files cannot be blamed at line level |
| All hunks skipped | Every hunk in the file was individually skipped |

### Dry-Run Mode

With `--dry-run`, the command prints the analysis plan without creating any
commits or running any rebase. The working tree is left unchanged.

## Output Format

The command prints one line per analyzed file (or per hunk for split files),
then a summary:

```
  src/auth.rs -> abc1234 "Add login form" (feature-auth)
  src/utils.rs -> def5678 "Add helper functions"
  src/new.rs -- skipped (new file)

Absorbed 2 hunk(s) from 2 file(s) into 2 commit(s)
```

For split files (hunks going to different commits):

```
  src/shared.rs [hunk 1/3] -> abc1234 "Add login form" (feature-auth)
  src/shared.rs [hunk 2/3] -> def5678 "Add dashboard" (feature-dashboard)
  src/shared.rs [hunk 3/3] -- skipped (pure addition)

Absorbed 2 hunk(s) from 1 file(s) into 2 commit(s)
```

For dry-run:

```
  src/auth.rs -> abc1234 "Add login form" (feature-auth)
  src/utils.rs -> def5678 "Add helper functions"

Dry run: would absorb 2 hunk(s) from 2 file(s) into 2 commit(s)
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
# Absorbed 1 hunk(s) from 1 file(s) into 1 commit(s)
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
# Absorbed 2 hunk(s) from 2 file(s) into 2 commit(s)
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
# Absorbed 1 hunk(s) from 1 file(s) into 1 commit(s)

# src/dashboard.rs changes are still in the working tree
```

### Absorb hunks from same file into different commits

```bash
# src/shared.rs has two hunks — one from each commit

git-loom absorb
#   src/shared.rs [hunk 1/2] -> abc1234 "Add login form"
#   src/shared.rs [hunk 2/2] -> def5678 "Add dashboard"
#
# Absorbed 2 hunk(s) from 1 file(s) into 2 commit(s)

# Each hunk was absorbed into its originating commit
```

### Skip ambiguous hunks

```bash
# src/shared.rs has a hunk where lines come from two different commits

git-loom absorb
#   src/auth.rs -> abc1234 "Add login form"
#   src/shared.rs [hunk 1/2] -> def5678 "Add dashboard"
#   src/shared.rs [hunk 2/2] -- skipped (lines from multiple commits)
#
# Absorbed 2 hunk(s) from 2 file(s) into 2 commit(s)

# The skipped hunk remains in the working tree
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
# Absorbed 2 hunk(s) from 2 file(s) into 2 commit(s)

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

### Per-Hunk Granularity

Absorption operates at the hunk level: each hunk in a file is independently
blamed and assigned to its originating commit. This was chosen because:

- It handles the common case where a file has changes spanning multiple
  earlier commits (e.g. a fix near the top and a fix near the bottom)
- It follows the approach used by `jj absorb` (Jujutsu)
- Hunks that cannot be assigned (pure additions, ambiguous) are left in the
  working tree for the user to handle manually

When all hunks in a file trace to the same commit, the file is absorbed as
a whole (using `git add` + commit). When hunks trace to different commits,
per-hunk patches are applied to the index via `git apply --cached`.

### Fixup Commits + Single Rebase

Rather than using the edit+continue pattern (which would require multiple
rebase stops), absorb creates fixup commits at HEAD and uses
`Weave::fixup_commit()` to reorder them. This was chosen because:

- It requires only a single rebase execution (faster, more reliable)
- It reuses the existing Weave infrastructure with no modifications
- It handles multiple target commits naturally (one fixup commit per target)
- It follows the same pattern as `fold commit+commit`

### Automatic Working Tree Preservation

Hunks and files that cannot be absorbed remain as uncommitted changes in the
working tree. The user can then handle them manually with `fold` or address
them in a subsequent `absorb` after further commits.

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
