# add

Stage files into the git index using short IDs, paths, or `zz` for all — with optional interactive hunk selection.

## Usage

```
git-loom add [-p] [<files...>] [-- <git args>...]
```

File arguments are optional: with none, `add` opens the same interactive hunk selector as `-p`, showing all changed files.

### Arguments

| Argument | Description |
|----------|-------------|
| `<files...>` | Files to stage: short IDs from `loom status`, relative paths, or `zz` to stage everything |

### Options

| Option | Description |
|--------|-------------|
| `-p, --patch` | Open the interactive hunk selector TUI |

### Git Options

Everything after a `--` separator goes to `git add` untouched, ahead of the pathspec loom builds — see [Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom add zz -- -f            # stage an ignored file too
git loom add src/main.rs -- -N   # record the path, not the content
```

Forwarded arguments run uncaptured, so git's own output (a `--dry-run` listing, `-v`) reaches you, and loom drops its own "Staged N file(s)" line — what was staged is the option's business, not loom's to claim.

Interactive staging (`-p`, or no file arguments) applies a patch rather than running `git add`, so it takes no forwarded arguments.

## What It Does

### Plain Staging

Resolves each argument to a file path (via short ID or filename) and stages it. If any argument is `zz`, all changes are staged immediately regardless of other arguments.

Prints `"Staged N file(s)"` on success, or `"Staged all changes"` when `zz` is used.

With no file arguments, `add` opens the hunk selector below instead of staging whole files.

### Interactive Hunk Staging (`-p`)

Opens a two-pane TUI showing all staged and unstaged hunks across the affected files. Staged hunks start selected; unstaged hunks start deselected. The user can toggle individual hunks (or entire files/directories) in either direction, then confirm to apply all changes atomically.

On confirm, prints `"Applied N change(s) across M file(s)"` or `"No changes to apply"` if nothing was toggled.

## File Resolution

Arguments (in both plain and `-p` modes) are resolved in this order:

1. **`zz`** — always stages everything (plain mode) or shows all files (`-p` mode)
2. **Short IDs** — file short IDs from `loom status` output (e.g. `a3`, `0f`)
3. **Plain paths** — relative file paths (e.g. `src/main.rs`)

## Examples

### Stage a file by short ID

```bash
git-loom add a3
# Staged 1 file(s)
```

### Stage multiple files

```bash
git-loom add a3 0f src/lib.rs
# Staged 3 file(s)
```

### Stage everything

```bash
git-loom add zz
# Staged all changes
```

### Interactive hunk selection for all files

```bash
git-loom add -p
# Opens TUI — confirm with c/Enter, cancel with q/Esc
```

### Interactive hunk selection for a specific file

```bash
git-loom add -p src/main.rs
# Opens TUI filtered to src/main.rs hunks
```

### Interactive hunk selection by short ID

```bash
git-loom add -p a3
# Opens TUI filtered to the file identified by short ID a3
```

## Prerequisites

- Must be in a git repository with a working tree (not bare)
- At least one file must have changes (staged, unstaged, or untracked) for `-p` mode
