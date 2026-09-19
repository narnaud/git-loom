---
name: git-loom
description: Use git-loom (loom) instead of raw git in repositories managed by git-loom. Applies whenever staging, committing, amending, splitting, reorganizing, dropping, updating, or pushing changes in a repo where `git loom status` succeeds.
---

# Working with git-loom

Loom manages feature branches woven into an integration branch. Raw Git history
rewrites desynchronize the weave.

## Detect and operate

Run `git loom status --agent` once. If it succeeds with a branch graph, use loom
for every history mutation; read-only Git (`status`, `log`, `blame`, etc.) is
allowed. Run `git loom status --agent` after every mutation.

Rules:

1. Always pass `--agent` (or set `LOOM_AGENT=1`). Each invocation ends with one
   JSON status as stderr's last line.
2. Use `-p`/`--patch` only on `split` and `fold` with a commit source, where it
   answers with a hunk listing (see Hunk selection). On `add`, `commit`, and
   `fold` over working-tree changes it opens a full-screen UI and is rejected.
3. Always pass `-m <message>` to `commit`, `split`, and commit `reword`. For
   `commit`, explicitly choose `-b <branch>` or integration branch `-i`.
4. Name files explicitly. Never use `zz`, except as the destination in
   `git loom fold <commit> zz` (uncommit). In particular, never pass it to
   `add`, `commit`, or `drop`; it includes unrelated local edits.
5. Never pipe answers into prompts. In agent mode, re-invoke with explicit args.
6. If a message says this skill differs from loom's shipped skill, run the named
   `git-loom agent init` command and tell the user to restart the session.

## Agent status

| Status / exit | Action |
|---|---|
| `{"status":"ok","messages":[...]}` / 0 | Success; report optional follow-up messages. |
| `needs_input` or `needs_confirmation` / 10 | No history changed. Show `prompt` and `options`, ask the user, then re-invoke per `hint`; `allow_other: true` permits another value. |
| `paused` / 0 | Conflict, not success. Follow Conflict recovery. Most loom commands are blocked. |
| `error` / 1 | Failed; inspect `git loom trace`. |
| no JSON / 2 | Malformed CLI invocation. |

## IDs and inspection

Status shows: `zz` = all local changes; two letters such as `fa` = branch or
file; hex prefix such as `3ac` = commit; `d0:1` = file 1 in commit `d0` (with
`status -f`). Short IDs work wherever that entity is accepted; names, paths,
and hashes also work. `status -a` includes hidden branches.

## Commands

| Intent | Command / rule |
|---|---|
| Stage | `git loom add <files>`; list files, never `zz`. |
| Commit | `git loom commit -b <branch> -m "<msg>" <files...>`; a new branch name creates it. Use `-i` for integration. Name files, or omit them to commit exactly the staged set; never `zz`. |
| Amend/fixup | `git loom fold <files...> <commit>`; for staged changes, `git loom fold <commit>`. |
| Auto-fixup | `git loom absorb`; `-n` dry-runs. |
| Move commits | `git loom fold <commit>... <branch>`; `-c` creates a new branch and rejects an existing name. `--above <commit>` / `--below <commit>` moves next to a commit, in any branch or the same one. |
| Uncommit | `git loom fold <commit> zz`. |
| Split | `git loom split <commit> -m "<msg>" <files...>`; `-p` splits inside one file. |
| Edit message | `git loom reword <commit> -m "<msg>"`. |
| Rename branch | `git loom reword <branch> -m <new-name>`. |
| Reorder | `git loom swap <a> <b>`. |
| Delete/discard/reset | `git loom drop <target> -y`; explicitly name commits, branches, or files; never `zz`. |
| Create branch | Usually commit with `-b <new-name>`; empty branch: `git loom branch new <name>`. |
| Merge/unmerge | `git loom branch merge <branch>` / `git loom branch unmerge <branch>`. |
| Pull-rebase | `git loom update -y`. |
| Push / PR | `git loom push <branch>`; `--no-pr` skips PR/review creation. A stacked push includes lower branches. Same-repo PRs target the branch below; GitHub fork PRs target upstream. |
| Diff/show | `git loom diff` / `git loom show`; short IDs work; Git options follow `--`. |
| Test branch | `git loom switch <branch>`. |

## Hunk selection

When two logical changes share a file, `-p` works at hunk level:
`git loom split <commit> -m "<msg>" -p`, `git loom fold -p <source> <target>`,
`git loom fold -p <commit> zz`. Each takes two calls.

The first returns `needs_input` / exit 10 with `items` (one per hunk: `id`,
`path`, `diff`, `selectable`) and a `fingerprint`. Nothing changed. Pick the
ids you want and re-run the command the `hint` gives, adding
`--hunks <id>` once per id, plus `--hunks-from <fingerprint>`.

- `selectable: false` marks an entry this command cannot take — a binary file
  under `fold`. A deletion and a submodule move whole; `split` takes them all
  whole. `options` already lists only the pickable ids.
- The fingerprint is checked against the current diff and a mismatch is
  refused, because ids are positional. Re-list rather than reusing an old one.
- Re-run the whole `hint`, including any `<files>` filter: dropping one changes
  the listing and the fingerprint check then fails.
- For `split`, leave at least one hunk for the second commit.

Never directly run `git rebase`, `git commit --amend`, `git cherry-pick`,
`git reset --hard`, `git push --force`, or `git merge` into integration.

## Preserve old commits

`git rebase --update-refs` moves every branch pointing into rewritten weave
history, so an in-weave branch is not a backup. Rewrite first, then preserve the
old SHA outside the weave:

```sh
git loom fold <files> <commit>
git branch <name> <old-sha>
```

Alternatively, `git tag <name> <old-sha>` works before or after; update-refs
only updates `refs/heads/`.

## Conflict recovery

1. Inspect with `git status` or `git loom diff --agent`.
2. Edit conflicts and stage with raw `git add <files>`. While paused,
   `git loom add` and all loom commands except `continue`, `abort`, `diff`,
   `show`, and `trace` are blocked.
3. Run `git loom continue --agent`; repeat if it pauses again.
4. Or run `git loom abort --agent`, which restores the original state,
   including staged and working-tree changes.

Context recovery: `git loom status --agent -f`, `git loom status --agent -a`,
`git loom show <target> --agent`, `git loom diff <target> --agent`, and
`git loom trace` (underlying Git commands from the last invocation).
