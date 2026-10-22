# commit

Create a commit on a feature branch without leaving the integration branch.

**Alias:** `ci`

## Usage

```
git loom commit [-b <branch> | -l] [-m <message>] [-p] [<files>...] [-- <git args>...]
```

### Arguments

| Argument | Description |
|----------|-------------|
| *(none)* | Commit what is staged |
| `zz` | Stage and commit all changes; wins over any other file argument |
| `<files>...` | File short IDs or paths: commit only these. Other staged files stay staged, out of the commit. |

### Options

| Option | Description |
|--------|-------------|
| `-b, --branch <branch>` | Target branch: a woven branch's name or short ID, or a new name to create it. Prompts if omitted. |
| `-l, --loose` | Commit on the integration branch itself, outside any feature branch |
| `-m, --message <message>` | Commit message. Opens your editor if omitted. |
| `-p, --patch` | Pick the hunks to commit in the [hunk picker](../guides/hunk-selection.md); files narrow it down |
| `--hunks <id>` | Select that hunk instead of opening the picker; repeat per hunk. Needs `-p` and `--hunks-from` — see [agent mode](agent.md#hunk-selection) |
| `--hunks-from <fingerprint>` | Fingerprint of the listing the `--hunks` ids came from |

### Git Options

Arguments after `--` go to `git commit`, unchecked — expert mode, see [Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom commit -m "wip" -- --no-verify
git loom commit -m "fix" -- --signoff
```

## What It Does

Creates the commit, then moves it to the top of the target branch. You stay on the integration branch.

### Choosing the branch

- **`-b <branch>`** — an existing woven branch receives the commit. A new name creates the branch at the upstream base and weaves it in. A branch that exists but is not woven is refused: [`merge`](merge.md) it first.
- **No `-b`** — a picker lists the woven branches, plus an option to create one. With no woven branch, loom asks for a name.
- **`-l`** — a loose commit on the integration branch.

When the integration branch has the same name as its upstream (`main` tracking `origin/main`), a commit without `-b` is loose and no picker opens.

### Picking hunks (`-p`)

Only the hunks you keep in the picker are committed. Everything else, staged or not, stays as it was.

### A hook that fixes files

When a `pre-commit` hook fixes files and fails, as formatters run from the `pre-commit` framework do, loom stages the fixed files and commits again, once. It only stages a fixed file again if it was fully staged, so unstaged work never gets into the commit. It does not retry when git opens the editor (no `-m`) or takes options after `--` outside agent mode, since you see the hook's output then.

`fold` of files, hunks or staged changes retries too, options after `--` or not. `absorb` does not, since a formatter rewrites whole files and the retry would put lines other commits own into one fixup: run the formatter yourself before absorbing.

```bash
git loom commit -b feature-auth -m "Check the token" src/auth.rs
# ! a commit hook changed `src/auth.rs`; committing again with its changes
# ✓ Created commit `hug` (4f825d2) on branch `feature-auth`
```

### A change that is already upstream

If upstream already has everything the commit changes, loom undoes it instead of creating an empty commit. Your changes come back:

```bash
git loom commit -b feature-auth -m "Restore the check" src/auth.rs
# ✗ Commit `4783c1b` is redundant — the history below it already has its change
#   › The `loom commit` was rolled back, so there is nothing left to drop
```

## Examples

### Commit staged changes, picking the branch

```bash
git loom commit
# ? Select target branch
# > feature-auth
#   feature-ui
# (opens the editor for the message)
```

### Commit everything to a branch

```bash
git loom commit -b feature-auth -m "add password validation" zz
# ✓ Created commit `hug` (4f825d2) on branch `feature-auth`
```

### Commit one file to a new branch

```bash
git loom commit -b feature-logging -m "add request logging" au
# ✓ Created branch `feature-logging` at `a1b2c3d`
# ✓ Created commit `sun` (6395f01) on branch `feature-logging`
```

### Loose commit on the integration branch

```bash
git loom commit -l -m "bump integration config" zz
```

### Commit some hunks of a file

```bash
git loom commit -b feature-auth -p au -m "fix auth check"
# Opens the hunk picker on src/auth.rs
```

## Conflicts

If moving the commit onto its branch conflicts, the commit pauses:

```bash
git loom commit -b feature-auth -m "add auth" zz
# ! Conflicts detected — resolve them with git, then run:
#   › loom continue   to complete the commit
#   › loom abort      to cancel and restore original state
```

```bash
git add <resolved-files> && git loom continue
# ✓ Created commit `pop` (b4c5d6e) on branch `feature-auth`
```

`git loom abort` undoes the commit and puts your changes back as they were, staged ones staged. See [`continue`](continue.md) and [`abort`](abort.md).

## Prerequisites

- On an integration branch (a branch with an upstream)
- Something to commit
