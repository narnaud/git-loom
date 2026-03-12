# absorb

Automatically distribute working tree changes into the commits that last touched the affected lines. Uses blame to determine the correct target for each hunk, then amends those commits — all in a single operation.

## Usage

```
git-loom absorb [-n] [files...]
```

### Options

| Option | Description |
|--------|-------------|
| `-n, --dry-run` | Show what would be absorbed without making changes |

### Arguments

| Argument | Description |
|----------|-------------|
| `[files...]` | Files to restrict absorption to (default: all tracked changed files) |

## How It Works

For each file with uncommitted changes:

1. Parses the unified diff into individual hunks
2. For each hunk, blames the modified/deleted lines to find their originating commit
3. If all hunks trace to the **same in-scope commit**, the whole file is absorbed
4. If hunks trace to **different commits**, each hunk is independently absorbed into its target
5. Hunks that can't be attributed (pure additions, ambiguous) are **skipped** and left in the working tree

After analysis, all assigned hunks are folded into their target commits in a single rebase operation.

## Examples

### Absorb all changes

```bash
git-loom absorb
#   src/auth.rs -> a1b2c3d "Add authentication"
#   src/utils.rs -> d4e5f6a "Add utility helpers"
# Absorbed 2 hunk(s) from 2 file(s) into 2 commit(s)
```

### Absorb hunks into different commits

```bash
# src/shared.rs has changes in two separate regions,
# each originating from a different commit
git-loom absorb
#   src/shared.rs [hunk 1/2] -> a1b2c3d "Add login form"
#   src/shared.rs [hunk 2/2] -> d4e5f6a "Add dashboard"
# Absorbed 2 hunk(s) from 1 file(s) into 2 commit(s)
```

### Dry run

```bash
git-loom absorb --dry-run
#   src/auth.rs -> a1b2c3d "Add authentication"
#   src/shared.rs [hunk 1/2] -> d4e5f6a "Add utility helpers"
#   src/shared.rs [hunk 2/2] -- skipped (pure addition)
# Dry run: would absorb 2 hunk(s) from 2 file(s) into 2 commit(s)
```

### Restrict to specific files

```bash
git-loom absorb src/auth.rs src/utils.rs
#   src/auth.rs -> a1b2c3d "Add authentication"
#   src/utils.rs -> d4e5f6a "Add utility helpers"
# Absorbed 2 hunk(s) from 2 file(s) into 2 commit(s)
```

## Prerequisites

- Must be on an integration branch
- Working tree must have uncommitted changes
- Target commits must be in scope (between merge-base and HEAD)
