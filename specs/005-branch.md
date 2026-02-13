# Spec 005: Branch

## Overview

`git loom branch` creates a new feature branch at a specified commit. It provides
a streamlined interface for creating branches that integrates with git-loom's
short ID system and upstream-aware workflow.

## Why Branch?

Creating feature branches in a stacked/integration branch workflow has friction:

- Requires knowing the exact commit hash for the branch point
- `git branch` doesn't validate against the integration context
- No interactive prompting for quick workflows

`git-loom branch` provides:

- Direct: target a commit by short ID, hash, or branch name
- Smart defaults: branches at the upstream merge-base when no target is given
- Interactive: prompts for name when not provided
- Safe: validates names and checks for duplicates before creating

## CLI

```bash
git-loom branch [name] [-t <target>]
```

**Arguments:**

- `[name]`: Branch name (optional; prompts interactively if omitted)
- `-t, --target <target>`: Commit hash, partial hash, short ID, or branch name
  (optional; defaults to upstream merge-base)

**Behavior:**

- With `name`: creates the branch non-interactively
- Without `name`: opens an interactive prompt for the branch name
- With `-t`: creates the branch at the specified target
- Without `-t`: creates the branch at the upstream merge-base commit

## What Happens

1. **Name resolution**: If no name is provided, an interactive prompt asks for one
2. **Validation**: The name is trimmed, checked for emptiness, validated via
   `git check-ref-format`, and checked for duplicates
3. **Target resolution**: The target is resolved to a full commit hash using
   the shared `resolve_target()` system (or defaults to merge-base)
4. **Creation**: A branch is created at the resolved commit using `git branch`

### Branch Ownership

When a branch is created, git-loom's status view assigns commit ownership:

- A branch "owns" commits from its tip down to the next branch boundary
  or the upstream base
- Creating a branch between existing commits splits ownership accordingly
- Branches at the merge-base with no owned commits appear as empty sections

### Rebase Survival

Branches created by git-loom survive integration rebases thanks to git's
`--update-refs` flag (Git 2.38+). When the integration branch is rebased onto
updated upstream, all feature branch refs are automatically updated to point
to the new rewritten commits.

## Target Resolution

The optional `-t <target>` uses the shared `resolve_target()` function:

**Resolution Order:**

1. **Local branch names** - Resolves to the branch's tip commit
   - Example: `git-loom branch feature-b -t feature-a` creates at feature-a's tip
2. **Git references** - Full/partial hashes, `HEAD`, etc.
   - Example: `git-loom branch feature-a -t abc123d` creates at that commit
3. **Short IDs** - Searches branches, then commits
   - Example: `git-loom branch feature-a -t fa` resolves short ID to a commit

**Default (no target):**

When no `-t` flag is provided, the branch is created at the merge-base between
HEAD and the upstream tracking branch. This is the most common use case: starting
a new feature from the integration point.

File targets are rejected with an error message.

## Prerequisites

- Git 2.38 or later (required for `--update-refs` during rebases)
- Must be in a git repository with a working tree (not bare)
- For the default target: must have upstream tracking configured
- For short ID targets: must have upstream tracking configured

## Name Validation

Branch names are validated in two steps:

1. **Empty check**: Rejects empty or whitespace-only names
2. **Git format check**: Uses `git check-ref-format --branch` to validate
   against git's naming rules (no `..`, no spaces, no control characters, etc.)
3. **Duplicate check**: Rejects names that match existing local branches

## Examples

### Create branch interactively

```bash
git-loom branch
# Prompts: ? Branch name ›
# User types: feature-authentication
# Created branch 'feature-authentication' at abc1234
```

### Create branch at merge-base

```bash
git-loom branch feature-auth
# Created branch 'feature-auth' at abc1234 (merge-base)
```

### Create branch at specific commit

```bash
git-loom status
# Shows: │●  ab  72f9d3 Fix bug

git-loom branch feature-auth -t ab
# Created branch 'feature-auth' at 72f9d3a
```

### Create branch at another branch's tip

```bash
git-loom branch feature-b -t feature-a
# Created branch 'feature-b' at feature-a's tip commit
```

### Create branch using git hash

```bash
git-loom branch feature-auth -t abc123d
# Created branch 'feature-auth' at abc123d
```

## Architecture

### Module: `branch.rs`

The branch command is a thin orchestration layer:

```
branch::run(name, target)
    ↓
name validation (trim, empty check, git check-ref-format, duplicate check)
    ↓
resolve_commit(repo, target)
    ↓
match target:
    None → git::gather_repo_info() → merge-base commit
    Some(t) → git::resolve_target(repo, t) → Target enum
        Commit(hash) → use hash
        Branch(name) → resolve to tip commit
        File(_) → error
    ↓
git_branch::create(workdir, name, hash)
```

**Key integration points:**

- **`git::resolve_target()`** - Shared resolution logic (see Spec 002)
- **`git::gather_repo_info()`** - Used for default merge-base target
- **`git_branch::validate_name()`** - Git-native name validation
- **`git_branch::create()`** - Wraps `git branch <name> <hash>`
- **`cliclack`** - Interactive prompt for branch name when not provided

### Module: `git_commands/git_branch.rs`

Low-level git operations:

- **`validate_name(name)`** - Calls `git check-ref-format --branch`
- **`create(workdir, name, hash)`** - Calls `git branch <name> <hash>`
- **`rename(workdir, old, new)`** - Calls `git branch -m <old> <new>`

## Design Decisions

### Simple `git branch` Over Rebase

Branch creation uses `git branch <name> <hash>` rather than an interactive
rebase approach. This was chosen because:

- **Simplicity**: One git command vs. complex rebase orchestration
- **Safety**: No risk of rebase conflicts or mid-rebase failures
- **Speed**: Instantaneous ref creation vs. commit replay
- **Rebase survival**: Guaranteed by `--update-refs` on subsequent rebases,
  not by the creation mechanism

### Default Target: Merge-Base

The default target (when `-t` is omitted) is the merge-base between HEAD
and the upstream tracking branch. This matches the most common workflow:
starting a new feature branch from the integration point where upstream
and local history diverge.

### Validation Before Creation

The command validates the branch name (format + uniqueness) before calling
git to create it. This provides clear, actionable error messages:

- `"Branch 'feature-a' already exists"` instead of a git error
- `"'my..branch' is not a valid branch name"` instead of a cryptic ref error

### Interactive Prompt

When no name is provided, `cliclack` prompts interactively. This follows
the same pattern as `reword` for branch renaming, providing a consistent
UX across git-loom commands.

### Git Version Requirement

git-loom requires Git 2.38+ (checked at startup) for the `--update-refs`
flag on rebase operations. This ensures feature branches survive rebases
without manual ref updates.
