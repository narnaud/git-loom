# Spec 015: Swap

## Overview

`git loom swap` reorders two commits within the same sequence. It provides
conflict recovery via `loom continue` / `loom abort`.

## Why Swap?

Reordering commits in raw git requires interactive rebase with manual `pick`
line editing: opening an editor, locating both lines, cutting and pasting
them, and saving — with no validation that they belong to the same sequence.

`git-loom swap` handles this with a single, validated command:

- Accepts commit hashes or short IDs
- Guards against cross-location swaps (different branch sections, or one in a
  section and the other on the integration line)

## CLI

```bash
git-loom swap <a> <b>
```

**Arguments:**

- `<a>`: A commit hash (full or short) or short ID — the first commit to swap
- `<b>`: A commit hash (full or short) or short ID — the second commit to swap

## What Happens

The two commits swap positions within their shared sequence. The sequence can
be either a branch section or the direct picks on the integration line.

**What changes:**

- The two commits exchange positions in the rebase todo list
- All descendant commits are replayed on top of the new ordering
- Branch refs are updated to point to the rebased tips
- On success, loom prints: `Swapped commits '<a>' and '<b>'`

**What stays the same:**

- Commit content and messages are preserved
- All other commits in the sequence remain in their original order
- Branches not involved in the rebase are unaffected

**Error cases:**

- `"Cannot swap a commit with itself"` — both arguments resolve to the same OID
- `"Cannot swap commits from different branch sections"` — the commits belong
  to two different branch sections
- `"Cannot swap commits from different locations (branch section vs integration line)"` —
  one commit is in a branch section and the other is a direct pick on the
  integration line
- `"Commit <oid> not found in weave graph"` — the commit is not part of the
  current integration topology

## Target Resolution

Arguments are resolved via `resolve_arg()` with the accept list:

```
[Commit]
```

Accepts full OID, partial OID prefix, or 2-char short ID. Branch names are
not accepted. See Spec 002 for the full resolution algorithm.

## Conflict Recovery

`swap` supports resumable conflict handling. If a conflict occurs during the
rebase:

1. loom saves state to `.git/loom/state.json` and pauses.
2. The user resolves conflicts with git, then runs `loom continue` to
   complete the swap, or `loom abort` to restore the original state.

**`LoomState.context` fields:**

- `display_a` (`string`): short hash of the first commit
- `display_b` (`string`): short hash of the second commit

**`after_continue` behavior:** Prints `Swapped '<a>' and '<b>'` to confirm the
operation completed successfully.

See [`continue`](../specs/014-continue-abort.md) and
[`abort`](../specs/014-continue-abort.md) for details.

## Prerequisites

- Both commits must be woven into the current integration branch.
- Both commits must be in the same container (same branch section or both on
  the integration line).

Uncommitted working tree changes are preserved automatically via
`git rebase --autostash`.

## Examples

### Reorder two commits on the integration line

```bash
$ git-loom status
  * abc123 Fix login bug
  * def456 Add dark mode
  * 789abc Refactor auth

$ git-loom swap abc123 def456
# Swapped commits `abc123` and `def456`
# dark mode is now before login bug
```

### Reorder two commits in a branch section using short IDs

```bash
$ git-loom status
  ┌ feature-ui
  │ * aa A1 – button layout
  │ * bb A2 – color scheme
  └─ integration

$ git-loom swap aa bb
# Swapped commits `aa` and `bb`
# color scheme is now before button layout
```

### Error: commits in different branch sections

```bash
$ git-loom swap ca1 cb1
# ! Cannot swap commits from different branch sections
```

## Design Decisions

### Same-container constraint

Swapping commits across different branch sections would silently move a commit
out of its owning branch, changing authorship attribution and branch
ownership in the weave graph. Loom treats this as an error and requires the
user to use `loom fold --move` to explicitly relocate a commit to a different
branch.

### No confirmation prompt

`swap` makes a targeted, reversible change (abortable via `loom abort`) and
requires no destructive side effects like branch deletion. A confirmation
prompt would add friction without safety benefit.
