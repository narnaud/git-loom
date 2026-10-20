# agent

Install the loom skill for an AI coding agent, and drive loom from agents or scripts with `--agent`.

For a walkthrough, see [Working with AI Agents](../guides/agents.md).

## Usage

```
git loom agent install [<agent>] [--project]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<agent>` | Agent to install the skill for. Only `claude` (the default). |

### Options

| Option | Description |
|--------|-------------|
| `--project` | Install into the repository (`.claude/skills/git-loom/SKILL.md`) instead of your home directory |

## What It Does

Writes the skill to `~/.claude/skills/git-loom/SKILL.md`, or into the repository with `--project`. The skill teaches the agent to use loom instead of raw git history rewrites.

Re-run it after upgrading loom: it reports whether the skill was installed, updated, or already up to date. The file belongs to loom, so local edits are overwritten.

In agent mode, every command checks the installed skills. One that differs from the skill this loom ships adds a message to the JSON output, telling the agent to run `git-loom agent install` and restart.

## Agent Mode

The global `--agent` flag, or the `LOOM_AGENT` environment variable set to any non-empty value other than `0`, makes loom machine-readable. It is never turned on automatically.

### Output

- **stdout** ends with exactly one JSON object on its **last line**. `show`, `diff`, `trace`, `absorb`, `worktree list` and `worktree path` print their own output before it; every other command prints only the JSON.
- **stderr** carries the human output: progress lines and the status tree.

| Status | Exit code | Meaning |
|--------|-----------|---------|
| `ok` | 0 | Success. `messages` lists what happened. |
| `needs_input` | 10 | A prompt would have opened. No history changed (files you named may already be staged). `options` lists the choices (`allow_other: true` also accepts a new value) and `hint` the command to re-run. |
| `needs_confirmation` | 10 | A yes/no question would have opened. No history changed. Re-run with `-y` to confirm. |
| `paused` | 0 | A conflict paused the operation. Resolve it, then [`continue`](continue.md) or [`abort`](abort.md). |
| `error` | 1 | The command failed; `message` says why. |

A malformed command line exits with 2 and prints no JSON.

### What Changes

- Prompts never open: they answer `needs_input` or `needs_confirmation`. Two questions come after the work is done, so they take the safe default instead: `update` keeps branches it would offer to remove (pass `-y` to remove them), and `push` leaves a branch of several commits without a PR unless you pass `--title`.
- `commit`, `split` and `reword` need `-m`; no editor opens.
- `-p` lists hunks instead of opening the picker (see below).
- `show` and `diff` never start a pager.
- `tui` is unavailable.
- `status` adds the branch graph to its JSON as `graph` (see [status](status.md#agent-mode)), and `push` adds a `push` report (see [push](push.md#agent-mode)).

### Hunk Selection

Every `-p` form (`add`, `commit`, `fold`, `split`) takes two calls. The first lists the hunks and changes nothing:

```bash
git loom split pop --agent -m "Fix the off-by-one" -p
# {"status":"needs_input","kind":"multiselect","prompt":"Select hunks",
#  "options":["src/parse.rs:1","src/parse.rs:2"],"fingerprint":"a91c3f2be417",
#  "items":[{"id":"src/parse.rs:1","path":"src/parse.rs","diff":"@@ -12,7 +12,7 @@ fn scan\n..."},
#           {"id":"src/parse.rs:2", ...}],
#  "hint":"re-run with: loom split pop -m 'Fix the off-by-one' -p --hunks <id> [--hunks <id>...] --hunks-from a91c3f2be417"}
```

The second re-runs the `hint` with the chosen ids, one `--hunks` per id:

```bash
git loom split pop --agent -m "Fix the off-by-one" -p --hunks src/parse.rs:1 --hunks-from a91c3f2be417
# {"status":"ok","messages":["Split `b41c298` into `cup` (2a0a929) and `pop` (d979b2b)"]}
```

- `--hunks` is the whole selection. Hunks already staged are listed with `"staged": true`: leaving one out unstages it with `add -p`, and keeps it staged but out of the commit with `commit -p` and `fold -p`.
- A binary file, a deleted file and a submodule are a single entry that moves whole; its `diff` is a label such as `(binary file)`. An untracked file is listed as `(new file, <n> line(s))`. Read the file at `path` for its content.
- If the diff changed since the listing, the fingerprint no longer matches and loom refuses the selection. List again.
- Keep every argument of the `hint`, including file filters: they shape the listing.

`--hunks` also works outside agent mode, but only a listing gives you the fingerprint.

## Examples

### Install the Claude skill

```bash
git loom agent install
# ✓ Installed Claude skill at `/home/me/.claude/skills/git-loom/SKILL.md`
#   › Restart Claude Code to pick up the new skill
```

### Commit without choosing a branch

```bash
git loom commit --agent -m "Fix login"
# {"status":"needs_input","kind":"select","prompt":"Select target branch",
#  "options":["feature-auth","feature-ui"],"allow_other":true,
#  "hint":"re-run with: loom commit -b <branch> -m <message> [files...] (a new name creates the branch), or -l for a loose commit on the integration branch"}

git loom commit --agent -b feature-auth -m "Fix login"
# {"status":"ok","messages":["Created commit `pop` (1a2b3c4) on branch `feature-auth`"]}
```

### A conflicting update

```bash
git loom update --agent -y
# {"status":"paused","message":"Conflicts detected — the `loom update` is paused",
#  "hint":"resolve conflicts, stage them, then run: loom continue (or loom abort)"}

git add src/auth.rs && git loom continue --agent
# {"status":"ok","messages":["Updated branch `integration` with `origin/main`"]}
```
