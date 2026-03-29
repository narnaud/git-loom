# fold

Fold source(s) into a target — a polymorphic command that amends files into commits, fixups commits together, moves commits between branches, or uncommits changes.

## Usage

```
git loom fold <target>
git loom fold <source>... <target>
git loom fold --patch [<files>...] <target>
git loom fold --create <commit> <new-branch>
```

When only a target is given, currently staged files are folded into the target commit. When two or more arguments are provided, the last argument is the target and all preceding arguments are sources.

### Options

| Option | Description |
|--------|-------------|
| `-p, --patch` | Interactively select hunks to stage, then fold them into the target commit. |
| `-c, --create` | Create a new branch and move the source commit into it. |

## Type Dispatch

The action depends on the types of the arguments, detected automatically:

| Source | Target | Action |
|--------|--------|--------|
| *(staged)* | Commit | **Amend staged**: fold currently staged files into the commit |
| File(s) | Commit | **Amend**: stage files into the commit |
| `zz` | Commit | **Amend all**: stage all changed files into the commit |
| Commit | Commit | **Fixup**: absorb source commit into target |
| Commit | Branch | **Move**: relocate commit to the branch |
| Commit | `zz` | **Uncommit**: remove commit, put changes in working directory |
| CommitFile | `zz` | **Uncommit file**: remove one file from a commit to working directory |
| CommitFile | Commit | **Move file**: move one file's changes between commits |
| Commit | New branch (`-c`) | **Create**: make a new branch and move the commit into it |

CommitFile sources use the `commit_sid:index` format shown by `git loom status -f` (e.g. `fa:0` for the first file in commit `fa`).

## Actions

### Fold staged files into a commit

When only a target is given, staged files are folded into the commit:

```bash
git add src/auth.rs
git loom fold ab
# Folds staged changes into commit ab
```

Only files in the git index are folded — unstaged changes to the same files are preserved. Errors with `"Nothing to commit"` if nothing is staged.

### Amend files into a commit

```bash
git loom fold src/auth.rs ab
# Stages src/auth.rs and amends it into commit ab
```

Multiple files can be folded at once:

```bash
git loom fold src/main.rs src/lib.rs HEAD
# Amends both files into the HEAD commit
```

Use `zz` to fold all working tree changes at once (staged and unstaged):

```bash
git loom fold zz ab
# Stages all changed files and amends them into commit ab
```

If `zz` is mixed with individual file arguments, `zz` takes precedence and all changed files are folded.

### Interactive hunk selection (`-p`)

With `-p`, an interactive TUI opens letting you pick individual hunks to fold into the target commit. The last argument is always the target commit.

```bash
git loom fold -p ab
# Opens hunk picker for all working tree changes
# Selected hunks are staged and folded into commit ab
```

Provide file arguments before the target to narrow the picker:

```bash
git loom fold -p src/auth.rs ab
# Opens hunk picker filtered to src/auth.rs
# Selected hunks are folded into commit ab
```

### Fixup a commit into another

Absorbs the source commit's changes into the target. The source disappears from history; the target keeps its message.

```bash
git loom fold c2 c1
# c2's changes are absorbed into c1, c2 disappears
```

The source commit must be newer than the target.

### Move a commit to another branch

Removes the commit from its current branch and appends it to the target branch's tip.

```bash
git loom fold d0 feature-b
# Commit d0 moves to feature-b, removed from its original branch
```

### Create a new branch and move a commit into it

Use `--create` (`-c`) to create a new branch and move the commit in one step. Works whether the commit is a loose commit on the integration line or already on an existing branch.

```bash
git loom fold -c d0 new-feature
# Creates new-feature and moves commit d0 into it
```

If the branch already exists, a warning is printed and the commit is moved there anyway — same as a normal `fold <commit> <branch>`.

```bash
git loom fold -c d0 existing-branch
# ! Branch `existing-branch` already exists — moving commit to it
```

### Uncommit to the working directory

Removes a commit from history and places its changes as unstaged modifications.

```bash
git loom fold ab zz
# Removes commit ab, its changes appear as unstaged modifications
```

### Uncommit a single file

Removes one file's changes from a commit, preserving the rest of the commit.

```bash
git loom fold ab:1 zz
# Removes the second file from commit ab to the working directory
```

### Move a file between commits

Moves one file's changes from one commit to another.

```bash
git loom fold c2:1 c1
# Moves the second file from c2 to c1
```

## Arguments

Arguments can be:

- **File paths** — files with changes in the working tree
- **Commit hashes** — full or partial git hashes
- **Branch names** — local branch names
- **Short IDs** — compact IDs from `git loom status`
- **Git references** — `HEAD`, `HEAD~2`, etc.
- **`zz`** — reserved token for the unstaged working directory

## Conflicts

The following fold operations support conflict recovery (pause/resume):

- Amend files into a non-HEAD commit
- Fixup a commit into another
- Move a commit to a branch
- Uncommit a commit to the working directory (non-HEAD)

If a supported fold hits a conflict, the operation is paused:

```bash
git loom fold d0 feature-b
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the fold
#   loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Moved `d0` to branch `feature-b` (now `e1f2a3b`)
```

The following fold operations **do not** support pause/resume and abort
immediately on conflict:

- Uncommit a single file (`CommitFile → zz`)
- Move a file between commits (`CommitFile → Commit`)
- Create a new branch and move a commit (`--create`)

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Must be in a git repository with a working tree
- For short ID arguments: must have upstream tracking configured
- All operations are atomic and automatically preserve uncommitted changes
