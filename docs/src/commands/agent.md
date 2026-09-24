# agent

Set up AI agent integration: install the loom skill for an AI coding agent, and drive loom itself with the machine-readable `--agent` mode.

## Usage

```
git loom agent init [<agent>] [--project]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<agent>` | AI agent to install the skill for. Currently `claude` (default). |

### Options

| Option | Description |
|--------|-------------|
| `--project` | Install into the repository (`.claude/skills/git-loom/SKILL.md`) instead of the home directory |

`agent init` is unrelated to [`init`](init.md), which sets up an integration branch.

## What It Does

### agent init

Installs a skill file at `~/.claude/skills/git-loom/SKILL.md` (or under the work tree root with `--project`) that teaches the agent to use loom instead of raw git — including the `--agent` invocation rules below. Re-run it after upgrading loom to refresh the skill:

- File absent → created (`Installed Claude skill at ...`)
- File differs → overwritten (`Updated Claude skill at ...`)
- File identical → untouched (`Claude skill already up to date`)

### Keeping the skill up to date

The skill is compiled into the loom binary, so loom can see when an installed copy no longer matches what it ships. In agent mode every invocation checks the installed skills — the home one, plus the in-repo one when inside a work tree — and warns on any that differ:

```json
{"status":"ok","messages":["The Claude git-loom skill at `~/.claude/skills/git-loom/SKILL.md` differs from the one this loom ships. Run `git-loom agent init` to refresh it (local edits are overwritten). Restart Claude Code to pick up the new skill."]}
```

The notice exists only in the JSON — it is not printed as a `!` line, since the agent parses the JSON and would otherwise read it twice. It rides on `ok` and `paused` responses; an `error` response carries no `messages`, and the check simply reports again on the next command that succeeds. Nothing is rewritten automatically, and a location with no skill installed is never mentioned.

Because the comparison is byte-for-byte, the installed skill is not a file to edit: any local change is reported as stale, and `agent init` overwrites it.

### Agent mode (`--agent`)

The global `--agent` flag (or the `LOOM_AGENT` environment variable, any value except `0`) splits loom's output by audience:

- **stdout is the machine stream**: exactly one JSON status object, always as the **last line** — read that line, not the whole stream. The output you asked for comes there first — the patch from `show` and `diff`, the log from `trace`, the plan from `absorb`. For everything else the last line is the only line.
- **stderr is the human stream**: the `✓`/`!`/`✗` progress lines, the rendered status tree (its data is in the JSON), and `update`'s fetch summaries. No JSON is written there.

`completions` ignores agent mode: it always prints the script alone.

| Status | Exit code | Meaning |
|--------|-----------|---------|
| `ok` | 0 | Success. `messages` collects the progress lines, including skipped optional follow-ups. |
| `needs_input` | 10 | A prompt would have opened; nothing was changed. `options` lists the choices, `hint` the command to re-run. `allow_other: true` means a new value is also accepted. |
| `needs_confirmation` | 10 | A yes/no question would have opened; nothing was changed. |
| `paused` | 0 | A rebase stopped on conflicts — resolve, then [`continue`](continue.md) or [`abort`](abort.md). |
| `error` | 1 | The command failed. |

In agent mode:

- Interactive prompts never render — they answer `needs_input`/`needs_confirmation` instead.
- `-p`/`--patch` answers with a hunk listing instead of opening the picker (see below) — on [`add`](add.md), [`commit`](commit.md), [`fold`](fold.md) and [`split`](split.md) alike.
- `commit`, `split`, and `reword` require `-m` (no editor is opened).
- `push` never opens a browser: PR creation is skipped and reported in `messages`.
- `update` skips the gone-branch pruning question (use `-y` to prune).
- `show`/`diff` disable the git pager.
- `status` attaches the whole branch graph to its `ok` object as `graph` — see [status](status.md#agent-mode).

Agent mode is never inferred from a missing terminal — it must be requested explicitly.

## Examples

### Install the Claude skill

```bash
git loom agent init
# ✓ Installed Claude skill at `C:\Users\me\.claude\skills\git-loom\SKILL.md`
#   › Restart Claude Code to pick up the new skill
```

### Install into the current repository

```bash
git loom agent init claude --project
# ✓ Installed Claude skill at `D:\myrepo\.claude\skills\git-loom\SKILL.md`
```

### An agent commits without picking a branch

```bash
git loom commit --agent -m "Fix login"
# {"status":"needs_input","kind":"select","prompt":"Select target branch",
#  "options":["feature-auth","feature-ui"],"allow_other":true,
#  "hint":"re-run with: loom commit -b <branch> -m <message> [files...] (a new name creates the branch), or -i for the integration branch itself"}

git loom commit --agent -b feature-auth -m "Fix login"
# {"status":"ok","messages":["Created commit `mqt` (1a2b3c4) on branch `feature-auth`"]}
```

### Picking hunks without the picker

Every `-p` form lists its hunks instead of drawing the picker. It takes two calls: the first lists, the second selects.

```bash
git loom split ab --agent -m "Fix the off-by-one" -p
# {"status":"needs_input","kind":"multiselect","prompt":"Select hunks",
#  "options":["src/parse.rs:1","src/parse.rs:2"],"fingerprint":"a91c3f2be417",
#  "items":[{"id":"src/parse.rs:1","path":"src/parse.rs",
#            "diff":"@@ -12,7 +12,7 @@ fn scan\n..."},
#           {"id":"src/parse.rs:2", ...}],
#  "hint":"re-run with: loom split ab -m <message> -p --hunks <id> [--hunks <id>...] --hunks-from a91c3f2be417"}

git loom split ab --agent -m "Fix the off-by-one" -p --hunks src/parse.rs:1 --hunks-from a91c3f2be417
# {"status":"ok","messages":["Split `b41c298` into `2a0a929` and `d979b2b`"]}
```

`items` lists every entry of the diff, and `options` repeats their ids. A binary file, a deletion and a submodule are one entry each and move whole; their `diff` is only a label — `(binary file)`, `(file deleted)`, `(submodule)` — so read the file at `path` if its content matters. On the working tree the fingerprint still covers a binary's content, so editing it after the listing invalidates the ids. With the one exception below, a `diff` is never truncated, so narrow a large listing with `split -p <files>` rather than expecting loom to cut it short.

The one exception is an untracked file's sole entry, which lists as `(new file, <n> line(s))`: loom built that entry from the bytes on disk, so reading the file gives exactly what the id stands for. Nothing else is summarized. Once the path is in the index — staged or `git add -N` — the entry is git's diff of the indexed content, which a clean or eol filter can make something else than the file on disk, so it is listed verbatim. So is a file a commit adds, which need not be on disk at all.

Re-run the `hint` as given, including any `<files>` filter — it carries every argument that shapes the listing.

Hunk ids are positional, so `--hunks-from` carries the fingerprint of the listing they came from. Loom recomputes it and refuses a selection taken from a diff that has since changed, rather than move whatever now sits at those positions:

```bash
# ab was rewritten in between, so the ids no longer number the same hunks
git loom split ab --agent -m "..." -p --hunks src/parse.rs:1 --hunks-from 35374d8ca902
# {"status":"error","message":"The hunks changed since the listing fingerprinted 35374d8ca902 (now 6b0f19d4c773)\nRe-run with -p alone to list them again"}
```

`--hunks` repeats, once per id, and takes no separated list: an id contains a path, and any separator is a character some path may hold. A comma-joined value errors and says so.

`--hunks` is the **whole** selection, exactly as confirming the picker with those entries ticked. That matters for a working-tree source (`add -p`, `commit -p`, `fold -p <files> <commit>`), where an already-staged hunk starts selected and is marked `"staged": true` in the listing: leave its id out and `add -p` unstages it, while `commit -p` and `fold -p` leave it out of what they create and keep it staged. For `add -p`, leaving it out is refused when the working tree changed those lines again, since the staged version then exists only in the index. A commit source has nothing selected to begin with, so there the distinction does not arise.

`--hunks` needs `-p` and works outside agent mode too, though the fingerprint only comes from a listing, so the first call still needs `--agent` or `LOOM_AGENT=1`.

### A conflicting update

```bash
git loom update --agent -y
# {"status":"paused","message":"Conflicts detected — the `loom update` is paused",
#  "hint":"resolve conflicts, stage them, then run: loom continue (or loom abort)"}

# resolve the conflicts, then:
git loom continue --agent
# {"status":"ok","messages":["Updated branch `integration` with `origin/main`"]}
```

## Prerequisites

- `agent init`: a resolvable home directory (or a git repository with `--project`)
- Agent mode: none beyond each command's own prerequisites
