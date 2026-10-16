# add

Stage files using short IDs, paths, or `zz` for everything — or pick individual hunks.

## Usage

```
git loom add [-p] [<files>...] [-- <git args>...]
```

With no files, `add` opens the hunk picker, as with `-p`.

### Arguments

| Argument | Description |
|----------|-------------|
| `<files>...` | File short IDs, paths, or `zz` for all changes. `zz` wins over any other argument. |

### Options

| Option | Description |
|--------|-------------|
| `-p, --patch` | Pick hunks to stage or unstage in the [hunk picker](../guides/hunk-selection.md); files narrow it down |
| `--hunks <id>` | Select that hunk instead of opening the picker; repeat per hunk. Needs `-p` and `--hunks-from` — see [agent mode](agent.md#hunk-selection) |
| `--hunks-from <fingerprint>` | Fingerprint of the listing the `--hunks` ids came from |

### Git Options

Arguments after `--` go to `git add`, unchecked — expert mode, see [Passing Options to Git](README.md#passing-options-to-git). Hunk picking takes none.

```bash
git loom add zz -- -f            # stage ignored files too
git loom add src/main.rs -- -N   # intent to add
```

## What It Does

### Staging files

Stages each named file as `git add` would.

### Picking hunks (`-p`)

The picker shows staged and unstaged hunks: staged ones start selected. Toggle them either way, then confirm with `Enter` to apply everything at once. `q` or `Esc` cancels.

## Examples

### Stage files by short ID or path

```bash
git loom add ma src/lib.rs
# ✓ Staged 2 file(s)
```

### Stage everything

```bash
git loom add zz
# ✓ Staged all changes
```

### Pick hunks in one file

```bash
git loom add -p ma
# Opens the hunk picker on src/main.rs
```

## Prerequisites

- A working tree with changes
