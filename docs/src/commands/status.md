# status

Show the branch-aware status: local changes, then your branches and their commits. This is the default command, so `git loom` alone runs it.

## Usage

```
git loom [status] [-f [<commit>...]] [-n <N>] [-a]
```

### Options

| Option | Description |
|--------|-------------|
| `-f, --files [<commit>...]` | Show the files changed in each commit, or only in the commits named |
| `-n, --context <N>` | Show `N-1` commits before the base, dimmed (default: [`loom.statusContext`](../configuration.md#loomstatuscontext), else 1) |
| `-a, --all` | Also show [hidden branches](#hidden-branches) |

## Output

```
╭─ zz [local changes]
│   ma  M src/main.rs
│   nf A  new_file.rs
│   un  ⁕ untracked.txt
│
│╭─ fb [feature-b] ✓
│●    mqt  d0472f9 Fix bug in feature B
│●    pkz  7a067a9 Start feature B
├╯
│
│╭─ fa [feature-a] ↑
│●    rsv  2ee61e1 Add feature A
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

From top to bottom:

1. **Local changes** (`zz`): conflicted files (`!!`), then changed files with their `git status --short` code, then untracked files (`⁕`). Shows `no changes` when the working tree is clean.
2. **Loose commits**: commits on the integration branch that belong to no feature branch.
3. **Feature branches**: empty ones first, then each branch with its commits, newest first.
4. **Upstream**: the commit your branches are based on.

### Symbols

| Symbol | Meaning |
|--------|---------|
| `●` | A commit |
| `├─` | Next branch of a stack, or another branch at the same tip |
| `!!` | Conflicted file |
| `⁕` | Untracked file |
| `✓` | Branch is pushed and up to date |
| `↑` | Branch differs from its pushed version |
| `✗` | Branch's remote was deleted |
| `⏫` | Upstream has new commits; run [`update`](update.md) |
| `·` | Context commit before the base (`-n`) |

### Short IDs

Every branch, commit and file gets a short ID — what you see is what you type in other commands. `zz` always means all local changes.

- **Commits** made by loom get a persistent ID of letters `k`–`z`, such as `mqt`. It comes from the commit's `Change-Id` trailer, so it survives every rewrite. Other commits get a hash prefix, such as `3a`, which changes when the commit is rewritten.
- IDs are as short as they can be while staying unique. When a new commit shares an ID's first letters, both IDs grow, and the shorter form stops working rather than point at the wrong commit.
- Any longer prefix of a letter ID works too; a hash ID works from four characters, as in git.

## Showing Files

```bash
git loom status -f rsv
```

```
│╭─ fa [feature-a]
│●    rsv  2ee61e1 Add feature A
│┊      rsv:0 M  src/feature.rs
│┊      rsv:1 A  tests/feature_test.rs
├╯
```

Commit files get IDs such as `rsv:0`, which [`fold`](fold.md) accepts to move or uncommit a single file.

## Branch Layouts

```
│╭─ fb [feature-b]        │╭─ fv [feature-a-v2]     │╭─ fs [feature-stale]
│●    mqt  4e046ab B      │├─ fa [feature-a]        ├╯
││                        │●    rsv  2ee61e1 A      │
│├─ fa [feature-a]        ├╯
│●    rsv  caa87a9 A
├╯
```

From left to right: `feature-b` **stacked** on `feature-a`; two branches **sharing** one tip; an **empty** branch at the base.

When upstream has moved on since your base:

```
│●  [origin/main] ⏫ 3 new commits
├╯ 204e309 (common base) 2025-07-06 Merge pull request #10
```

## Hidden Branches

Branches starting with [`loom.hideBranchPattern`](../configuration.md#loomhidebranchpattern) (default `local-`) are hidden, commits included — handy for local-only configuration. Every other command still accepts them. `--all` shows them.

## Agent Mode

With [`--agent`](agent.md#agent-mode), the tree goes to stderr and stdout gets the same graph as JSON, under `graph`:

```json
{"status":"ok","graph":{
  "schema": 1,
  "integration_branch": "integration",
  "cwd_prefix": "",
  "local_changes": {"id": "zz", "files": [
    {"id":"ma","path":"src/main.rs","index":"M","worktree":" ","state":"tracked"}]},
  "branches": [
    {"names": [{"id":"fu","name":"feature-ui","remote":null}],
     "stacked_on": "feature-api", "stacked_on_hidden": false,
     "commits": [{"id":"qvn","hash":"9c1d044","oid":"9c1d0448…",
                  "subject":"feat(ui): settings panel","change_id":"I9c1d…","files":[]}]}
  ],
  "loose_commits": [],
  "upstream": {"label":"origin/main","base_hash":"7f3e9b0","base_oid":"7f3e9b0c…",
               "base_subject":"chore(release): 2.4.0","base_date":"2026-09-14","commits_ahead":0},
  "context_commits": []
}}
```

| Field | Meaning |
|-------|---------|
| `schema` | Bumped on any breaking change to this format |
| `cwd_prefix` | The directory paths are relative to |
| `branches[]` | In display order. `names[]` lists branches sharing one tip. |
| `stacked_on` | The branch below in a stack, or `null` |
| `stacked_on_hidden` | The branch below is hidden: `push` refuses this branch |
| `remote` | `synced`, `different`, `gone`, or `null` if never pushed |
| `commits[]` | Newest first; each branch lists only its own commits |
| `state` | `conflicted`, `tracked` or `untracked`; `index` and `worktree` hold the `XY` status code |
| `commits[].files` | Filled in with `-f` |

## Prerequisites

- On a branch with an upstream
