# fold

Fold source(s) into a target — a polymorphic command that amends files into commits, fixups commits together, moves commits between branches, or uncommits changes.

## Usage

```
git loom fold <target>
git loom fold <source>... <target>
git loom fold -p [<files>...] <target>
git loom fold -p <commit1> <commit2>
git loom fold -p <commit> zz
git loom fold --create <commit>... <new-branch>
git loom fold <commit>... --above <commit>
git loom fold <commit>... --below <commit>
```

When only a target is given, currently staged files are folded into the target commit. When two or more arguments are provided, the last argument is the target and all preceding arguments are sources. With `--above` or `--below`, the option names the target and every positional argument is a source commit.

### Options

| Option | Description |
|--------|-------------|
| `-p, --patch` | Interactively select hunks before folding. Three forms depending on argument types (see below). Cannot be combined with `-c`, which moves whole commits. |
| `--hunks <id>` | Select that hunk instead of opening the picker, for the two commit-source forms; repeat it per hunk. Needs `-p` and `--hunks-from`; see [agent mode](agent.md). |
| `--hunks-from <fingerprint>` | Fingerprint of the listing `--hunks` came from. Loom refuses a selection taken from a diff that has since changed. |
| `-c, --create` | Create a new branch and move the source commit(s) into it. |
| `--above <commit>` | Move the source commit(s) directly above (newer than) this commit. |
| `--below <commit>` | Move the source commit(s) directly below (older than) this commit. |

## Type Dispatch

The action depends on the types of the arguments, detected automatically:

| Source | Target | Action |
|--------|--------|--------|
| *(staged)* | Commit | **Amend staged**: fold currently staged files into the commit |
| File(s) | Commit | **Amend**: stage files into the commit |
| `zz` | Commit | **Amend all**: stage all changed files into the commit |
| Commit | Commit | **Fixup**: absorb source commit into target |
| Commit | Branch | **Move**: relocate commit(s) to the branch |
| Commit | `zz` | **Uncommit**: remove commit, put changes in working directory |
| CommitFile | `zz` | **Uncommit file**: remove one file from a commit to working directory |
| CommitFile | Commit | **Move file**: move one file's changes between commits |
| Commit | New branch (`-c`) | **Create**: make a new branch and move the commit(s) into it |
| Commit | Commit (`--above` / `--below`) | **Move next to**: reorder or relocate commit(s) right above or below another commit |

CommitFile sources use the `commit_sid:index` format shown by `git loom status -f` (e.g. `fa:0` for the first file in commit `fa`).

## Actions

### Fold staged files into a commit

When only a target is given, staged files are folded into the commit:

```bash
git add src/auth.rs
git loom fold ab
# Folds staged changes into commit ab
```

Only files in the git index are folded — unstaged changes to the same files are preserved. Errors with `"Nothing to commit"` if nothing is staged.

### Amend files into a commit

```bash
git loom fold src/auth.rs ab
# Stages src/auth.rs and amends it into commit ab
```

Multiple files can be folded at once:

```bash
git loom fold src/main.rs src/lib.rs HEAD
# Amends both files into the HEAD commit
```

Use `zz` to fold all working tree changes at once (staged and unstaged):

```bash
git loom fold zz ab
# Stages all changed files and amends them into commit ab
```

If `zz` is mixed with individual file arguments, `zz` takes precedence and all changed files are folded.

### Interactive hunk selection (`-p`)

With `-p`, an interactive TUI opens for hunk-level selection. There are three forms depending on the argument types.

**Form 1 — pick working-tree hunks → fold into commit:**

```bash
git loom fold -p ab
# Opens hunk picker for all working-tree changes
# Selected hunks are staged and folded into commit ab
```

A picked binary or deleted file is staged whole here, the way [`split -p`](split.md) does.

Provide file arguments before the target to narrow the picker:

```bash
git loom fold -p src/auth.rs ab
# Opens hunk picker filtered to src/auth.rs
```

**Form 2 — pick hunks from a commit → move into another commit:**

