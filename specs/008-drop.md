# Spec 008: Drop

> **Normative.** This document defines required `git loom drop` behavior.

## CLI

```bash
git-loom drop <target> [-y]
```

`<target>` accepts a full/partial commit hash, Git ref, local branch name,
short ID, file, or `zz`. `-y` skips file/change-discard confirmation.

Resolve with `resolve_arg(accept = [File, Branch, Commit, Unstaged])` using
Spec 002. Exact local branch names resolve as branches before Git refs; Git
refs (including `HEAD`) resolve as commits; branch and commit short IDs retain
their types. `zz` means all local changes.

Requirements: Git 2.38+, a non-bare repository, upstream tracking for short
IDs, and (for branch targets) a branch in the integration range.

## Commit Targets

Remove the commit by interactive rebase and replay descendants. Descendant
OIDs and affected branch refs change; other content, messages, topology except
the removed commit, and refs outside the ancestry chain do not.

Dropping a commit never deletes a branch. If it is the last commit owned by
one or more branches, remove their section and merge entry and park every such
branch at its base, unwoven and reusable by `loom commit -b <branch>`. This
also applies to co-located branches and an inner stacked branch. Prompt:

```text
Drop commit `<id>` <subject>, leaving branches `<a>`, `<b>` empty?
```

Success prints `Dropped commit <id>` followed by
`branches <a>, <b> now empty, at the base`. Move the refs with rebase
`update-ref` lines so `loom abort` restores their original tips. Before
rewriting, reject an affected branch checked out in another non-prunable
worktree (Spec 004).

Conflicts are resumable through `loom continue` / `loom abort`: save state in
`.git/loom/state.json`, block other commands as specified by Spec 014, and
restore the original operation on abort.

## Branch Targets

Dropping a branch removes its ref and, unless commits are shared, all commits
it owns. Branch drop is atomic and hard-fail: any failure restores the original
repository rather than leaving resumable state.

| Case | Required behavior |
| --- | --- |
| Tip equals merge-base | Delete only the ref, without confirmation: it has no commit to lose. Do not rebase or require a clean working tree. Print `Dropped empty branch <name>`. |
| Woven (tip is off the first-parent line) | Remove its section and merge entry, drop its owned commits, update affected refs, then delete its ref atomically. |
| Non-woven (tip is on the first-parent line) | Drop its owned commits from the integration line and delete its ref. |
| Co-located woven | Preserve shared commits and merge topology, assign the section to the first surviving sibling in branch order, and delete only the target ref. |
| Co-located non-woven | Preserve shared commits and delete only the target ref. |
| Inner/stacked woven (tip inside another branch's section) | Delete only the ref; the outer branch keeps every commit and nothing is rewritten. Use the sibling-keeping messages with the outer branch as the sibling. |
| Inner/stacked non-woven (tip on the first-parent line) | Follow the non-woven rule: its owned commits leave the integration line and the branches stacked on it are rewritten. |

Confirmation and success must both state how much is removed, so `-y` users
still learn the size of the drop. The count is the branch's own commits, not
everything between its tip and the merge-base: for a woven branch its weave
section, minus the part an inner branch keeps and commits the integration line
picks too. The merge entry the drop also removes is not counted. With a zero count, say neither a number nor a sibling:
`Drop branch <name>?` and `Dropped branch <name>`. Messages:
`Drop branch <name> and its <n> commits?` and
`Dropped branch <name> and its <n> commits` (singular `1 commit`). When the
branch owns no commit of its own, name the sibling keeping them instead, in
both messages: `Drop branch <name>, keeping its commits on <sibling>?` and
`Dropped branch <name>, its commits stay on <sibling>`. When no ref names the
section keeping them, say `in history` in place of the sibling; never show a
generated section label as a branch.

A woven or non-woven branch must be between merge-base and `HEAD`. Otherwise
error `Branch '<name>' is not woven into the integration branch` and hint to
use `git branch -d <name>` directly.

Dropping the outer branch preserves an inner branch. Preserve commits and refs
on other branches and direct integration commits; rewrite only affected
descendants and refs. Automatically preserve uncommitted changes. The
merge-base-only case works with any working-tree state.

## File and `zz` Targets

Prompt unless `-y`; affect no commits, refs, or other files.

| Target/status | Prompt | Operation | Success |
| --- | --- | --- | --- |
| Tracked `M`, `D`, or `R` | `Discard changes to '<path>'?` | `git restore --staged --worktree <path>` | `Restored '<path>'` |
| Staged new file (`A`) | `Delete '<path>'?` | `git rm --force <path>` (index and disk) | `Deleted '<path>'` |
| Untracked (`??`) | `Delete '<path>'?` | Delete from disk | `Deleted '<path>'` |
| `zz` | `Discard all local changes?` | `git restore --staged --worktree .`, then `git clean -fd` | `Discarded all local changes` |

`zz` restores all tracked modifications and deletes all untracked files and
directories; ignored files remain. With no changes, error exactly
`No local changes to discard`.

File and `zz` operations are atomic: either all requested changes are
discarded or the repository remains unchanged.

## Examples

```bash
git-loom drop ab          # commit/branch short ID, according to its type
git-loom drop src/main.rs # restore or delete one file after confirmation
git-loom drop zz -y       # discard all tracked and untracked changes
```

Dropping a woven branch removes its owned commits and merge entry; dropping a
co-located branch removes only that ref and transfers section ownership.
