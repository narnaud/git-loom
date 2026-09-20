# status

Show the branch-aware commit graph. This is the default command when running `git loom` with no arguments.

## Usage

```
git loom [status] [-f [COMMIT...]] [N]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `N` | Number of commits to show at and before the base (default: [`loom.statusContext`](../configuration.md#loomstatuscontext), else 1) |

### Options

| Option | Description |
|--------|-------------|
| `-f, --files [COMMIT...]` | Show files changed in each commit, optionally filtered to specific commits |
| `-a, --all` | Show all branches including hidden ones |

## Output

The status displays a branch-aware commit graph using UTF-8 box-drawing characters, showing commits grouped by feature branch:

```
╭─ [local changes]
│   !! conflicted.rs
│    M file.txt
│   A  new_file.rs
│    ⁕ untracked.txt
│
│╭─ fb [feature-b] ✓
│●    mqt  Fix bug in feature B d0472f9
│●    pkz  Start feature B 7a067a9
├╯
│
│╭─ fa [feature-a] ↑
│●    rsv  Add feature A 2ee61e1
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

### Sections

The graph is rendered top-to-bottom with these sections:

1. **Local changes** — shown only if the working tree has modifications, new files, or deletions. Files are split into three groups:
   - **Conflicted files** are shown first with a `!!` marker in bold red (filename also bold red). These appear during an in-progress rebase or merge.
   - **Tracked changes** are listed next with a 2-char `XY` status matching `git status --short` (index green, worktree red).
   - **Untracked files** are listed last with a `⁕` marker (magenta). When there are more than 5 untracked files, they are displayed in a multi-column grid layout sized to the terminal width.

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
| `!!` | Conflicted file marker (bold red) |
| `⁕` | Untracked file marker (magenta) |
| `⏫` | Upstream has new commits |
| `·` | Context commit before the base (dimmed) |
| `✓` | Branch remote is in sync (green) |
| `↑` | Branch tip differs from its remote (yellow) |
| `✗` | Branch remote is gone (red) |

### Short IDs

Each branch, commit, and file in the output is assigned a short ID — a compact identifier you can use with other *git-loom* commands. What you see in the status is what you type.

A commit's short ID comes first on its line, in a fixed column, with the abbreviated hash at the end of the line. Commits that carry a `Change-Id` trailer (every commit loom creates, see [`loom.changeId`](../configuration.md#loomchangeid)) get a **persistent** ID made of the letters `k`–`z`, such as `mqt`: it is derived from the Change-Id, not from the hash, so it survives `update`, `fold`, `swap`, `split`, and every other rewrite. Any longer prefix of the ID also works, and so does the full `Change-Id` value. A commit without a Change-Id — made with plain `git commit`, or cherry-picked from elsewhere — falls back to a hex prefix of its hash, such as `3a`, which changes whenever the commit is rewritten.

IDs are the shortest prefix that tells commits apart. When a new commit happens to share the first letters of an existing one, both IDs grow by a letter and the old shorter form stops resolving, so a stale ID can never point at the wrong commit.

## Showing Files

Use `-f` to show the files changed in each commit:

```
git loom status -f
```

```
│╭─ fa [feature-a]
│●    mqt  Add feature A 2ee61e1
│┊      mqt:0 M  src/feature.rs
│┊      mqt:1 A  tests/feature_test.rs
├╯
```

To show files for specific commits only, pass their short IDs or git hashes after `-f`:

```
git loom status -f mqt
git loom status -f mqt pkz
git loom status -f abc1234
```

Only the listed commits display their file list; all other commits are rendered normally. Unknown identifiers are silently ignored.

## Branch Topologies

### Independent branches

Each feature branch forks from the integration line independently:

```
│╭─ fb [feature-b]
│●    mqt  Fix bug in feature B d0472f9
├╯
│
│╭─ fa [feature-a]
│●    rsv  Add feature A 2ee61e1
├╯
```

### Stacked branches

Feature-b is stacked on top of feature-a:

```
│╭─ fb [feature-b]
│●    mqt  Second commit on feature-b 4e046ab
│●    pkz  First commit on feature-b 0b85ca7
││
│├─ fa [feature-a]
│●    rsv  Second commit on feature-a caa87a9
│●    tqn  First commit on feature-a 18faee8
├╯
```

### Co-located branches

Multiple branches pointing to the same commit:

```
│╭─ fv [feature-a-v2]
│├─ fa [feature-a]
│●    rsv  Add feature A 2ee61e1
├╯
```

### Upstream ahead

When upstream has new commits beyond the common base:

```
●    mqt  Fix typo abc1234
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

Context commits are dimmed and display-only (no short ID, not actionable). The default is `loom.statusContext`, falling back to 1: the base alone, no extra context.

## Hidden Branches

Branches whose names start with the configured prefix (default: `local-`) are hidden from the status output by default. Both the branch section and its commits are fully suppressed — they do not appear as loose commits either.

This is useful for keeping local-only branches (personal configuration, secrets) out of the status view without removing them from the integration branch.

```bash
git loom --all          # show all branches including hidden
git loom status --all   # same, explicit
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
