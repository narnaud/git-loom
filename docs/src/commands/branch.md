# branch

Create a feature branch, empty or from existing commits.

**Alias:** `br`

To start a branch with a new commit, [`commit -b <name>`](commit.md) is usually simpler. To bring in a branch that already exists, use [`merge`](merge.md).

## Usage

```
git loom branch [<name>] [--on <branch|commit>]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<name>` | Name of the new branch. Prompts if omitted. |

### Options

| Option | Description |
|--------|-------------|
| `--on <branch\|commit>` | Where the branch starts: a branch or commit, by name, short ID or hash. Default: the upstream base. |

## What It Does

Where the new branch starts depends on `--on`:

- **Nothing**: an empty branch at the upstream base. Its first commit makes it a branch of its own.
- **A woven branch**: an empty branch at that branch's tip. Its first commit stacks on it.
- **A commit inside a branch**: splits the branch in two. The new branch takes that commit and the ones below it; the existing branch keeps the commits above and is stacked on the new one.
- **A loose commit** on the integration branch: the loose commits up to and including it become the new branch. `--on HEAD` takes all of them.

`git loom branch <name> <commit>` is refused: write `--on <commit>`.

A name starting with the [hidden prefix](../configuration.md#loomhidebranchpattern) prints a warning, since the branch won't show in `git loom status`.

## Target Resolution

1. **Branch names**: the branch's tip
2. **Git revisions**: hashes, `HEAD`, `HEAD~2`, …
3. **Short IDs** of branches and commits

## Examples

### Empty branch at the base

```bash
git loom branch feature-auth
# ✓ Created branch `feature-auth` at `abc1234`
```

### Stack on another branch

```bash
git loom branch feature-b --on feature-a
# ✓ Created branch `feature-b` at `5c1d9e2`
```

### Turn loose commits into a branch

```bash
git loom branch feature-a --on HEAD
# ✓ Created branch `feature-a` at `3e8b1c0`
# ✓ Woven `feature-a` into integration branch
```

## Conflicts

If turning commits into a branch conflicts, `branch` rolls back and deletes the new branch, so you can retry with the same name.

## Prerequisites

- On an integration branch (a branch with an upstream), unless `--on` names a commit directly
