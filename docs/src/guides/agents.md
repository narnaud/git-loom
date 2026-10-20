# Working with AI Agents

AI coding agents know git, not loom. Left alone, an agent amends with
`git commit --amend` or reorders with `git rebase -i`, which tangles your
integration branch. This tutorial sets up [Claude Code](https://claude.com/claude-code)
to drive loom instead, and shows what happens when it does.

It assumes a repository already set up with [`git loom init`](tutorial.md#getting-started).

## 1. Install the skill

```bash
git loom agent install
# ✓ Installed Claude skill at `/home/me/.claude/skills/git-loom/SKILL.md`
#   › Restart Claude Code to pick up the new skill
```

The skill tells the agent to use loom for every history change in a repository
where `git loom status` works. Other repositories are unaffected. Restart
Claude Code to load it, then ask for the status: the agent should answer by
running `git loom status --agent`.

To share the skill with your team, install it into the repository instead and
commit the file:

```bash
git loom agent install --project
# ✓ Installed Claude skill at `/src/app/.claude/skills/git-loom/SKILL.md`
```

Everyone then needs the same loom version: with any other, loom reports the
committed skill as out of date.

## 2. Ask for results, not commands

You don't need to know which loom command does what. Describe the result you
want:

| You ask | The agent runs |
|---------|----------------|
| "Commit the login form to a new feature-auth branch" | `git loom commit -b feature-auth -m "Add login form" src/auth.rs` |
| "Put this CSRF fix into the login form commit" | `git loom fold src/auth.rs pop` |
| "Move the logging helper to feature-dashboard" | `git loom fold sun feature-dashboard` |
| "Split the validation helpers out of that commit" | `git loom split pop -m "Add validation helpers" src/validation.rs` |
| "Pull main and rebase everything" | `git loom update -y` |
| "Open a PR for feature-auth" | `git loom push feature-auth` |

The agent names files explicitly, so unrelated edits in your working tree stay
out of its commits. It always passes a message with `-m`, and it pushes only
when you ask, because a push is visible to others. Its `update -y` also
removes local branches that were merged upstream or deleted on the remote.

## 3. Loom never guesses

The agent spells out every choice — which branch, which files — from what you
asked. When a command still leaves a question open, loom doesn't guess or wait
at a prompt: it answers with the question, changing no history, and the agent
brings it to you. Here the agent left out the branch:

```console
$ git loom commit --agent -m "Limit user name length" src/auth.rs
{"status":"needs_input","kind":"select","prompt":"Select target branch",
 "options":["feature-auth","feature-dashboard"],"allow_other":true,
 "hint":"re-run with: loom commit -b <branch> -m <message> [files...] (a new name creates the branch), or -l for a loose commit on the integration branch"}
```

The agent asks you which branch, then re-runs the command with your answer.

## 4. Let it pick hunks

When one file holds two unrelated changes, the agent uses `-p` as you would.
Instead of opening the hunk picker, loom lists the hunks:

```console
$ git loom commit --agent -b feature-auth -m "Trim user names" -p src/auth.rs
{"status":"needs_input","kind":"multiselect","prompt":"Select hunks",
 "options":["src/auth.rs:1","src/auth.rs:2"],"fingerprint":"3d12f8f83f89",
 "items":[{"id":"src/auth.rs:1","path":"src/auth.rs","diff":"@@ -1,5 +1,5 @@\n..."},
          {"id":"src/auth.rs:2","path":"src/auth.rs","diff":"@@ -15,6 +15,6 @@\n..."}],
 "hint":"re-run with: loom commit -b feature-auth -m 'Trim user names' -p src/auth.rs --hunks <id> [--hunks <id>...] --hunks-from 3d12f8f83f89"}
```

The agent reads the diffs, picks, and re-runs:

```console
$ git loom commit --agent -b feature-auth -m "Trim user names" -p src/auth.rs --hunks src/auth.rs:1 --hunks-from 3d12f8f83f89
{"status":"ok","messages":["Applied 1 change(s) across 1 file(s)","Created commit `hug` (c420bf8) on branch `feature-auth`"]}
```

If the file changes between the two calls, the fingerprint no longer matches
and loom refuses the selection rather than commit the wrong hunk.

## 5. Conflicts

A conflict pauses the operation, as it does for you, and the agent can't
mistake it for success:

```console
$ git loom update --agent -y
{"status":"paused","message":"Conflicts detected — the `loom update` is paused",
 "hint":"resolve conflicts, stage them, then run: loom continue (or loom abort)",
 "messages":["Fetched latest changes"]}
```

The agent resolves the files, stages them with `git add`, and runs
`git loom continue` — or `git loom abort` if you'd rather back out. Other loom
commands are refused until then.

## 6. Keep the skill current

The skill ships inside the loom binary. After a loom upgrade the installed copy
is out of date, and loom tells the agent so on its next command:

```
The Claude git-loom skill at `/home/me/.claude/skills/git-loom/SKILL.md` differs from the one this loom ships. Run `git-loom agent install` to refresh it (local edits are overwritten). Restart Claude Code to pick up the new skill.
```

Run the install again (the agent can do it), then restart Claude Code. Don't
edit the installed skill: loom reports any change as out of date, and the next
install overwrites it.

## Other agents and scripts

`agent install` supports Claude Code only. Any other agent, or your own script,
can drive loom through agent mode: add `--agent` to every command, or set
`LOOM_AGENT=1`.

- Read the **last line of stdout**: it is always one JSON object with a
  `status`, except for a malformed command line (exit 2). Ignore stderr; it is
  for people.
- `ok` (exit 0) succeeded. `needs_input` and `needs_confirmation` (exit 10)
  changed no history and give the command to re-run. `paused` (exit 0) is a
  conflict, not a success. `error` (exit 1) failed.
- `git loom status --agent` returns the whole branch graph, with every short ID.

See [agent](../commands/agent.md) for the full contract.
