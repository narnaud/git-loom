# Spec 016: Diff

## Overview

`git loom diff` shows diffs using short IDs alongside all the standard `git
diff` reference forms. It is a thin, short-ID–aware wrapper around `git diff`
that delegates actual rendering to git, so all pager, color, and diff-driver
configuration is respected automatically.

## Why Diff?

`git diff` is a daily-use command, but its reference syntax requires full or
partial hashes that are not immediately visible. `git-loom status` displays
short IDs for every commit and every changed file; `loom diff` makes those IDs
directly usable:

```
# Raw git workflow
git log --oneline           # find the hash
git diff abc1234            # copy-paste it

# With git-loom
git-loom status             # see "ab" next to the commit
git-loom diff ab            # use it directly
```

Beyond convenience, the command composes naturally: the same short IDs shown
in status work identically in `diff`, `show`, `reword`, `fold`, and every
other loom command.

## CLI

```bash
git-loom diff [args...]
```

**Alias:** `di`

**Arguments:**

- `[args...]`: Zero or more space-separated tokens. Each token is one of:
  - A **file** short ID (e.g. `ma`) or a repository-relative file path
  - A **commit** short ID (e.g. `ab`), partial hash, or full hash
  - A **commit range** of the form `<left>..<right>`, where each side is a
    commit short ID, hash, branch name, `HEAD`, or any other git reference

  Tokens can be mixed freely (e.g. a commit and a file in the same invocation).

## What Happens

### When No Arguments Are Given

`git diff` is invoked with no additional arguments, showing the diff between
the working tree and the index (unstaged changes), exactly as `git diff` does.

**What changes:** nothing — this is a read-only inspection command.

**What stays the same:** everything.

### When a Commit Is Given

The token is resolved to a full hash (via short ID lookup or direct git
reference lookup) and passed to `git diff`. This shows the diff between the
given commit and the working tree, including both staged and unstaged changes.

**What changes:** nothing.

**What stays the same:** everything.

### When a File Is Given

The token is resolved to a repository-relative file path (via short ID lookup
or direct path lookup) and passed to `git diff HEAD -- <path>`. This shows all
changes to that file since `HEAD`, meaning both staged and unstaged changes are
included in a single view.

**What changes:** nothing.

**What stays the same:** everything.

### When a Commit Range Is Given (`left..right`)

Each side of the `..` is resolved leniently: short IDs and hashes are looked
up and replaced with full hashes; anything that cannot be resolved (branch
names, `HEAD`, tags, etc.) is passed through to git as-is. The resulting
`<hash>..<hash>` range is forwarded to `git diff`.

This means all standard git range forms work:

```
git-loom diff HEAD~3..HEAD   # last three commits
git-loom diff main..HEAD     # divergence from main
git-loom diff ab..3c         # short IDs on both sides
```

**What changes:** nothing.

**What stays the same:** everything.

### When a Commit and a File Are Both Given

If the invocation contains both a commit token and a file token, the commit is
included in the `git diff` argument list before the `--` separator, and the
file path is appended after `--`. This limits the diff to the specified file
at the given commit.

## Target Resolution

Single tokens (not ranges) are resolved using `resolve_arg()` with the accept
list `[File, Commit]` — file resolution is tried before commit resolution.
This means a short ID that matches both a file and a commit is interpreted as a
file. See Spec 002 for the full resolution algorithm.

Range endpoints use a lenient resolver: short ID and hash lookup are attempted,
but if resolution fails the raw token is passed to git unchanged. This allows
branch names, `HEAD`, `HEAD~N`, and tags to work in ranges without error.

## Conflict Recovery

`loom diff` is a read-only command and never runs a rebase. It does not save
`LoomState`, does not appear in the command trace, and does not support
`loom continue` or `loom abort`.

## Prerequisites

- A non-bare git repository (working directory required).
- For short ID resolution: upstream tracking configured on the current branch
  (same requirement as `git-loom status`).
- Short IDs are optional; full hashes and standard git references work without
  upstream tracking.

## Examples

### Show unstaged changes

```
git-loom diff
# Equivalent to: git diff
```

### Diff a single commit by short ID

```
git-loom status
# ●   ab  Fix authentication bug

git-loom diff ab
# Shows what changed in that commit vs the working tree
```

### Diff a file by short ID

```
git-loom status
# M  ma  src/auth/login.rs

git-loom diff ma
# Shows all changes to src/auth/login.rs since HEAD (staged + unstaged)
```

### Diff a commit range using short IDs

```
git-loom status
# ●   d0  Add login endpoint
# ●   ab  Fix authentication bug

git-loom diff ab..d0
# Shows what changed between those two commits
```

### Diff a range using standard git references

```
git-loom diff HEAD~3..HEAD
git-loom diff main..HEAD
# Both work without short ID lookup
```

### Limit diff to a specific file at a specific commit

```
git-loom diff ab ma
# Equivalent to: git diff <hash-of-ab> -- src/auth/login.rs
```

## Design Decisions

### Delegation to git diff

The command forwards its resolved arguments to `git diff` and lets git do the
rendering. This preserves all user configuration: pager settings, color
themes, diff drivers (e.g. for binary files), and external diff tools. Loom
does not reimplement diff output.

### Lenient range endpoint resolution

Range endpoints (`left..right`) use a lenient resolver that falls back to the
raw token on failure, rather than rejecting unrecognised references. This is
intentional: ranges frequently mix short IDs with standard git refs like
`HEAD`, branch names, or tags. Strict resolution would block these common
forms. If a token is genuinely invalid, git will report the error with its
usual diagnostic.

### File diffs always compared against HEAD

When a file is specified without an explicit commit, the diff is run as
`git diff HEAD -- <path>`. This shows the total change to the file since the
last commit, combining staged and unstaged changes into one view. This matches
the most common intent ("what have I changed in this file?") and avoids
confusion between `git diff` (unstaged only) and `git diff --cached` (staged
only).

### File before commit in resolution priority

When a single token could be either a file short ID or a commit short ID, the
file interpretation wins. Short IDs for files are allocated from the working
tree change list and are typically two characters; commit short IDs are also
two characters. Preferring file resolution matches the principle of least
surprise: if you see a changed file in status and type its ID into `diff`, you
get the file diff, not an unexpected commit diff.
