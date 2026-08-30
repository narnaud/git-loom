# Spec 019: Agent Integration (`agent init` and `--agent` mode)

## Overview

Agent integration makes loom usable by AI coding agents (Claude Code first).
It has two parts:

- **`git-loom agent init`** installs a skill file that teaches the agent to use
  loom instead of raw git for history-mutating work.
- **Agent mode** (the global `--agent` flag, or the `LOOM_AGENT` environment
  variable) makes every loom invocation end with a single machine-readable JSON
  status line, and replaces interactive prompts with structured answers the
  agent can relay to the user.

## Why Agent Integration?

Loom is designed for humans: every optional argument falls back to an
interactive prompt (branch pickers, confirmations) or a full-screen hunk picker
(`-p`). A headless agent that runs `git-loom commit -m "fix"` without `-b`
hits a prompt that cannot render, and has no way to learn which branches it
could have passed. Without packaged guidance, agents also fall back to raw
`git rebase`/`git commit --amend`, which desynchronizes the weave.

Agent mode turns every prompt into data: the list of choices, and the exact
command to re-run with the choice filled in. `agent init` ships the playbook so
the agent knows to prefer loom in the first place.

## CLI

### `agent init`

```bash
git-loom agent init [<agent>] [--project]
```

**Arguments:**

- `<agent>`: which AI agent to install the skill for. Currently only `claude`
  (the default). Other agents may be added later.

**Flags:**

- `--project`: install into the repository (`.claude/skills/git-loom/SKILL.md`
  relative to the work tree root) instead of the home directory. Requires
  running inside a git repository.
- `--dir <path>` (hidden): override the install base directory; the
  `skills/git-loom/SKILL.md` suffix is still appended. Used by tests.
  Conflicts with `--project`.

`agent init` is unrelated to `git-loom init` (integration-branch setup); the
help text says so. It needs no repository (unless `--project` is given), works
while a loom operation is paused, and is never trace-logged.

### Agent mode

```bash
git-loom --agent <command> [...]
git-loom <command> --agent [...]
LOOM_AGENT=1 git-loom <command> [...]
```

**Flags:**

- `--agent` (global): enable agent mode for this invocation.

The `LOOM_AGENT` environment variable enables agent mode when set to any value
other than `0`. Flag and variable are equivalent (OR-ed). The variable is
inherited by child processes — including loom's own re-invocation as the git
sequence editor during rebases; that path has no prompts today, and any prompt
added to a child path in the future must honor agent mode the same way.

Agent mode is never inferred: piping loom's output or running it without a
terminal does not enable it.

## What Happens

### The JSON status line

In agent mode, **every invocation ends with exactly one single-line JSON
object, printed as the last line of stderr**:

- stdout carries only command payload (the status graph, `show`/`diff`
  output).
- stderr carries the human-readable progress lines (`✓`/`!`/`✗`) plus, last,
  the JSON status.

The possible statuses:

```json
{"status":"ok","messages":["Created commit `1a2b3c4` on branch `feature-auth`"]}
```

Emitted when the command succeeds. `messages` collects the success and warning
lines the command printed (some commands print several; none may be present).

```json
{"status":"needs_input","kind":"select","prompt":"Select target branch",
 "options":["feature-auth","feature-ui"],"allow_other":true,
 "hint":"re-run with: loom commit -b <branch> -m <message> [files...] (a new name creates the branch)"}
```

Emitted when the command would have opened an interactive prompt **before
touching history**. No commits, branches, or refs were changed (staging
requested on the same invocation, e.g. `zz`, may already have happened —
re-invoking completes it). `kind` is `select`, `text`, or
`multiselect`. `options` lists the choices (present for `select`/
`multiselect`); each option is a plain string directly reusable as a CLI
argument. `allow_other` is `true` when a value outside the list is also
accepted (e.g. a new branch name). `hint` states how to re-invoke with the
answer supplied.

```json
{"status":"needs_confirmation","prompt":"Discard changes to `src/main.rs`?",
 "hint":"re-run with: loom drop <target> -y"}
```

Emitted when the command would have asked a yes/no question before touching
history. Nothing was changed.

```json
{"status":"paused","message":"Conflicts detected — the `loom update` is paused",
 "hint":"resolve conflicts, stage them, then run: loom continue (or loom abort)"}
```

Emitted when a rebase or merge stopped on conflicts and the operation is
paused (see Spec 014). For humans this case is a warning with exit code 0; the
distinct status prevents an agent from mistaking it for success. An optional
`messages` array (same collection as `ok`) carries the success lines printed
before the pause.

