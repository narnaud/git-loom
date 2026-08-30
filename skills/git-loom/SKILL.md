---
name: git-loom
description: Use git-loom (loom) instead of raw git in repositories managed by git-loom. Applies whenever staging, committing, amending, splitting, reorganizing, dropping, updating, or pushing changes in a repo where `git loom status` succeeds.
---

# Working with git-loom repositories

git-loom (invoked as `git loom` or `loom`) manages an *integration branch* that
weaves several *feature branches* together. All history-mutating work must go
through loom — raw git rewrites desynchronize the weave.

## When this skill applies

Run `git loom status` once. If it succeeds and shows a branch graph, this is a
loom-managed repository: use loom for every history-mutating operation below.
Plain read-only git commands (`git log`, `git status`, `git blame`, ...) remain
fine.

## Invocation rules (critical)

1. **Always pass `--agent`** (or set `LOOM_AGENT=1`). Every invocation then
   ends with exactly one JSON status as the **last line of stderr**.
2. **Never use `-p`/`--patch`** — it opens a full-screen UI and is rejected in
   agent mode.
3. **Always pass `-m <message>`** to `commit`, `split`, and `reword` (commit
   targets), and be explicit about targets (`-b <branch>` for commit) —
   omitted arguments would need a prompt.
4. Interpret the JSON status:
   - `{"status":"ok","messages":[...]}` — success (exit 0). `messages` may
     note skipped optional follow-ups (e.g. PR creation) with how to do them.
   - `{"status":"needs_input",...}` / `{"status":"needs_confirmation",...}`
     (exit 10) — **no history was changed**. Present `prompt` and `options` to
     the user, then re-invoke following `hint`. `allow_other: true` means a
     value outside `options` is also accepted (e.g. a new branch name).
   - `{"status":"paused",...}` (exit 0) — a rebase stopped on conflicts. Not
     a success: resolve the conflicted files, stage them (`git loom add`),
     then `git loom continue --agent` — or `git loom abort --agent` to roll
     everything back. While paused, most other loom commands are blocked.
   - `{"status":"error","message":...}` (exit 1) — the command failed;
     `git loom trace` shows the underlying git commands.
   - Exit 2 with no JSON — the invocation itself was malformed (CLI usage
     error).

## The workflow loop

Run `git loom status --agent` first and after every mutation. It prints a
graph of the integration branch, its feature branches, commits, and local
changes — each with a **short ID**:

- `zz` — the working tree / all local changes
- two letters (e.g. `fa`, `ma`) — a branch (`feature-auth`) or file (`main.rs`)
- hex prefix (e.g. `d0`, `3ac`) — a commit (prefix of the printed hash)
- `d0:1` — file #1 inside commit `d0` (visible with `status -f`)

Short IDs are accepted anywhere a branch, commit, or file is expected, but
plain names, paths, and hashes always work too — prefer whichever you already
know. `git loom status -f` lists files per commit; `-a` includes hidden
branches.

## Command mapping: use loom, not git

| Instead of | Use |
|---|---|
| `git add <files>` | `git loom add <files>` (`zz` = stage everything) |
| `git commit` | `git loom commit -b <branch> -m "<msg>" [files...]` — commits onto a feature branch without leaving integration; a new branch name creates the branch; no files = the staged changes; `zz` = all changes |
| `git commit --amend` (files into HEAD or any commit) | `git loom fold <files...> <commit>` (staged changes: `git loom fold <commit>`) |
| `git rebase -i` + fixup | `git loom fold <commit> <commit>` or `git loom absorb` (auto-distributes working-tree changes into the commits that introduced those lines; `-n` for a dry run) |
| moving a commit to another branch | `git loom fold <commit> <branch>` (`-c` creates a new branch from it) |
| uncommitting | `git loom fold <commit> zz` |
| splitting a commit | `git loom split <commit> -m "<msg>" <files...>` |
| `git commit --amend -m` / editing any message | `git loom reword <commit> -m "<msg>"` |
| renaming a branch | `git loom reword <branch> -m <new-name>` |
| reordering commits | `git loom swap <a> <b>` |
| `git reset` / deleting a commit or branch / discarding changes | `git loom drop <target> -y` (`zz` discards all local changes — confirm with the user first) |
| creating a branch | usually just `git loom commit -b <new-name> ...`; empty branch: `git loom branch new <name>` |
| merging a branch into integration | `git loom branch merge <branch>` / `git loom branch unmerge <branch>` |
| `git pull --rebase` | `git loom update -y` |
| `git push` (+ PR) | `git loom push <branch>` (`--no-pr` to skip PR/review creation) |
| `git diff` / `git show` | `git loom diff` / `git loom show` (short IDs work; unknown options pass through) |
| checking out a branch to test it | `git loom switch <branch>` |

## What NOT to do

In a loom-managed repository, never run these directly — they desynchronize
the weave:

- `git rebase` (any form)
- `git commit --amend`
- `git cherry-pick`
- `git reset --hard`
- `git push --force`
- `git merge` into the integration branch

Also never pipe answers into loom prompts and never pass `-p`/`--patch`; in
agent mode prompts are answered by re-invoking with explicit arguments.

## Conflict handling

When a status line says `"paused"`, or any loom command reports that an
operation is paused:

1. Inspect conflicts with `git status` / `git loom diff --agent`.
2. Edit the conflicted files, then stage them: `git loom add <files> --agent`.
3. `git loom continue --agent` — repeats `paused` if new conflicts appear.
4. To give up instead: `git loom abort --agent` restores the original state,
   including staged and working-tree changes.

## Recovering context

- `git loom status --agent -f` — files changed in each commit, with `sid:index` IDs
- `git loom status --agent -a` — include hidden branches
- `git loom show <target> --agent` / `git loom diff <target> --agent` — inspect commits
- `git loom trace` — the git commands the last loom invocation actually ran
