# Spec 001: Status

## Overview

`git loom` (or `git loom status`) displays a branch-aware commit graph in a
GitButler CLI-inspired style. It shows the commits between the current branch's
upstream tracking branch and HEAD, grouped by feature branch.

## Prerequisites

- The user must be on a local branch (not detached HEAD).
- The branch must have an upstream tracking branch configured (e.g. `origin/main`).

## Output Format

The log is rendered top-to-bottom using UTF-8 box-drawing characters:

Independent branches (each forked from integration line):

```
╭─ [local changes]
│    M file.txt
│   A  new_file.rs
│    ⁕ untracked.txt
│
│╭─ [feature-b]
│●   d0472f9 Fix bug in feature B
│●   7a067a9 Start feature B
├╯
│
│╭─ [feature-a]
│●   2ee61e1 Add feature A
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

Stacked branches (feature-b on top of feature-a):

```
│╭─ [feature-b]
│●   4e046ab B2: second commit on feature-b
│●   0b85ca7 B1: first commit on feature-b
││
│├─ [feature-a]
│●   caa87a9 A2: second commit on feature-a
│●   18faee8 A1: first commit on feature-a
├╯
│
● 2bda89d (upstream) [origin/main] Initial commit
```

Co-located branches (multiple branches pointing to the same commit):

```
│╭─ [feature-a-v2]
│├─ [feature-a]
│●   2ee61e1 Add feature A
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

When several branches share the same tip commit, they are displayed as
multiple header lines above the same set of commits. The newest branch
(alphabetically last) appears on top with `│╭─`, and additional branches
use `│├─`.

Branches at the upstream base (no commits in range):

