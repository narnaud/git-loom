# status

Show the branch-aware commit graph. This is the default command when running `git-loom` with no arguments.

## Usage

```
git-loom [status] [-f [COMMIT...]] [N]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `N` | Number of context commits to show before the base (default: 1) |

### Options

| Option | Description |
|--------|-------------|
| `-f, --files [COMMIT...]` | Show files changed in each commit, optionally filtered to specific commits |
| `-a, --all` | Show all branches including hidden ones |

## Output

The status displays a branch-aware commit graph using UTF-8 box-drawing characters, showing commits grouped by feature branch:

```
╭─ [local changes]
│    M file.txt
│   A  new_file.rs
│    ⁕ untracked.txt
│
│╭─ [feature-b] ✓
│●   d0472f9 Fix bug in feature B
│●   7a067a9 Start feature B
├╯
│
│╭─ [feature-a] ↑
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

2. **Feature branches** — each branch is rendered as a side branch with its name in brackets, followed by its commits, closed with `├╯`. A remote tracking indicator appears after the closing `]` when an upstream has been configured for the branch.

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
| `✓` | Branch remote is in sync (green) |
| `↑` | Branch has unpushed commits (yellow) |
| `✗` | Branch remote is gone (red) |

### Short IDs

Each branch, commit, and file in the output is assigned a short ID — a compact identifier you can use with other git-loom commands. What you see in the status is what you type.

## Showing Files

Use `-f` to show the files changed in each commit:

```
git-loom status -f
```

```
│╭─ fa [feature-a]
│●    d0 Add feature A
│┊      d0:0 M  src/feature.rs
│┊      d0:1 A  tests/feature_test.rs
├╯
```

To show files for specific commits only, pass their short IDs or git hashes after `-f`:

```
git-loom status -f d0
git-loom status -f d0 ab
git-loom status -f abc1234
```

Only the listed commits display their file list; all other commits are rendered normally. Unknown identifiers are silently ignored.

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

## Hidden Branches

Branches whose names start with the configured prefix (default: `local-`) are hidden from the status output by default. Both the branch section and its commits are fully suppressed — they do not appear as loose commits either.

This is useful for keeping local-only branches (personal configuration, secrets) out of the status view without removing them from the integration branch.

```bash
git-loom --all          # show all branches including hidden
git-loom status --all   # same, explicit
```

The hidden prefix is configurable (see [Configuration](../configuration.md#loomhidebranchpattern)).

## Theming

The graph colors adapt to the terminal background via the global `--theme` flag:

```bash
git loom --theme light status   # Light terminal background
git loom --theme dark status    # Dark terminal background
git loom --theme auto status    # Auto-detect (default)
```

See [Configuration](../configuration.md#--theme) for details.

## Prerequisites

- Must be on a local branch (not detached HEAD)
- Branch must have an upstream tracking branch configured
