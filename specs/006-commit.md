# Spec 006: Commit

## Overview

`git loom commit` creates a commit on a feature branch without leaving the
integration branch. It stages files, creates the commit at HEAD, then uses
the Weave graph model (Spec 004) to move it to the target feature branch
via a single interactive rebase.

## Why Commit?

Creating a commit on a feature branch within an integration workflow has friction:

- Requires switching branches, losing the integration context
- After committing, the integration branch must be manually updated
- Easy to accidentally commit "loose" on the integration branch itself

`git-loom commit` provides:

- **Stay on integration**: Never leave the integration branch
- **Branch-targeted**: Commits land on the right feature branch
- **Auto-rebase**: Integration topology is updated automatically via rebase
- **Flexible staging**: Stage by short ID, filename, or all at once

## CLI

```bash
git-loom commit [-b <branch>] [-m <message>] [files...]
```

**Arguments:**

- `-b, --branch <branch>`: Target feature branch (name or short ID). Optional;
  prompts interactively if omitted.
- `-m, --message <message>`: Commit message. Optional; opens editor if omitted.
- `[files...]`: Files to stage before committing. Accepts short IDs, filenames,
  or the reserved token `zz`.

**Staging behavior:**

- No file args: uses already-staged files (index as-is)
- `zz`: stages all unstaged changes (like `git add -A`)
- Short IDs / filenames: stages only those files
- `zz` mixed with other args: `zz` wins (stages everything)

## What Happens

### Prerequisites

- **Must be on an integration branch** (a branch with upstream tracking and
  woven feature branches). If the user is on a plain feature branch or detached
  HEAD, the command errors: *"Must be on an integration branch to use commit.
  Use `git commit` directly on feature branches."*
- **Must have something to commit**. After staging resolution, if the index is
  empty, the command errors: *"Nothing to commit."*

### Flow

1. **Stage resolution**: Apply the staging rules (see CLI section above).
2. **Branch resolution**: Determine the target feature branch.
3. **Message resolution**: Get the commit message.
4. **Commit at HEAD**: Create the commit on the integration branch.
5. **Move via rebase**: Use fold's Move rebase action to relocate the commit
   to the target feature branch. `--update-refs` keeps all branch refs correct.

### Branch Resolution

When `-b` is provided:

- If it matches an existing woven feature branch (by name or short ID): use it.
- If it matches an existing branch that is **not** woven: error, *"Branch
  '<name>' is not woven into the integration branch."*
- If it doesn't match any existing branch: treat it as a new branch name,
  validate it, create the branch at the merge-base, then proceed.

When `-b` is omitted:

- Present an interactive picker listing all woven feature branches.
- Include an option to create a new branch (prompts for name).
- If there are no woven feature branches: prompt to create a new one.

### New Branch Creation

When the target branch doesn't exist:

1. Validate the name (same rules as `git-loom branch`: trim, empty check,
   `git check-ref-format`, duplicate check).
2. Create the branch at the merge-base commit.
3. The commit will land on this new branch.
4. The branch is woven into the integration branch before the rebase step,
   so `--update-refs` tracks it.

**Parallel topology for empty branches:** When the target is a new (empty)
branch and other woven branches already exist, the new branch is rebased onto
the merge-base so it forks from there — not from the integration tip (which
may be a merge commit from previously woven branches). This ensures parallel
branch topology: each branch section forks independently from the merge-base
rather than stacking on top of existing merges.

### Commit at HEAD

The staged changes are committed directly at HEAD on the integration branch.
This is a regular `git commit` — the commit temporarily lives at the tip of
the integration branch before being relocated by the rebase step.

### Move via Weave

After the commit is created at HEAD, it is relocated to the target feature
branch using the Weave graph model (Spec 004):

1. Build a `Weave` from the repository state.
2. For empty branches (branch at merge-base with no commits): call
   `add_branch_section()` and `add_merge()` to create the merge topology.
3. Call `move_commit(head_oid, branch_name)` to relocate the commit from
   the integration line to the target branch section.
4. Serialize and execute via `run_rebase()`.

This is a single atomic rebase operation. The `--update-refs` flag ensures
all branch refs stay correct.

**Conflicts**: If the rebase encounters conflicts (e.g., the new commit
conflicts with other commits in the topology), the operation stops and the
user resolves conflicts with standard git tools (`git rebase --continue`).

## Target Resolution

The `-b <branch>` argument uses the shared `resolve_target()` function for
short ID resolution, but is restricted to branches only:

**Resolution Order:**

1. **Local branch names** - Exact match for woven feature branches
2. **Short IDs** - Resolves branch short IDs to branch names
3. **New branch** - If no match, treated as a new branch name to create

Commit hashes and file targets are rejected: *"Commit target must be a
branch."*

## File Resolution

The `[files...]` positional arguments accept:

- **`zz`** (reserved token): Stages all unstaged changes.
- **Short IDs**: File short IDs as shown in `git-loom status` output.
- **File paths**: Literal file paths (relative or absolute), passed to
  `git add`.

When `zz` appears anywhere in the file list, it takes precedence and all
unstaged changes are staged regardless of other arguments.

## Prerequisites

- Git 2.38 or later (required for `--update-refs` during rebases)
- Must be in a git repository with a working tree (not bare)
- Must be on an integration branch with upstream tracking configured
- Working tree must have staged changes or files to stage

## Examples

### Commit to existing branch interactively