```
│╭─ [feature-a]
│●   2ee61e1 Add feature A
├╯
│
│╭─ [feature-stale]
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

Local branches whose tip is the merge-base commit are shown as empty
branch sections (header and close, no commits) above the upstream marker.
Branches that track the same upstream remote as the integration branch
(e.g. `main` tracking `origin/main`) are excluded.

Loose commits (on the integration line, no feature branch):

```
╭─ [local changes]
│   no changes
│
●   abc1234 Fix typo
●   def5678 Refactor utils
│
● ff1b247 (upstream) [origin/main] Initial commit
```

Upstream ahead (upstream has new commits beyond the common base):

```
●   abc1234 Fix typo
│
│●  [origin/main] ⏫ 3 new commits
├╯ 204e309 (common base) 2025-07-06 Merge pull request #10
```

Context commits (history before the base):

```
● ff1b247 (upstream) [origin/main] Initial commit
· abc1234 2025-07-05 Previous work
· def5678 2025-07-04 Earlier change
```

When invoked with a positional argument (`git loom status 3` or `git loom 3`),
N-1 extra commits before the merge-base are shown below the upstream marker.
They are rendered dimmed with a `·` prefix and are display-only (no short ID,
not actionable). The default is 1 (no extra context).

### Sections (top to bottom)

1. **Local changes** (optional): shown only if the working tree has
   modifications, new files, or deletions. Introduced with `╭─ [local changes]`.
   Files are split into two groups, tracked changes first, then untracked files:
   - **Tracked changes** (staged/unstaged modifications, additions, deletions):
     each file is listed with a 2-char `XY` status (index + worktree), matching
     `git status --short`. The index char is colored green and the worktree char
     is colored red.
   - **Untracked files** (`??` status): shown after tracked changes with a ` ⁕`
     marker (magenta) instead of the `XY` status. When there are more than 5
     untracked files and output is a TTY, they are displayed in a multi-column
     grid layout (top-to-bottom, left-to-right) sized to the terminal width.
     Columns are separated by `│`. In non-TTY mode or with 5 or fewer files,
     single-column layout is used.

2. **Feature branches**: each local branch whose tip is reachable from HEAD
   (or at the merge-base) is rendered as a side branch. The branch name
   appears on its own line in brackets (`│╭─ [branch-name]`), followed by
   its commits (`│●`), and closed with `├╯`. When multiple branches share
   the same tip commit (co-located), they are shown as multiple header lines
   above the same commits, with the newest on top. Branches at the
   merge-base with no commits in range are shown as empty sections (header
   and close only).

3. **Loose commits**: commits not belonging to any detected feature branch are
   shown on the main integration line (`●`).

4. **Upstream / common base marker**: the bottom of the log shows the merge-base
   (common ancestor) between HEAD and the upstream tracking branch. When upstream
   is up-to-date: `● <hash> (upstream) [<remote>/<branch>] <message>`.
   When upstream has moved ahead, a side-branch indicator is shown:
   `│●  [<remote>/<branch>] ⏫ N new commits` followed by
   `├╯ <hash> (common base) <date> <message>`.

5. **Context commits** (optional): when a context count > 1 is given, extra
   commits before the merge-base are shown below the upstream marker, dimmed
   with a `·` prefix. These are display-only and carry no short ID.

### Symbols

| Symbol | Meaning |
|--------|---------|
| `╭─`   | Start of a section (local changes or first branch in a stack/group) |
| `├─`   | Start of a subsequent branch within a stack or co-located group |
| `│`    | Continuation of the integration line (dotted) |
| `││`   | Continuation between stacked branches |
| `●`    | A commit |
| `├╯`   | End of a side branch (or stack), merging back to integration line |
| `XY`    | 2-char file status (`X`=index, `Y`=worktree) for tracked changes, matching `git status --short`. `X` is green, `Y` is red. Values: `M` modified, `A` added, `D` deleted, `R` renamed, ` ` unchanged |
| ` ⁕`    | Untracked file marker (magenta). Replaces `??` for untracked files |
| `⏫`  | Upstream has new commits ahead of the common base |
| `·`    | Context commit before the base (dimmed, display-only) |

### Commit line format

Each commit is displayed as: `<short-hash> <first line of commit message>`

Short hashes are unique abbreviations that respect the repository's
`core.abbrev` setting.

## Branch Detection

Feature branches are detected automatically: all local branches whose tip
commit is in the range `upstream..HEAD` (inclusive of HEAD) or at the
merge-base commit are considered feature branches. The current branch (the
integration branch) is excluded from side branches. Branches that track
the same upstream remote as the integration branch (e.g. `main` tracking
`origin/main`) are also excluded.

## Hidden Branches

Branches whose names match the configured prefix (default: `local-`) are
**hidden** from the status display by default. Both the branch section and
all commits owned by the hidden branch are suppressed — they do not appear
as loose commits either. This is useful for keeping local-only branches
(secrets, personal configuration) out of the status view without removing
them from the integration branch.

The hidden prefix is configurable via:

```
git config loom.hideBranchPattern "local-"
```

Set to an empty string to disable hiding:

```
git config loom.hideBranchPattern ""
```

Pass `--all` to show all branches regardless of the configured pattern:

```
git-loom --all
git-loom status --all
```

Hidden branches remain fully accessible to all other loom commands (fold,
drop, commit, push, etc.).

## CLI

| Command | Behavior |
|---------|----------|
| `git-loom` | Shows the status (default command) |
| `git-loom status` | Shows the status (explicit) |
| `git-loom 3` | Shows status with 2 context commits before the base |
| `git-loom status 3` | Same as above (explicit) |
| `git-loom --all` | Shows all branches including hidden ones |
| `git-loom status --all` | Same as above (explicit) |

## Design Decisions

- **Colored output**: ANSI colors are used for readability.
  Colors can be disabled with `--no-color` or the `NO_COLOR` environment variable.
- **No merge commit handling**: merge commits are displayed like regular
  commits. There is no special visual treatment for merges.

## Branch Topology

Feature branches are expected to be stacked linearly on top of each other.
Given feature-a (A1→A2) and feature-b (B1→B2), the commit history is:

```
B2 → B1 → A2 → A1 → upstream
          ^          ^
          feature-a  upstream tip
^
feature-b
```

The topological walk naturally groups commits by branch in this model.
Parallel branches forking from the same point are not a supported topology.
