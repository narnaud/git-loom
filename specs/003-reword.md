# Spec 003: Reword

## Overview

`git loom reword` allows users to modify commit messages or rename branches using
either traditional git hashes or git-loom short IDs. This command makes history
editing more intuitive by accepting the same short IDs shown in `git-loom status`,
while preserving git's safety and correctness through native rebase operations.

## Why Reword?

Interactive rebase (`git rebase -i`) is powerful but has friction:

- Requires understanding the rebase todo syntax
- Opens an editor showing many commits when you only want to change one
- Uses `reword` action which isn't intuitive for newcomers
- Doesn't work well with short IDs or quick CLI workflows

`git-loom reword` provides a streamlined interface:

- Direct: target a commit or branch by its visible ID
- Fast: one command, no todo file editing
- Familiar: uses the same IDs shown in status output
- Safe: uses native git operations under the hood

## CLI

```bash
git-loom reword <target> [-m <message>]
```

**Arguments:**

- `<target>`: A commit identifier (full hash, partial hash, or short ID) or a
  branch name (full name or short ID)
- `-m, --message <message>`: New commit message or branch name (optional)

**Behavior:**

- With `-m`: applies the change non-interactively
- Without `-m`: opens the git editor for commits; errors for branches

## What Happens

### When Target is a Commit

The commit message is changed, and all descendant commits are replayed to
update their hashes. This uses git's native interactive rebase internally,
ensuring hooks, configuration, and edge cases are handled correctly.

**Key behaviors:**

