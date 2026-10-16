# switch

Check out any branch to test it on its own, without merging it.

**Alias:** `sw`

## Usage

```
git loom switch [<branch>]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<branch>` | A local branch (name or short ID) or a remote one such as `origin/feature-x`. A picker opens if omitted. |

## What It Does

- **Local branch**: checks it out, like `git switch`.
- **Remote branch** with no local copy: checks out its commit on a detached HEAD, creating no local branch.

Nothing is rewritten. Switch back to your integration branch the same way.

## Examples

```bash
git loom switch fx
# ✓ Switched to `feature-x`

git loom switch origin/colleague-work
# ✓ Detached HEAD at `origin/colleague-work`

git loom switch integration
# ✓ Switched to `integration`
```

## Prerequisites

- No staged changes or modified tracked files (untracked files are fine)
