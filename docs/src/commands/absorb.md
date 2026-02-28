# absorb

Automatically distribute working tree changes into the commits that last touched the affected lines. Uses blame to determine the correct target for each file, then amends those commits — all in a single operation.

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

1. Parses the unified diff to find which original lines are modified or deleted
2. Blames the file at HEAD to determine which commit last touched each original line
3. If all modified/deleted lines trace to the **same in-scope commit**, the file is assigned to that commit
4. Otherwise the file is **skipped** with an explanation

After analysis, all assigned files are folded into their target commits in a single rebase operation.

## Examples

### Absorb all changes

```bash
git-loom absorb
# Absorbed 3 files into 2 commits
```

### Dry run

```bash
git-loom absorb --dry-run
# Would absorb:
#   src/auth.rs → a1b2c3d "Add authentication"
#   src/utils.rs → d4e5f6a "Add utility helpers"
# Skipped:
#   src/main.rs — modified lines span multiple commits
```

### Restrict to specific files

```bash
git-loom absorb src/auth.rs src/utils.rs
# Absorbed 2 files into 1 commit
```

## Prerequisites

- Must be on an integration branch
- Working tree must have uncommitted changes
- Target commits must be in scope (between merge-base and HEAD)