Running another command while an operation is already paused reports
`{"status":"error"}` (the command did not run — reporting `paused` would let
an agent mistake the pre-existing pause for its own command's progress); the
`message` names `loom continue` / `loom abort` as the way forward. Only
`continue`, `abort`, `diff`, `show`, `trace`, `completions` and `agent` run
while paused — `add` included, which is why conflict resolutions are staged
with raw `git add`.

```json
{"status":"error","message":"Branch 'foo' is not woven into the integration branch"}
```

Emitted when the command fails. The same message is also printed in
human-readable form.

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | `ok` or `paused` |
| 1 | `error` |
| 2 | usage error (malformed invocation, reported by the CLI parser — no JSON) |
| 10 | `needs_input` or `needs_confirmation` — re-invoke with explicit arguments |

The JSON `status` field is authoritative; the exit code is a convenience for
agents that do not parse stdout/stderr.

### Prompt sites

Every interactive prompt behaves as follows in agent mode. Prompts are
classified **pre-flight** (nothing has been changed yet — the command answers
`needs_input`/`needs_confirmation` and exits) or **post-mutation** (the main
work already succeeded — the command takes a safe default, reports `ok`, and
mentions the skipped action in `messages`).

| Command / prompt | Class | Agent-mode behavior |
|---|---|---|
| `commit` branch picker (no `-b`) | pre-flight | `needs_input` (select, `allow_other`) listing woven branches; hint: `loom commit -b <branch> -m <message> [files...]` (a new name creates the branch) |
| `commit` editor (no `-m`) | pre-flight | `needs_input` (text); hint: pass `-m <message>` |
| `split` message editor (no `-m`) | pre-flight | `needs_input` (text); hint: pass `-m <message>` |
| `split` file picker (no files, no `-p`) | pre-flight | `needs_input` (multiselect) listing the commit's files; hint: `loom split <target> -m <message> <files...>` |
| `reword` commit editor (no `-m`) | pre-flight | `needs_input` (text); hint: pass `-m <message>` |
| `reword` branch-rename prompt (no `-m`) | pre-flight | `needs_input` (text); hint: `loom reword <target> -m <new-name>` |
| `drop` confirmations | pre-flight | `needs_confirmation`; hint: `loom drop <target> -y` |
| `push` branch picker (no branch) | pre-flight | `needs_input` (select) listing woven branches; hint: `loom push <branch>` |
| `push` Gerrit-suspicion confirmation | pre-flight | `needs_confirmation`; hint: `git config loom.remote-type gerrit` (or `plain`), then re-run |
| `push` Gerrit `wip/` prefix choice (`--no-pr`) | pre-flight | `needs_input` (select) with the three choices; no flag exists to answer it — ask the user, then rename with `loom reword` or re-run interactively |
| `push` PR title (GitHub/Azure, multi-commit branch) | post-mutation | branch is already pushed → skip PR creation, report `ok`; `messages` notes the skip |
| `push` browser opening (`gh pr create --web`, `az repos pr create --open`) | post-mutation | never opens a browser in agent mode → skip PR creation, report `ok`; `messages` notes the skip and how to create the PR |
| `update` gone-branch prune confirmation | post-mutation | the pull-rebase already succeeded → skip pruning, report `ok`; `messages` notes the skipped branches and `loom update -y` |
| `branch new` name prompt (no name) | pre-flight | `needs_input` (text); hint: `loom branch new <name>` |
| `branch merge` / `branch unmerge` / `switch` pickers | pre-flight | `needs_input` (select) listing candidates; hint: `loom branch merge <branch>` etc. |
| `init` upstream picker (several candidates) | pre-flight | `needs_input` (select) listing the remote branches |

### `-p` / `--patch` rejection

The hunk pickers are full-screen terminal UIs and cannot run in agent mode.
Passing `-p`/`--patch` to `add`, `commit`, `fold`, or `split` in agent mode
fails immediately — before anything is staged — with:

```
--patch is interactive and unavailable in agent mode
Pass explicit files instead
```

reported as `status: error`, exit code 1. A second guard at the picker itself
backstops any future call path.

### Pager suppression

`show` and `diff` invoke git with inherited stdio. In agent mode loom passes
`-c core.pager=cat` so a pty-hosted agent can never hang inside a pager.

### What does not change

- The status graph, `show`, and `diff` payloads keep their human format on
  stdout (already color-free when not a terminal).
- Normal interactive use (no flag, no variable) is completely unchanged.
- Conflict pauses still exit 0 (see Spec 014); agent mode only adds the
  `paused` JSON status.

### `agent init` install behavior

**What changes:**

- The parent directories are created if missing.
- Target file absent → written; reports `Installed Claude skill at <path>`.
- Target present with different content → overwritten; reports `Updated ...`.
- Target present and identical → untouched; reports `already up to date`.

**What stays the same:**

- The repository: `agent init` never reads or writes git history or state.
- Any other files in the skills directory.

There is no `--force`: the file is loom-owned and regenerated from the binary,
so refreshing it after a loom upgrade is the desired behavior (re-run
`agent init`).

Default target: `<home>/.claude/skills/git-loom/SKILL.md`. With `--project`:
`<worktree>/.claude/skills/git-loom/SKILL.md`.

## Target Resolution

Not applicable — `agent init` takes no repository identifiers, and agent mode
changes no argument resolution (short IDs resolve exactly as in Spec 002).

## Conflict Recovery

`agent init` never runs a rebase. Agent mode does not change conflict recovery
(Spec 014); it only reports the pause as `{"status":"paused"}`. `loom
continue --agent` and `loom abort --agent` follow the same JSON contract:
another conflict during `continue` reports `paused` again; completion reports
`ok`.

## Prerequisites

- `agent init` without `--project`: a resolvable home directory.
- `agent init --project`: run inside a git repository with a work tree.
- Agent mode: none beyond the command's own prerequisites.

## Examples

### Committing without a branch — the agent learns the choices

```
$ git-loom commit --agent -m "Fix login validation"
```

```json
{"status":"needs_input","kind":"select","prompt":"Select target branch",
 "options":["feature-auth","feature-ui"],"allow_other":true,
 "hint":"re-run with: loom commit -b <branch> -m <message> [files...] (a new name creates the branch)"}
```

```
# exit code 10; the agent shows the options to the user, then:
$ git-loom commit --agent -b feature-auth -m "Fix login validation"
```

```json
{"status":"ok","messages":["Created commit `1a2b3c4` on branch `feature-auth`"]}
```

### Dropping a file — confirmation becomes data

```
$ git-loom drop --agent ma
```

```json
{"status":"needs_confirmation","prompt":"Discard changes to `src/main.rs`?",
 "hint":"re-run with: loom drop <target> -y"}
```

```
$ git-loom drop --agent ma -y
```

```json
{"status":"ok","messages":["Restored `src/main.rs`"]}
```

### A conflicting update

```
$ git-loom update --agent -y
```

```json
{"status":"paused","message":"Conflicts detected — the `loom update` is paused",
 "hint":"resolve conflicts, stage them, then run: loom continue (or loom abort)",
 "messages":["Fetched latest changes"]}
```

```
# exit code 0 — but not ok: the agent resolves conflicts, then
$ git-loom continue --agent
```

```json
{"status":"ok","messages":["Updated branch `integration` with `origin/main`"]}
```

### Installing the skill

```
$ git-loom agent init
✓ Installed Claude skill at `C:\Users\me\.claude\skills\git-loom\SKILL.md`
  › Restart Claude Code to pick up the new skill

$ git-loom agent init
✓ Claude skill already up to date
```

## Design Decisions

### JSON on stderr, not stdout

`show` and `diff` hand stdout to git directly, so loom cannot guarantee a
clean stdout stream — and the status graph is itself a stdout payload worth
keeping separate from the machine contract. In agent mode stdout is therefore
reserved for payload, and the JSON status is the **last line of stderr**,
printed after all child processes have finished.

### Exit code 10, not 2

The CLI parser already exits 2 for malformed invocations. An agent must be
able to tell "I called this wrong" (2) from "the command needs an answer"
(10) without parsing anything.

### Pre-flight prompts unwind; post-mutation prompts degrade

Answering `needs_input` implies nothing happened, so it is only correct for
prompts that fire before the repository is touched. The two prompts that fire
after the main work succeeded (`update`'s prune confirmation, `push`'s PR
title/browser step) instead take the safe default — skip the optional
follow-up — and report `ok` with the skipped action in `messages`. Reporting
`needs_input` there would falsely tell the agent the push or update did not
happen.

### One status line even on success

An agent that sees no JSON cannot distinguish "success" from "crashed before
reporting". Every invocation in agent mode ends with exactly one status line,
success included, so absence of the line is itself a signal (abnormal
termination).

### Explicit opt-in, never TTY detection

Integration tests, shell pipelines, and `git loom | less` all run without a
terminal but expect the human contract. Agent mode changes prompt behavior
and stream routing, so it activates only by explicit `--agent` or
`LOOM_AGENT`.

### Options are plain strings

`options` entries are exactly the strings to pass back on the CLI (branch
names, file paths). No id/label indirection: the agent copies the chosen
option into the hinted command.
