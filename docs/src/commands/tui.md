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
┌ Status ──────────────────────────────┐┌ Diff ─────────────────────────┐
│ ╭─ zz [local changes]                ││ diff --git a/src/main.rs ...  │
│ │   ma M  src/main.rs                ││ @@ -1,3 +1,4 @@               │
│ │╭─ fa [feature-a] ✓                 ││ +new line                     │
│ │●  mqt  Add parser (2 files)        ││ ...                           │
│ ├╯                                   ││                               │
│ ●  9999999 (upstream) ...            ││                               │
└──────────────────────────────────────┘└───────────────────────────────┘
 Navigate: ↑/↓ | Close/open: ←/→ | Select: space | Commit: c/C | ...
```

Short IDs are displayed like in `git loom status`, so the tree doubles as a cheat-sheet for manual commands. Commit rows leave out the abbreviated hash: the short ID already names the commit for every loom command.

### Navigation

| Key | Effect |
|-----|--------|
| `↑`/`k`, `↓`/`j` | Move the cursor (tree focused) or scroll the diff (diff focused) |
| `←`/`h` / `→`/`l` | Close / open the current row; `←` on a file row jumps to its parent |
| `Enter` | Toggle open/close (or confirm a fold target — see below) |
| `Tab` | Switch focus between tree and diff pane |
| `Ctrl-←` / `Ctrl-→` | Narrow / widen the left pane (2% per press, clamped to 10–90%) |
| `PgUp`/`PgDn` | Scroll the diff by a page |
| Mouse click / wheel | Focus, move, scroll |
| `R` / `F5` | Reload the tree from the repo |
| `+` (or `=`) / `-` | Show one more / one fewer context commit before the base (starts at [`loom.statusContext`](../configuration.md#loomstatuscontext), never goes below 1) |
| `Esc` | Cancel fold, commit, move, rename, or new-branch mode → clear selection → quit (first that applies) |
| `q` / `Ctrl-C` | Quit |

Commits are collapsed by default; opening one reveals a row per changed file. Local changes start expanded. Expansion state survives reloads.

### Selection

`Space` toggles selection of the current row (marked `✓`) and advances the cursor. Actions use the selection when one exists, otherwise the cursor row. Selection is cleared on reload, and collapsing a row drops the selection of the rows it hides.

A selection holds one kind of row — working files, commits, branches, commit files, or the `[local changes]` header — since no loom command takes a mixed list of targets. Pressing `Space` on a row of another kind is refused with a notice and changes nothing; press `Esc` to clear the selection and start another one.

### Actions

Every action suspends the TUI, runs the regular loom command — prompts and editors work exactly as on the command line — prints its output, waits for Enter, then reloads the tree.

| Key | Command | Arguments |
|-----|---------|-----------|
| `c` | [`commit`](commit.md) | Two-step: the selected working files — or the `[local changes]` header for everything — become the commit, then the tree shows it where it will land. `↑`/`↓` move it between the integration branch and every woven branch, `Enter` commits, `Esc` cancels. The message is prompted as usual. What you select is what goes in: the index is never consulted, so a file staged outside the TUI is committed whole or not at all. The destinations are the branches the tree already shows — to commit to a new one, create it with `b` first. |
| `C` | [`add -p`](add.md) + [`commit`](commit.md) | Same as `c`, but the hunk selector opens first: pick the hunks you want, confirm, and then place the commit as usual. It always shows every local change, whatever the cursor is on — the selection plays no part. Confirming stages what you kept, exactly as `loom add -p` would, and the commit then takes the index: the placeholder counts its files and the diff pane shows it. What you left out stays a working change. Cancelling the selector changes nothing; cancelling the placement afterwards leaves your hunks staged, ready for another `C` or a plain `loom commit` — not `c`, which names whole files and would replace what you picked. |
| `f` | [`fold`](fold.md) | Two-step: `f` captures the selection (or cursor row) as sources; `↑`/`↓` then move through the rows they can fold into, tagged with what happens there: `[AMEND]` (files into a commit, or a commit fixed up into an older one), `[MOVE]` (a commit file to another commit), `[UNCOMMIT]` (back to the local changes), `[NOOP]` (a source itself). `Enter` folds, `Esc` cancels. While picking a target, other action keys are inactive. |
| `F` | [`fold -p`](fold.md) | Like `f`, but pick the hunks first: on files or local changes, the working-tree hunks to fold into a commit (`[AMEND]`) — like `C`, the selector shows every local change, whatever the cursor or selection; on a commit, its hunks to uncommit (`[UNCOMMIT]`) or move into an older commit (`[MOVE]`). Nothing changes until the target is confirmed. |
| `m` | [`fold --above`/`--below`](fold.md) | Move one commit. `↑`/`↓` walk it through every place it can go, the tree redrawn with it there and tagged `[MOVE]`, the cursor on it: above or below any other commit, or into an empty branch. It starts where it is; `Enter` moves it, `Esc` cancels. |
| `s` | [`split`](split.md) | Split a commit in two. On commit files — the selection, or the one under the cursor — those files become the first commit and the rest of the commit the second, which keeps the original message; the editor asks for the first one's message. On a commit row, it works like `S`. At least one file must stay behind; a single-file commit splits by hunk, with `S`. |
| `S` | [`split -p`](split.md) | Like `s`, but pick the hunks first: the selector shows the whole diff of the commit under the cursor (or of the selected commit files' commit), and the hunks you keep become the first commit. Keeping none or all of them changes nothing, and a commit with a single hunk has nothing to split. |
| `b` | [`branch new`](branch.md) | The branch appears in the tree where it will land once created — at the cursor commit or branch tip (its `-t` target), else at the base — and you type its name right there. `Enter` creates it, `Esc` or an empty name cancels. |
| `a` | [`absorb`](absorb.md) | Absorb the selected working files, else the one under the cursor, else every local change. A menu shows the plan first — each file or hunk and the commit it goes into, or why it is skipped — and `Enter` on the action runs it, `Cancel` or `Esc` leaves with nothing changed. |
| `d` | [`drop`](drop.md) | Selected working files, all at once; else the cursor commit, branch, working file, or the `[local changes]` header (drops everything, like `drop zz`). A menu asks for confirmation: `Enter` on the action runs it, `Cancel` or `Esc` leaves. |
| `r` | [`reword`](reword.md) | Cursor commit: opens the editor. Cursor branch: edit the name in place on its row — `Enter` renames, `Esc` cancels. |

### Diff Pane

| Row | Diff shown |
|-----|------------|
| `[local changes]` | Staged + unstaged changes (`git diff HEAD`) |
| Working file | That file's diff; untracked files show their content as added lines |
| Branch name | Everything the branch owns (nothing while a new branch is still being named) |
| Commit | `git show` with stats and patch |
| Commit file | That file's change within the commit |
| Commit being placed with `c` | The changes it will hold |
| Commit being placed with `C` | The staged changes it will commit |

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
