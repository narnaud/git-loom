# Spec 001: Status

> **Normative.** This document defines `git loom status` behavior.

## CLI and prerequisites

The current checkout MUST be a local branch with an upstream tracking branch.

| Command | Behavior |
| --- | --- |
| `git-loom`, `git-loom status` | Show branch-aware status; status is the default command. |
| `git-loom [status] <N>` | Show `N-1` context commits before the base. Default `N` is git config `loom.statusContext`, else 1; config values below 1 or unparsable are ignored. |
| `git-loom [status] --all` | Include branches hidden by `loom.hideBranchPattern`. |
| `git-loom status -f`, `--files` | Show changed files under every displayed commit. |
| `git-loom status -f <id>...` | Show files only for identified commits/branches; accept loom commit short IDs and any hash accepted by `git rev-parse`; silently ignore unknown IDs. |

ANSI color is enabled unless `--no-color` or `NO_COLOR` disables it. Paths are relative to the current working directory, as in `git status`: from `<repo>/src`, `src/main.rs` is `main.rs`.

## Rendering

Output runs top-to-bottom in this order:

1. Local changes, only when changes exist.
2. Feature-branch sections.
3. Loose integration-line commits.
4. Upstream or common-base marker.
5. Optional context commits.

Commits use `<short-id> <short-hash> <commit-message-first-line>`: the commit's short ID (Spec 002), padded to the widest commit ID in the output, then the abbreviated hash. Hashes are unique abbreviations respecting `core.abbrev`. Merge commits have no special treatment.

| Symbol | Normative meaning |
| --- | --- |
| `╭─` | Local-changes section or first branch in a stack/group. |
| `├─` | Later branch in a stack or co-located group. |
| `│`, `││` | Integration-line or stacked-branch continuation. |
| `●` | Commit. |
| `├╯` | Side branch/stack closes into integration. |
| `!!` | Unresolved conflict; marker and filename are bold red. |
| `XY` | Tracked status from `git status --short`; index `X` is green, worktree `Y` red. Values include `M`, `A`, `D`, `R`, and space. |
| `⁕` preceded by one space | Untracked file (magenta), replacing `??`. |
| `⏫` | Upstream commits ahead of the common base. |
| `·` | Dimmed, display-only context commit with no short ID. |
| `✓` | Configured branch remote exists at local tip (green). |
| `↑` | Configured branch remote exists at another tip: local-only commits, rewritten publication, or both (yellow). |
| `✗` | Configured upstream remote ref no longer exists (red). |

### Local changes

The section begins `╭─ [local changes]`. Files are ordered by group:

1. conflicted files (`!!`);
2. tracked changes (`XY`);
3. untracked files (`⁕` preceded by one space).

With more than five untracked files on a TTY, render a terminal-width multi-column grid filled top-to-bottom then left-to-right, with columns separated by `│`. Use one column for non-TTY output or at most five files.

### Branches and ownership

A feature branch is any local branch whose tip is in `upstream..HEAD` (including HEAD) or equals the base. Exclude:

- the current integration branch;
- branches tracking the same upstream remote as the integration branch.

Each section has a bracketed header (`│╭─ [name]`), owned commits (`│●`), and `├╯`. A branch at the base is an empty header/close section. If several branches share a tip, render multiple headers over one commit set: alphabetically last on top with `│╭─`, then `│├─`.

Ownership walks parents from each branch tip and stops at the base or another branch tip. Thus a stacked branch owns only commits above the branch below it. Adjacent sections are stacked (`│├─`, `││`) when the last commit of one parents the first commit of the next. The same adjacency determines stacked pushes (Spec 011).

Feature branches normally remain parallel: each forks from the base and is woven into the integration branch by its own merge commit (Specs 004 and 006).

Append the applicable remote indicator (`✓`, `↑`, or `✗`) after `]`; show none for a never-pushed local-only branch.

### Loose commits and base

Commits owned by no feature branch appear on the main line as `●`.

- Upstream at base: `● <hash> (upstream) [<remote>/<branch>] <message>`.
- Upstream ahead: `│●  [<remote>/<branch>] ⏫ N new commits`, then `├╯ <hash> (common base) <date> <message>`.
- For context count `N > 1`, render `N-1` earlier commits after the marker as dimmed `· <hash> <date> <message>`. They are not actionable and receive no short ID.

Minimal topology examples:

```text
│╭─ [feature-b]          │╭─ [feature-a-v2]     │╭─ [feature-stale]
│●   bbbbbbb B           │├─ [feature-a]        ├╯
│├─ [feature-a]          │●   aaaaaaa A          │
│●   aaaaaaa A           ├╯                      ● base (upstream) [origin/main]
├╯
```

These respectively show stacking, co-location, and an empty base branch.

## Hidden branches

Branch names beginning with `loom.hideBranchPattern` (default `local-`) are hidden by default. Their sections and owned commits are both suppressed; owned commits MUST NOT reappear as loose commits. Empty configuration disables hiding. `--all` overrides hiding. Hidden branches remain valid targets for every other loom command.

```bash
git config loom.hideBranchPattern "local-"
git config loom.hideBranchPattern "" # disable
```
