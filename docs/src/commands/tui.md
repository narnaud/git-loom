# tui

Open an interactive, full-screen status view: the branch-aware tree on the left, the diff of the item under the cursor on the right, with common loom actions one keypress away.

## Usage

```
git loom tui
```

No arguments or flags. The global `--theme` and `--no-color` options apply.

## What It Does

Shows the same tree as `git loom status` with files enabled, plus a live diff pane. Navigate to the thing you see, press one key, and the underlying loom command runs with the right arguments filled in. After every action the tree reloads.

```
┌ Status ──────────────────────┐┌ Diff ─────────────────────────┐
│ ╭─ z0 [local changes]        ││ diff --git a/src/main.rs ...  │
│ │   a1 M  src/main.rs        ││ @@ -1,3 +1,4 @@               │
│ │╭─ b0 [feature-a] ✓         ││ +new line                     │
│ │● d4f2 Add parser (2 files) ││ ...                           │
│ ├╯                           ││                               │
│ ●  9999999 (upstream) ...    ││                               │
└──────────────────────────────┘└───────────────────────────────┘
 Navigate: ↑/↓ | Fold/unfold: ←/→ | Select: space | Commit: c | ...
```

Short IDs are displayed like in `git loom status`, so the tree doubles as a cheat-sheet for manual commands.

### Navigation

| Key | Effect |
|-----|--------|
| `↑`/`k`, `↓`/`j` | Move the cursor (tree focused) or scroll the diff (diff focused) |
| `→`/`l` / `←`/`h` | Unfold / fold the current row; `←` on a file row jumps to its parent |
| `Enter` | Toggle fold (or confirm a fold target — see below) |
| `Tab` | Switch focus between tree and diff pane |
| `PgUp`/`PgDn` | Scroll the diff by a page |
| Mouse click / wheel | Focus, move, scroll |
| `R` / `F5` | Reload the tree from the repo |
| `Esc` | Cancel fold mode → clear selection → quit (first that applies) |
| `q` / `Ctrl-C` | Quit |

Commits are collapsed by default; unfolding reveals one row per changed file. Local changes start expanded. Expansion state survives reloads.

### Selection

`Space` toggles selection of the current row (marked `✓`) and advances the cursor. Actions use the selection when one exists, otherwise the cursor row. Selection is cleared on reload.

### Actions

Every action suspends the TUI, runs the regular loom command — prompts and editors work exactly as on the command line — prints its output, waits for Enter, then reloads the tree.

| Key | Command | Arguments |
|-----|---------|-----------|
| `c` | [`commit`](commit.md) | Selected working files; nothing relevant selected → the index as-is. Branch and message are prompted as usual. |
| `f` | [`fold`](fold.md) | Two-step: `f` captures the selection (or cursor row) as sources; move the cursor to the target and press `Enter` (`Esc` cancels). While picking a target, other action keys are inactive. |
| `b` | [`branch new`](branch.md) | Cursor commit or branch as the `-t` target when on one; name is prompted. |
| `d` | [`drop`](drop.md) | Cursor commit, branch, or working file; confirmation prompt as usual. |
| `r` | [`reword`](reword.md) | Cursor commit (opens editor) or branch (prompts rename). |

### Diff Pane

| Row | Diff shown |
|-----|------------|
| `[local changes]` | Staged + unstaged changes (`git diff HEAD`) |
| Working file | That file's diff; untracked files show their content as added lines |
| Branch name | Everything the branch owns |
| Commit | `git show` with stats and patch |
| Commit file | That file's change within the commit |

## Conflicts

If an action pauses on conflicts, the TUI exits with the standard guidance — every other command is blocked while an operation is paused, so the TUI cannot stay open:

```bash
# ! A `loom fold` is paused due to conflicts.
#   Resolve them, then run `loom continue` to resume,
#   or `loom abort` to cancel.
```

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Must be on an integration branch with upstream tracking configured (same requirement as [`status`](status.md))
- An interactive terminal — the TUI is unavailable in [agent mode](agent.md)
