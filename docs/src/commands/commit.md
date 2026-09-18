# commit

Create a commit on a feature branch without leaving the integration branch.

## Usage

```
git loom commit [-b <branch> | -i] [-m <message>] [-p] [files...] [-- <git args>...]
```

Alias: `ci`

### Options

| Option | Description |
|--------|-------------|
| `-b, --branch <branch>` | Target feature branch (name or short ID). Prompts if omitted. |
| `-i, --integration` | Commit to the integration branch itself (loose commit), skipping the branch prompt. Mutually exclusive with `-b`. |
| `-m, --message <message>` | Commit message. Opens editor if omitted. |
| `-p, --patch` | Interactively select hunks to stage before committing. |

### File Arguments

| Argument | Description |
|----------|-------------|
| *(none)* | Uses already-staged files (index as-is) |
| `zz` | Stages all unstaged changes (like `git add -A`) |
| *short IDs / filenames* | Stages only those specific files |

When `zz` appears alongside other file arguments, `zz` wins and stages everything.

### Git Options

Everything after a `--` separator goes to `git commit` untouched — see [Passing Options to Git](README.md#passing-options-to-git):

```bash
git loom commit -m "wip" -- --no-verify
git loom commit -m "fix" -- --signoff
git loom commit -m "port" -- "--author=Someone <someone@example.com>"
```

They shape the commit loom creates, not the rebase that relocates it onto the feature branch.

## What It Does

1. **Stage** — applies the staging rules based on file arguments
2. **Branch resolution** — determines the target feature branch
3. **Message resolution** — gets the commit message (flag or editor)
4. **Commit** — creates the commit
5. **Relocate** — moves the commit to the target feature branch, updating all branch refs and integration topology automatically

### Patch Mode

With `-p`, an interactive TUI opens before staging, letting you pick individual hunks to include in the commit. Any file arguments narrow the picker to those files; omitting them (or using `zz`) shows all changes.

If specific files are given alongside `-p`, any other staged files are saved aside first so they don't accidentally end up in the commit. They are restored automatically afterward.

### Loose Commit

When `-b` is omitted and the integration branch name matches the upstream's local counterpart (e.g. `main` tracking `origin/main`), the commit is created directly on the integration branch as a **loose commit**. No branch targeting or rebase is needed. This works regardless of whether local commits or woven branches already exist.

Branches with names that differ from their upstream (e.g. `integration` tracking `origin/main`) need `-b` or `-i`.

`-i` forces a loose commit on any integration branch, whatever its name and whatever branches are woven into it — for the occasional change that belongs to the integration branch itself. Unlike `git commit -i`, it selects the *target* of the commit, not extra paths to include.

### Branch Resolution

When the integration branch has diverged (woven branches exist):

- If `-b` matches a woven feature branch: uses it
- If `-b` matches an unwoven branch: error
- If `-b` doesn't match any branch: creates a new branch at the merge-base and weaves it
- If `-b` is omitted: interactive picker with all woven branches + option to create a new one
- If `-i` is given: no branch resolution at all — the commit lands on the integration tip

### Changes the branch already has

Step 5 replays the new commit onto the branch's base, which is the current
upstream. If everything the commit changes is already there, it has nothing left
to apply, so loom undoes the commit rather than report one it did not create.
Your changes come back, and there is no commit left to drop:

```console
$ loom commit -b feature-auth -m "Restore the check"
# ✗ Commit `4783c1b` is redundant — the history below it already has its change
#   › The `loom commit` was rolled back, so there is nothing left to drop
```

### New Branch Creation

When the target branch doesn't exist, *git-loom* validates the name, creates the branch at the merge-base, and weaves it into the integration topology — all automatically.

## Examples

### Interactive

```bash
git loom commit
# ? Select target branch
# > feature-auth
#   feature-ui
# (opens editor for commit message)
```

### Fully specified

```bash
git loom commit -b feature-auth -m "add password validation" zz
# Stages all changes, commits to feature-auth
```

### Specific files by short ID

```bash
git loom commit -b feature-auth ar -m "fix auth check"
# Stages only src/auth.rs (short ID: ar), commits to feature-auth
```

### To a new branch

```bash
git loom commit -b feature-logging -m "add request logging" zz
# Creates feature-logging, weaves it, stages all, commits
```

### Loose commit on a fresh integration branch

```bash
git loom commit -m "initial scaffold" zz
# No -b flag, branch matches remote → creates loose commit directly
```

### Loose commit on a custom-named integration branch

```bash
git loom commit -i -m "bump integration config" zz
# Commits on the integration tip, no branch picker
```

### Interactive hunk selection

```bash
git loom commit -b feature-auth -p -m "fix auth check"
# Opens hunk picker for all changes
# Only selected hunks are staged and committed to feature-auth
```

### Hunk selection for specific files

```bash
git loom commit -b feature-auth -p ar -m "partial auth fix"
# Opens hunk picker filtered to src/auth.rs
# Other staged files are saved aside and restored after the commit
```

## Conflicts

If the rebase that moves the commit to its target branch hits a conflict, the
operation is **paused** rather than aborted. The committed content is safe in
git history; loom saves recovery state to `.git/loom/state.json` and exits
with code 0.

```bash
git loom commit -b feature-auth -m "add auth" zz
# ✓ Created branch `feature-auth` at `a1b2c3d`
# ! Conflicts detected — resolve them with git, then run:
#   loom continue   to complete the commit
#   loom abort      to cancel and restore original state
```

Resolve conflicts, then:

```bash
git add <resolved-files>
git loom continue
# ✓ Created commit `b4c5d6e` on branch `feature-auth`
```

Or cancel and return to the original state (the commit content comes back as
unstaged working-tree changes):

```bash
git loom abort
# ✓ Aborted `loom commit` and restored original state
```

See [`continue`](continue.md) and [`abort`](abort.md) for details.

## Prerequisites

- Must be on an integration branch (has upstream tracking and woven feature branches)
- Must have something to commit (staged or stageable changes)
