# diff

Show a diff using short IDs, like `git diff`.

## Usage

```
git loom diff [args...] [--staged] [--all]
```

Alias: `di`

Each argument is a file, commit, or commit range. Arguments can be mixed freely in a single invocation.

### Arguments

| Argument | Description |
|----------|-------------|
| `[args...]` | Files (short ID or path), commits (short ID, hash), or ranges (`left..right`) |

### Options

| Option | Description |
|--------|-------------|
| `--staged` (alias `--cached`) | Show staged changes (index vs `HEAD`) |
| `-a`, `--all` | Show all changes, staged and unstaged combined (working tree vs `HEAD`) |

`--staged` and `--all` are mutually exclusive. With neither flag, only unstaged changes are shown, exactly like `git diff`.

## What It Does

### When No Arguments Are Given

Shows unstaged changes in the working tree — identical to `git diff` with no arguments. Use `--staged` to show staged changes instead, or `--all` to show both.

### When a Commit Is Given

Resolves the token to a full hash and passes it to `git diff`, showing the diff between that commit and the working tree.

### When a File Is Given

Resolves the token to a file path and runs `git diff -- <path>`, showing unstaged changes to that file. With `--staged` the staged changes are shown (`git diff --staged -- <path>`); with `--all` both are shown (`git diff HEAD -- <path>`).

### When a Commit Range Is Given

Tokens of the form `left..right` are resolved on each side and forwarded to `git diff`. Branch names, `HEAD`, and tags that can't be resolved as short IDs are passed through to git unchanged, so all standard range forms work:

```bash
git loom diff HEAD~3..HEAD
git loom diff main..HEAD
git loom diff ab..3c
```

### When a Commit and a File Are Both Given

The commit is placed before `--` and the file after, limiting the diff to that file at that commit.

## Target Resolution

Single tokens (not ranges) are resolved in this order:

1. **Files** — short ID or repository-relative path (checked before commits)
2. **Commits** — short ID, partial hash, or full hash

Range endpoints use lenient resolution: if a token cannot be resolved as a short ID or hash it is passed to git as-is, allowing `HEAD`, branch names, and tags.

## Examples

### Show unstaged changes

```bash
git loom diff
# Equivalent to: git diff
```

### Show staged changes

```bash
git loom diff --staged
# Equivalent to: git diff --staged
```

### Show all changes, staged and unstaged

```bash
git loom diff --all
# Equivalent to: git diff HEAD
```

### Diff a commit by short ID

```bash
git loom diff ab
# Shows the diff between commit "ab" and the working tree
```

### Diff a file by short ID

```bash
git loom diff ma
# Shows unstaged changes to the file with short ID "ma"
```

### Diff a commit range

```bash
git loom diff ab..d0
# Shows what changed between those two commits
```

### Limit diff to a file at a specific commit

```bash
git loom diff ab ma
# Equivalent to: git diff <hash-of-ab> -- src/auth/login.rs
```

## Prerequisites

- A non-bare git repository.
- For short IDs: upstream tracking configured on the current branch (same as `git loom status`).