- Works on any commit in history, not just the most recent
- Handles root commits (the repository's first commit)
- Preserves merge commits and empty commits
- Stashes/restores working tree changes automatically
- Aborts cleanly on errors, never leaving the repository in a dirty state

**What changes:**

- Target commit gets new message (and new hash)
- All descendant commits get new hashes (but same content/messages)
- Branch refs are updated to point to new commit hashes

**What stays the same:**

- Commit content (files, diffs)
- Commit topology (parents, children, merge structure)
- Other branches not in the ancestry chain

### When Target is a Branch

The branch is renamed using `git branch -m`. This is a simple operation that
requires the `-m` flag (since branches don't have "messages" to edit).

## Target Resolution

The `<target>` is interpreted using the shared `resolve_target()` function from
the `git` module, which uses the following resolution strategy:

**Resolution Order**:

1. **Local branch names** - Exact match for local branch names
   - Branch names resolve to branches (for renaming with `-m`)
   - Example: `git-loom reword feature-a -m new-name` renames the branch
2. **Git references** - Full/partial hashes, `HEAD`, etc.
   - These resolve to commits
   - Example: `git-loom reword abc123 -m "New message"` rewords the commit
3. **Short IDs** - Searches branches, then commits, then files
   - Branch shortids resolve to branches
   - Commit shortids resolve to commits
   - File shortids resolve to files

For the reword command, file targets are rejected with an error message.

**Key behaviors:**

- **Branch names always resolve to branches**, not commits
- To reword the commit at a branch tip, use its commit hash or commit shortid
- `git-loom reword feature-a -m new-name` renames the branch (requires -m)
- `git-loom reword fa -m new-name` also renames if 'fa' is the branch shortid
- `git-loom reword abc123` rewords a commit (opens editor if no -m)
- Git references work without upstream tracking
- Short IDs require upstream tracking (same IDs shown in status)
- The resolution logic is **shared** across all commands that accept entity
  identifiers

## Prerequisites

### For Commit Rewording

- Any git repository
- Target commit must exist
- For shortIDs: must be on a branch with upstream tracking configured

### For Branch Renaming

- Target branch must exist
- Must provide `-m` with new name
- Can use full branch name or shortid

## Examples

### Reword commit with editor

```bash
git-loom status
# Shows: │●   ab72f9 Fix bug

git-loom reword ab
# Opens editor with "Fix bug"
```

### Reword commit directly

```bash
git-loom reword ab -m "Fix authentication bug in login flow"
# Changes message non-interactively
```

### Reword using git hash

```bash
git-loom reword abc123d -m "Better commit message"
# Works with any partial hash
```

### Reword root commit

```bash
git log --oneline | tail -1
# Shows: abc1234 Initial commit

git-loom reword abc1234 -m "Initial commit with project structure"
# Works on first commit despite having no parent
```

### Rename branch

```bash
git-loom status
# Shows: │╭─ fa [feature-a]

git-loom reword fa -m feature-authentication
# Renames feature-a → feature-authentication
```

## Architecture

### Module: `reword.rs`

The reword command is a thin orchestration layer that delegates to shared
utilities:

```
reword::run(target, message)
    ↓
git::resolve_target(repo, target) → Target enum
    ↓
match Target:
    Commit(hash) → reword_commit(repo, hash, message)
    Branch(name) → reword_branch(repo, name, new_name)
    File(_) → error (files cannot be reworded)
```

**Key integration points:**

- **`git::resolve_target()`** - Shared resolution logic (see Spec 002)
- **`git::gather_repo_info()`** - Used internally by resolve_target for short IDs
- **`shortid::IdAllocator`** - Used internally by resolve_target for consistent IDs
- **Native git commands** - All mutation operations (rebase, amend, branch rename)

The reword module contains only the domain-specific logic for commit message
editing and branch renaming. All entity resolution is delegated to the `git`
module's shared utilities.

## Design Decisions

### Shared Resolution Logic

Target resolution is implemented as a **shared utility** in the `git` module,
not as reword-specific logic. This design enables:

- **Reusability**: Other commands (amend, goto, etc.) can use the same resolver
- **Consistency**: All commands interpret identifiers identically
- **Maintainability**: Resolution behavior is defined once, tested once
- **Composability**: Commands focus on their domain logic, not parsing

Any valid git reference (hash, `HEAD`, `HEAD~2`, branch name) works, plus
short IDs when available. This hybrid approach means:

- Users can use familiar git syntax without learning new conventions
- Short IDs are a convenience, not a requirement
- The command works even without upstream tracking (using hashes)
- Migration from raw git commands is seamless

See **Spec 002: Short IDs** for the full resolution algorithm and design
rationale.

### Native Git Operations

All mutation operations (rebase, amend, branch rename) call git CLI commands
rather than using libgit2 APIs. This choice prioritizes:

- **Correctness**: git's rebase has decades of edge-case handling
- **Compatibility**: respects user's git configuration and hooks
- **Maintainability**: rebase behavior changes are handled by git updates
- **Transparency**: operations match what users would do manually

Using native commands means git-loom is a workflow tool, not a git reimplementation.

**Implementation: Self-as-Sequence-Editor**

To non-interactively mark a specific commit for editing during rebase, git-loom
uses a clever technique: it sets itself as the `GIT_SEQUENCE_EDITOR`:

1. Start `git rebase -i` with `GIT_SEQUENCE_EDITOR="git-loom internal-sequence-edit <hash>"`
2. Git calls git-loom to edit the todo file
3. Git-loom replaces `pick <hash>` with `edit <hash>` and exits
4. Rebase stops at that commit, ready for amending

This approach:
- Works reliably across platforms (no shell script files)
- Avoids parsing rebase todo file formats in the main code path
- Keeps the sequence editing logic separate and testable
- Doesn't interfere with user's configured editor (used later during amend)

### Atomic Operations

If any step fails (rebase start, amend, continue), the operation aborts cleanly,
rolling back to the original state. This matches user expectations: either the
operation succeeds completely or nothing changes. Leaving a repository mid-rebase
requires manual recovery that most users aren't equipped to handle.

### Branch Renaming Requires -m

While commit rewording can open an editor (showing the current message), branch
renaming requires the `-m` flag. Why?

- Branches have names, not multi-line messages
- Opening an editor with just "feature-a" isn't useful
- The operation is clearer when explicit: "rename X to Y"

This asymmetry reflects the nature of the entities being modified.

### Short ID Consistency

Short IDs must match exactly what `git-loom status` shows. This guarantee is
maintained by the **shared resolution system** in the `git` module:

- Both status and reword use `git::resolve_target()`
- Both use the same `collect_entities()` ordering
- Both use the same `IdAllocator` collision resolution

What you see in status is what you type in reword (and any future command).
This consistency is architectural: all commands delegate to the same resolver,
so drift is impossible.

## Future Enhancements

Possible improvements for future consideration:

### Dry-Run Mode

Preview what would change without mutating history. Useful for teaching
and verification before running destructive operations.

### Batch Operations

Reword multiple commits in a single command. More efficient than running
reword multiple times, especially for large history rewriting.
