# Spec 010: Update

## Overview

`git loom update` fetches the latest upstream changes (including tags and pruning
deleted remote branches) then rebases the current integration branch onto the
upstream using a topology-aware weave model. Feature-branch commits that are
already present in the new upstream (directly merged or cherry-picked) are
automatically filtered out before the rebase. Submodules are updated if any are
configured.

## Why Update?

After initializing an integration branch with `git loom init`, the upstream
remote will continue to receive new commits. The update command brings those
changes into the local integration branch in a single step:

- **Fetch** all upstream changes, including tags, and prune deleted remote branches
- **Rebase** local commits onto the updated upstream, keeping feature branches
  on the correct side of the topology
- **Filter** commits already merged or cherry-picked upstream, preventing
  conflicts from replaying duplicate content
- **Submodule sync** automatically when the project uses submodules

Without loom, a plain `git rebase --rebase-merges` can place new upstream
commits inside a feature branch section instead of on the base line, corrupting
the integration topology. loom update avoids this by generating a clean rebase
todo where every branch section resets to the upstream tip.

## CLI

```bash
git-loom update [--yes]
```

**Flags:**

- `--yes` / `-y`: Skip the confirmation prompt when removing branches with gone upstreams.

## What Happens

### Normal Update

**What changes:**

1. **Validation**: HEAD must be on a branch (not detached), the branch must have
   an upstream tracking ref, and the repository must have a working tree.
2. **Fetch**: All upstream changes are fetched, including tags. Moved tags
   are force-updated. Deleted remote branches are pruned locally.
3. **Upstream commit filtering**: Before rebasing, any feature-branch commits
   already present in the new upstream are removed from the rebase todo. This
   uses two detection strategies (see "Upstream Commit Filtering" below).
4. **Weave-based rebase**: The integration topology is rebuilt using the weave
   model (see Spec 004). Every branch section resets to the upstream tip,
   ensuring new upstream commits land on the base line — not inside feature
   branches. Uncommitted working tree changes are automatically stashed and
   restored.
5. **Submodule update** (conditional): If `.gitmodules` exists, submodules are
   initialized and updated recursively.
6. **Gone upstream cleanup**: Any local branches whose configured upstream
   tracking branch no longer exists (pruned in step 2) are listed and the user
   is prompted once to remove them. Use `--yes` to skip the prompt.

**What stays the same:**
- Feature branch refs are kept in sync via `--update-refs`
- Merge topology (branch sections and merge commits) is preserved
- Working tree changes are preserved via autostash
- Branches without tracking configuration are not affected by gone-upstream
  cleanup

### Fallback (no integration topology)

When the current branch has upstream tracking but no weave topology (e.g. a
plain branch with no woven feature branches), loom falls back to a standard
`git rebase --autostash --update-refs --rebase-merges`. This handles
non-integration branches that still benefit from `loom update` for fetching
and submodule sync.

## Upstream Commit Filtering

Before the rebase, loom scans each feature-branch commit to determine whether
its content is already in the new upstream. Matched commits are dropped from
the rebase todo, preventing conflicts from replaying content that is already
in the base.

Two detection strategies are applied in order:

1. **Exact OID ancestry**: if the new upstream is a descendant of a
   feature-branch commit's OID, the commit was directly merged. This is a
   fast check using the commit graph.

2. **Patch-ID matching**: if a feature-branch commit was cherry-picked to
   upstream (same diff, different OID), its patch-ID is compared against the
   set of patch-IDs for all new upstream commits. Patch-ID computation is
   batched into a single pipeline for efficiency, regardless of the number
   of commits.

When a branch section becomes empty after filtering (all its commits are
already upstream), the section and its merge entry are removed from the
todo. The branch ref is left as-is — fully merged branches are typically
cleaned up by the gone-upstream step or manually by the user.

If patch-ID computation fails (e.g. git is unavailable for the pipeline
commands), a warning is displayed and filtering falls back to OID ancestry
only. This is a graceful degradation — duplicate commits may cause conflicts
during rebase, but no data is lost.

## Conflict Recovery

The update command supports resumable conflict handling via `loom continue`
and `loom abort` (see Spec 014).

When the rebase encounters a merge conflict:

