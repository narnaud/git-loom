# Spec 022: Worktree

> **Normative.** This document defines `git loom worktree` (`new`, `done`, `rm`, `list`), the worktree rail in `status`, and how loom runs inside a linked worktree.

## Model

The integration branch is checked out in the main worktree; each linked worktree holds one feature branch. A branch owned by a worktree is **never woven** while it is out: `worktree new` unweaves it, `worktree done` rebases it onto the current base and weaves it back (Specs 004, 005). Integration therefore has no branch checked out elsewhere, and the guard of Spec 004 fires only in the one degenerate state named under [`worktree done`](#worktree-done). Nothing in a worktree is ever discarded: every subcommand refuses a dirty or paused worktree.

Slots (`use`/`free`, persistent detached worktrees) are deferred; a detached worktree is listed but is not a target of `new` or `done`.

## CLI

```bash
git-loom worktree                      # = list
git-loom worktree new <branch> [path] [-t <target>]
git-loom worktree done [<worktree>]
git-loom worktree rm <worktree>
git-loom worktree list
git-loom wt ...                        # alias
```

| Argument / flag | Meaning |
| --- | --- |
| `<branch>` | Branch to give a worktree: an existing local branch (woven or not), or a new name validated as in Spec 005. |
| `[path]` | Worktree directory. Default `<loom.worktreeDir>/<branch>`; `/` in the branch name become subdirectories. |
| `-t, --target` | Fork point for a **new** branch, resolved as in Spec 005. With an existing branch, error `Branch '<name>' exists; -t applies to a new branch only`. Default: the upstream merge-base. |
| `<worktree>` | See [Resolution](#resolution). `done` without it, run inside a linked worktree, means that worktree; run elsewhere, error `Not in a worktree; name one`. |

### Configuration

| Key | Set by | Meaning |
| --- | --- | --- |
| `loom.worktreeDir` | user | Default parent of new worktrees. Default: sibling `<repo>.worktrees` of the main worktree. |
| `loom.integration` | user, optional | Integration branch override, for layouts the [main-worktree rule](#running-inside-a-worktree) cannot resolve: a bare repository with worktrees only, a main worktree parked on another branch, several integration branches. |
| `branch.<name>.loomBase` | `new`; cleared by `done`/`rm` | Base branch of `<name>` while it is in a worktree; overrides `loom.integration` for that branch. |

## Resolution

`<worktree>` accepts, in order: the branch short ID of the rail line (Spec 002), an exact local branch name, a path (absolute, or relative to the current directory) equal to a worktree's directory. The main worktree is never a target: `'<x>' is the main worktree`. A detached worktree is a target of `rm` only, by path. Unknown: `Worktree '<x>' not found`.

## Running inside a worktree

The integration context of Spec 001 is extended. HEAD on a branch resolves its **base ref** in this order: `branch.<name>.loomBase`; its upstream; `loom.integration` when set; the branch checked out in the main worktree when it has an upstream and is not HEAD. `loomBase` comes first because `push` sets an upstream on every feature branch it publishes (Spec 011): that upstream is where the branch goes, never what it forks from. The **integration branch** of `new` and `done` is HEAD's branch when its base ref is its upstream, else the local branch the base ref names. With a base ref that is a local branch (a *base-ref context*):

- the base is `merge-base(HEAD, <base>)`, the range and ownership rules of Spec 001 apply with it in place of the upstream, and the marker reads `● <hash> (base) [<base-branch>] <subject>`;
- every command that needs an integration context accepts it, with short IDs allocated over that graph; `commit` defaults to the integration line, `-b` names a branch in the range;
- `update` errors `loom update is not available on a worktree branch; run loom wt done, then update the integration branch`; `init` and `push` are unchanged;
- a loose commit in the range is a commit of HEAD's branch; there is no `⏫` line since a local base is never ahead.

Detached HEAD errors as today (Spec 001); a branch with none of the four errors `Branch '<name>' has no upstream and no integration branch could be found` with the hint `Set loom.integration, or check out the integration branch in the main worktree`. Setting a real upstream to obtain a base is forbidden: it would redirect `git push`.

Loom state lives in the worktree's own git directory (`.git/worktrees/<name>/loom/state.json`, Spec 014); a paused operation in one worktree does not block another, with one exception: `continue` and `abort` run in a feature worktree that has no state of its own also read the state of the worktree holding its integration branch, and when that is a `worktree done` of this branch, error `The operation is paused in '<integration path>'; run this there`.

## Integration worktree

`new` of a woven branch and `done` rewrite the integration branch. They always run that step in the worktree that has the integration branch checked out, whichever directory loom runs from: rewriting the ref from elsewhere would leave that worktree's index and files behind (Spec 004). When no worktree has it, error `Integration branch '<name>' is not checked out in any worktree`. Uncommitted changes there are preserved around the rewrite (autostash), and the index put back as Spec 004 requires.

## `worktree new`

1. Validate the name (Spec 005) and the path: `Path '<path>' already exists` unless it is an empty directory. Error `Branch '<name>' is checked out at '<path>'` when a worktree already has it; `'<name>' is the integration branch` for the base branch.
2. New name: create the branch at the target (Spec 005), without weaving.
3. Existing woven branch: run `branch unmerge` (Spec 005) in the [integration worktree](#integration-worktree), **hard-fail**: on conflict abort the rebase, restore the repository, create nothing, and error `Cannot unweave '<name>' automatically; run loom branch unmerge <name>, then retry`. A branch stacked on it or co-located with it is refused before anything moves: `Branch '<name>' shares commits with '<other>'; unweave them first`.
4. Existing unwoven branch: nothing moves.
5. `git worktree add <path> <name>`, set `branch.<name>.loomBase` to the integration branch.

Success prints `Created worktree for <name>` then the path, alone, on the last line. The hidden-name warning of Spec 005 applies to a new name.

## `worktree done`

Preconditions, checked before anything moves, on the target worktree:

| Condition | Exact error |
| --- | --- |
| Tracked modification, staged change, or untracked file | `Worktree '<path>' has uncommitted changes; commit or drop them there first` |
| `loom/state.json` in its git directory, or a git rebase/merge in progress | `Worktree '<path>' has a paused operation; run loom continue or loom abort there first` |
| Its branch is already woven (after a removal failure below) | `Branch '<name>' is already woven; run loom wt rm <name>` |
| Integration branch checked out in no worktree | `Integration branch '<name>' is not checked out in any worktree` |
| Paused operation in the integration worktree | As Spec 014 reports it |

Then, in order:

1. **Rebase.** With `<base>` the base of the integration branch (Spec 001), when `merge-base(<name>, <base>)` is not `<base>`, run `git rebase <base>` **in the target worktree**, where ref, index and files move together (Spec 004). Hard-fail: on conflict `git rebase --abort` there and error `Cannot rebase '<name>' onto the current base; run git rebase <hash> in '<path>', then retry`. The worktree is clean by precondition, so nothing is stashed.
2. **Weave.** `branch merge <name>` (Spec 005) in the [integration worktree](#integration-worktree): `git merge --no-ff`, no commit of the branch rewritten.
3. `git worktree remove <path>`;
4. clear `branch.<name>.loomBase`.

Removal failure after a successful weave is not an error of the weave: print `Woven <name>; worktree '<path>' could not be removed: run loom wt rm <name> from '<integration path>'` (the usual case: `done` run from inside the worktree on a platform that cannot remove the current directory). The branch is then woven and checked out there, the one state in which the guard of Spec 004 refuses rewrites below it, `update` included; `rm` ends it.

Success: `Woven <name> into <integration>` then `Removed worktree '<path>'`.

### Conflict recovery

Only the merge pauses, in the integration worktree. `command` is `"worktree done"`; `rollback` is the one `branch merge` saves (Spec 005), plus:

| Field | Meaning |
| --- | --- |
| `branch_name` | Branch being woven. |
| `worktree_path` | Its worktree, removed on `continue`. |

`loom continue`, run in the integration worktree, finishes the merge and performs steps 3–4 above with the same failure handling. `loom abort` runs `git merge --abort`; the branch stays unwoven, rebased, in its worktree. Run from the feature worktree, both error as [Running inside a worktree](#running-inside-a-worktree) says.

## `worktree rm`

Refuses on the same uncommitted-changes and paused-operation conditions as `done`. Then `git worktree remove <path>` and clear `branch.<name>.loomBase`. The branch survives with its commits, woven or not; nothing else changes. Success: `Removed worktree '<path>'`. A detached worktree is removed by path.

## `worktree list` and the status rail

The rail is `git worktree list` minus the current worktree, one line each, rendered below the base marker and above the context commits of Spec 001:

```
● 31ddbe8 (upstream) [origin/main] <subject>
┆
┆ ⌂ ta [tui-actions] ../git-loom.worktrees/tui-actions  +2 ~
┆ ⌂ ab [agent-bugfix] .claude/worktrees/agent-bugfix  +1 ⏸
┆ ⌂ ../git-loom.worktrees/scratch  detached
```

| Element | Meaning |
| --- | --- |
| `┆` | Rail: these lines are not in HEAD's history. |
| `<id>` | The branch's short ID (Spec 002), allocated with the other branch IDs; `list` prints the same IDs. Omitted when detached. |
| `<path>` | Relative to the current directory, as Spec 001 prints paths. |
| `+N` | Commits in `<base>..<tip>` for the worktree's branch, computed with its own base ref. Omitted when 0. |
| `~` | Any tracked modification or untracked file there (red). |
| `⏸` | Paused loom state or git rebase/merge in progress there. |
| `detached` | No branch checked out. |

Branches matching `loom.hideBranchPattern` hide their rail line unless `--all` (Spec 001). Bare and prunable worktrees are not listed. `worktree list` prints the rail alone, without the leading `┆`, and exits 0 with no output when there is none.

### Status flags

| Invocation | Effect |
| --- | --- |
| `status` | Rail as above, always. |
| `status --no-worktrees` | Omit the rail. |
| `status -w <worktree>`, `--worktree <worktree>` | Render the status **of that worktree**: exactly what `status` prints there (its local changes, its graph, its base marker, its rail, which then lists the current worktree), with paths relative to the current directory. The value is required, so it never swallows `<N>`. Other flags (`-f`, `--all`, `<N>`, `--no-worktrees`) apply to that view. A detached target errors `Worktree '<path>' is detached`. |

Short IDs shown by `-w <worktree>` belong to that worktree's graph: `show` and `diff` accept them with `-w <worktree>`, which shadows git's `-w` before the `--` separator as loom's `-a` already does (Specs 016, 021), while `add`, `commit`, `fold` and every other index or ref mutation refuse them: `'<id>' is in worktree '<path>'; run this command there`.

### Agent JSON (Spec 019)

The graph gains `"worktrees"`: an array in rail order of `{"id","branch","path","ahead","dirty","paused"}`; `id` and `branch` are `null` when detached. It is absent with `--no-worktrees`. `"upstream"` gains `"kind": "upstream"|"base"`, `label` being the base branch name in a base-ref context. With `-w <worktree>` the graph is that worktree's and `"worktree"` holds its path; it is `null` otherwise. `worktree list --agent` emits `{"status":"ok","worktrees":[...]}` with the same entries. `worktree new --agent` reports `{"status":"ok","branch":"<name>","path":"<path>"}`.

## Prerequisites

- Git 2.40+; the worktree loom runs in has a working tree (the repository itself may be bare, with `loom.integration` set).
- `new` and `done` require an integration branch, resolved as in [Running inside a worktree](#running-inside-a-worktree), checked out in some worktree.
- `done` requires a clean, unpaused target worktree, and no paused operation in the integration worktree (Spec 014).

## Examples

```bash
git-loom wt new tui-actions            # unwoven branch, worktree, prints the path
cd ../git-loom.worktrees/tui-actions && git-loom commit -m "feat(tui): rows"
git-loom status -w ta                  # from anywhere: that worktree's status
git-loom wt done ta                    # weave, remove
git-loom wt rm ab                      # abandon a worktree, keep the branch
```
