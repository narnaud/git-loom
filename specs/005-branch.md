# Spec 005: Branch

## Overview

`git loom branch` manages feature branches in the integration workflow. It
supports creating new branches, weaving existing branches into the integration
topology, and removing branches from integration without deleting them.

## Why Branch?

Managing feature branches in a stacked/integration branch workflow has friction:

- Requires knowing the exact commit hash for the branch point
- `git branch` doesn't validate against the integration context
- No interactive prompting for quick workflows
- No easy way to weave/unweave existing branches into/from integration

`git-loom branch` provides:

- **Create**: target a commit by short ID, hash, or branch name
- **Merge**: weave an existing branch into integration with an interactive picker
- **Unmerge**: remove a branch from integration while keeping it intact
- Smart defaults: branches at the upstream merge-base when no target is given
- Interactive: prompts for name/selection when not provided
- Safe: validates names and checks for duplicates before creating

## CLI

### `branch new` (alias: `create`)

```bash
git-loom branch [name] [-t <target>]       # implicit "new"
git-loom branch new [name] [-t <target>]    # explicit "new"
git-loom branch create [name] [-t <target>] # alias
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

### `branch merge`

```bash
git-loom branch merge [branch] [--all]
```

**Arguments:**

- `[branch]`: Branch name (optional; shows interactive picker if omitted)
- `-a, --all`: Also show remote branches without a local counterpart

**Behavior:**

- Weaves an existing, non-woven branch into the integration branch
- Uses `git merge --no-ff` to create the merge topology
- If a remote branch is selected (with `--all`), a local tracking branch is
  created automatically before weaving
- On merge conflict, saves state and pauses for `loom continue` / `loom abort`
- Errors if the branch is already woven or doesn't exist

### `branch unmerge`

```bash
git-loom branch unmerge [branch]
```

**Arguments:**

- `[branch]`: Branch name or short ID (optional; shows interactive picker if omitted)

**Behavior:**

- Removes a branch from the integration topology without deleting the branch ref
- The branch's commits are rebased out of the integration branch
- The branch ref is preserved, pointing at its original commits
- Errors if the branch is not woven into the integration branch

### Reserved Names

The subcommand names `new`, `create`, `merge`, and `unmerge` are reserved and
cannot be used as branch names (clap matches subcommands first).

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

Weaving occurs when the branch target is on the **first-parent line** from HEAD
to the merge-base (i.e., a loose commit on the integration line), including HEAD
itself. Branching at HEAD moves all first-parent commits into the new branch
section with a merge commit.

These cases are no-ops:

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

## Conflict Recovery

### `branch new` (weaving)

When `branch new` triggers a weave operation that encounters conflicts, it uses
hard-fail: the rebase is automatically aborted and an error is reported. No
state is saved — the user must resolve the situation and retry.

### `branch merge`

`branch merge` supports resumable conflict handling via `loom continue` and
`loom abort` (see Spec 014).

When the merge encounters a conflict:

1. The state is saved to `.git/loom/state.json`
2. loom reports that the operation is paused and exits

The saved state contains:
- `branch_name`: the branch being woven into integration

After the user resolves the conflict:

- `loom continue` — completes the merge and prints the success message
- `loom abort` — calls `git merge --abort` and restores the original state

While the operation is paused, most other loom commands are blocked. Only
`loom show`, `loom diff`, `loom trace`, `loom continue`, and `loom abort`
are permitted.

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

### Merge an existing branch into integration

```bash
git-loom branch merge feature-auth
# ✔ Woven 'feature-auth' into integration branch
```

### Merge with interactive picker

```bash
git-loom branch merge
# ? Select branch to weave ›
#   feature-auth
#   feature-logging
# ✔ Woven 'feature-auth' into integration branch
```

### Merge including remote branches

```bash
git-loom branch merge --all
# ? Select branch to weave ›
#   feature-auth           (local)
#   origin/feature-logging (remote, creates local tracking branch)
```

### Unmerge a branch from integration

```bash
git-loom branch unmerge feature-auth
# ✔ Unwoven 'feature-auth' from integration branch
# Branch ref 'feature-auth' is preserved
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
