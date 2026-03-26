# show

Show the diff and metadata for a commit, like `git show`.

## Usage

```
git loom show <target>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | Commit hash, branch name, or short ID |

## What It Does

Displays the commit metadata (author, date, message) and diff for the resolved commit, exactly like `git show`. Uses git's native pager when running in a terminal.

- When given a **commit** (hash, partial hash, or short ID): shows that commit
- When given a **branch** (name or short ID): shows the branch's tip commit

## Target Resolution

The target is resolved in this order:

1. **Branch names** — exact match resolves to the branch tip commit
2. **Git references** — full/partial hashes, `HEAD`, etc. resolve to commits
3. **Short IDs** — branch short IDs resolve to the branch tip, commit short IDs to commits

## Examples

### Show a commit by short ID

```bash
git loom show ab
# Displays commit info and diff for the commit with short ID "ab"
```

### Show a commit by hash

```bash
git loom show 9f484b6
```

### Show the tip of a branch

```bash
git loom show feature-a
# Shows the latest commit on feature-a
```

## Prerequisites

- Any git repository
- For short IDs: must be on a branch with upstream tracking configured
