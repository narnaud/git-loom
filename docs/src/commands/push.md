# push

Push a feature branch to the remote. Automatically detects the remote type and uses the appropriate push strategy.

## Usage

```
git-loom push [branch]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[branch]` | Branch name or short ID (optional; interactive picker if omitted) |

## Remote Type Detection

Detection priority (first match wins):

1. **Explicit config** — `git config loom.remote-type` set to `github` or `gerrit`
2. **URL heuristics** — remote URL contains `github.com` → GitHub
3. **Hook inspection** — `.git/hooks/commit-msg` contains "gerrit" → Gerrit
4. **Fallback** — Plain Git

## Push Strategies

### Plain Git (default)

```bash
git push --force-with-lease --force-if-includes -u <remote> <branch>
```

Uses `--force-with-lease` because woven branches are frequently rebased. `--force-if-includes` adds extra safety.

### GitHub

```bash
git push -u <remote> <branch>
gh pr create --web --head <branch>
```

Pushes the branch, then opens the GitHub PR creation page in the browser. If `gh` is not installed, the push succeeds with a message suggesting to install it.

### Gerrit

```bash
git push -o topic=<branch> <remote> <branch>:refs/for/<target>
```

Uses the `refs/for/` refspec and sets the topic to the branch name.

## Examples

### Push to a plain remote

```bash
git-loom push feature-a
# Pushed 'feature-a' to origin
```

### Push to GitHub

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

### Interactive selection

```bash
git-loom push
# ? Select branch to push
# > feature-a
#   feature-b
# Pushed 'feature-a' to origin
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
