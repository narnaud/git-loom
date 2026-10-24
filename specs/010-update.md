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

- **Fetch** all upstream changes, including tags, and prune deleted remote
  branches — on the push remote too, when it differs (fork workflow)
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

**Alias:** `up`

```bash
git-loom update [<branch>] [--yes]
```

**Arguments:**

- `<branch>`: A woven branch, by name or short ID, to update from its remote
  branch before the rebase (see "Updating a Branch from its Remote" below).

**Flags:**

- `--yes` / `-y`: Skip the confirmation prompt when removing branches that are
  fully merged upstream or whose upstream is gone.

**Configuration:**

- `loom.pruneGoneBranches` (boolean): When `true`, those branches are removed
  without prompting, as if `--yes` had been passed. Defaults to `false`.

## What Happens

### Normal Update

**What changes:**

1. **Validation**: HEAD must be on a branch (not detached), the branch must have
   an upstream tracking ref, and the repository must have a working tree.
2. **Fetch**: All upstream changes are fetched from the remote the integration
   branch tracks, including tags. Moved tags are force-updated. Deleted remote
   branches are pruned locally. When the push remote differs from that remote
   (fork workflow), it is fetched and pruned as well — branches only, no tags.
3. **Branch update** (with `<branch>`): the branch's section takes its remote
   branch's commits (see "Updating a Branch from its Remote" below), which
   the next steps treat like its own.
4. **Upstream commit filtering**: Before rebasing, any feature-branch commits
   already present in the new upstream are removed from the rebase todo. This
   uses two detection strategies (see "Upstream Commit Filtering" below).
5. **Weave-based rebase**: The integration topology is rebuilt using the weave
   model (see Spec 004). Every branch section resets to the upstream tip,
   ensuring new upstream commits land on the base line — not inside feature
   branches. Uncommitted working tree changes are automatically stashed and
   restored.
6. **Submodule update** (conditional): If `.gitmodules` exists, submodules are
   initialized and updated recursively.
7. **Branch cleanup**: Two kinds of local branches are listed, and the user
   is prompted once to remove them all:
   - branches fully merged upstream: the new upstream contains their tip, or
     every commit was filtered out in step 4 (cherry-picked). Only branches
     reachable from the integration branch count, and none at or below the
     integration base (the first commit of its first-parent line that the
     upstream contains): a local `main` sitting in upstream history is not
     loom's to remove, and neither is the branch named after the upstream
     (`main` for `origin/main`) wherever it sits.
   - branches whose upstream resolves, through its remote's fetch refspecs,
     to a ref that no longer exists (pruned in step 2). An upstream Git cannot
     resolve (unknown remote, no matching refspec) and a local upstream
     (remote `.`) never count: no fetch prunes them. `status` reports `gone`
     by the same rule.

   A branch updated in step 3 is not offered as merged on account of its old
   tip, which may be upstream; it is when step 4 empties its remote version,
   which the rebase then leaves where it was.

   The prompt names each branch and its kind itself, so it stands alone as a
   TUI menu (Spec 020); without a prompt (`--yes`, the config below, agent
   mode) the two kinds are listed as warnings instead.
   Use `--yes`, or set `loom.pruneGoneBranches` to `true` in git config, to
   skip the prompt. Branches are force-deleted (`git branch -D`); each one
   prints a success message with the tip it had, so it can be revived. If a
   branch cannot be deleted (checked out in another worktree, for instance),
   a warning is printed instead: `"Skipped branch '<name>' — could not delete
   it (run 'loom trace' for the git error)"` — the remaining branches are
   still processed.

**What stays the same:**
- Feature branch refs are kept in sync via `--update-refs`
- Merge topology (branch sections and merge commits) is preserved
- Working tree changes are preserved via autostash, and staged changes come
  back staged (Spec 014)
- Branches without tracking configuration are not affected by gone-upstream
  cleanup

### Fallback (weave cannot be built)

A branch without woven branches still builds a weave. When the weave cannot be
built (an octopus merge, Spec 004), loom warns with the cause and falls back to
`git rebase --autostash --update-refs --rebase-merges=rebase-cousins`, which
puts every branch on the new upstream as the weave does (Git's default keeps a
branch forked below a merge-base that moved on its old base). Git still skips
commits already upstream, but branches already merged upstream are not
offered for removal, and a named `<branch>` is refused.

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
todo. The same applies to an inner (stacked) branch whose commits were all
filtered out: its ref is dropped from the todo rather than moved onto the
next commit of the outer branch, which the inner branch never contained. In
both cases the rebase leaves the ref untouched, and the branch is offered for
removal in the cleanup step. Fully merged branches are not necessarily gone
on the remote: in a fork workflow the merged pull request lives on the fork,
which keeps its branch.

If patch-ID computation fails (e.g. git is unavailable for the pipeline
commands), a warning is displayed and filtering falls back to OID ancestry
only. This is a graceful degradation — duplicate commits may cause conflicts
during rebase, but no data is lost.

A section that took its remote's commits holds commits outside HEAD's
history, so `git cherry` runs for its tip as well as for HEAD.

## Updating a Branch from its Remote

