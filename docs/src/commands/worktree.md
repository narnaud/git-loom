# worktree

Work on several integration branches side by side, one per [git worktree](https://git-scm.com/docs/git-worktree).

Each worktree holds its own integration branch, tracking the same remote branch as the others. Every loom command works in a worktree exactly as it does in the main checkout: `status`, `commit`, `update`, `push`, `continue`... act on that worktree's integration branch, and a paused operation stays private to the worktree it started in.

## Usage

```
git loom worktree new <name>
git loom worktree list
git loom worktree drop <worktree>
git loom worktree cd [<worktree>]
```

`worktree` has the alias `wt`, `list` the alias `ls`, and `drop` the alias `rm`.

## Layout

For a main checkout in `repo`, the worktree `hotfix` lives beside it in `repo-hotfix` and holds `integration-hotfix`:

```
repo/          integration          (tracks origin/main)
repo-hotfix/   integration-hotfix   (tracks origin/main)
repo-bar/      integration-bar      (tracks origin/main)
```

A worktree made with plain git works too: run [`git loom init`](init.md) inside it and it gets `integration-<name>` the same way.

## One integration branch per feature branch

A feature branch is woven into one integration branch at a time. Rewriting it in one worktree (a `fold`, a `reword`...) would leave the other with its old commits, so [`branch merge`](branch.md#branch-merge) refuses a branch already woven into another worktree's integration branch. To move a branch across, unweave it where it is, then weave it where you want it:

```bash
cd ../repo-hotfix && git loom branch unmerge feature-x
cd ../repo     && git loom branch merge feature-x
```

## worktree new

```
git loom worktree new <name>
```

Creates `<repo>-<name>` beside the main checkout, whichever worktree you run it from, with `integration-<name>` at the tip of the upstream and tracking it. It tracks the same upstream as the main checkout's branch, even when you run it from a branch you switched to; when that branch has none, the upstream is chosen as [`init`](init.md) does.

```bash
git loom wt new hotfix
# ✓ Created worktree /src/repo-hotfix on integration-hotfix tracking origin/main
```

## worktree list

```
git loom worktree list
```

One line per worktree, the main checkout first: 🏠 for the main checkout or 🔗 for a linked worktree, short ID, name, checked-out branch and path, with `*` on the worktree you are in and `dirty` where there are uncommitted or untracked changes.

```bash
git loom wt ls
# 🏠 re   repo   [integration] /src/repo *
# 🔗 ho   hotfix [integration-hotfix] /src/repo-hotfix dirty
```

## worktree drop

```
git loom worktree drop <worktree>
```

`<worktree>` is a name, a branch, a short ID from `worktree list` or a path; a name wins over a short ID spelled the same. When two worktrees share a name, name the one you mean by its path.

Removes the worktree, then its `integration-<name>` branch, unless that branch holds commits no other branch, tag or remote has (loose commits on the integration branch): the branch is then kept, and a warning says how many commits only it holds. Any other checked-out branch is kept. Feature branches woven into it stay, unwoven.

`drop` refuses the main checkout, the worktree you are in, a worktree with uncommitted or untracked changes, one with an operation in progress (a paused loom command, a rebase or a merge), and a detached HEAD holding commits no branch, tag or remote has. Ignored files (build output, a local `.env`...) are not changes: they are deleted with the worktree.

```bash
git loom wt drop ho
# ✓ Removed worktree /src/repo-hotfix
# ✓ Deleted branch integration-hotfix
```

## worktree cd

```
git loom worktree cd [<worktree>]
```

Prints the worktree's path. A process cannot change its parent shell's directory, so the `loom` function the [shell setup](../shell-setup.md) defines turns it into a real `cd`. Without `<worktree>`, it goes back to the main checkout from a linked worktree, and offers a picker from the main checkout.

```bash
loom wt cd hotfix   # into repo-hotfix
loom wt cd          # back to repo
```
