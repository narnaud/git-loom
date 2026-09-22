# Spec 004: Weave

> **Normative.** This document defines the graph model and rebase execution used to rewrite integration topology.

## Pipeline

History-changing commands build the repository graph, mutate it in memory, serialize a complete `--rebase-merges` todo, then execute one native interactive rebase. Loom generates the todo from the graph; it does not patch Git's generated todo.

## Graph model

### Base

The base is where the integration first-parent line meets upstream. Starting at HEAD, follow first parents and choose the first commit contained by upstream. Normally this equals `merge-base HEAD <upstream>`.

This first-parent rule is required when upstream fast-forwards to a woven branch tip: that tip may be a merge's second parent, so the ordinary merge-base is off the integration line and would incorrectly classify/discard the remaining weave. If first-parent traversal reaches a root without meeting upstream, use the merge-base.

### Branch sections

Each woven section contains:

- reset target: base or another section;
- oldest-first commits;
- canonical label;
- every branch ref at the tip, including co-located branches.

Each commit has one rebase command: `Pick` (default), `Edit` (pause; reword), or `Fixup` (absorb into previous commit; fold).

### Integration line

The base-to-HEAD first-parent line contains regular commits (optionally carrying non-woven refs) and merge entries referencing sections. Preserve existing merge messages; let Git generate messages for new merges.

## Graph construction

Walk first parents from HEAD to the base:

- For a merge, follow its second parent to collect branch commits, match refs by tip, and create a section plus merge entry.
- For a regular commit, create an integration entry and attach any branch ref at that commit as non-woven.
- Skip empty branches whose tip is the base.

Building requires an upstream-configured integration branch. Reword alone may fall back to a simpler linear rewrite outside this context.

## Mutations

Mutations MUST remain repository-free until serialization/execution.

| Mutation | Required effect |
| --- | --- |
| Drop commit | Remove it; if its section becomes empty, remove that section and merge entry. |
| Drop branch | Remove its section and merge entry. |
| Fixup commit | Move source directly after target and mark it `Fixup`. |
| Edit commit | Mark it `Edit`. |
| Add branch section / merge | Add corresponding topology, including an empty/new branch. |
| Weave branch | Move non-woven integration commits to a new section and add its merge. |
| Reassign branch | Transfer a co-located woven section to a surviving branch. |

### Move commit

Remove the commit from its current location and append it to the target branch tip, subject to:

- **Inner/stacked target:** if its ref is an `update-ref` on a commit inside another section, insert immediately after that commit and transfer only the target ref to the insertion. Replay higher commits on top; leave other refs at the old tip.
- **Co-located target:** split the section. Existing commits and other refs remain in the original section; create a stacked section containing only the moved commit for the target ref.

## Serialization

Emit branch sections first in dependency order. Each resets to its fork and ends with a label. Then emit the integration line and merges referencing those labels. Preserve existing merge messages, use Git defaults for new merges, and retain non-woven refs through `update-ref` directives.

## Execution and recovery

Replay the complete base-to-HEAD range with `--rebase-merges`. Preserve/create merge topology, branch refs, uncommitted changes, and empty commits.

Conflict policy belongs to the caller, except for a stop `rerere` resolved in full under `rerere.autoUpdate`: every weave rebase carries past that one itself (Spec 014), so no caller ever sees it.

- Resumable owners (`update`, `commit`, `absorb`, `drop commit`, `swap`, `branch merge`, supported `reword` replay, and simple supported `fold` paths) return `Stopped`, save `.git/loom/state.json`, and allow `loom continue` or `loom abort`.
- Out-of-scope paths (including `split`, excluded `fold` paths, and non-pausing reword failures) explicitly abort and restore the original repository. Reword's supported replay conflict is governed by Spec 003.

A rebase carrying `edit` steps MUST halt on a commit that replays empty (`--empty=stop`, spelled `ask` before Git 2.45). Under `--empty=drop` Git discards a commit whose changes the new base already has while still honoring its `edit` line, stopping on the commit below, and the caller rewrites that one and loses the target. A rebase whose result the caller reports MUST halt the same way, for every commit it reads back through a ref following it (`update-ref`, `_loom-track`) or counts: dropping that commit slides its ref onto the commit below, which the caller then reports as the one the user moved.

At such a halt loom MUST refuse if the emptied commit is one it is rewriting or following — every commit the todo marks `edit`, every commit the caller reads back or counts, plus any a later phase depends on — and otherwise drop it, reporting it, and carry on. This applies to `continue` as well: `--empty` outlives `git rebase --continue`, and a halt there is not a conflict, so a resumable owner MUST record the commits it protects in `LoomState`. The refusal MAY offer `loom drop` only for a commit the user named; for a commit the operation lands on (a fold or absorb target) dropping it leaves nothing to land on.

A pause MUST still be verified before anything is rewritten: the commit git recorded for the stop (`stopped-sha`) MUST be the one marked `Edit`, falling back to author and message where git recorded none, the replayed hash being new. A failed verification MUST abort the rebase, restore, and rewrite nothing. A todo that marks no `edit` for the requested commit MUST fail before the rebase starts: once it completes there is nothing left to abort.

| Command | Graph operations |
| --- | --- |
| branch (Spec 005) | weave branch |
| commit (Spec 006) | add section/merge; move commit |
| drop (Spec 008) | drop commit/branch; reassign branch |
| fold (Spec 007) | fixup/move/edit; add section/merge |
| reword (Spec 003), split (Spec 013) | edit commit |
| swap (Spec 015) | reorder commits/branch sections |
| absorb (Spec 012) | fixup commit |

Only the commands listed above mutate via the Weave; status, init, update, push, and branch rename do not (update may use its execution/recovery infrastructure).

## Staging safety

Every weave rebase runs with `--autostash`, whose replay reaches the working
tree only: a staged *modification* comes back unstaged, on a rebase that
completed as much as on one that was aborted. Every caller MUST therefore put
the index back on both paths (Spec 014). The non-resumable
`run_rebase_or_abort` does it for its own callers; a pre-flight refusal, an
abort that left the rebase on disk, and an index left unmerged by a replay git
could not finish are the exceptions — that index was never loom's to touch.

## Worktree ref safety

Before rebase, enumerate `git worktree list --porcelain` and reject the entire operation if **any branch the todo can move** is checked out in another non-prunable worktree. This includes refs moved by `update-ref` and HEAD's own branch, moved on rebase completion. The error MUST name the branch and worktree path.

The current worktree is exempt because ref, index, and files move together. git lists the main worktree at the common dir with a trailing `/.git` stripped, which is the checkout only when the git dir sits inside it (not for a submodule or `--separate-git-dir`), so the exemption MUST also match `git rev-parse --absolute-git-dir` stripped the same way — absolute, never common, so a linked worktree never exempts the main one. Ignore detached-HEAD worktrees (no branch), bare worktrees, and prunable worktrees whose directories are gone. This guard prevents another worktree retaining its old index/files while its ref moves, which would appear there as phantom staged changes.
