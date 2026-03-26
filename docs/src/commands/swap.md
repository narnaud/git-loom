# swap

Swap two commits within the same sequence.

## Usage

```
git loom swap <a> <b>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<a>` | Commit hash or short ID — first commit |
| `<b>` | Commit hash or short ID — second commit |

## What It Does

Swaps the positions of two commits within their shared sequence (a branch section or the integration line). All descendant commits are replayed in the new order.

Both commits must belong to the same sequence — swapping commits across different branch sections, or between a branch section and the integration line, is an error.

## Target Resolution

Accepts full OID, partial OID prefix, or 2-char short ID. Branch names are not accepted.

## Examples

### Swap two commits on the integration line

```bash
git loom swap abc123 def456
# Swapped commits `abc123` and `def456`
```

### Swap two commits in a branch section using short IDs

```bash
git loom swap aa bb
# Swapped commits `aa` and `bb`
```

### Error: commits in different branch sections

```bash
git loom swap ca1 cb1
# ! Cannot swap commits from different branch sections
```

## Conflicts

If a conflict occurs during the rebase, the operation is paused:

```bash
git loom swap abc123 def456
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the swap
#   loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Swapped `abc123` and `def456`
```

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Both commits must be woven into the current integration branch
- Both commits must be in the same sequence (same branch section or both on the integration line)
- Uncommitted working tree changes are preserved automatically