```bash
git-loom status
# Shows woven branches: feature-auth (fa), feature-ui (fu)
# Shows unstaged files: src/auth.rs (ar)

git-loom commit
# Stages: nothing (uses already-staged files)
# Prompts: ? Select target branch ›
#   ● feature-auth
#   ○ feature-ui
#   ○ Create new branch
# Opens editor for commit message
# Creates commit at HEAD, rebases it onto feature-auth
```

### Commit with all options specified

```bash
git-loom commit -b fa -m "add password validation" zz
# Stages all changes, commits at HEAD, rebases onto feature-auth
```

### Commit specific files by short ID

```bash
git-loom status
# Shows: M  ar  src/auth.rs
# Shows: M  ml  src/mail.rs

git-loom commit -b fa ar -m "fix auth check"
# Stages only src/auth.rs, commits at HEAD, rebases onto feature-auth
```

### Commit to a new branch

```bash
git-loom commit -b feature-logging -m "add request logging"
# 'feature-logging' doesn't exist yet
# Creates branch at merge-base, weaves it
# Creates commit at HEAD, rebases it onto feature-logging
```

### Interactive with new branch creation

```bash
git-loom commit zz
# Stages everything
# Prompts: ? Select target branch ›
#   ● feature-auth
#   ○ feature-ui
#   ○ Create new branch
# User selects "Create new branch"
# Prompts: ? Branch name ›
# User types: feature-logging
# Opens editor for commit message
# Creates branch, commits at HEAD, rebases onto feature-logging
```

## Architecture

### Module: `commit.rs`

The commit command orchestrates staging, branch resolution, commit creation,
and relocation via fold's rebase infrastructure:

```
commit::run(branch, message, files)
    ↓
resolve_staging(files)
    match files:
        []       → use index as-is
        [zz, ..] → git add -A
        [f1, f2] → resolve each (short ID or path), git add each
    ↓
verify_index_not_empty()
    ↓
resolve_branch(branch)
    match branch:
        Some(b) → resolve_target(b) → must be branch
                   if not found → validate name, create at merge-base
        None    → interactive picker (woven branches + create new)
    ↓
resolve_message(message)
    match message:
        Some(m) → use directly
        None    → open editor
    ↓
git_commit::commit(workdir, message)   // commit at HEAD
    ↓
Weave::from_repo(repo)
    ↓
if branch_is_empty:
    graph.add_branch_section(branch)   // create merge topology
    graph.add_merge(branch)
    ↓
graph.move_commit(head_oid, branch)    // relocate commit
    ↓
run_rebase(workdir, base_oid, todo)
```

**Key integration points:**

- **`git::resolve_target()`** - Shared resolution logic (see Spec 002)
- **`git::gather_repo_info()`** - To list woven branches and their merge order
- **`git_branch::create()`** - For creating new branches at merge-base
- **`git_branch::validate_name()`** - Git-native name validation
- **Weave graph model** - Topology mutation and rebase execution (Spec 004)
- **`cliclack`** - Interactive branch picker and name prompt

### Unified Weave Strategy

Both the empty-branch and non-empty-branch paths use the same Weave-based
approach:

1. Build a `Weave` from the repository state.
2. For empty branches, add the branch section and merge topology to the graph.
3. Call `move_commit()` to relocate the commit.
4. Serialize and execute a single interactive rebase.

This unified approach replaces the earlier split where empty branches required
manual stash/reset/rebase-onto/merge gymnastics:

- **Preserves merge resolution**: Existing merge commits keep their conflict
  resolutions intact.
- **Atomic**: A single rebase operation, not N sequential steps.
- **Consistent**: All topology-modifying operations in git-loom use the same
  Weave mechanism.
- **Parallel topology**: Empty branches are correctly placed as parallel
  sections forking from the merge-base, not stacked on existing merges.

## Design Decisions

### No Loose Commits

The commit command always targets a feature branch. Committing directly on the
integration branch (a "loose" commit) is not allowed because:

- **Clarity**: Every commit belongs to a feature branch, making the graph clean
- **Consistency**: The integration branch is purely a merge of feature branches
- **Reversibility**: Features can be unwoven cleanly when every commit is owned

### `zz` as Reserved Token

The token `zz` is reserved to mean "stage everything." This was chosen because:

- **Ergonomic**: Two keystrokes, easy to type quickly
- **Mnemonic**: Visually distinct from short IDs (which are derived from content)
- **Non-conflicting**: `zz` is excluded from short ID generation to prevent
  collisions

When `zz` appears alongside explicit file arguments, `zz` wins and stages
everything. This avoids ambiguity: if you typed `zz`, you want everything
staged.

### Branch-First Design

The `-b` flag is the primary interface. The interactive picker is a convenience
for when you don't remember the branch name. This mirrors the `branch` command's
pattern where the name can be provided as an argument or prompted for.

### Commit-then-Move over Direct Branch Commit

The command creates the commit at HEAD first, then moves it via rebase, rather
than creating it directly on the feature branch tip. This was chosen because:

- **Reuses Weave**: The move operation uses the same `move_commit()` mutation
  as `fold <commit> <branch>`, avoiding a parallel code path
- **Simplicity**: No need to check out the feature branch, apply changes, commit,
  then switch back — just commit where you are and relocate
- **Working tree stays clean**: The staged changes are committed normally; the
  rebase only rearranges commit order, not file content
- **Atomic**: If the rebase fails (conflict), the commit still exists and can
  be recovered or resolved

### Conflicts Are User-Resolved

If the rebase encounters conflicts, the operation pauses and the user resolves
them with standard git tools. This was chosen over automatic abort because:

- **Progress preservation**: The commit is already created; aborting would lose
  work
- **Familiarity**: Users know how to resolve merge conflicts
- **Transparency**: Conflicts are real issues that need human judgment
