# Spec 003: Reword

> **Normative.** This document defines commit-message editing and branch renaming by `git loom reword`.

## CLI

```bash
git-loom reword <target> [-m <message>]
```

`<target>` is a commit hash/revision/short ID or a local branch name/short ID. `-m, --message <message>` supplies the new commit message or branch name. Without `-m`, commits open the Git editor and branches use a single-line interactive prompt whose placeholder is the current name.

## Resolution

Use shared resolution (Spec 002), with this command-specific precedence:

1. exact local branch names resolve as branches;
2. Git revisions (full/partial hashes, `HEAD`, etc.) resolve as commits;
3. short IDs resolve branch, then commit, then file.

Files are rejected. A branch name always means branch rename; use its tip hash or commit short ID to reword the tip commit. Git revisions work without upstream tracking; short IDs require it.

## Commit target

Change the target message and replay descendants with native interactive rebase. This MUST:

- work for any existing commit, including a root commit;
- preserve commit contents, messages of other commits, topology (including merges), and empty commits;
- update rewritten descendant hashes and affected branch refs;
- leave branches outside the ancestry chain unchanged;
- automatically stash and restore working-tree changes, staged ones as staged
  (Spec 014);
- keep the commit's `Change-Id` (Spec 002): with `-m` it is re-appended to the new message unless that message already carries one; with the editor it is restored after editing. A commit without one receives a fresh one when generation is enabled.

Rebuilding a descendant merge can conflict, including a merge previously resolved manually. With `rerere`, Git replays the resolution but the rebase still stops; with `rerere.autoUpdate` the reword then carries on by itself, and otherwise it pauses for the user to stage it (Spec 014).

On replay conflict, retain the amended message and pause with exactly this guidance:

```text
! Conflicts detected — resolve them with git, then run:
  `loom continue`   to complete the reword
  `loom abort`      to cancel and restore original state
```

`loom continue` completes the reword. `loom abort` restores the original message, HEAD, and all branch refs (Spec 014). Outside a paused conflict, the operation is atomic and MUST NOT leave an incidental rebase state. A target whose replay is dropped MUST refuse and restore, naming the commit (Spec 004).

## Branch target

Rename through `git branch -m`. With `-m`, rename non-interactively; without it, prompt as described above. Confirming the unchanged name is a no-op. The target branch must exist.

Minimal examples:

```bash
git-loom reword ab -m "Fix authentication bug" # commit short ID
git-loom reword feature-a -m feature-auth       # branch name
git-loom reword abc1234                         # Git editor
```

## Prerequisites

- Any Git repository and an existing target are sufficient for Git-reference commit rewording.
- Branch rename requires an existing local branch.
- Any short-ID target requires a current branch with upstream tracking and successful shared graph gathering (Spec 002).
