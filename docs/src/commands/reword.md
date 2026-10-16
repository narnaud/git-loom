# reword

Change a commit's message, or rename a branch.

**Alias:** `rw`

## Usage

```
git loom reword <target> [-m <message>] [-- <git args>...]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | A commit (short ID or hash) or a branch (name or short ID) |

### Options

| Option | Description |
|--------|-------------|
| `-m, --message <message>` | The new message or branch name. Without it, your editor opens for a commit, a prompt for a branch. |

### Git Options

For a commit, arguments after `--` go to the `git commit --amend` that rewrites it, unchecked — expert mode, see [Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom reword mqt -- --reset-author
```

## What It Does

### Commit

Replaces the message and replays the commits above it. Content and branches are unchanged, and so is the short ID of a commit loom created. On an integration branch, any commit `loom status` shows can be reworded; outside one, any commit in HEAD's history with no merge between it and HEAD, the root included. A merge commit cannot be reworded.

### Branch

Renames the branch. A name starting with the [hidden prefix](../configuration.md#loomhidebranchpattern) prints a warning, since the branch disappears from `git loom status`.

## Target Resolution

1. **Branch names**: rename the branch
2. **Git revisions**: hashes, `HEAD`, … reword the commit
3. **Short IDs**: a branch ID renames, a commit ID rewords

To reword the commit at a branch tip, name the commit, not the branch.

## Examples

### Reword a commit

```bash
git loom reword mqt -m "Fix authentication bug in login flow"
# ✓ Updated commit message for `mqt` (was: ab12cd3, now: e45f678)
```

### Rename a branch

```bash
git loom reword fa -m feature-authentication
# ✓ Renamed branch `feature-auth` to `feature-authentication`
```

## Commits Already Upstream

If upstream already has everything the commit changes, loom refuses rather than rewrite something else:

```bash
git loom reword mqt
# ✗ Commit `4783c1b` is redundant — the history below it already has its change
#   › Nothing was rewritten. Run `loom update` if it landed upstream, or `loom drop 4783c1b -y` to remove it now
```

## Conflicts

Rewording replays the merges above the commit. A merge whose conflicts you resolved by hand conflicts again, and the reword pauses:

```bash
git loom reword mqt -m "Fix authentication bug"
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the reword
#   › loom abort      to cancel and restore original state
```

```bash
git add shared.rs && git loom continue
# ✓ Updated commit message for `mqt` (was: ab12cd3, now: e45f678)
```

With git's `rerere` enabled, your recorded resolution is replayed for you: stage it and continue. With `rerere.autoUpdate` too, the reword carries on by itself. See [`continue`](continue.md) and [`abort`](abort.md).
