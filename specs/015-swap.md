# Spec 015: Swap

## Overview

`git loom swap` reorders two commits within the same sequence, or exchanges the
positions of two branch sections on the integration line. It is a single
command that handles both use cases and provides conflict recovery via
`loom continue` / `loom abort`.

## Why Swap?

Reordering commits or branches in raw git requires interactive rebase with
manual `pick` line editing:

- Swapping two commits means opening an editor, locating both lines, cutting
  and pasting them, and saving — with no validation that they belong to the
  same sequence.
- Reordering two woven branches requires understanding the merge topology,
  correctly moving both the `pick` lines _and_ the `label`/`merge` entries,
  and then deleting the branch ref cleanup step — getting any of it wrong
  corrupts the integration branch.

`git-loom swap` handles both cases with a single, validated command:

- Accepts commit hashes, short IDs, or branch names
- Validates that both arguments are the same type (both commits or both branches)
- Guards against cross-location swaps (different branch sections, or one in a
  section and the other on the integration line)
- Guards against stacking dependencies that would be broken by a branch swap

## CLI

```bash
git-loom swap <a> <b>
```

**Arguments:**

- `<a>`: A commit hash (full or short), short ID, or branch name — the first
  target to swap
- `<b>`: A commit hash (full or short), short ID, or branch name — the second
  target to swap

Both arguments must resolve to the same kind of target: both commits or both
branches. Mixing types (one commit, one branch) is an error.

## What Happens

### When Arguments Are Commits

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

### When Arguments Are Branches

The two branch sections swap their positions on the integration line. The merge
entries and all commits in both sections are replayed in the new order.

**What changes:**

- The two branch sections exchange positions in the weave topology
- Their corresponding merge entries on the integration line are also swapped
- All commits in both sections are replayed onto the new base
- On success, loom prints: `Swapped branches '<a>' and '<b>'`

**What stays the same:**

- Commit content and messages within each branch are preserved
- All other branch sections remain in their original order
- Branches not involved in the rebase are unaffected

**Error cases:**

- `"Branch '<name>' not found in weave graph"` — the branch is not woven into
  the integration branch
- `"Cannot swap co-located branches"` — both names resolve to the same branch
  section
- `"Cannot swap branches: '<C>' is stacked on '<A or B>'"` — any branch
  (including the two being swapped) is stacked on the other; swapping would
  break the stacking dependency

## Target Resolution

Arguments are resolved via `resolve_arg()` with the accept list:

```
[Branch, Commit]
```

Priority order (first match wins):

1. **Branch name** — exact match against a local branch ref
2. **Commit hash or short ID** — full OID, partial OID prefix, or 2-char
   short ID

Because `Branch` is checked first, a name that also happens to be a valid
commit prefix resolves as a branch. See Spec 002 for the full resolution
algorithm.

## Conflict Recovery

`swap` supports resumable conflict handling. If a conflict occurs during the
rebase:

1. loom saves state to `.git/loom/state.json` and pauses.
2. The user resolves conflicts with git, then runs `loom continue` to
   complete the swap, or `loom abort` to restore the original state.

**`LoomState.context` fields:**

- `display_a` (`string`): human-readable label for the first target (short
  hash for commits, branch name for branches)
- `display_b` (`string`): human-readable label for the second target

**`after_continue` behavior:** Prints `Swapped '<a>' and '<b>'` to confirm the
operation completed successfully.

See [`continue`](../specs/014-continue-abort.md) and
[`abort`](../specs/014-continue-abort.md) for details.

## Prerequisites

- Both targets must be woven into the current integration branch.
- Both targets must be the same type (both commits or both branches).
- For branch swap: no stacking dependencies between the two branches or any
  third branch stacked on either of them.

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

### Reorder two branch sections

```bash
$ git-loom status
  ┌ feature-auth
  │ * ca1 Login form
  │ * ca2 JWT tokens
  ├ feature-ui
  │ * cb1 Button styles
  └─ integration

$ git-loom swap feature-auth feature-ui
# Swapped branches `feature-auth` and `feature-ui`
# feature-ui now appears before feature-auth on the integration line
```

### Error: commits in different branch sections

```bash
$ git-loom swap ca1 cb1
# ! Cannot swap commits from different branch sections
```

### Error: stacked branch dependency

```bash
$ git-loom swap feature-auth feature-base
# ! Cannot swap branches: 'feature-auth' is stacked on 'feature-base'
```

## Design Decisions

### Branch before Commit in resolution priority

`resolve_arg()` checks `Branch` before `Commit` so that a branch name is never
accidentally interpreted as a commit hash prefix. Commit short IDs (2-char)
and OID prefixes still resolve correctly for any input that is not a known
branch name.

### Stacking guard covers all branches, not just the two being swapped

The stacking check iterates all branch sections, not just the two targets.
This catches the case where a third branch C is stacked on A or B: swapping
A and B would place B's base below A's commits, leaving C's `reset_target`
pointing to a branch that is now in the wrong position. The error message
names the dependent branch to help the user understand what needs to be
rearranged first.

### Same-container constraint for commit swap

Swapping commits across different branch sections would silently move a commit
out of its owning branch, changing authorship attribution and branch
ownership in the weave graph. Loom treats this as an error and requires the
user to use `loom fold --move` to explicitly relocate a commit to a different
branch.

### No confirmation prompt

`swap` makes a targeted, reversible change (abortable via `loom abort`) and
requires no destructive side effects like branch deletion. A confirmation
prompt would add friction without safety benefit.
