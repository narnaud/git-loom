# add

Stage files into the git index using short IDs, paths, or `zz` for all — with optional interactive hunk selection.

## Usage

```
git-loom add [-p] [<files...>]
```

Without `-p`, at least one file argument is required. With `-p`, files are optional (omitting them shows all changed files).

### Arguments

| Argument | Description |
|----------|-------------|
| `<files...>` | Files to stage: short IDs from `loom status`, relative paths, or `zz` to stage everything |

### Options

| Option | Description |
|--------|-------------|
| `-p, --patch` | Open the interactive hunk selector TUI |

## What It Does

### Plain Staging

Resolves each argument to a file path (via short ID or filename) and stages it. If any argument is `zz`, all changes are staged immediately regardless of other arguments.

Prints `"Staged N file(s)"` on success, or `"Staged all changes"` when `zz` is used.

If no arguments are provided and `-p` is not set, the command exits with an error.

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
