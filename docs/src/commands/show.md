# show

Show a commit's message and diff, like `git show`, or every commit of a branch.

**Alias:** `sh`

## Usage

```
git loom show [<target>] [-- <git args>...]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | A commit or a branch, by short ID, hash or name. Default: the commit at the top of `git loom status`. |

### Git Options

Arguments after `--` go to `git show` — see [Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom show pop -- --stat
git loom show pop -- -- src/main.rs
```

## What It Does

- **A commit**: shows it, as `git show` does.
- **A feature branch**: shows every commit the branch owns, newest first — the commits `git loom status` lists under it. Hidden branches work too.
- **The integration branch**: shows its loose commits.

Merge commits can't be shown this way; use `git show`. A branch outside your integration branch shows only its tip commit.

## Target Resolution

1. **Branch names**
2. **Git revisions**: hashes, `HEAD`, …
3. **Short IDs** of branches and commits

## Examples

```bash
git loom show pop          # one commit
git loom show feature-a    # every commit of the branch
```

## Prerequisites

- For short IDs and branches: on a branch with an upstream
