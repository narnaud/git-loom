# push

Push a feature branch to the remote. Automatically detects the remote type and uses the appropriate push strategy.

## Usage

```
git-loom push [branch] [--no-pr]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[branch]` | Branch name or short ID (optional; interactive picker if omitted) |

### Flags

| Flag | Description |
|------|-------------|
| `--no-pr` | Push without creating a PR or Gerrit review (see below) |

## Remote Type Detection

Detection priority (first match wins):

1. **Explicit config** — `git config loom.remote-type` set to `github`, `azure`, or `gerrit`
2. **URL heuristics** — remote URL contains `github.com` → GitHub
3. **URL heuristics** — remote URL contains `dev.azure.com` → Azure DevOps
4. **Hook inspection** — `.git/hooks/commit-msg` contains "gerrit" → Gerrit
5. **Fallback** — Plain Git

## Push Remote Selection

Detection priority (first match wins):

1. **Explicit config** — `git config loom.push-remote <remote>`
2. **GitHub fork convention** — if the integration remote is named `upstream` and `origin` exists, push to `origin`
3. **Fallback** — integration branch's remote

For non-standard fork setups (e.g., integration branch tracks `origin` but you push to `personal`), set:

```bash
git config loom.push-remote personal
```

## Push Strategies

### Plain Git (default)

```bash
git push --force-with-lease --force-if-includes -u <remote> <branch>
```

Uses `--force-with-lease` because woven branches are frequently rebased. `--force-if-includes` adds extra safety.

### GitHub

Pushes the branch with `--force-with-lease`, then checks whether a PR already exists for the branch:

- **PR exists** — prints the PR URL (`PR updated: https://github.com/owner/repo/pull/42`) without opening the browser
- **No PR** — opens the GitHub PR creation page in the browser via `gh pr create --web`

If `gh` is not installed, the push succeeds with a message suggesting to install it.

In a **fork workflow** (tracking `upstream/main`), pushes go to `origin` (your fork) and the PR targets the upstream repository automatically.

If the branch being pushed is the upstream target branch itself, PR creation is skipped.

### Azure DevOps

Pushes the branch with `--force-with-lease`, then checks whether a PR already exists for the branch:

- **PR exists** — prints the PR URL (`PR updated: https://dev.azure.com/...`) without opening the browser
- **No PR** — opens the Azure DevOps PR creation page in the browser via `az repos pr create --open`

`--detect` auto-detects the organization and project from the remote URL. If `az` is not installed, the push succeeds with a message suggesting to install it.

### Gerrit

```bash
git push -o topic=<branch> <remote> <branch>:refs/for/<target>
```

Uses the `refs/for/` refspec and sets the topic to the branch name.

## Pushing Without a PR or Review

Use `--no-pr` when you want to push a branch to the remote without triggering PR or review creation — for example, to back up a branch, share work-in-progress, or push to a staging ref.

| Remote type | `--no-pr` behavior |
|-------------|-------------------|
| Plain | Same as normal (force-with-lease push) |
| GitHub | Skips `gh pr create` |
| Azure DevOps | Skips `az repos pr create` |
| Gerrit | Plain push to branch ref instead of `refs/for/` (see below) |

### Gerrit: `wip/` prefix warning

In Gerrit, pushing directly to a branch ref (not `refs/for/`) creates a remote branch that requires a **project admin** to delete. To protect against accidental non-deletable branches, `--no-pr` on Gerrit prompts when the branch name doesn't start with `wip/`:

```
? Branch `feature-a` is not prefixed with `wip/` — a Gerrit admin will be needed to delete the remote branch later
> Push as `feature-a` (admin required to delete it later)
  Push as `wip/feature-a` instead
  Cancel
```

- **Push as-is** — pushes to `remote/feature-a`; an admin is needed to delete it later
- **Push as `wip/<branch>`** — pushes with refspec `feature-a:wip/feature-a`; your local branch name is unchanged
- **Cancel** — aborts the push

If the branch already starts with `wip/`, no prompt is shown.

## Examples

### Push to a plain remote

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
```

### Push to GitHub (new PR)

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
# (browser opens to PR creation page)
```

### Push to GitHub (PR already exists)

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
# PR updated: https://github.com/owner/repo/pull/42
```

### Push to Azure DevOps (new PR)

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
# (browser opens to PR creation page)
```

### Push to Azure DevOps (PR already exists)

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
# PR updated: https://dev.azure.com/org/project/_git/repo/pullrequest/42
```

### Push to Gerrit

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin (Gerrit: refs/for/main)
```

### Interactive selection

```bash
git-loom push
# ? Select branch to push
# > feature-a
#   feature-b
# Pushed 'feature-a' to origin
```

### Push without opening a PR (GitHub)

```bash
git-loom push feature-a --no-pr
# Pushed 'feature-a' to origin
```

### Push without a review, renaming to wip/ (Gerrit)

```bash
git-loom push feature-a --no-pr
# ? Branch `feature-a` is not prefixed with `wip/`...
# > Push as `wip/feature-a` instead
# Pushed 'feature-a' to origin as 'wip/feature-a'
```

### Override remote type

```bash
git config loom.remote-type gerrit
git-loom push feature-a
# Pushed 'feature-a' to origin (Gerrit: refs/for/main)
```

## Prerequisites

- Must be on an integration branch with upstream tracking
- The target branch must be woven into the integration branch
- Network access to the remote
- `gh` CLI (optional, for GitHub PR creation)
- `az` CLI (optional, for Azure DevOps PR creation)
