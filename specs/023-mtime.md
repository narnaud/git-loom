# Spec 023: Modification Times

> **Normative.** This document defines how loom keeps working-tree modification times across the rebases it runs, so a rewrite that leaves a file's bytes unchanged does not make a build tool rebuild it.

## Scope

Every rebase loom runs: each weave rebase (Spec 004) and the plain pull-rebase of `update` (Spec 010). There is no CLI; the behavior is implicit and never fails the operation it serves.

A rebase checks out the upstream, replays each commit, then replays its autostash, so every file the replayed commits name and every dirty file is rewritten with a fresh mtime even when its bytes end up as they started — a no-op todo churns as much as a reword. Loom therefore records those mtimes before the rebase and puts them back once it is over, on the files whose bytes are unchanged.

## Record

Taken after pre-flight and immediately before `git rebase` is spawned, and held in memory by the invocation that took it: it is never written to disk and never outlives the command. For each candidate file it holds the repository-relative path, the mtime, and the blob id of the raw bytes (filters not applied).

Candidate paths are the union of every path named by a commit in `<upstream>..HEAD` (a merge against its first parent; the whole history for `--root`) and every path differing between HEAD and the index or working tree. Only regular files are recorded: a missing path, a symlink and a submodule are skipped. Untracked files are never autostashed, so they are not candidates.

A record that cannot be taken is noted in the trace and the rebase runs without one.

## Restore

Runs when the invocation that took the record finds its rebase over — `git rebase` finished, a `--continue` or `--skip` the invocation drove itself finished it (an `edit` it asked for, an empty replay it skips, a conflict `rerere` resolved), or it ran `git rebase --abort` itself. The record is taken out before it is acted on, so no exit acts on it twice.

- Reporting a pause to the user (Spec 014's conflict and `edit` warnings) drops the record unused: whatever they build before going on reads intermediate bytes, and an older mtime put back later would present that build as current. `loom continue` and `loom abort` are later invocations, find no record, and leave the mtimes git gave.
- For each recorded path that is still a regular file whose mtime moved and whose raw bytes hash to the recorded blob, the mtime is set back to the recorded one. Nothing else is written: a path whose bytes differ, a missing path, a new path and an unmerged path (its bytes carry markers) keep what the rebase gave them.
- After at least one restore, `git update-index -q --unmerged --refresh` runs once so the index stat data agrees and later commands do not re-hash those files.
- A file that cannot be opened for writing (read-only, locked) is skipped.
- Every failure is traced and never reaches the caller: the rewrite has landed by then.

Whether a path is restored depends on bytes alone, so the same rule holds under any `core.autocrlf` or filter setting: a file git renormalized on checkout differs and gets its new mtime.

## Effect by command

| Command | Files keeping their mtime |
| --- | --- |
| `reword`, `swap`, `branch` | All |
| `fold`, `absorb`, `drop`, `split`, `commit` | Every file whose final bytes equal its starting bytes |
| `update` | Those upstream did not change |
| A failed rebase the command aborts itself | All, except after `absorb`, whose rollback hard-resets once the abort has run (Spec 014) |
| Any operation that paused for the user; `loom continue`; `loom abort` | None |

## Limits

The files are rewritten during the rebase and only their timestamps are put back. A build that reads a file while the rebase runs compiles intermediate content, and the restored, older mtime then presents its output as current. Keep watcher-driven builds off the tree while a loom operation runs.
