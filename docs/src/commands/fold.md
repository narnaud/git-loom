# fold

Fold source(s) into a target — a polymorphic command that amends files into commits, fixups commits together, moves commits between branches, or uncommits changes.

## Usage

```
git-loom fold <source>... <target>
```

The last argument is always the target. All preceding arguments are sources. At least two arguments are required.

## Type Dispatch

The action depends on the types of the arguments, detected automatically:

| Source | Target | Action |
|--------|--------|--------|
| File(s) | Commit | **Amend**: stage files into the commit |
| Commit | Commit | **Fixup**: absorb source commit into target |
| Commit | Branch | **Move**: relocate commit to the branch |
| Commit | `zz` | **Uncommit**: remove commit, put changes in working directory |
| CommitFile | `zz` | **Uncommit file**: remove one file from a commit to working directory |
| CommitFile | Commit | **Move file**: move one file's changes between commits |

CommitFile sources use the `commit_sid:index` format shown by `git loom status -f` (e.g. `fa:0` for the first file in commit `fa`).

## Actions

### Amend files into a commit

```bash
git-loom fold src/auth.rs ab
# Stages src/auth.rs and amends it into commit ab
```

Multiple files can be folded at once:

```bash
git-loom fold src/main.rs src/lib.rs HEAD
# Amends both files into the HEAD commit
```

### Fixup a commit into another

Absorbs the source commit's changes into the target. The source disappears from history; the target keeps its message.

```bash
git-loom fold c2 c1
# c2's changes are absorbed into c1, c2 disappears
```

The source commit must be newer than the target.

### Move a commit to another branch

Removes the commit from its current branch and appends it to the target branch's tip.

```bash
git-loom fold d0 feature-b
# Commit d0 moves to feature-b, removed from its original branch
```

### Uncommit to the working directory

Removes a commit from history and places its changes as unstaged modifications.

```bash
git-loom fold ab zz
# Removes commit ab, its changes appear as unstaged modifications
```

### Uncommit a single file

Removes one file's changes from a commit, preserving the rest of the commit.

```bash
git-loom fold ab:1 zz
# Removes the second file from commit ab to the working directory
```

### Move a file between commits

Moves one file's changes from one commit to another.

```bash
git-loom fold c2:1 c1
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

## Prerequisites

- Must be in a git repository with a working tree
- For short ID arguments: must have upstream tracking configured
- All operations are atomic and automatically preserve uncommitted changes
