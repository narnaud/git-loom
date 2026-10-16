# swap

Swap the positions of two commits in the same branch, or both on the integration line.

## Usage

```
git loom swap <a> <b>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<a>`, `<b>` | The two commits: short IDs, hashes, or any git revision. Branch names are refused. |

## What It Does

Exchanges the two commits and replays the commits above them. Both must sit in the same sequence: the same branch, or both loose on the integration line. To move a commit anywhere else, use [`fold --above/--below`](fold.md#move-a-commit-next-to-another).

## Examples

```bash
git loom swap pkz mqt
# ✓ Swapped commits `4f825d2` and `711d082`
```

```bash
git loom swap rsv mqt
# ✗ Cannot swap commits from different branch sections
```

## Commits Already Upstream

If upstream already has everything one of the commits changes, swap refuses:

```bash
git loom swap pkz mqt
# ✗ Commit `4f825d2` is redundant — the history below it already has its change
#   › Nothing was rewritten. Run `loom update` if it landed upstream, or `loom drop 4f825d2 -y` to remove it now
```

## Conflicts

If the replay conflicts, the swap pauses:

```bash
git loom swap pkz mqt
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the swap
#   › loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Swapped commits `4f825d2` and `711d082`
```

See [`continue`](continue.md) and [`abort`](abort.md).
