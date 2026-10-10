# unmerge

Remove a branch from the integration topology without deleting the branch ref.

## Usage

```
git loom unmerge [branch]
```

## Arguments

| Argument | Description |
|----------|-------------|
| `[branch]` | Branch name or short ID (optional; shows interactive picker if omitted) |

## What It Does

1. **Branch selection** — uses the provided name/short ID, or shows an interactive picker listing woven branches
2. **Validation** — checks that the branch is actually woven into the integration branch
3. **Unweave** — rebases the integration branch to remove the branch's merge topology
4. **Preserve** — the branch ref is kept intact, pointing at its original commits

This is different from [`drop`](drop.md), which deletes the branch entirely.

If the unweave rebase encounters conflicts, it aborts automatically and reports an error — no state is saved and no `loom continue` is available.

## Examples

### Unmerge a specific branch

```bash
git loom unmerge feature-auth
# ✓ Unmerged `feature-auth` from integration branch
```

### Interactive picker

```bash
git loom unmerge
# ? Select branch to unmerge ›
#   feature-auth
#   feature-logging
# ✓ Unmerged `feature-auth` from integration branch
```
