# init

Initialize a new integration branch tracking a remote upstream. This is the entry point for starting a git-loom workflow.

## Usage

```
git-loom init [name]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[name]` | Branch name (optional, defaults to `integration`) |

## What It Does

1. Creates a new local branch at the upstream tip
2. Configures upstream tracking (e.g. `origin/main`)
3. Switches HEAD to the new branch

All three happen in a single atomic operation.

### Upstream Detection

The upstream is resolved automatically in priority order:

1. **Current branch's upstream** — if you're on `main` tracking `origin/main`, the integration branch will also track `origin/main`
2. **Remote scan** — scans all remotes for branches named `main`, `master`, or `develop`
3. **Interactive prompt** — if multiple candidates are found, you're asked to choose
4. **Error** — if no remote tracking branches are found

## Examples

### Default

```bash
git-loom init
# Initialized integration branch 'integration' tracking origin/main
```

### Custom name

```bash
git-loom init my-integration
# Initialized integration branch 'my-integration' tracking origin/main
```

### Error: branch already exists

```bash
git-loom init
# error: Branch 'integration' already exists
```

### Error: no remotes

```bash
git-loom init
# error: No remote tracking branches found.
# Set up a remote with: git remote add origin <url>
```

## Prerequisites

- Must be in a git repository with a working tree
- At least one remote with a fetchable branch must be configured
