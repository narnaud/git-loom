# absorb

Fold each uncommitted change into the commit that last touched the same lines.

## Usage

```
git loom absorb [-n] [<files>...] [-- <git args>...]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<files>...` | File short IDs or paths to absorb (default: every changed tracked file) |

### Options

| Option | Description |
|--------|-------------|
| `-n, --dry-run` | Show where each change would go, without changing anything |

### Git Options

Arguments after `--` go to the `git commit` behind each fixup, unchecked — expert mode, see [Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom absorb -- --no-verify
```

## What It Does

For each hunk, absorb finds the commit that last changed the lines it modifies or deletes, among the commits on your integration branch. Hunks of one file can go to different commits. All the fixups happen in one rewrite.

A hunk stays in the working tree when its target can't be told:

- it only adds lines, so there is nothing to trace back (use [`fold`](fold.md) instead)
- its lines come from several commits, or from upstream
- the file is binary or new

## Examples

### Absorb all changes

```bash
git loom absorb
#   src/auth.rs -> a1b2c3d "Add authentication" (feature-auth)
#   src/shared.rs [hunk 1/2] -> a1b2c3d "Add authentication" (feature-auth)
#   src/shared.rs [hunk 2/2] -> d4e5f6a "Add dashboard" (feature-dashboard)
# ✓ Absorbed 3 hunk(s) from 2 file(s) into 2 commit(s)
```

### Dry run

```bash
git loom absorb -n
#   src/auth.rs -> a1b2c3d "Add authentication" (feature-auth)
#   src/shared.rs -- skipped (pure addition)
# Dry run: would absorb 1 hunk(s) from 1 file(s) into 1 commit(s)
```

### Restrict to some files

```bash
git loom absorb src/auth.rs src/utils.rs
```

## Commits Already Upstream

If upstream already has everything a target commit changes, absorb refuses and leaves your changes in the working tree:

```bash
git loom absorb
# ✗ Commit `4783c1b` is redundant — the history below it already has its change
#   › Nothing was rewritten. Run `loom update` if it landed upstream
```

## Conflicts

If the rewrite conflicts, absorb pauses:

```bash
git loom absorb
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the absorb
#   › loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Absorbed 1 hunk(s) from 1 file(s) into 1 commit(s)
```

See [`continue`](continue.md) and [`abort`](abort.md).

## Prerequisites

- On an integration branch with uncommitted changes