```bash
git loom fold -p c2 c1
# Opens commit-diff picker for c2
# Selected hunks are removed from c2 and added to c1
```

The source (`c2`) must be newer than the target (`c1`). A submodule or a deleted file moves whole. Binary files are not supported: picking one alongside real hunks folds the hunks and leaves it where it is, warning before it rewrites anything:

```
! Left behind, no hunk to move: logo.png
  › To move one whole, take its `<commit>:<index>` id from `loom status -f` and run `loom fold <id> c1`
```

Form 3 below leaves a binary file behind the same way; a picked deletion comes back as an unstaged deletion.

**Form 3 — pick hunks from a commit → uncommit to working tree:**

```bash
git loom fold -p ab zz
# Opens commit-diff picker for ab
# Selected hunks are removed from ab and appear as unstaged modifications
```

All `-p` forms error with `"No hunks selected"` if nothing is selected.

### Fixup a commit into another

Absorbs the source commit's changes into the target. The source disappears from history; the target keeps its message.

```bash
git loom fold c2 c1
# c2's changes are absorbed into c1, c2 disappears
```

The source commit must be newer than the target.

### Move a commit to another branch

Removes the commit from its current branch and appends it to the target branch's tip.

```bash
git loom fold d0 feature-b
# Commit d0 moves to feature-b, removed from its original branch
```

Several commits can go in one move. They are ordered oldest-first whatever order you list them in — ancestors before their descendants, and commits from unrelated branches by commit date — so they travel in a single rebase and land in history order.

```bash
git loom fold d0 d1 d2 feature-b
# d0, d1 and d2 all move to feature-b
```

A single commit can be resumed with `git loom continue` if it conflicts. A move of several rolls back instead, leaving history as it was.

A branch that ended at `d0` (a stacked branch) stays behind: it ends at the commit before, or at the base if `d0` was its only commit. It never follows the commit into `feature-b`. A branch left empty this way is named in the result:

```bash
git loom fold d0 feature-b
# ✓ Moved d0 to branch feature-b (now e1f2a3b)
#   › branch feature-x now empty, at the base
```

The target can be a branch stacked inside another one: the commit lands right after that branch's tip, and the branch stacked on top is replayed over it.

```bash
git loom fold d0 feature-a
# feature-c is stacked on feature-a: d0 becomes feature-a's tip,
# feature-c's commits now build on d0
```

### Move a commit next to another commit

`--above` and `--below` place the commit relative to another commit instead of at a branch tip. The target can be in the same branch (a reorder), in another branch, or on the integration line. "Above" and "below" read as in `git loom status`, where newer commits are drawn higher.

```bash
git loom fold d2 --below d0
# d2 is pulled down to sit right under d0

git loom fold d0 --above c1
# d0 leaves its branch and lands right after c1, in c1's branch
```

Several commits move as one block, in history order, and land together:

```bash
git loom fold d0 d1 --above c1
# c1, d0, d1
```

A branch whose tip was the target follows an `--above` move: the moved commit becomes its new tip, so `--above` a branch tip is the same as moving onto that branch — except when several branches share that tip, where all of them advance, while moving onto a named branch splits the section and advances only that one. With `--below`, the target keeps its branches. As with any move, a branch that ended at the moved commit stays behind, and a branch left empty is parked at its base and named in the result.

A move that would change nothing is refused:

```bash
git loom fold d1 --above d0
# ✗ Commit `d1` is already directly above `d0`
```

A single commit can be resumed with `git loom continue` if it conflicts. A move of several rolls back instead.

### Create a new branch and move a commit into it

Use `--create` (`-c`) to create a new branch and move the commit in one step. Works whether the commit is a loose commit on the integration line or already on an existing branch.

```bash
git loom fold -c d0 new-feature
# Creates new-feature and moves commit d0 into it
```

You can list several commits to move them all into the new branch. They are ordered oldest-first so the new branch preserves their history order.

```bash
git loom fold -c d0 d1 d2 new-feature
# Creates new-feature and moves d0, d1, d2 into it
```

