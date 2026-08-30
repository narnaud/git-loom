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

### Agent mode (`--agent`)

The global `--agent` flag (or the `LOOM_AGENT` environment variable, any value except `0`) makes every loom invocation end with exactly one JSON status as the **last line of stderr**; stdout stays reserved for command payload (status graph, `show`/`diff` output).

| Status | Exit code | Meaning |
|--------|-----------|---------|
| `ok` | 0 | Success. `messages` collects the progress lines, including skipped optional follow-ups. |
| `needs_input` | 10 | A prompt would have opened; nothing was changed. `options` lists the choices, `hint` the command to re-run. `allow_other: true` means a new value is also accepted. |
| `needs_confirmation` | 10 | A yes/no question would have opened; nothing was changed. |
| `paused` | 0 | A rebase stopped on conflicts — resolve, then [`continue`](continue.md) or [`abort`](abort.md). |
| `error` | 1 | The command failed. |

In agent mode:

- Interactive prompts never render — they answer `needs_input`/`needs_confirmation` instead.
- `-p`/`--patch` is rejected (the hunk picker is a full-screen UI).
- `commit`, `split`, and `reword` require `-m` (no editor is opened).
- `push` never opens a browser: PR creation is skipped and reported in `messages`.
- `update` skips the gone-branch pruning question (use `-y` to prune).
- `show`/`diff` disable the git pager.

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
#  "hint":"re-run with: loom commit -b <branch> -m <message> [files...] (a new name creates the branch)"}

git loom commit --agent -b feature-auth -m "Fix login"
# {"status":"ok","messages":["Created commit `1a2b3c4` on branch `feature-auth`"]}
```

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
