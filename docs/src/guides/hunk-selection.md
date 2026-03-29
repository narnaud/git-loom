# Selecting Hunks with -p

Sometimes you want to stage or commit only part of your changes — a few specific lines rather than entire files. The `-p` (patch) flag opens an interactive TUI that lets you pick individual hunks before the operation runs. It works with `add`, `commit`, and `fold`.

## The TUI

```
╭─ Files ──────────╮╭─ Diff ─────────────────────────╮
│ M  main.rs       ││ [✓] Hunk 1/3 (staged)          │
│ ▼ src/           ││ @@ -10,4 +10,6 @@              │
│   MM lib.rs      ││ -old line                      │
│   A  new.rs      ││ +new line                      │
│ ?? README.md     ││                                │
╰──────────────────╯╰────────────────────────────────╯
 Navigate: ↑/↓ or j/k | Switch Pane: tab | Toggle: space | Confirm: c or Enter | Quit: q or Esc
```

The left pane lists files with `git status`–style codes (`M`, `MM`, `A`, `??`, `D`). The right pane shows diff hunks — check the ones you want, leave the rest unchecked, then confirm.

| Key | Action |
|-----|--------|
| `↑` / `k`, `↓` / `j` | Navigate up/down |
| `Tab` / `Shift+Tab` | Switch between left and right pane |
| `Space` | Toggle hunk (right pane) or all hunks in file/directory (left pane) |
| `c` / `Enter` | Confirm selections |
| `q` / `Esc` / `Ctrl+C` | Cancel without changes |

## Staging hunks (`add -p`)

To stage a subset of changes before any commit:

```bash
$ git loom add -p
# Opens TUI showing all changed files
```

Filter to specific files:

```bash
$ git loom add -p src/auth.rs
$ git loom add -p a3          # using a short ID
```

## Committing hunks (`commit -p`)

Commit only selected hunks to a feature branch, without staging anything first:

```bash
$ git loom commit -b feature-auth -p
# Opens TUI for all working tree changes
# Only selected hunks are committed to feature-auth
```

You can narrow the picker to specific files:

```bash
$ git loom commit -b feature-auth -p src/auth.rs -m "partial auth fix"
# TUI shows only src/auth.rs hunks
# Other staged files are saved aside and restored after the commit
```

## Amending hunks into a past commit (`fold -p`)

Fold only selected working tree hunks into an existing commit:

```bash
$ git loom fold -p d0
# Opens TUI for all working tree changes
# Selected hunks are staged and folded into commit d0
```

Narrow to specific files by listing them before the target:

```bash
$ git loom fold -p src/auth.rs d0
# TUI shows only src/auth.rs hunks
# Selected hunks are folded into d0; the rest stay in the working tree
```

## Common patterns

### Split a file's changes across two commits

You edited `src/auth.rs` and want different hunks in different branches.

```bash
$ git loom commit -b feature-auth -p src/auth.rs -m "tighten auth check"
# Pick the first hunk → committed to feature-auth

$ git loom commit -b feature-ui -p src/auth.rs -m "restyle auth form"
# Pick the remaining hunk → committed to feature-ui
```

### Amend only part of a file into a past commit

```bash
$ git loom fold -p src/auth.rs d0
# Opens TUI filtered to src/auth.rs
# Pick only the hunks that belong in d0
# Unselected hunks stay in the working tree unchanged
```

### Stage interactively, then commit

If you prefer to separate the two steps:

```bash
$ git loom add -p
# Select exactly what to stage

$ git loom commit -b feature-auth -m "fix auth check"
# Commits whatever is staged
```

See also: [add](../commands/add.md) · [commit](../commands/commit.md) · [fold](../commands/fold.md)
