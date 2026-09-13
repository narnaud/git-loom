# Spec 016: Diff

This specification is normative.

## Overview

`git loom diff` shows diffs using short IDs alongside all the standard `git
diff` reference forms. It is a thin, short-ID–aware wrapper around `git diff`
that delegates actual rendering to git, so all pager, color, and diff-driver
configuration is respected automatically.

## CLI

```bash
git-loom diff [args...] [--staged] [--all] [-- <git args>...]
```

**Alias:** `di`

**Arguments:**

- `[args...]`: Zero or more space-separated tokens. Each token is one of:
  - A **file** short ID (e.g. `ma`) or a repository-relative file path
  - A **commit** short ID (e.g. `ab`), partial hash, or full hash
  - A **commit range** of the form `<left>..<right>`, where each side is a
    commit short ID, hash, branch name, `HEAD`, or any other git reference

  Tokens can be mixed freely (e.g. a commit and a file in the same invocation).

**Options:**

- `--staged` (alias `--cached`): Show staged changes (index vs `HEAD`) instead
  of unstaged changes.
- `-a`, `--all`: Show all changes, staged and unstaged combined (working tree
  vs `HEAD`).

`--staged` and `--all` are mutually exclusive. With neither flag, the command
shows unstaged changes only, exactly like `git diff`.

Everything after a `--` separator goes to `git diff` untouched, so `--stat`,
`-w`, `--name-only`, `--color-words`, `-U5` and the rest of the `git diff`
surface all work — including `-a` (`--text`), which loom's own `-a` shadows
before the separator. See spec 021 for the convention.

## Required Translation

### When No Arguments Are Given

`git diff` is invoked with no additional arguments, showing the diff between
the working tree and the index (unstaged changes), exactly as `git diff` does.

With `--staged`, `git diff --staged` is invoked instead, showing staged
changes (index vs `HEAD`). With `--all`, `git diff HEAD` is invoked, showing
both staged and unstaged changes in one view.

### When a Commit Is Given

The token is resolved to a full hash (via short ID lookup or direct git
reference lookup) and passed to `git diff`. This shows the diff between the
given commit and the working tree, including both staged and unstaged changes.

### When a File Is Given

The token is resolved to a repository-relative file path (via short ID lookup
or direct path lookup) and passed to `git diff -- <path>`, showing unstaged
changes to that file. The `--staged` and `--all` flags apply the same way as
with no arguments: `--staged` shows staged changes
(`git diff --staged -- <path>`) and `--all` shows both
(`git diff HEAD -- <path>`).

### When a Commit Range Is Given (`left..right`)

Each side of the `..` is resolved leniently: short IDs and hashes are looked
up and replaced with full hashes; anything that cannot be resolved (branch
names, `HEAD`, tags, etc.) is passed through to git as-is. The resulting
`<hash>..<hash>` range is forwarded to `git diff`.

### When an Unknown Option Is Given

Before the `--` separator, loom rejects it: an option loom does not define is an
error, with a tip to pass it after `--`. After the separator it is forwarded in
the order it was given, ahead of the pathspec loom builds from file tokens.

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

```bash
git-loom diff ma -- --name-only
git-loom diff ab..HEAD -- -U5
git-loom diff ab ma
```

The last form becomes `git diff <hash-of-ab> -- <path-of-ma>`. Git performs
rendering and diagnostics, preserving pager, color, diff-driver, and external
diff configuration. See Spec 021 for forwarding semantics.
