# split

Split a commit into two sequential commits by selecting which files go into each.

## Usage

```
git-loom split [-m <message>] <target>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | Commit hash, short ID, or `HEAD` |

### Options

| Option | Description |
|--------|-------------|
| `-m, --message <message>` | Message for the first commit. Opens editor if omitted. |

## What It Does

1. **Resolve** — finds the target commit
2. **File picker** — shows an interactive multi-select of the files changed in the commit
3. **First commit** — creates a new commit with the selected files and the provided message (or opens the git editor if `-m` was omitted)
4. **Second commit** — creates a second commit with the remaining files, reusing the original commit message

### HEAD vs Non-HEAD

- **HEAD commit**: a simple `reset --mixed` + re-commit sequence (no rebase needed)
- **Non-HEAD commit**: uses an edit-and-continue rebase to pause at the target, split it, then replay the rest of the history

Both paths are atomic — if anything fails, the operation is aborted and the original state is restored.

## Constraints

- The commit must change **at least two files** (otherwise there is nothing to split)
- **Merge commits** cannot be split
- You must select at least one file for each side (cannot put everything in one commit)
- Branch and file targets are rejected — only commits are accepted

## Examples

### Split the HEAD commit

```bash
git-loom split HEAD
# ? Select files for the first commit
# > [x] src/auth.rs
#   [ ] src/main.rs
# (opens editor for the first commit message)
# ✔ Split `abc123d` into 2 commits
```

### Split a commit by short ID with a message

```bash
git-loom split ab -m "refactor: extract helpers"
# ? Select files for the first commit
# > [x] src/helpers.rs
#   [ ] src/lib.rs
# ✔ Split `ab12345` into 2 commits
```

## Prerequisites

- Must be in a git repository with a working tree
- The target commit must have at least two changed files
- All operations are atomic and automatically preserve uncommitted changes
