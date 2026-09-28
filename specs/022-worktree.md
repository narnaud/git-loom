# Spec 022: Worktrees

This specification is normative.

## Model

Each worktree holds its own integration branch, tracking the same upstream as
the others. Loom needs nothing else to work in a linked worktree: every command
acts on the worktree's checked-out branch, and a paused operation is private to
its worktree (its state lives in the worktree's own git dir, Spec 014). A
feature branch is woven into one integration branch at a time; `branch merge`
refuses one woven elsewhere (Spec 005).

`worktree` has alias `wt`. Its commands only create, list and remove worktrees;
plain git worktrees are listed and removed the same way.

## Naming

The worktree `<name>` of the main worktree `<dir>` lives in the sibling
directory `<dir>-<name>` and holds `integration-<name>`: the name `init` gives
there by default (Spec 009). A worktree's name is its directory name with the
main worktree's directory name and a dash stripped from its front, or the
whole directory name when it does not start that way; the main worktree's name
is its directory name.

## CLI

```bash
git-loom worktree new <name>
git-loom worktree list
git-loom worktree drop <worktree>
```

`<worktree>` accepts a worktree's name, its checked-out branch, its short ID,
or its path, tried in that order. Names are not unique: an argument the first
matching rule finds in several worktrees is refused, naming their paths.

## Short IDs

Worktrees get IDs from their names, like branches (Spec 002), allocated after
every status entity of the current worktree so that they never change an ID
`loom status` shows. Without an integration branch in the current worktree,
only worktrees are allocated.

## new

Create `<dir>-<name>` beside the main worktree, from whichever worktree it
runs, checking out a new `integration-<name>` at the tip of the upstream and
tracking it. The upstream is the main worktree branch's, so that every
integration branch tracks the same one, even from a worktree switched to a
pushed feature branch; when it has none, it is detected as `init` does in a
linked worktree (Spec 009). Refuse, before creating anything, when git lists no checkout as
the main worktree (a submodule, a separate git dir, a bare repository), the
name holds a path separator, the branch name is invalid or taken, or the
directory exists. A failed `git worktree add` leaves no branch behind.
Success: `✓ Created worktree <path> on integration-<name>
tracking <upstream>`.

## list

One line per registered worktree, the main one first, prunable ones skipped:
`<id> <name> [<branch>] <path>`, `<branch>` being `detached` for a detached
HEAD, then `*` on the current worktree and `dirty` when `git status` reports
any change there. The lines go to stdout, `--agent` included (Spec 019).

## drop

Remove a linked worktree and its `integration-<name>` branch. Refuse, before
removing anything, on the main or the current worktree, a worktree with
uncommitted or untracked changes (or whose status fails), one with a paused
loom operation, rebase or merge, or a detached HEAD holding commits on no
branch, tag or remote-tracking ref.

Ignored files go with the directory, as with `git worktree remove`.

Only a checked-out `integration-<name>` is deleted; any other branch stays.
It is deleted only when every non-merge commit it holds is on another branch,
a tag or a remote-tracking ref, so nothing but weave merges is lost;
otherwise it is kept and named in a warning, with the count of commits only
it holds. Its feature branches stay, unwoven.
