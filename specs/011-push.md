# Spec 011: Push

## Overview

`git loom push` pushes a single woven feature branch to the remote. It detects
the remote type (plain Git, GitHub, Gerrit) and uses the appropriate push
strategy. It never pushes the integration branch or multiple branches at once.

## Why Push?

When working with stacked/woven feature branches, pushing a branch to a remote
varies significantly depending on the hosting platform:

- **Plain Git** needs `--force-with-lease` because the branch is rebased
- **GitHub** benefits from opening a pull request after pushing
- **Gerrit** requires a special `refs/for/` refspec and topic option

`git loom push` unifies these under one command with automatic remote detection.

## CLI

```bash
git-loom push [branch]
```

**Arguments:**

- `branch` (optional): Branch name or short ID. If omitted, an interactive
  picker is shown.

**Behavior:**

- Resolves the target branch (explicit argument or interactive picker)
- Validates the branch is woven into the integration branch
- Detects the remote type
- Pushes using the appropriate strategy

## Remote Type Detection

Detection priority (first match wins):

1. **Explicit config**: `git config loom.remote-type` — values: `github`, `gerrit`
2. **URL heuristics**: Remote URL contains `github.com` → GitHub
3. **Hook inspection**: `.git/hooks/commit-msg` contains "gerrit" (case-insensitive) → Gerrit
4. **Fallback**: Plain

## Push Strategies

### Plain (default)

```bash
git push --force-with-lease --force-if-includes -u <remote> <branch>
```

Uses `--force-with-lease` because woven branches are frequently rebased and
need force pushing. `--force-if-includes` adds an extra safety check that the
local ref includes the remote ref.

### GitHub

```bash
git push -u <remote> <branch>
gh pr create --web --head <branch>
```

Pushes the branch, then opens the GitHub PR creation page in the browser via
the `gh` CLI. If `gh` is not installed, prints a helpful message with a link
to install it. If a PR already exists, `gh` handles that gracefully.

### Gerrit

```bash
git push -o topic=<branch> <remote> <branch>:refs/for/<target>
```

Uses the Gerrit `refs/for/` refspec to create or update a change. Sets the
topic to the branch name for grouping related changes.

## Branch Selection

- **Explicit argument**: Resolved via `resolve_target()`, must be a woven branch
- **Interactive picker**: Lists woven branches from `info.branches` via `cliclack::select`
- No "create new" option (unlike commit — we're pushing existing branches)

## Error Cases

### No woven branches

```bash
git-loom push
# error: No woven branches to push. Create a branch with 'git loom branch' first.
```

### Branch not woven

```bash
git-loom push stray-branch
# error: Branch 'stray-branch' is not woven into the integration branch.
```

### Target is not a branch

```bash
git-loom push abc123
# error: Target must be a branch, not a commit.
```

### gh CLI not installed (GitHub remote)

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
# Install 'gh' CLI to create pull requests: https://cli.github.com
```

### Push fails (e.g., no network)

```bash
git-loom push feature-a
# error: git push ... failed:
# fatal: Could not read from remote repository.
```

## Examples

### Push to a plain remote

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
```

### Push to GitHub (with gh CLI)

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
# (browser opens to PR creation page)
```

### Push to Gerrit

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin (Gerrit: refs/for/main)
```

### Interactive branch selection

```bash
git-loom push
# ? Select branch to push
# > feature-a
#   feature-b
#   feature-c
# Pushed 'feature-a' to origin
```

### Override remote type via config

```bash
git config loom.remote-type gerrit
git-loom push feature-a
# Pushed 'feature-a' to origin (Gerrit: refs/for/main)
```

## Design Decisions

### Force-with-lease for plain pushes

Woven branches are rebased as part of normal loom operations (fold, drop,
commit). Force pushing is expected, but `--force-with-lease` prevents
accidentally overwriting changes pushed from another machine.

### No force push for GitHub

GitHub PRs track force pushes and display them in the PR timeline. A simple
push (without `--force-with-lease`) is sufficient because GitHub handles
the branch update detection on its own. If the push is rejected because
the remote has diverged, the user gets a clear error from git.

### gh CLI as optional dependency

The `gh` CLI is not required. When absent, the push still succeeds — only
the PR creation step is skipped with a helpful installation message.

### Single branch only

Pushing multiple branches at once would be confusing and error-prone.
Each push is explicit and deliberate.

## Prerequisites

- Must be in a git repository with a working tree (not bare)
- Current branch must be an integration branch (has upstream tracking)
- At least one woven branch must exist
- Network access to the remote (for `git push`)
- Git 2.38 or later (checked globally at startup)
- `gh` CLI (optional, for GitHub PR creation)
