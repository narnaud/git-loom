# unmerge

Take a branch out of the integration branch, keeping the branch itself.

## Usage

```
git loom unmerge [<branch>]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<branch>` | A woven branch, by name or short ID. A picker opens if omitted. |

## What It Does

Removes the branch's commits from the integration branch. The branch keeps them, so [`merge`](merge.md) brings it back. To delete the branch and its commits instead, use [`drop`](drop.md).

- Branches sharing the same tip leave together, and the result names each one.
- A branch with another stacked on it is refused: unmerge the upper branch first.

## Examples

```bash
git loom unmerge feature-auth
# ✓ Unmerged `feature-auth` from integration branch
```

## Conflicts

If removing the commits conflicts, `unmerge` rolls back and leaves everything as it was.
