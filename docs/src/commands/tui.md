# tui

An interactive, full-screen status: the branch tree on the left, the diff of the row under the cursor on the right, and loom's actions one key away.

## Usage

```
git loom tui
```

No arguments. The global `--theme` and `--no-color` options apply.

## Layout

```
┌ Status ──────────────────────────────┐┌ Diff ─────────────────────────┐
│ ╭─ zz [local changes]                ││ diff --git a/src/main.rs ...  │
│ │   ma M  src/main.rs                ││ @@ -1,3 +1,4 @@               │
│ │╭─ fa [feature-a] ✓                 ││ +new line                     │
│ │●  pop  Add parser (2 files)        ││ ...                           │
│ ├╯                                   ││                               │
│ ●  9999999 (upstream) ...            ││                               │
└──────────────────────────────────────┘└───────────────────────────────┘
 Commit: c/C | Fold: f/F | Branch: b | Drop: d | Help: ? | Quit: q
```

The tree is the one `git loom status -f` shows, with the same short IDs. Commits start collapsed; open one to see its files.

## Navigation

| Key | Effect |
|-----|--------|
| `↑`/`k`, `↓`/`j` | Move the cursor, or scroll the diff when it has focus |
| `←`/`h`, `→`/`l`, `Enter` | Close / open the current row |
| `Tab` | Switch focus between tree and diff |
| `PgUp`/`PgDn` | Scroll the diff by a page |
| `Ctrl-←` / `Ctrl-→` | Narrow / widen the tree pane |
| Mouse | Click to focus and move, wheel to scroll |
| `Space` | Select the row (`✓`) and move down |
| `+` (or `=`) / `-` | One more / one fewer context commit before the base |
| `R` / `F5` | Reload |
| `L` | Show the action log |
| `T` | Show the latest [trace](trace.md) |
| `?` | Show every key |
| `Esc` | Cancel the current mode, else clear the selection. Never quits. |
| `q` / `Ctrl-C` | Quit |

Actions take the selected rows, or the row under the cursor when nothing is selected. A selection holds one kind of row: working files, commits, branches, commit files, or the `[local changes]` header.

## Actions

| Key | Command | What it does |
|-----|---------|--------------|
| `c` | [`commit`](commit.md) | Commit the selected files (or the `[local changes]` header for everything). The new commit appears in the tree: move it with `↑`/`↓` to the integration branch or any woven branch, then `Enter`. To commit to a new branch, create it with `b` first. |
| `C` | [`add -p`](add.md) + `commit` | Pick hunks first, then place the commit as with `c`. What you pick is staged, so cancelling the placement leaves it staged. |
| `f` | [`fold`](fold.md) | Fold the selection: move to a target with `↑`/`↓` — each tagged `[AMEND]`, `[MOVE]` or `[UNCOMMIT]` — then `Enter`. |
| `F` | [`fold -p`](fold.md) | Like `f`, picking hunks first: working-tree hunks to amend into a commit, or a commit's hunks to move or uncommit. |
| `m` | [`fold --above/--below`](fold.md) | Move a commit: `↑`/`↓` show it in each place it can go, `Enter` moves it there. |
| `s` | [`split`](split.md) | On commit files: those files become the first commit. On a commit: like `S`. |
| `S` | [`split -p`](split.md) | Pick the hunks that become the first commit. |
| `b` | [`branch`](branch.md) | Type the new branch's name where it will appear: at the cursor commit or branch, else at the base. |
| `a` | [`absorb`](absorb.md) | Show where each change would go, then absorb on confirmation. |
| `d` | [`drop`](drop.md) | Drop the selected files, or the row under the cursor, after confirmation. |
| `r` | [`reword`](reword.md) | On a commit: edit its message. On a branch: rename it in place. |
| `M` | [`merge`](merge.md) | Pick a branch to merge, local or remote. |
| `U` | [`unmerge`](unmerge.md) | Take a branch out of the integration branch, after confirmation. |
| `u` | [`update`](update.md) | Fetch and rebase onto upstream. On a pushed branch, a menu also offers to take the branch's remote version, replacing its commits. |
| `p` | [`push`](push.md) | Push a branch with its stack and open its pull request. |
| `P` | [`push -f`](push.md) | Force-push, after confirmation — for a push refused because the branch changed on the remote. |

Actions run with the TUI still up. Questions appear as popups and menus, and only an editor (a commit message, a reword) takes over the terminal. When an action finishes, its result shows in the status bar, or an error popup opens, and the tree reloads. `L` shows each action's full output.

> [!NOTE]
> Git can't ask for a password inside the TUI. If `u` or `p` need credentials, use a credential helper or ssh-agent, or run the command from a terminal once.

## Conflicts

When an action pauses on a conflict — or when you open the TUI while an operation is paused — the tree gives way to the paused view. It lists the changed files, conflicts marked `!!`, and the diff pane shows conflict markers:

```
┌ Paused: loom fold — step 2/5 ────────┐┌ Diff ─────────────────────────┐
│ Conflicts — resolve them, stage each ││ <<<<<<< HEAD                  │
│                                      ││ old line                      │
│  !! src/main.rs                      ││ =======                       │
│  M  src/lib.rs                       ││ new line                      │
│                                      ││ >>>>>>> 1a2b3c4 Add parser    │
└──────────────────────────────────────┘└───────────────────────────────┘
 Continue: c | Abort: a | Stage: Space | Trace: T | Help: ? | Quit: q
```

Resolve the files in your editor, then:

| Key | Effect |
|-----|--------|
| `Space` | Stage the file under the cursor and move to the next |
| `c` | [Continue](continue.md) |
| `a` | [Abort](abort.md), after confirmation |
| `R` / `F5` | Reload, after resolving outside the TUI |

## Prerequisites

- On a branch with an upstream, as for [`status`](status.md)
- An interactive terminal: the TUI is not available in [agent mode](agent.md)