Like any move of several commits, `-c` is not resumable: a conflict rolls it back rather than pausing for `git loom continue`.

`-c` creates, so a name that is already taken is refused. Moving onto a branch that exists is a plain fold, and accepting the name here would let a typo drop your commits into another branch.

```bash
git loom fold -c d0 existing-branch
# ✗ Branch `existing-branch` already exists
#   Use `loom fold <commit>... existing-branch` to move commits onto it
```

### Uncommit to the working directory

Removes a commit from history and places its changes as unstaged modifications.

```bash
git loom fold ab zz
# Removes commit ab, its changes appear as unstaged modifications
```

The changes are merged back into the working tree three-way, so a later commit
that edited nearby lines does not break the apply (`-p` hunk selections are the
exception — they carry no blob ids to merge through). It still fails when they
overlap for real, or when you have uncommitted changes in one of the same
files. Either way nothing is left half-done: history and your uncommitted
changes both go back to where they were.

If `ab` was the only commit of a branch, the branch survives, empty, at the base it built on — ready for `git loom commit -b <branch>` once the change is reworked:

```bash
git loom fold ab zz
# ✓ Uncommitted ab to working directory
#   › branch feature-x now empty, at the base
```

### Uncommit a single file

Removes one file's changes from a commit, preserving the rest of the commit.

```bash
git loom fold ab:1 zz
# Removes the second file from commit ab to the working directory
```

### Submodules

A submodule pointer moves through the index, and your submodule checkout is
never touched. Uncommitting a bump leaves it as an unstaged change; uncommitting
the commit that added a submodule leaves the directory untracked. Uncommitting a
*removal* stages the deletion instead, because a submodule that is still checked
out cannot show as deleted in the working tree.

`-p` shows a submodule as a single `(submodule)` entry: take it or leave it,
there is nothing inside to pick apart.

### Move a file between commits

Moves one file's changes from one commit to another.

```bash
git loom fold c2:1 c1
# Moves the second file from c2 to c1
```

## Arguments

Arguments can be:

- **File paths** — files with changes in the working tree
- **Commit hashes** — full or partial git hashes
- **Branch names** — local branch names
- **Short IDs** — compact IDs from `git loom status`
- **Git references** — `HEAD`, `HEAD~2`, etc.
- **`zz`** — reserved token for the unstaged working directory

## Commits that are already upstream

A fold replays the commits it touches onto the current upstream. If everything a
commit changes is already there, it has nothing left to apply, and loom refuses
rather than report a commit you never touched:

```console
$ loom fold d0 feature-b
# ✗ Commit `4783c1b` is redundant — the history below it already has its change
#   › Nothing was rewritten. Run `loom update` if it landed upstream, or `loom drop 4783c1b -y` to remove it now
```

This covers the commit you move and the commit you fold into. A redundant commit
that is neither is dropped, and loom says so.

The hint always names the redundant commit, which is not always one you would want
gone: when it is the *target* of the fold, dropping it removes what you were
folding into. Nothing is rewritten either way.

## Conflicts

The following fold operations support conflict recovery (pause/resume):

- Amend files into a non-HEAD commit
- Fixup a commit into another
- Move a commit to a branch
- Move a commit above or below another commit
- Uncommit a commit to the working directory (non-HEAD)

If a supported fold hits a conflict, the operation is paused:

```bash
git loom fold d0 feature-b
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the fold
#   loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Moved `d0` to branch `feature-b` (now `e1f2a3b`)
```

The following fold operations **do not** support pause/resume and abort immediately on conflict:

- All `-p` (patch mode) forms — any conflict causes an automatic abort and restores the original state
- Uncommit a single file (`CommitFile → zz`)
- Move a file between commits (`CommitFile → Commit`)
- Create a new branch and move a commit (`--create`)
- Any move of several commits, to a branch or with `--above`/`--below`

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Must be in a git repository with a working tree
- For short ID arguments: must have upstream tracking configured
- All operations are atomic and automatically preserve uncommitted changes
