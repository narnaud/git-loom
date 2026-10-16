# Selecting Hunks with -p

To stage, commit, fold or split part of a file rather than all of it, add `-p` (patch). An interactive picker opens before the command runs. It works with `add`, `commit`, `fold` and `split`.

## The Picker

```
╭─ Files ──────────╮╭─ Diff ─────────────────────────╮
│ M  main.rs       ││ [✓] Hunk 1/3 (staged)          │
│ ▼ src/           ││ @@ -10,4 +10,6 @@              │
│   MM lib.rs      ││ -old line                      │
│   A  new.rs      ││ +new line                      │
│ ?? README.md     ││                                │
╰──────────────────╯╰────────────────────────────────╯
 Toggle: space | Confirm: Enter | Help: ? | Cancel: Esc
```

Files are on the left, their hunks on the right. Check the hunks you want, then confirm. A binary, deleted or empty file, or a submodule, has no hunks to choose from: it shows as one `Whole file` entry.

| Key | Action |
|-----|--------|
| `↑`/`k`, `↓`/`j` | Move |
| `Tab` / `Shift+Tab` | Switch pane |
| `Space` | Toggle a hunk, or every hunk of a file or directory |
| `Enter` | Confirm |
| `q` / `Esc` / `Ctrl+C` | Cancel, changing nothing |
| `?` | Help |

## Commands

| Command | Picks from | Result |
|---------|------------|--------|
| `git loom add -p [<files>]` | Local changes | Selected hunks staged, the others unstaged |
| `git loom commit -b <branch> -p [<files>]` | Local changes | Selected hunks committed to the branch |
| `git loom fold -p [<files>] <commit>` | Local changes | Selected hunks amended into the commit |
| `git loom fold -p <source> <target>` | A commit | Selected hunks moved into an older commit |
| `git loom fold -p <commit> zz` | A commit | Selected hunks uncommitted |
| `git loom split -p <commit>` | A commit | Selected hunks become the first of two commits |

Listing files narrows the picker to them. Hunks you leave out are untouched, except with `add -p`, where unticking a staged hunk unstages it.

## Example: Two Changes in One File

You edited `src/auth.rs` for two features. Send each hunk to its own branch:

```bash
git loom commit -b feature-auth -p src/auth.rs -m "tighten auth check"
# Pick the first hunk

git loom commit -b feature-ui -p src/auth.rs -m "restyle auth form"
# Pick the remaining hunk
```

See also: [add](../commands/add.md) · [commit](../commands/commit.md) · [fold](../commands/fold.md) · [split](../commands/split.md)
