# split

Split a commit into two sequential commits by selecting which files (or hunks) go into the first.

## Usage

```
git loom split [-p] [-m <message>] <target> [<files>...] [-- <git args>...]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | Commit hash, short ID, or `HEAD` |
| `<files>...` | Files for the first commit. Shows an interactive picker if omitted. Ignored when `-p` is used. |

### Options

| Option | Description |
|--------|-------------|
| `-m, --message <message>` | Message for the first commit. Opens editor if omitted. |
| `-p, --patch` | Interactively pick individual hunks for the first commit |
| `--hunks <id>` | Take that hunk for the first commit instead of opening the picker; repeat it per hunk. Needs `-p` and `--hunks-from`; see [agent mode](agent.md). |
| `--hunks-from <fingerprint>` | Fingerprint of the listing `--hunks` came from. Loom refuses a selection taken from a diff that has since changed. |

### Git Options

Anything after a `--` separator reaches both `git commit`s split makes — see
[Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom split ab -m "Extract config" config.rs -- --no-verify
```

Loom's own `-m` goes last, so a forwarded `-m` lands in both messages ahead
of it.

Loom then checks that the result is the commit split in two: two non-empty
commits on the original parent that add up to the original tree. `--dry-run`,
`--amend`, `-a` or a pathspec that commits the working tree's copy of a file
all fail that check, and loom puts the commit back as it was rather than
rewriting history around it.

## What It Does

### File-based split (default)

Shows an interactive multi-select of all files changed in the commit. The files you pick go into the first commit; the rest stay in the second commit, which keeps the original message.

You can skip the picker by listing `<files>` on the command line. The commit must touch at least two files.

### Hunk-based split (`-p`)

Opens the hunk picker TUI showing every hunk in the commit. All hunks start **unselected** (no-op). Toggle hunks with `Space`; selected hunks go into the first commit, unselected hunks stay in the second. Works on single-file commits.

### HEAD vs non-HEAD

- **HEAD commit**: `reset --mixed HEAD~1` then re-commit in two steps — no rebase needed.
- **Non-HEAD commit**: uses an edit-and-continue rebase to pause at the target, split it, then replay descendants.

Both paths preserve any pre-existing staged changes and abort cleanly on error.

## Examples

### Split HEAD interactively by file

```bash
git loom split HEAD
# ? Select files for the first commit
# > [x] src/auth.rs
#   [ ] src/main.rs
# (opens editor for the first commit message)
# ✓ Split `abc123d` into `pkz` (def456a) and `mqt` (789bcd0)
```

### Split HEAD by file non-interactively

```bash
git loom split HEAD -m "refactor: extract auth" src/auth.rs
# ✓ Split `abc123d` into `pkz` (def456a) and `mqt` (789bcd0)
```

### Split a commit by short ID using the hunk picker

```bash
git loom split -p osy -m "fix: extract bounds check"
# (hunk picker TUI opens — toggle hunks for first commit)
# ✓ Split `ab12345` into `pkz` (cd67890) and `osy` (ef01234)
```

### Split a non-HEAD commit by file

```bash
git loom split osy -m "refactor: extract helpers" src/helpers.rs
# ✓ Split `ab12345` into `pkz` (cd67890) and `osy` (ef01234)
```

## Prerequisites

- Must be in a git repository with a working tree
- Target must be a commit (not a branch, file, or `zz`)
- Merge commits cannot be split
- File-based split requires the commit to touch at least two files
- Git ≥ 2.38
