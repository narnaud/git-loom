# Spec 006: Commit

## Overview

`git loom commit` creates a commit on a feature branch without leaving the
integration branch. It stages files, creates the commit, and automatically
relocates it to the target feature branch, updating the integration topology.

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
4. **Commit creation**: Create the commit.
5. **Relocation**: Move the commit to the target feature branch, updating all
   branch refs and the integration topology automatically.

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
   format check, duplicate check).
2. Create the branch at the merge-base commit.
3. The commit will land on this new branch.
4. The branch is woven into the integration topology automatically.

**Parallel topology for empty branches:** When the target is a new (empty)
branch and other woven branches already exist, the new branch forks from the
merge-base — not from the integration tip. This ensures parallel branch
topology: each branch section forks independently from the merge-base rather
than stacking on top of existing merges.

### Commit and Relocate

The staged changes are committed, then automatically relocated to the target
feature branch. The integration topology is updated in a single atomic
operation — all branch refs stay correct.

**Conflicts**: If the operation encounters conflicts (e.g., the new commit
conflicts with other commits in the topology), it stops and the user resolves
conflicts with standard git tools.

## Target Resolution

The `-b <branch>` argument uses the shared resolution strategy (see Spec 002),
restricted to branches only:

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

- Git 2.38 or later
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

### Commit-then-Relocate

The command creates the commit first, then relocates it to the target branch,
rather than switching to the feature branch to commit directly. This means:

- **No branch switching**: The user stays on the integration branch throughout
- **Working tree stays clean**: The staged changes are committed normally; only
  commit ordering changes, not file content
- **Atomic**: If the operation fails (conflict), the commit still exists and
  can be recovered or resolved

### Conflicts Are User-Resolved

If the operation encounters conflicts, it pauses and the user resolves them
with standard git tools. This was chosen over automatic abort because:

- **Progress preservation**: The commit is already created; aborting would lose
  work
- **Familiarity**: Users know how to resolve merge conflicts
- **Transparency**: Conflicts are real issues that need human judgment
