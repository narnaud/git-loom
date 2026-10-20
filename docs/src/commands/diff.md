# diff

Show a diff, like `git diff`, naming files and commits by short ID.

**Alias:** `di`

## Usage

```
git loom diff [<arg>...] [--staged | --all] [-- <git args>...]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<arg>...` | Files (short ID or path), commits (short ID or hash), or ranges such as `pop..hug`. Mix them freely. |

### Options

| Option | Description |
|--------|-------------|
| `--staged` (alias `--cached`) | Staged changes only |
| `-a, --all` | Staged and unstaged changes together |

Without either, `diff` shows unstaged changes, like `git diff`.

### Git Options

Arguments after `--` go to `git diff` — see [Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom diff -- --stat
git loom diff pop..hug -- --name-only
git loom diff -- -a            # git's --text; loom's own -a is --all
```

## What It Does

| Arguments | Diff |
|-----------|------|
| *(none)* | Your unstaged changes |
| A file | That file's unstaged changes (or staged, or both, with `--staged`/`--all`) |
| A commit | Between that commit and your working tree |
| A range `a..b` | Between the two ends; each end can also be a branch, tag or `HEAD` |
| A commit and a file | That file, between the commit and your working tree |

A single target can't be a branch name or a merge commit: use a range.

## Examples

```bash
git loom diff --all          # everything not committed
git loom diff ma             # one file, by short ID
git loom diff pop            # since commit pop
git loom diff pop..hug       # between two commits
git loom diff pop ma         # one file, since commit pop
```

## Prerequisites

- For short IDs: on a branch with an upstream
