# worktree

Work on several integration branches side by side, one per [git worktree](https://git-scm.com/docs/git-worktree).

**Alias:** `wt`

Each worktree holds its own integration branch, tracking the same upstream. Every loom command works there as in the main checkout, and a paused operation stays private to its worktree.

## Usage

```
git loom worktree new <name>
git loom worktree [list]
git loom worktree drop <worktree>
git loom worktree path [<worktree>]
```

`worktree` alone runs `list`. `list` has the alias `ls`, and `drop` the alias `rm`.

## Layout

For a main checkout in `repo`, the worktree `hotfix` lives beside it in `repo-hotfix`:

```
repo/          integration          (tracks origin/main)
repo-hotfix/   integration-hotfix   (tracks origin/main)
```

A worktree made with `git worktree add` works too: run [`git loom init`](init.md) inside it.

## One Integration Branch per Feature Branch

A feature branch is woven into one integration branch at a time, so [`merge`](merge.md) refuses a branch woven into another worktree. To move it, unmerge it there first:

```bash
cd ../repo-hotfix && git loom unmerge feature-x
cd ../repo        && git loom merge feature-x
```

## worktree new

Creates `<repo>-<name>` beside the main checkout, on a new `integration-<name>` branch tracking the main checkout's upstream.

```bash
git loom wt new hotfix
# ✓ Created worktree /src/repo-hotfix on integration-hotfix tracking origin/main
```

## worktree list

Lists the worktrees, main checkout first, with their short IDs. `*` marks the one you are in, `dirty` one with uncommitted changes.

```bash
git loom wt
# 🏠 re   repo   [integration] /src/repo *
# 🔗 ho   hotfix [integration-hotfix] /src/repo-hotfix dirty
```

## worktree drop

Removes a worktree, named by short ID, name, branch or path. Its integration branch is deleted too, unless it holds commits found nowhere else; then it is kept, with a warning. Woven feature branches stay.

`drop` refuses the main checkout, the worktree you are in, and a worktree with uncommitted changes or a paused operation. Ignored files are deleted with the worktree.

```bash
git loom wt drop ho
# ✓ Removed worktree /src/repo-hotfix
# ✓ Deleted branch integration-hotfix
```

## worktree path

Prints a worktree's path, for `cd`. Without an argument it prints the main checkout's path from a linked worktree, and opens a picker from the main checkout.

```bash
cd "$(git loom wt path hotfix)"   # bash, zsh
cd (git loom wt path)             # fish, PowerShell
```

```bat
for /f "delims=" %p in ('git loom wt path hotfix') do cd /d "%p"
```
