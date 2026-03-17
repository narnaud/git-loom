# swap

Swap two commits within the same sequence, or exchange the positions of two branch sections.

## Usage

```
git-loom swap <a> <b>
```

Both arguments must be the same type — both commits or both branches. Mixing types is an error.

### Arguments

| Argument | Description |
|----------|-------------|
| `<a>` | Commit hash, short ID, or branch name — first target |
| `<b>` | Commit hash, short ID, or branch name — second target |

## What It Does

### When Arguments Are Commits

Swaps the positions of two commits within their shared sequence (a branch section or the integration line). All descendant commits are replayed in the new order.

Both commits must belong to the same sequence — swapping commits across different branch sections, or between a branch section and the integration line, is an error.

### When Arguments Are Branches

Exchanges the positions of two branch sections on the integration line. All commits in both sections are replayed in the new order. Swap is refused if any branch (including the two targets) is stacked on the other, since that would break the stacking dependency.

## Target Resolution

1. **Branch name** — exact match against a local branch ref
2. **Commit hash or short ID** — full OID, partial OID prefix, or 2-char short ID

Because branch names are checked first, a name that also happens to be a valid hash prefix resolves as a branch.

## Examples

### Swap two commits on the integration line

```bash
git-loom swap abc123 def456
# Swapped commits `abc123` and `def456`
```

### Swap two commits in a branch section using short IDs

```bash
git-loom swap aa bb
# Swapped commits `aa` and `bb`
```

### Swap two branch sections

```bash
git-loom swap feature-auth feature-ui
# Swapped branches `feature-auth` and `feature-ui`
```

### Error: commits in different branch sections

```bash
git-loom swap ca1 cb1
# ! Cannot swap commits from different branch sections
```

### Error: stacked branch dependency

```bash
git-loom swap feature-auth feature-base
# ! Cannot swap branches: 'feature-auth' is stacked on 'feature-base'
```

## Conflicts

If a conflict occurs during the rebase, the operation is paused:

```bash
git-loom swap feature-auth feature-ui
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the swap
#   loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git-loom continue
# ✓ Swapped `feature-auth` and `feature-ui`
```

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Both targets must be woven into the current integration branch
- Both targets must be the same type (both commits or both branches)
- For branch swap: no stacking dependencies between the two branches or any third branch stacked on either of them
- Uncommitted working tree changes are preserved automatically