`loom update <branch>` takes the remote version of a woven branch: the
section is replaced with the commits of its remote branch — the upstream
`branch.<name>.merge` names, fetched in step 2 — that the new upstream lacks,
oldest first, keeping its merge entry; the rebase then replays them like any
other section, and the branch is reported as `Updated branch `<name>` with
`<remote>` (<hash> <subject>, was <old hash>)`: the remote's tip, and the
tip it replaced, the way back. Nothing is compared: whatever the local
branch had and the remote lacks — commits, amendments — goes, which is what
naming the branch means. A remote version the new upstream already has in
full leaves the branch fully merged, offered for removal in step 7; a remote
at the local tip is reported as up to date. The update proceeds as usual
either way. The plain `update` never touches a branch's commits.

A remote branch on a remote step 2 did not fetch (neither the integration nor
the push remote, or a push remote whose fetch failed) is fetched alone, without
tags, right after it; a failure stops the update before anything is
rewritten, so a stale tracking ref is never taken.

Refused, before anything is rewritten: a branch that is not woven (`git
branch -f` updates it); one sharing its section's commits with another ref —
a co-located branch, an inner branch on its commits, or the branch it is
stacked under — which the remote's commits could not place; one whose remote
carries a merge commit, which a section cannot hold; and one without an
upstream, with a local branch as upstream (remote `.`), or whose remote
branch is gone. The name is resolved before the fetch, so a wrong one costs
nothing.

## Conflict Recovery

The update command supports resumable conflict handling via `loom continue`
and `loom abort` (see Spec 014).

When the rebase encounters a merge conflict:

1. The spinner stops with an error indicator
2. The state is saved to `.git/loom/state.json`
3. loom reports that the operation is paused and exits successfully

The saved state is the `update` context of Spec 014.

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
- Git 2.43 or later (checked globally at startup)
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
after, staged ones as staged.

### Update with submodules

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated submodules
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
```

### Update a branch from its remote

`feature-a` was pushed, then a teammate (or you, elsewhere) amended it and
pushed two more commits:

```bash
git-loom update feature-a
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
# ✓ Updated branch `feature-a` with `origin/feature-a` (def5678 Second new commit, was 1a2b3c4)
```

`feature-a` now holds the remote's commits, rebased onto the new upstream
like the rest; the commit it had, `1a2b3c4`, is the way back.

### Update with a branch merged upstream

The stacked branch `feat1` was merged upstream while `feat2`, built on top of
it, was not:

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
#   › `feat1`: fully merged upstream
# Remove local branch? [y/N] y
# ✓ Removed branch `feat1` (was def5678)
```

`feat2` keeps only its own commits, rebased onto the new upstream tip.

### Update with gone-upstream branches

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
#   › `old-feature`: upstream gone
#   › `closed-pr`: upstream gone
# Remove 2 local branches? [y/N]
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

### Update with a gone branch that cannot be deleted

```bash
git-loom update
# ✓ Fetched latest changes
# ✓ Rebased onto upstream
# ✓ Updated branch `integration` with `origin/main` (abc1234 Latest upstream commit)
#   › `work-in-progress`: upstream gone
# Remove local branch? [y/N] y
# ⚠ Skipped branch `work-in-progress` — could not delete it (run `loom trace` for the git error)
```

The branch (checked out in another worktree here) is skipped but the rest
of the cleanup proceeds normally.

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

### Full Upstream Synchronization

The fetch step synchronizes all upstream state, not just branch commits:

- **Tags** are force-updated so moved tags (e.g., release tags re-pointed
  after a hotfix) reflect the remote state
- **Deleted remote branches** are pruned locally, keeping the remote-tracking
  state clean

This provides a complete sync rather than a minimal one.

### Fetching the Push Remote

In a fork workflow the feature branches live on the push remote (`loom.pushRemote`,
or the GitHub `upstream`/`origin` convention — see Spec 011), not on the remote
the integration branch tracks. That remote is the one whose deletions matter for
gone-upstream detection, so `update` fetches it too, with `--prune`, using the
same resolution as `loom push`. Without it the fork's remote-tracking refs stay
stale forever and no branch is ever reported as gone.

Only the remotes loom itself uses are fetched — not `--all`, which would also hit
read-only remotes added for cherry-picking. Single-remote setups resolve to the
same remote and pay nothing.

Tags are not fetched from the push remote: the integration remote's tags are the
authoritative ones, and force-updating from a fork could move them backwards.

A failure to fetch the push remote is a warning, not an error: the integration
branch can still be rebased onto its own upstream.

### A Branch Is Named, Never Detected

Taking a branch's remote version replaces its commits wholesale rather than
appending the new ones: the remote may have amended the shared commits too (a
rewritten stack). Which branch to take is the user's to say: after a rebase a
branch and its remote share no hashes, and deciding whether a local commit is
"on the remote" needs an identity rule (Change-Id, patch) with holes a reword,
a stack or a cherry-pick falls through, each one a guard and a warning. The
user knows when a branch was advanced elsewhere — another machine, a
colleague's push, an agent updating the pull request — and naming it is the
whole signal; the old tip in the report is the way back.

### Automatic Working Tree Preservation

Uncommitted changes are automatically preserved during the rebase via
`--autostash`. Users don't need to manually stash before updating.

### Current Branch Only

The update always rebases the current branch onto its upstream; `<branch>`
only names a woven branch to take from its remote, never another branch to
rebase.
