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
could have passed, or that `-i` would commit to the integration branch
itself. Without packaged guidance, agents also fall back to raw
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
object, printed as the last line of stdout**. The two streams split by
audience:

- stdout is the machine stream: an agent, an IDE or any other tool reads that
  stream alone, and the object is always its **last line**. For most commands
  it is the only line; the output an agent asked for comes first — `show`'s
  and `diff`'s patch, `trace`'s log dump, `absorb`'s plan. A reader takes the
  last line, never the whole stream.
- stderr is the human stream: the progress lines (`✓`/`!`/`✗`), the rendered
  status tree and `update`'s fetch summaries. No JSON is written there.

`completions` never runs in agent mode: its script is for a shell, so it
prints the script alone whatever `--agent` or `LOOM_AGENT` say.

The status graph is also emitted as structured data, inside the `ok` object
(see [The status graph](#the-status-graph)); the tree on stderr is the same
information rendered for a person.

The possible statuses:

```json
{"status":"ok","messages":["Created commit `mqt` (1a2b3c4) on branch `feature-auth`"]}
```

Emitted when the command succeeds. `messages` collects the success and warning
lines the command printed (some commands print several; none may be present).
A message from `commit`, `split`, `reword`, or `fold` naming a commit it
created or rewrote gives its persistent short ID before the hash,
`` `mqt` (1a2b3c4) ``, when the commit has one (Spec 002); the ID survives
later rewrites, so the agent can chain commands on it without re-running
`status`. A commit without a Change-Id is named by hash alone; `swap` and
`drop` name commits by hash.

```json
{"status":"needs_input","kind":"select","prompt":"Select target branch",
 "options":["feature-auth","feature-ui"],"allow_other":true,
 "hint":"re-run with: loom commit -b <branch> -m <message> [files...] (a new name creates the branch), or -i for the integration branch itself"}
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
history. Nothing was changed. `prompt` may span lines: the first is the
question, the rest detail what confirming does (`loom drop` lists each path).

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

### The status graph

`loom status` attaches its whole graph to the `ok` object under `graph`. No
other command emits it. The model mirrors the rendered tree — same sections,
same order (Spec 001), same short IDs — so an agent never parses glyphs:

```json
{"status":"ok","graph":{
  "schema": 1,
  "integration_branch": "integration",
  "cwd_prefix": "",
  "local_changes": {
    "id": "zz",
    "files": [{"id":"ma","path":"src/main.rs","index":"M","worktree":" ",
               "state":"tracked"}]
  },
  "branches": [
    {"names": [{"id":"fu","name":"feature-ui","remote":null}],
     "stacked_on": "feature-api", "stacked_on_hidden": false,
     "commits": [
       {"id":"qvn","hash":"9c1d044","oid":"9c1d0448…",
        "subject":"feat(ui): render the settings panel","change_id":"I9c1d…",
        "files":[{"id":"qvn:0","path":"src/ui.rs","index":"M","worktree":" "}]}
     ]},
    {"names": [{"id":"fa","name":"feature-api","remote":"synced"}],
     "stacked_on": null, "stacked_on_hidden": false,
     "commits": [{"id":"mqt","hash":"1a2b3c4","oid":"1a2b3c4d…",
                  "subject":"feat(api): add the settings endpoint",
                  "change_id":"I1a2b…","files":[]}]}
  ],
  "loose_commits": [],
  "upstream": {"label":"origin/main","base_hash":"7f3e9b0","base_oid":"7f3e9b0c…",
               "base_subject":"chore(release): 2.4.0","base_date":"2026-09-14",
               "commits_ahead":0},
  "context_commits": []
}}
```

- `schema` is an integer, bumped on any breaking change to this shape.
- `branches` lists branch *groups* in render order: empty ones first, then the
  top of each stack downward. A group holds several `names` when branches are
  co-located on one tip, ordered alphabetically last first as the tree draws
  them.
- `stacked_on` names the group directly below in the stack, else `null`. It
  is the same edge that draws `││` and that a stacked push follows, named as
  the push names it: for a co-located group that is its *last* `names` entry
  (Spec 011). Ownership is already resolved: a stacked branch lists only the
  commits above the branch below it, so the agent never walks parents.
- `stacked_on_hidden` is `true` when the branch below matches
  `loom.hideBranchPattern`: `push` refuses the group (Spec 011), and hiding
  removes the edge, so `stacked_on` is `null` unless `--all` shows it.
- `remote` is `synced`, `different`, `gone`, or `null` for a branch never
  pushed — the `✓`/`↑`/`✗` indicators, or none.
- `state` on a working-tree file is `conflicted`, `tracked` or `untracked`:
  the three groups the local-changes section renders, already classified.
  `index` and `worktree` carry the raw `XY` characters alongside.
- A commit's `id` is its short ID (persistent letters with a Change-Id, a hash
  prefix without), `hash` the abbreviated hash, `oid` the full one.
- `files` is populated only under `-f`; ids are `<commit id>:<n>` counting
  from 0, exactly as `status -f` prints them.
- Paths are relative to `cwd_prefix`, as every other loom surface prints them.
- Hidden branches and `--all` behave as they do on the tree: IDs are allocated
  before hiding, so they do not shift.

### Prompt sites

Every interactive prompt behaves as follows in agent mode. Prompts are
classified **pre-flight** (nothing has been changed yet — the command answers
`needs_input`/`needs_confirmation` and exits) or **post-mutation** (the main
work already succeeded — the command takes a safe default, reports `ok`, and
mentions the skipped action in `messages`).

| Command / prompt | Class | Agent-mode behavior |
|---|---|---|
| `commit` branch picker (no `-b`, no `-i`) | pre-flight | `needs_input` (select, `allow_other`) listing woven branches; hint: `loom commit -b <branch> -m <message> [files...]` (a new name creates the branch), or `-i` for the integration branch itself. With `-p` it is raised before the hunk listing, and its hint keeps `-p` and the file filter: the branch prompt answered without them stages whole files, which would undo the picking |
| `commit` editor (no `-m`) | pre-flight | `needs_input` (text); hint: pass `-m <message>`; with `-p` it keeps `-p` and the file filter |
| `split` message editor (no `-m`) | pre-flight | `needs_input` (text); hint repeats the invocation with `-m <message>`, `-p` and the file filter included |
| `split` file picker (no files, no `-p`) | pre-flight | `needs_input` (multiselect) listing the commit's files; hint: `loom split <target> -m <message> <files...>` |
| any `-p` hunk picker | pre-flight | `needs_input` (multiselect) listing the hunks (see [Hunk selection](#hunk-selection)) |
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

### Hunk selection

The hunk pickers are full-screen terminal UIs, so agent mode answers every one
of them with the hunk listing instead. This covers `add -p`, `commit -p` and
all three `-p` forms of `fold`, plus `split -p`. A guard at the picker itself
backstops any future call path, which MUST answer as data before reaching it.

```json
{"status":"needs_input","kind":"multiselect","prompt":"Select hunks",
 "options":["src/fold.rs:1"],"fingerprint":"a91c3f2be417",
 "items":[{"id":"src/fold.rs:1","path":"src/fold.rs",
           "diff":"@@ -120,7 +120,9 @@ fn resolve\n...","selectable":true},
          {"id":"logo.png:1","path":"logo.png",
           "diff":"(binary file)","selectable":false}],
 "hint":"re-run with: loom fold -p c2 c1 --hunks <id> [--hunks <id>...] --hunks-from a91c3f2be417"}
```

Listing hunks is pre-flight: nothing is staged, committed or rewritten. The
agent re-runs the same command with the ids it picked.

A source is either a **commit** (`split -p`, `fold -p <source> <target>`,
`fold -p <commit> zz`) or the **working tree** (`add -p`, `commit -p`,
`fold -p [<files>...] <commit>`). They differ in one way: a working-tree entry
that is already staged starts selected, and `items` marks it `"staged": true`.
`--hunks` replaces the selection wholesale — exactly as confirming the picker
with those entries ticked. A staged id left out of it is unstaged by `add -p`;
`commit -p` and `fold -p` keep it out of what they create and put it back
staged afterwards, like any other staged work they set aside. A commit source
has nothing selected to begin with, so there the two readings coincide.
Unstaging reverse-applies to the index alone, so where it is for good — `add -p`
and the TUI's `C` — leaving out a staged id or unticking it is refused when the
working tree changed a line it adds again (or changed a staged binary at all):
that content would exist nowhere afterwards. `commit -p` and `fold -p` need no
such refusal, since what they put back carries the content itself.

Applying a selection is all or nothing: a step that fails puts the index back.
A picked hunk whose context a staged hunk being unstaged in the same file
changes no longer applies, and is refused rather than placed by a looser match.

- `items` lists every entry the picker itself would show, in that order,
  including the ones this command cannot take, so the ids it did not get are
  still accounted for. A file whose diff carries no text at all — a mode-only
  change, a pure rename — reaches neither, and a commit with nothing but those
  errors ``No hunks to select in `<hash>`⏎Its changes carry no text -p can
  pick, or the given files matched none`` instead of listing nothing.
- `id` is `<path>:<n>`, `n` counting from 1 within the file. It is passed back
  verbatim, one `--hunks` per id, so a path may hold any character including a
  comma. There is no separator to escape and none to get wrong.
- `diff` is the hunk verbatim and is never truncated — the agent picks from it.
  Verbatim up to UTF-8: a byte that is not valid UTF-8 lists as U+FFFD, and a
  selection carrying that hunk fails the apply and rolls back.
  A listing is as large as the diff; narrow it with `<files>`. The one
  exception is an **untracked** file's sole `@@ -0,0` entry, listed as
  `(new file, <n> line(s))`, counting the added lines: loom synthesized that
  text from the bytes it read off disk, so reading the file gives exactly what
  the id stands for. Nothing else is summarized. Once the path is in the index
  — staged or `git add -N` — the text is git's diff of the *indexed* content,
  which a clean or eol filter makes something else than the file on disk (a
  one-line LFS pointer for a huge file), so it stays verbatim. So does a
  commit's new file, which need not be on disk at all; a second entry on the
  same file; and filling a tracked empty file, which is a change and not a new
  file. The fingerprint covers the content the listing left out, so any edit to
  the file invalidates the ids.
- `selectable` marks what this command can take, which differs per command: a
  binary file has no hunk a commit-source `fold` can move (Spec 007), while
  `split` takes it whole (Spec 013) and a working-tree source stages it by
  path, so only the commit-source `fold` forms mark it `false`. A submodule
  entry and a deletion are selectable everywhere: they travel whole.
- `options` repeats the selectable ids, so an agent reading only the common
  `needs_input` fields cannot pick a rejected one.
- `fingerprint` digests every commit the operation touches — the source it
  lists and the target it lands in, including the target of
  `fold -p [<files>...] <commit>` — and the whole listing: paths, hunk texts
  and whether each is staged, unselectable entries included. The target
  matters because the replay re-resolves it from the revspec the agent typed,
  and a relative one can name a different commit by then.
- A listing MUST have at least one selectable entry. A commit with none is an
  error (``No hunks to select in `<hash>`⏎It changes only binary files, which
  -p cannot move``), never a prompt no answer satisfies.

**CLI:**

```bash
git-loom add -p [<files>...] --hunks <id> [--hunks <id>...] --hunks-from <fingerprint>
git-loom commit -b <branch> -m <message> -p [<files>...] --hunks <id>... --hunks-from <fingerprint>
git-loom split <target> -m <message> -p --hunks <id> [--hunks <id>...] --hunks-from <fingerprint>
git-loom fold -p [<files>...] <commit> --hunks <id> [--hunks <id>...] --hunks-from <fingerprint>
git-loom fold -p <source> <target> --hunks <id> [--hunks <id>...] --hunks-from <fingerprint>
git-loom fold -p <commit> zz --hunks <id> [--hunks <id>...] --hunks-from <fingerprint>
```

`--hunks` repeats, once per id, and MUST NOT take a separated list: an id
contains a path, and every separator is a character some path is allowed to
hold. A value that is not an id errors as one, naming the repeated form when it
holds a comma. `--hunks` requires `-p` and `--hunks-from`, excludes `fold -c`,
and is accepted with or without agent mode. The selected hunks
then follow the interactive path exactly, including its hard-fail and rollback
rules.

`hint` MUST repeat every argument that shapes the operation, shell-quoted:
`split`'s `-m` message and `<files>` filter, and the revisions naming the
commits. `-m <message>` stays a placeholder only in the prompt asking for it. One left
out makes the replay list a different set and fail the fingerprint check
instead of working. The same rule applies to `split`'s missing-`-m` prompt,
whose hint keeps `-p` and the filter rather than pointing at a file-level
split. Forwarded git arguments (Spec 021) end every `-p` hint, after the
selection flags, as `-- <git args>`.

Ids are positional, so `--hunks-from` is what keeps a stale selection from
moving whatever now sits at those positions. Recompute the fingerprint from the
current diff and refuse a mismatch — never resolve the ids against it:

| Condition | Error |
| --- | --- |
| Fingerprint mismatch | ``The hunks changed since the listing fingerprinted <given> (now <current>)⏎Re-run with -p alone to list them again`` |
| Id absent from the diff | ``No hunk `<id>` in this diff`` |
| Id not `<path>:<n>` with `n` plain digits from 1 | ``Invalid hunk id `<id>`⏎Ids look like `src/main.rs:1`` |
| Id of an unselectable entry | ``` `fold -p` cannot move `<id>`: a binary file has no hunk ``` |
| Staged id left out, its lines changed again in the working tree | ``Unstaging `<id>` would lose what only the index holds: the working tree changed `<path>` there again⏎Keep `<id>` staged`` |

`commit -p` sets aside every staged path its picker did not return, and only
once it returned a selection: every exit that carries none — a listing, a
cancelled picker, a refused `--hunks` — leaves the index it was handed.

### Pager suppression

`show` and `diff` invoke git with inherited stdio. In agent mode loom passes
`-c core.pager=cat` so a pty-hosted agent can never hang inside a pager.

### What does not change

- `show` and `diff` payloads keep their raw git format on stdout (already
  color-free when not a terminal). The status graph keeps its rendered form
  too, on stderr with the rest of the human output, and gains the JSON
  representation described above.
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

### The staleness check

The skill is embedded in the binary, so loom can always tell whether what a
user has installed is what it ships: the installed file either matches the
embedded one byte for byte or it does not. That is the same comparison
`agent init` uses to decide whether to rewrite — no version number is
involved.

In agent mode — and only there — every invocation ends by checking the
installed skills, just before the JSON status is emitted:

- The locations checked are the home install and, when run inside a work tree,
  the in-repo one. A location where no skill file exists is skipped silently:
  a user who never ran `agent init` is not nagged.
- A file whose content differs from the embedded skill adds one entry to the
  JSON `messages` array:

```
The Claude git-loom skill at `<path>` differs from the one this loom ships.
Run `git-loom agent init` to refresh it (local edits are overwritten).
Restart Claude Code to pick up the new skill.
```

  (one line in the JSON; wrapped here for reading). The command named is
  `git-loom agent init --project` for an in-repo install.
- Unlike every other message, the notice is **not** also printed as a human
  `!` line. Its only reader is the agent, which parses the JSON, and agent
  mode is the only mode that emits it — printing it too would just make the
  agent read the same sentence twice.
- The notice therefore rides on the `ok` and `paused` statuses only, since an
  `error` response carries no `messages`. Nothing is lost: the check runs on
  every agent-mode invocation, so a stale skill is reported again on the next
  command that succeeds.

The consequence of byte comparison is that the installed skill cannot be
edited locally: any change reads as stale and is reported until `agent init`
overwrites it. This follows from the file already being loom-owned — the same
edit was being silently discarded by `agent init` before this check existed.

The check is advisory and never rewrites anything: loom does not touch files
outside the repository as a side effect of an unrelated command. The agent
relays the warning, and either runs `agent init` itself (it touches no git
state) or lets the user decide.

The check cannot fail the command it rides along with: an unresolvable home
directory, an unreadable file, or a missing repository simply produces no
warning. It does not run in the sequence-editor subprocess, which has agent
mode disabled outright.

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
 "hint":"re-run with: loom commit -b <branch> -m <message> [files...] (a new name creates the branch), or -i for the integration branch itself"}
```

```
# exit code 10; the agent shows the options to the user, then:
$ git-loom commit --agent -b feature-auth -m "Fix login validation"
```

```json
{"status":"ok","messages":["Created commit `mqt` (1a2b3c4) on branch `feature-auth`"]}
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

### Byte comparison, not a skill version number

A version marker in the skill would let the warning name what is installed and
what is available, and would leave a locally edited skill alone. It would also
have to be bumped by hand on every skill edit — a step that, when forgotten,
silently defeats the whole check, and that no test can truly enforce. Byte
comparison cannot be forgotten and reuses the comparison `agent init` already
makes. The cost, a skill that must not be edited in place, is one the file's
loom-owned lifecycle already imposed.

### The check warns, it never auto-installs

Rewriting the skill automatically would be defensible (the file is loom-owned)
but it would make `loom commit --agent` write to the user's home directory as
a side effect of committing. A warning in `messages` reaches the agent, which
can run `agent init` itself or ask the user — the same outcome, with the write
staying an explicit act.

### JSON on stdout, human output on stderr

The streams split by audience, not by kind: stdout is what a tool reads,
stderr is what a person reads. An agent then consumes one stream and needs no
rule for skipping human text, and a human watching a transcript still sees the
tree and the progress lines.

What an agent asked for is machine-side output even when it is text: `show`
and `diff` hand stdout to git directly, and `trace`'s log and `absorb`'s plan
are the answer to the command. That is why the contract is the **last line**
of stdout rather than the whole of it: those print first, and loom prints the
object after all child processes have finished. The status tree is the
exception, on stderr, because the object already carries the graph. For a
command with nothing of its own to print, the last line is the only line.

`--agent` is absent from the completion scripts: it is for tools driving loom,
not for anyone typing at a prompt.

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
