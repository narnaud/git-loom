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
- Without `-m`: opens the git editor for commits; prompts interactively for branch names

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

The branch is renamed using `git branch -m`. When the `-m` flag is not provided,
an interactive prompt asks for the new branch name, showing the current name as
a placeholder for convenience.

## Target Resolution

The `<target>` is interpreted using the shared resolution strategy (see Spec 002):

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
- The resolution logic is shared across all commands that accept entity
  identifiers

## Prerequisites

### For Commit Rewording

- Any git repository
- Target commit must exist
- For shortIDs: must be on a branch with upstream tracking configured

### For Branch Renaming

- Target branch must exist
- Can provide `-m` with new name, or use interactive prompt
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

### Rename branch interactively

```bash
git-loom status
# Shows: │╭─ fa [feature-a]

git-loom reword fa
# Prompts: ? New branch name › feature-a
# User types: feature-authentication
# Renames feature-a → feature-authentication
```

### Rename branch non-interactively

```bash
git-loom reword fa -m feature-authentication
# Directly renames feature-a → feature-authentication without prompting
```

## Design Decisions

### Shared Resolution Logic

Target resolution is shared across all commands, not specific to reword. Any
valid git reference (hash, `HEAD`, `HEAD~2`, branch name) works, plus short IDs
when available. This means:

- Users can use familiar git syntax without learning new conventions
- Short IDs are a convenience, not a requirement
- The command works even without upstream tracking (using hashes)
- All commands interpret identifiers identically

See **Spec 002: Short IDs** for the full resolution algorithm and design
rationale.

### Native Git Operations

All mutation operations use native git commands rather than reimplementing git
internals. This ensures:

- **Correctness**: git's rebase has decades of edge-case handling
- **Compatibility**: respects user's git configuration and hooks
- **Transparency**: operations match what users would do manually

git-loom is a workflow tool, not a git reimplementation.

### Atomic Operations

Either the reword succeeds completely or the repository is left in its original
state. The user is never left in a mid-rebase state that requires manual
recovery.

### Branch Renaming: Interactive vs Non-Interactive

Branch renaming supports both interactive and non-interactive workflows:

**Interactive (no `-m` flag):**
- Shows the current branch name as a placeholder
- Allows users to see and edit the name inline

**Non-interactive (with `-m` flag):**
- Ideal for scripts and automation
- Direct: "rename X to Y" with no prompting

Unlike commit rewording (which opens a full editor), branch renaming uses a
single-line prompt because branch names are simple strings, not multi-line
messages.

### Short ID Consistency

Short IDs must match exactly what `git-loom status` shows. What you see in
status is what you type in reword (and any future command). All commands use
the same resolution logic, so drift between displayed and accepted IDs is
impossible.

## Future Enhancements

Possible improvements for future consideration:

### Dry-Run Mode

Preview what would change without mutating history. Useful for teaching
and verification before running destructive operations.

### Batch Operations

Reword multiple commits in a single command. More efficient than running
reword multiple times, especially for large history rewriting.
