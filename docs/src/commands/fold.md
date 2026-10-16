# fold

Fold sources into a target: amend changes into a commit, fix up one commit into another, move commits or files between branches, or uncommit.

**Aliases:** `amend`, `fixup`, `mv`, `rub`

## Usage

```
git loom fold <commit>
git loom fold <source>... <target>
git loom fold <commit>... --above <commit>
git loom fold <commit>... --below <commit>
git loom fold -c <commit>... <new-branch>
git loom fold -p [<files>...] <commit>
git loom fold -p <commit> <commit>
git loom fold -p <commit> zz
```

The last argument is the target and the others are sources. With only a target, the staged changes are folded into it. With `--above`/`--below`, the option names the target.

### Options

| Option | Description |
|--------|-------------|
| `-c, --create` | Create a new branch and move the source commits into it |
| `--above <commit>` | Move the source commits directly above (after) this commit |
| `--below <commit>` | Move the source commits directly below (before) this commit |
| `-p, --patch` | Pick hunks in the [hunk picker](../guides/hunk-selection.md) instead of folding whole files or commits |
| `--hunks <id>` | Select that hunk instead of opening the picker; repeat per hunk. Needs `-p` and `--hunks-from` — see [agent mode](agent.md#hunk-selection) |
| `--hunks-from <fingerprint>` | Fingerprint of the listing the `--hunks` ids came from |

### Git Options

Folds that amend a commit with files or hunks pass arguments after `--` to that `git commit`, unchecked — expert mode, see [Passing Options to Git](README.md#passing-options-to-git). Folds that move, fix up, or uncommit whole commits take none.

```bash
git loom fold src/auth.rs mqt -- --no-verify
```

## Type Dispatch

The action follows from what the arguments are:

| Source | Target | Action |
|--------|--------|--------|
| *(staged changes)* | Commit | **Amend** the staged changes into the commit |
| Files or `zz` | Commit | **Amend** those files (or all changes) into the commit |
| Commit | Commit | **Fix up**: merge the source into the target |
| Commit | Branch | **Move** the commit to the top of the branch |
| Commit | `--above`/`--below` commit | **Move** the commit next to another commit |
| Commit | New branch (`-c`) | **Create** a branch holding the commit |
| Commit | `zz` | **Uncommit**: back to the working tree |
| Commit file (`mqt:1`) | Commit | **Move a file** to another commit |
| Commit file (`mqt:1`) | `zz` | **Uncommit a file** |

Commit files use the `<commit>:<index>` IDs shown by [`git loom status -f`](status.md#showing-files).

## Actions

### Amend a commit

```bash
git loom fold src/auth.rs mqt     # amend one file
git loom fold zz mqt              # amend all changes
git add src/auth.rs && git loom fold mqt   # amend what is staged
```

Unstaged changes to the same files stay in the working tree when you fold only the staged ones.

### Fix up a commit

```bash
git loom fold pkz mqt
# pkz's changes go into mqt; pkz disappears, mqt keeps its message
```

The source must be newer than the target.

### Move commits to a branch

```bash
git loom fold tqn feature-dashboard
# ✓ Moved `tqn` (was: 6395f01, now: db04256) to branch `feature-dashboard`
```

Name several commits to move them together; they keep their history order whatever order you list them in. The target can be a branch lower in a stack: the commit lands on its tip and the branches above are replayed on it.

A branch left with no commits stays, empty, at the base, and the result says so:

```bash
git loom fold mqt feature-b
# ✓ Moved `mqt` (was: a337eda, now: e1f2a3b) to branch `feature-b`
#   › branch feature-x now empty, at the base
```

### Move a commit next to another

`--above` and `--below` place the commit right after or right before another commit, in the same branch (a reorder), another branch, or the integration line. "Above" reads as in `git loom status`, where newer commits are higher.

```bash
git loom fold rsv --below mqt     # reorder within a branch
git loom fold mqt --above wsl     # land right after wsl, in wsl's branch
```

Moving a commit above a branch tip makes it the branch's new tip.

### Create a branch from commits

```bash
git loom fold -c mqt tqn new-feature
# Creates new-feature holding mqt and tqn
```

The name must be new: `-c` refuses an existing branch, so a typo can't drop your commits into another one.

### Uncommit

```bash
git loom fold osy zz       # the whole commit
git loom fold osy:1 zz     # one file of it
```

The changes come back as unstaged changes (a new file as untracked). If they don't apply cleanly — over uncommitted edits to the same files, for example — the fold fails and nothing changes. (After a `loom continue`, loom saves them to a patch file instead and says where.) A branch whose only commit this was stays, empty, at the base.

### Move a file to another commit

```bash
git loom fold pkz:1 wsl
# The second file of pkz moves to wsl
```

### Fold hunks (`-p`)

```bash
git loom fold -p mqt                # working-tree hunks → amend mqt
git loom fold -p src/auth.rs mqt    # same, picking only from src/auth.rs
git loom fold -p pkz wsl            # hunks of pkz → move into the older wsl
git loom fold -p osy zz             # hunks of osy → uncommit
```

A binary file, a deleted file or a submodule is a single entry that moves whole.

## Commits Already Upstream

If upstream already has everything a commit you're folding from or into changes, loom refuses rather than fold into something that would vanish:

```bash
git loom fold mqt feature-b
# ✗ Commit `a337eda` is redundant — the history below it already has its change
#   › Nothing was rewritten. Run `loom update` if it landed upstream, or `loom drop a337eda -y` to remove it now
```

Any other such commit is dropped along the way, and loom says so.

## Conflicts

These folds pause on a conflict:

- Amending a commit other than the last one, `-p` included
- Fixing up a commit
- Moving a single commit, to a branch or with `--above`/`--below`
- Uncommitting a whole commit

```bash
git loom fold mqt feature-b
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the fold
#   › loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Moved `mqt` (was: a337eda, now: e1f2a3b) to branch `feature-b`
```

Every other fold — moving several commits, `-c`, moving or uncommitting hunks with `-p`, and moving or uncommitting a single file — rolls back on a conflict and leaves everything as it was. See [`continue`](continue.md) and [`abort`](abort.md).