1. The spinner stops with an error indicator
2. The state is saved to `.git/loom/state.json`
3. loom reports that the operation is paused and exits successfully

The saved state contains:
- `branch_name`: the current integration branch name
- `upstream_name`: the upstream tracking ref (e.g. `origin/main`)
- `skip_confirm`: whether `--yes` was passed

After the user resolves the conflict:

- `loom continue` — continues the rebase, then runs submodule update, reports
  the updated upstream commit, and proposes removal of gone-upstream branches
- `loom abort` — aborts the rebase and restores the original branch state

While the operation is paused, most other loom commands are blocked. Only
`loom show`, `loom diff`, `loom trace`, `loom continue`, and `loom abort`
are permitted.

## Prerequisites

- Must be in a git repository with a working tree (not bare)
- Current branch must have an upstream tracking ref (use `git-loom init` first)
- Git 2.38 or later (checked globally at startup)
- Network access to the remote (for `git fetch`)

## Examples

### Update with no local changes

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
```

### Update with woven feature branches

```bash
# Before: feature-a and feature-b are woven into integration
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
```

New upstream commits land on the base line. Feature branches remain on their
own branch sections with refs correctly updated.

### Update when feature commits are cherry-picked upstream

```bash
# Before: feature-a has commits F1, F2, F3. Upstream cherry-picked F1 and F2.
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
```

F1 and F2 are detected as duplicates via patch-ID matching and dropped from
the rebase todo. Only F3 remains on the feature-a branch after update.

### Update with dirty working tree (autostashed)

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
```

Uncommitted changes are automatically stashed before the rebase and restored
after.

### Update with submodules

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated submodules
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
```

### Update with gone-upstream branches

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
# ⚠ 2 local branches with a gone upstream:
# old-feature
# closed-pr
# Remove them? [y/N]
```

### Error: merge conflict

```bash
git-loom update
# ✓ Fetched latest changes
# ✗ Rebase paused due to conflicts
# ⚠ Conflicts detected — resolve them with git, then run:
# `loom continue`   to complete the update
# `loom abort`      to cancel and restore original state
```

### Error: not on an integration branch

```bash
git-loom update
# error: Branch `some-branch` has no upstream tracking branch
# Run `git-loom init` to set up an integration branch
```

### Error: detached HEAD

```bash
git-loom update
# error: HEAD is detached
# Switch to an integration branch
```

## Design Decisions

### Weave-Based Rebase

Plain `git rebase --rebase-merges` preserves merge topology literally, which
can place new upstream commits on the wrong side of merge commits — inside a
feature branch section instead of on the base line. The weave model generates
a clean todo where every branch section uses `reset onto`, ensuring branches
are correctly rebased onto the new upstream tip. This is the primary
motivation for the weave-based rebase in update.

### Upstream Commit Filtering

When a feature-branch commit has been cherry-picked or merged into upstream,
replaying it during rebase can cause conflicts — the content already exists
in the base. Rather than relying on git's `--empty=drop` (which only detects
emptiness after applying, and can still conflict during application), loom
proactively removes these commits from the rebase todo before the rebase
starts. This avoids both conflicts and empty commits.

Patch-ID matching is batched into a single pipeline (`git log -p | git patch-id
--stable` and `git diff-tree -p --stdin | git patch-id --stable`) regardless
of the number of commits, keeping performance constant.

### Fallback to Plain Rebase

When the weave model cannot be constructed (e.g. the branch has upstream
tracking but no integration topology), loom falls back to a standard git
rebase. This ensures `loom update` works on any branch with upstream tracking,
not just fully initialized integration branches.

### Full Upstream Synchronization

The fetch step synchronizes all upstream state, not just branch commits:

- **Tags** are force-updated so moved tags (e.g., release tags re-pointed
  after a hotfix) reflect the remote state
- **Deleted remote branches** are pruned locally, keeping the remote-tracking
  state clean

This provides a complete sync rather than a minimal one.

### Automatic Working Tree Preservation

Uncommitted changes are automatically preserved during the rebase via
`--autostash`. Users don't need to manually stash before updating.

### No Arguments

The update command takes no positional arguments because it always operates on
the current branch's upstream. There is no ambiguity about what to update.
