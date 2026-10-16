# split

Split a commit in two, choosing the files or hunks that go into the first one.

## Usage

```
git loom split [-p] [-m <message>] <target> [<files>...] [-- <git args>...]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | The commit: short ID or hash |
| `<files>...` | Files for the first commit; a picker opens if omitted. With `-p`, they narrow the hunk picker. |

### Options

| Option | Description |
|--------|-------------|
| `-m, --message <message>` | Message of the first commit. Opens your editor if omitted. |
| `-p, --patch` | Pick hunks for the first commit in the [hunk picker](../guides/hunk-selection.md) |
| `--hunks <id>` | Select that hunk instead of opening the picker; repeat per hunk. Needs `-p` and `--hunks-from` — see [agent mode](agent.md#hunk-selection) |
| `--hunks-from <fingerprint>` | Fingerprint of the listing the `--hunks` ids came from |

### Git Options

Arguments after `--` go to both `git commit`s split makes, unchecked — expert mode, see [Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom split mqt -m "Extract config" config.rs -- --no-verify
```

## What It Does

The files or hunks you pick become the first commit, with the new message. The rest stays in the second commit, which keeps the original message — and short ID, for a commit loom created.

- **By file** (default): the commit must touch at least two files, and each side gets at least one.
- **By hunk** (`-p`): works on a single-file commit too; leave at least one hunk for the second commit.

## Examples

### Pick the files

```bash
git loom split mqt
# ? Select files for the first commit
# > [x] src/validation.rs
#   [ ] src/auth.rs
# (opens the editor for the first commit's message)
# ✓ Split `291658f` into `wsl` (8c2b2fa) and `mqt` (5e1a9c2)
```

### Name the files

```bash
git loom split mqt -m "add validation helpers" src/validation.rs
```

### Pick hunks

```bash
git loom split -p mqt -m "fix: extract bounds check"
# Opens the hunk picker on the commit's changes
```

## Conflicts

`split` never pauses: if replaying the commits above the split conflicts, it rolls back and leaves everything as it was.

## Prerequisites

- The target is a commit on your integration branch, not a merge
