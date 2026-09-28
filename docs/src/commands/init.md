# init

Initialize a new integration branch tracking a remote upstream. This is the entry point for starting a *git-loom* workflow.

## Usage

```
git loom init [name]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[name]` | Branch name (optional, defaults to `integration`, or `integration-<name>` in a linked worktree) |

## What It Does

1. Creates a new local branch at the upstream tip
2. Configures upstream tracking (e.g. `origin/main`)
3. Switches HEAD to the new branch

All three happen in a single atomic operation.

### Upstream Detection

The upstream is resolved automatically in priority order:

1. **Current branch's upstream** — if you're on `main` tracking `origin/main`, the integration branch will also track `origin/main`
2. **Main worktree's upstream** — in a linked worktree, the upstream of the branch checked out in the main worktree
3. **Remote scan** — scans all remotes for branches named `main`, `master`, or `develop`
4. **Interactive prompt** — if multiple candidates are found, you're asked to choose
5. **Error** — if no remote tracking branches are found

On GitHub, if a remote named `upstream` exists (the fork workflow), its default branch is used ahead of all of the above, including the main worktree's upstream.

## Examples

### Default

```bash
git loom init
# Initialized integration branch 'integration' tracking origin/main
```

### Custom name

```bash
git loom init my-integration
# Initialized integration branch 'my-integration' tracking origin/main
```

### In a linked worktree

Each worktree can hold its own integration branch. The default name comes from the worktree's directory. If that directory's name starts with `<repo>-`, where `<repo>` is the main worktree's directory name, the prefix is stripped wherever the directory is located: `<repo>-<name>` gives `integration-<name>`. Otherwise the whole directory name is used: `integration-<directory>`.

```bash
git worktree add --detach ../repo-foo
cd ../repo-foo
git loom init
# Initialized integration branch 'integration-foo' tracking origin/main
```

### Error: branch already exists

```bash
git loom init
# error: Branch 'integration' already exists
```

### Error: no remotes

```bash
git loom init
# error: No remote tracking branches found.
# Set up a remote with: git remote add origin <url>
```

## Prerequisites

- Must be in a git repository with a working tree
- At least one remote with a fetchable branch must be configured
