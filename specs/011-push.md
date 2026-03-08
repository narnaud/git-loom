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
git-loom push [branch] [--no-pr]
```

**Arguments:**

- `branch` (optional): Branch name or short ID. If omitted, an interactive
  picker is shown.

**Flags:**

- `--no-pr`: Push without creating a PR or Gerrit review. For GitHub and Azure
  DevOps, skips the `gh pr create` / `az repos pr create` step. For Gerrit,
  pushes directly to the branch ref instead of `refs/for/` (see below).

**Behavior:**

- Resolves the target branch (explicit argument or interactive picker)
- Validates the branch is woven into the integration branch
- Detects the remote type
- Pushes using the appropriate strategy

## Remote Type Detection

Detection priority (first match wins):

1. **Explicit config**: `git config loom.remote-type` — values: `github`, `azure`, `gerrit`
2. **URL heuristics**: Remote URL contains `github.com` → GitHub
3. **URL heuristics**: Remote URL contains `dev.azure.com` → Azure DevOps
4. **Hook inspection**: `.git/hooks/commit-msg` contains "gerrit" (case-insensitive) → Gerrit
5. **Fallback**: Plain

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
git push --force-with-lease --force-if-includes -u <remote> <branch>
# If PR exists:
#   Pushed 'feature-a' to origin
#   PR updated: https://github.com/owner/repo/pull/42
# If no PR:
gh pr create --web --head <head> --base <target> --repo <owner/repo>
```

Pushes the branch with `--force-with-lease` (same safety as plain), then
checks whether a PR already exists for the branch using `gh pr list`. If a
PR exists, prints its URL without opening the browser. If no PR exists,
opens the GitHub PR creation page in the browser via the `gh` CLI. If `gh`
is not installed, prints a helpful message with a link to install it.

**Fork workflow:** When the integration branch tracks `upstream/main` (a fork
setup), feature branches are pushed to `origin` (the user's fork) instead.
The `--head` argument is prefixed with the fork owner (e.g. `user:branch`)
and `--repo` points to the upstream repository so the PR targets the correct
repo.

**Upstream branch skip:** If the branch being pushed is the upstream target
branch itself (e.g. pushing `main` when tracking `origin/main`), PR creation
is skipped and the push falls back to the plain force-with-lease strategy.

### Azure DevOps

```bash
git push --force-with-lease --force-if-includes -u <remote> <branch>
# If PR exists:
#   Pushed 'feature-a' to origin
#   PR updated: https://dev.azure.com/org/project/_git/repo/pullrequest/42
# If no PR:
az repos pr create --open --source-branch <branch> --target-branch <target> --detect
```

Pushes the branch with `--force-with-lease` (same safety as plain), then checks
whether a PR already exists for the branch using `az repos pr list`. If a PR exists,
prints its URL without opening the browser. If no PR exists, opens the Azure DevOps
PR creation page in the browser via the `az` CLI. `--detect` lets the Azure CLI
auto-detect the organization and project from the repository's remote URL. If `az`
is not installed, prints a helpful message with a link to install it.

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

### az CLI not installed (Azure DevOps remote)

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
# Install 'az' CLI to create pull requests: https://learn.microsoft.com/cli/azure/install-azure-cli
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

### Push to Azure DevOps (with az CLI)

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

## Gerrit --no-pr Behavior

When `--no-pr` is used with a Gerrit remote, the push goes directly to the
branch ref (not `refs/for/`), which creates or updates a remote branch rather
than a Gerrit change.

**If the branch starts with `wip/`:** pushed directly (no prompt) — Gerrit
projects typically allow users to delete their own `wip/` branches.

**If the branch does not start with `wip/`:** an interactive prompt is shown,
since creating a non-`wip/` remote branch in Gerrit requires a project admin
to delete it later:

```
? Branch `feature-a` is not prefixed with `wip/` — a Gerrit admin will be needed to delete the remote branch later
> Push as `feature-a` (admin required to delete it later)
  Push as `wip/feature-a` instead
  Cancel
```

- **Push as-is**: pushes `feature-a` to `remote/feature-a` with `--force-with-lease`
- **Push as `wip/<branch>`**: pushes with refspec `feature-a:wip/feature-a` — no admin needed to delete it
- **Cancel**: aborts with `Push cancelled`

## Design Decisions

### Force-with-lease for all pushes

Woven branches are rebased as part of normal loom operations (fold, drop,
commit). Force pushing is expected, but `--force-with-lease` prevents
accidentally overwriting changes pushed from another machine. This applies
to all remote types (plain, GitHub, and Gerrit's underlying push).

### gh CLI as optional dependency

The `gh` CLI is not required. When absent, the push still succeeds — only
the PR creation step is skipped with a helpful installation message.

### GitHub Fork Workflow

In a fork setup where the integration branch tracks `upstream/main`, the push
remote is automatically switched to `origin` (the user's fork). PR creation
targets the upstream repository by resolving the `upstream` remote URL. The
`--head` argument includes the fork owner prefix so GitHub can match the PR
source correctly.

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
- `az` CLI (optional, for Azure DevOps PR creation)
