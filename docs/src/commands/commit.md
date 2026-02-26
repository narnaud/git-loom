# commit

Create a commit on a feature branch without leaving the integration branch.

## Usage

```
git-loom commit [-b <branch>] [-m <message>] [files...]
```

### Options

| Option | Description |
|--------|-------------|
| `-b, --branch <branch>` | Target feature branch (name or short ID). Prompts if omitted. |
| `-m, --message <message>` | Commit message. Opens editor if omitted. |

### File Arguments

| Argument | Description |
|----------|-------------|
| *(none)* | Uses already-staged files (index as-is) |
| `zz` | Stages all unstaged changes (like `git add -A`) |
| *short IDs / filenames* | Stages only those specific files |

When `zz` appears alongside other file arguments, `zz` wins and stages everything.

## What It Does

1. **Stage** — applies the staging rules based on file arguments
2. **Branch resolution** — determines the target feature branch
3. **Message resolution** — gets the commit message (flag or editor)
4. **Commit** — creates the commit
5. **Relocate** — moves the commit to the target feature branch, updating all branch refs and integration topology automatically

### Branch Resolution

- If `-b` matches a woven feature branch: uses it
- If `-b` matches an unwoven branch: error
- If `-b` doesn't match any branch: creates a new branch at the merge-base and weaves it
- If `-b` is omitted: interactive picker with all woven branches + option to create a new one

### New Branch Creation

When the target branch doesn't exist, git-loom validates the name, creates the branch at the merge-base, and weaves it into the integration topology — all automatically.

## Examples

### Interactive

```bash
git-loom commit
# ? Select target branch
# > feature-auth
#   feature-ui
# (opens editor for commit message)
```

### Fully specified

```bash
git-loom commit -b feature-auth -m "add password validation" zz
# Stages all changes, commits to feature-auth
```

### Specific files by short ID

```bash
git-loom commit -b feature-auth ar -m "fix auth check"
# Stages only src/auth.rs (short ID: ar), commits to feature-auth
```

### To a new branch

```bash
git-loom commit -b feature-logging -m "add request logging" zz
# Creates feature-logging, weaves it, stages all, commits
```

## Prerequisites

- Must be on an integration branch (has upstream tracking and woven feature branches)
- Must have something to commit (staged or stageable changes)
