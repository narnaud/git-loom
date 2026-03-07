# status

Show the branch-aware commit graph. This is the default command when running `git-loom` with no arguments.

## Usage

```
git-loom [status] [-f] [N]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `N` | Number of context commits to show before the base (default: 1) |

### Options

| Option | Description |
|--------|-------------|
| `-f, --files` | Show files changed in each commit |

## Output

The status displays a branch-aware commit graph using UTF-8 box-drawing characters, showing commits grouped by feature branch:

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

### Sections

The graph is rendered top-to-bottom with these sections:

1. **Local changes** — shown only if the working tree has modifications, new files, or deletions. Files are split into two groups:
   - **Tracked changes** are listed first with a 2-char `XY` status matching `git status --short` (index green, worktree red).
   - **Untracked files** are listed after with a ` ⁕` marker (magenta). When there are more than 5 untracked files, they are displayed in a multi-column grid layout sized to the terminal width.

2. **Feature branches** — each branch is rendered as a side branch with its name in brackets, followed by its commits, closed with `├╯`.

3. **Loose commits** — commits not belonging to any feature branch, shown on the main integration line.

4. **Upstream marker** — the merge-base between HEAD and the upstream tracking branch.

### Symbols

| Symbol | Meaning |
|--------|---------|
| `╭─` | Start of a section |
| `├─` | Start of a subsequent branch in a stack |
| `│` | Integration line continuation |
| `││` | Continuation between stacked branches |
| `●` | A commit |
| `├╯` | End of a side branch |
| ` ⁕` | Untracked file marker (magenta) |
| `⏫` | Upstream has new commits |
| `·` | Context commit before the base (dimmed) |

### Short IDs

Each branch, commit, and file in the output is assigned a short ID — a compact identifier you can use with other git-loom commands. What you see in the status is what you type.

## Branch Topologies

### Independent branches

Each feature branch forks from the integration line independently:

```
│╭─ [feature-b]
│●   d0472f9 Fix bug in feature B
├╯
│
│╭─ [feature-a]
│●   2ee61e1 Add feature A
├╯
```

### Stacked branches

Feature-b is stacked on top of feature-a:

```
│╭─ [feature-b]
│●   4e046ab Second commit on feature-b
│●   0b85ca7 First commit on feature-b
││
│├─ [feature-a]
│●   caa87a9 Second commit on feature-a
│●   18faee8 First commit on feature-a
├╯
```

### Co-located branches

Multiple branches pointing to the same commit:

```
│╭─ [feature-a-v2]
│├─ [feature-a]
│●   2ee61e1 Add feature A
├╯
```

### Upstream ahead

When upstream has new commits beyond the common base:

```
●   abc1234 Fix typo
│
│●  [origin/main] ⏫ 3 new commits
├╯ 204e309 (common base) 2025-07-06 Merge pull request #10
```

### Context commits

Show history before the base with a positional argument (`git loom 3` or `git loom status 3`):

```
● ff1b247 (upstream) [origin/main] Initial commit
· abc1234 2025-07-05 Previous work
· def5678 2025-07-04 Earlier change
```

Context commits are dimmed and display-only (no short ID, not actionable). The default is 1 (no extra context).

## Prerequisites

- Must be on a local branch (not detached HEAD)
- Branch must have an upstream tracking branch configured
