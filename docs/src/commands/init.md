# init

Create an integration branch tracking a remote branch, and switch to it. This is where a *git-loom* workflow starts.

## Usage

```
git loom init [<name>]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<name>` | Branch name. Default: `integration`, or `integration-<name>` in a linked [worktree](worktree.md). |

## What It Does

Creates the branch at the tip of the upstream it finds, sets it to track that upstream, and checks it out. The upstream is, in order:

1. On GitHub, the default branch of a remote named `upstream` (the fork workflow)
2. The upstream of the branch you are on, e.g. `origin/main` when you are on `main`
3. In a linked worktree, the upstream of the main checkout's branch
4. Each remote's default branch, else its `main`, `master` or `develop`, asking you to choose when there are several

In a linked worktree, the default name comes from the worktree's directory: `repo-foo` beside `repo` gives `integration-foo`, any other directory `integration-<directory>`.

### GitHub forks

When `origin` is a GitHub fork and no remote points at the original repository, `init` asks [`gh`](https://cli.github.com/) for it and offers to add it as `upstream`, fetch it and track it. That's the setup `gh repo clone` makes for a fork, so `git loom push` then pushes to your fork and opens pull requests on the original. Without `gh`, nothing changes. In agent mode, or when nobody can answer, `init` keeps tracking `origin` and prints the commands that switch to `upstream`.

## Examples

```bash
git loom init
# ✓ Initialized integration branch `integration` tracking `origin/main`

git loom init my-integration
# ✓ Initialized integration branch `my-integration` tracking `origin/main`
```

On a GitHub fork:

```bash
git loom init
#   › Adds `upstream` (git@github.com:cli/cli.git) and fetches it
# ? `origin` is a fork of `cli/cli`: track it as remote `upstream`? Yes
# ✓ Added remote `upstream` for `cli/cli`
# ✓ Initialized integration branch `integration` tracking `upstream/trunk`
```

## Prerequisites

- A remote with at least one branch
- No existing branch with the same name
