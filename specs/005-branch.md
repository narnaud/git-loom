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
2. **Validation**: The name is trimmed, checked for emptiness, validated against
   git's naming rules, and checked for duplicates
3. **Target resolution**: The target is resolved to a full commit hash using
   the shared resolution system (see Spec 002), or defaults to the merge-base
4. **Creation**: A branch is created at the resolved commit using `git branch`

### Branch Ownership

When a branch is created, git-loom's status view assigns commit ownership:

- A branch "owns" commits from its tip down to the next branch boundary
  or the upstream base
- Creating a branch between existing commits splits ownership accordingly
- Branches at the merge-base with no owned commits appear as empty sections

### Weaving

When a branch is created at a commit that is strictly between the merge-base
and HEAD, git-loom automatically **weaves** it into the integration branch.
This restructures the linear history into a merge topology where the branch's
commits appear as a side branch joined by a merge commit.

**Before** (linear):
```
origin/main → A1 → A2 → A3 → HEAD
```

**After** `git-loom branch feature-a -t A2`:
```
origin/main → A1 → A2 (feature-a)
            ↘              ↘
             A3' --------→ merge (HEAD)
```

**When weaving triggers:**

Weaving occurs only when the branch target is on the **first-parent line** from
HEAD to the merge-base (i.e., a loose commit on the integration line). These
cases are no-ops:

- **Branch at HEAD**: No commits to split off, topology stays linear.
- **Branch at merge-base**: Branch has no owned commits in the range, no topology
  change needed.
- **Branch inside an existing side branch**: The commit is already part of a
  merge topology (reachable through a merge second-parent), so no restructuring
  is needed. The branch ref is created and ownership is split, but the topology
  stays unchanged.

**Dirty working tree:**

- If the working tree has uncommitted changes, they are automatically stashed
  before the operation and restored after. The user does not need to manually
  stash or commit before weaving.

**Conflicts:**

If the operation encounters conflicts, it stops and the user must resolve them
manually using standard git tools.

### Rebase Survival

Branches created by git-loom survive integration rebases. When the integration
branch is rebased onto updated upstream, all feature branch refs are automatically
updated to point to the new rewritten commits.

## Target Resolution

The optional `-t <target>` uses the shared resolution strategy (see Spec 002):

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

- Git 2.38 or later
- Must be in a git repository with a working tree (not bare)
- For the default target: must have upstream tracking configured
- For short ID targets: must have upstream tracking configured

## Name Validation

Branch names are validated before creation:

1. **Empty check**: Rejects empty or whitespace-only names
2. **Format check**: Validates against git's naming rules (no `..`, no spaces,
   no control characters, etc.)
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

## Design Decisions

### Weave on Creation

When a branch is created between existing commits, git-loom automatically
restructures the topology into a merge-based layout. This was chosen because:

- **Immediate topology**: The merge topology is established right away, so
  `git-loom status` shows the correct branch sections immediately
- **Consistency**: All feature branches in the integration branch appear as
  merge-based side branches, matching the target mental model

### Default Target: Merge-Base

The default target (when `-t` is omitted) is the merge-base between HEAD
and the upstream tracking branch. This matches the most common workflow:
starting a new feature branch from the integration point where upstream
and local history diverge.

### Validation Before Creation

The command validates the branch name (format + uniqueness) before creating
it. This provides clear, actionable error messages:

- `"Branch 'feature-a' already exists"` instead of a git error
- `"'my..branch' is not a valid branch name"` instead of a cryptic ref error

### Interactive Prompt

When no name is provided, an interactive prompt asks for one. This follows
the same pattern as `reword` for branch renaming, providing a consistent
UX across git-loom commands.
