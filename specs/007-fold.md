# Spec 007: Fold

> **Normative.** This document defines `git loom fold`, which combines a source into a target selected by resolved argument types.

## CLI

```bash
git-loom fold <target>
git-loom fold <source>... <target>
git-loom fold --create <commit>... <new-branch>
git-loom fold -p [<files>...] <commit>
git-loom fold -p <commit1> <commit2>
git-loom fold -p <commit> zz
```

With one argument, fold the current index into that target. With two or more, the final argument is the target and all preceding arguments are sources.

- `-c, --create`: create a new branch at the resolved Weave base and move one or more source commits into it. The target name MUST NOT exist. Sources may be loose or already branch-owned. Order commits oldest-first (ancestors before descendants; unrelated lines by committer date), independent of input order.
- `-p, --patch`: select hunks interactively according to [Patch mode](#patch-mode--p).
- `zz`: reserved `Unstaged` target/source representing the working directory/all its changes.
- `commit_sid:index` (for example `fa:0`): `CommitFile` shown by `git loom status -f`.

## Resolution and dispatch

Arguments use shared resolution (Spec 002) and `resolve_arg()` with accepted kinds `[Commit, CommitFile, File, Unstaged]`; branch names/short IDs are additionally recognized where the dispatch target permits a branch. Standard Git revisions and short IDs are tried before a filesystem fallback. The fallback accepts an existing path only when it has uncommitted changes. Paths may be CWD-relative or absolute and are converted to repository-relative form. The final positional argument alone determines the target.

| Source(s) | Target | Action | Multiple sources |
| --- | --- | --- | --- |
| current index | Commit | amend staged changes into commit | no |
| File | Commit | amend current file changes into commit | yes |
| `zz` | Commit | amend all current changes into commit | no (`zz` wins if mixed with files) |
| Commit | Commit | fixup source into target | no |
| Commit | Branch | move commits to branch tip | yes |
| Commit | `zz` | remove commit to working tree | no |
| CommitFile | `zz` | remove one file's commit changes to working tree | no |
| CommitFile | Commit | move one file's changes between commits | no |
| Commit | new branch with `-c` | create branch and move commits | yes |

### Required diagnostics

Errors are verbatim; `⏎` marks a line break within a message.

| Condition | Exact error |
| --- | --- |
| Single argument, empty index | `Nothing to commit` |
| Single argument, non-commit target | ``'<arg>' did not resolve to a commit`` |
| File into branch | `Cannot fold files into a branch⏎Target a specific commit` |
| Branch as source | ``Cannot fold a branch⏎Use `git loom branch` for branch operations`` |
| `zz` into `zz` | `Cannot fold files into unstaged — files are already in the working directory` |
| `zz` into branch | `Cannot fold files into a branch⏎Target a specific commit` |
| `zz` source with clean tree | `No changes to fold — working tree is clean` |
| Mixed source types | `Cannot mix different source types (files, commits, commit files)` |
| Source commit at/below integration base | ``Commit `<hash>` is not in the integration scope⏎Only commits above the integration base can be moved`` (regardless of source count) |
| `-c` target name exists | ``Branch `<name>` already exists⏎Use `loom fold <commit>... <name>` to move commits onto it`` |
| Multiple commits into Commit or `zz` | `Only one commit source is allowed` |
| Multiple commit-file sources | `Only one commit file source is allowed` |
| CommitFile into Branch | ``Cannot fold a commit file into a branch⏎Target a specific commit or use `zz` to uncommit`` |

## File/current-change amendments

### Current index into Commit

`fold <commit>` requires at least one staged change and accepts any commit, including HEAD. Amend exactly the index into the target; preserve unstaged changes, including unstaged edits to the same files. The target and descendants receive new hashes; their messages and unaffected content remain.

### File(s) into Commit

`fold <file>... <commit>` stages each specified file's staged/unstaged changes and amends them into any target commit, including HEAD. A file without changes errors exactly ``File '<path>' has no changes to fold``. Preserve uncommitted changes in all other files. Rewrite the target and descendants without changing existing messages.

### `zz` into Commit

`fold zz <commit>` amends every staged/unstaged working-tree change. `zz` mixed with file sources takes precedence and includes all changes. A clean tree uses the exact diagnostic above. Any commit, including HEAD, is valid.

## Commit fixup

`fold <source-commit> <target-commit>` absorbs the source changes into the target, retains the target message, removes the source, and rewrites later affected hashes while preserving topology and unrelated branches. Source MUST be a newer descendant of target; otherwise error exactly `Source commit must be newer than target commit`. Preserve uncommitted changes.

## Commit move and branch creation

`fold <commit>... <branch>` removes each source from its old position and appends it to the target branch in one rebase. `--create` first creates its non-existing target at the Weave base. Update source/target refs and affected hashes; preserve uncommitted changes.

Order multiple sources oldest-first regardless of input: ancestors precede descendants, unrelated lines sort by committer date. A single ordinary move is resumable on conflict. Multiple-source moves and every `--create` move hard-fail and roll back the entire operation; abort rebase, delete a branch created by `-c`, and restore staged changes as staged.

Topology rules:

- If target has no section (for example, an empty branch at base), create its section and merge entry before moving.
- If target is co-located, only it advances; other refs remain at the shared old tip.
- An inner/stacked target is an `update-ref` at a commit inside an outer section. Insert immediately after its tip, advance only that ref, and replay commits above it. Moving from outer to inner therefore reorders the section; a ref co-located with the inner tip stays there.
- Any branch ending at the moved source stays behind. Move it to the preceding commit, or, if the source was its sole commit, park it at the base it built on: upstream for its own section or the parent branch tip for a stacked section. It MUST NOT follow the moved commit into a branch whose history it did not contain.
- Remove any source section/merge entry left empty.
- Name every parked branch in success output: ``branch `<name>` now empty, at the base``.

## Commit to working tree

`fold <commit> zz` removes one commit and exposes its changes as unstaged modifications.

- **HEAD:** mixed-reset to `HEAD~1`.
- **Non-HEAD:** capture the diff against the commit's own parent, drop the commit through Weave rebase, then three-way apply that diff onto the rewritten history. Later nearby edits may merge; a true overlap or pre-existing worktree edit to a touched file causes apply failure.

Preserve unrelated uncommitted changes. Before rebase completion, any apply failure rolls history and all uncommitted state back. After a paused rebase is completed with `loom continue`, rollback is no longer possible: if re-apply fails, leave the commit dropped and save the diff as `<git-dir>/loom/unapplied-<n>.patch`, choosing a new number and never overwriting an earlier patch.

If this was the only commit of one or more branches, preserve and park all those refs at their build base. A section branch parks at upstream and loses its merge; a branch stacked on another parks at its parent's tip. Include the empty-branch success line above. The user can recommit with `loom commit -b <branch>`.

Other commits retain content/messages, unrelated branches remain unchanged, and non-HEAD descendants receive new hashes.

## CommitFile operations

### CommitFile to `zz`

`fold <commit_sid:index> zz` removes that file's changes from the commit and exposes them unstaged while retaining the commit and its message.

- Require the file in that commit; otherwise error exactly `File '<path>' has no changes in commit <short_hash>`.
- At HEAD, reverse-apply the file diff, amend HEAD without it, then re-apply the diff to the worktree.
- Below HEAD, reverse-apply, create a temporary commit, fix it into the target through Weave, then re-apply.
- Re-apply and rollback use the same three-way and post-continue patch preservation rules as whole-commit uncommit.

Preserve every other file/commit and all other uncommitted changes; rewrite affected descendants.

### CommitFile to Commit

`fold <commit_sid:index> <commit>` atomically removes one file's changes from source and adds them to target.

- Source and target must differ; otherwise `Source and target are the same commit`.
- Require the file in source, using the exact missing-file error above.
- Create reverse and forward temporary commits and fix each into its respective target in one Weave rebase.
- If either half fails, including the forward diff no longer applying at target, roll back history and uncommitted changes.

Both messages, all other files, topology, unrelated branches, and pre-existing changes remain; affected commits receive new hashes.

## Patch mode (`-p`)

All forms open the interactive hunk picker and require at least one selection; otherwise error `No hunks selected`.

### Working-tree hunks into Commit

```bash
git-loom fold -p [<files>...] <commit>
```

Show the current working-tree diff, filtered by optional paths (`zz` means all). Stage only selected hunks and amend them into target. Unselected changes remain unstaged. Rewrite target/descendants; retain messages and unrelated content.

### Commit hunks into older Commit

```bash
git-loom fold -p <source> <target>
```

When both resolve to commits, show source's commit diff. Source and target must differ, and source must be a newer descendant; otherwise error exactly `Source commit must be newer than target commit`. Remove selected hunks from source and add them to target, preserving messages, topology, and unselected hunks while rewriting affected hashes.

Binary/deleted files cannot supply patch hunks. If no text selection is possible, error exactly `No text hunks selected — binary and deleted files are not supported with -p`.

A submodule is one whole entry in the picker, taken or left entire. It travels as the commit's own whole-file diff, applied with `--cached`: a picked hunk carries no file mode and would land the entry as a plain blob. `split -p` does the same (Spec 013).

This move uses two edit-and-continue phases: first remove hunks from source, then add them to target. Target's new OID is unknown until phase one, so `_loom-track` carries its pre-phase-one OID into phase two.

### Commit hunks to working tree

```bash
git-loom fold -p <commit> zz
```

Show the commit diff, remove selected text hunks while preserving the commit/message and unselected hunks, and apply the selection as unstaged worktree changes. Rewrite non-HEAD descendants and preserve other worktree changes. Binary/deleted files use the same exact error as above.

Unlike a captured whole-file diff, a hunk selection carries no blob IDs and cannot use three-way replay. If selected lines or context no longer match, re-apply hard-fails and rolls back the whole operation.

### Patch safety

Every `-p` form is hard-fail: on any internal rebase conflict, abort automatically and restore original history, index, and worktree. Never save `LoomState`; `loom continue`/`loom abort` do not apply. Always save aside and restore pre-existing staged changes, on success or failure.

## Non-patch conflict recovery

These operations pause on rebase conflict and save the listed `LoomState.context`:

| Operation | Context |
| --- | --- |
| Files/current changes into commit | `op: "FilesIntoCommit"`; original hash, file count, saved staged patch |
| Commit fixup | `op: "CommitIntoCommit"`; source and target hashes |
| Single commit move | `op: "CommitToBranch"`; commit hash and branch name |
| Commit to worktree | `op: "CommitToUnstaged"`; commit hash and captured diff |

`loom continue` dispatches `after_continue`, removes `_loom-track`, and prints the operation's success message. `loom abort` restores original history, staged state, and working-tree state (Spec 014). The post-continue unapplied-patch exception is defined above.

Multiple moves, all `-c` moves, all `-p` forms, and CommitFile move failures save no resumable state and auto-rollback as specified in their sections.

## General invariants and prerequisites

- Require Git 2.38+ and a non-bare repository working tree.
- Short-ID arguments require upstream tracking.
- Preserve uncommitted/index changes except where the requested operation intentionally consumes them; restore them exactly on abort/hard-fail.
- Move submodule entries (gitlinks) through the index alone, in every form including `-p`: apply their whole-file diff with `--cached`, never stage them by path, and never move a submodule checkout. Uncommitting to `zz` then needs no working-tree replay: a bump is unstaged as soon as the index entry moves, and an addition becomes an untracked directory. A removal is the exception — while its checkout is still on disk the restored entry matches it and nothing would show, so stage that deletion rather than lose it.
- Successful fold operations retain messages and unaffected content unless a rule above explicitly removes/moves that content.

Minimal examples:

```bash
git-loom fold ab                         # staged changes -> commit
git-loom fold src/main.rs HEAD           # file changes -> commit
git-loom fold fix target                 # commit fixup
git-loom fold c1 c2 feature-b            # ordered multi-move
git-loom fold --create c1 c2 feature-new # create at base and move
git-loom fold ab:0 zz                    # one committed file -> worktree
git-loom fold -p source target           # selected commit hunks -> commit
```
