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
git loom fold src/auth.rs pop -- --no-verify
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
| Commit file (`pop:1`) | Commit | **Move a file** to another commit |
| Commit file (`pop:1`) | `zz` | **Uncommit a file** |

Commit files use the `<commit>:<index>` IDs shown by [`git loom status -f`](status.md#showing-files).

## Actions

### Amend a commit

```bash
git loom fold src/auth.rs pop     # amend one file
git loom fold zz pop              # amend all changes
git add src/auth.rs && git loom fold pop   # amend what is staged
```

Unstaged changes to the same files stay in the working tree when you fold only the staged ones.

### Fix up a commit

```bash
git loom fold hug pop
# hug's changes go into pop; hug disappears, pop keeps its message
```

The source must be newer than the target.

### Move commits to a branch

```bash
git loom fold sun feature-dashboard
# ✓ Moved `sun` (was: 6395f01, now: db04256) to branch `feature-dashboard`
```

Name several commits to move them together; they keep their history order whatever order you list them in. The target can be a branch lower in a stack: the commit lands on its tip and the branches above are replayed on it.

A branch left with no commits stays, empty, at the base, and the result says so:

```bash
git loom fold pop feature-b
# ✓ Moved `pop` (was: a337eda, now: e1f2a3b) to branch `feature-b`
#   › branch feature-x now empty, at the base
```

### Move a commit next to another

`--above` and `--below` place the commit right after or right before another commit, in the same branch (a reorder), another branch, or the integration line. "Above" reads as in `git loom status`, where newer commits are higher.

```bash
git loom fold dog --below pop     # reorder within a branch
git loom fold pop --above cup     # land right after cup, in cup's branch
```

Moving a commit above a branch tip makes it the branch's new tip.

### Create a branch from commits

```bash
git loom fold -c pop sun new-feature
# Creates new-feature holding pop and sun
```

The name must be new: `-c` refuses an existing branch, so a typo can't drop your commits into another one.

### Uncommit

```bash
git loom fold hat zz       # the whole commit
git loom fold hat:1 zz     # one file of it
```

The changes come back as unstaged changes (a new file as untracked). If they don't apply cleanly — over uncommitted edits to the same files, for example — the fold fails and nothing changes. (After a `loom continue`, loom saves them to a patch file instead and says where.) A branch whose only commit this was stays, empty, at the base.

### Move a file to another commit

```bash
git loom fold hug:1 cup
# The second file of hug moves to cup
```

### Fold hunks (`-p`)

```bash
git loom fold -p pop                # working-tree hunks → amend pop
git loom fold -p src/auth.rs pop    # same, picking only from src/auth.rs
git loom fold -p hug cup            # hunks of hug → move into the older cup
git loom fold -p hat zz             # hunks of hat → uncommit
```

A binary file, a deleted file or a submodule is a single entry that moves whole.

## Commits Already Upstream

If upstream already has everything a commit you're folding from or into changes, loom refuses rather than fold into something that would vanish:

```bash
git loom fold pop feature-b
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
git loom fold pop feature-b
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the fold
#   › loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Moved `pop` (was: a337eda, now: e1f2a3b) to branch `feature-b`
```

Every other fold — moving several commits, `-c`, moving or uncommitting hunks with `-p`, and moving or uncommitting a single file — rolls back on a conflict and leaves everything as it was. See [`continue`](continue.md) and [`abort`](abort.md).
