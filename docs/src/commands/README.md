# Commands Overview

```
Usage: git-loom [OPTIONS] [COMMAND]

Workflow:
  init              Initialize a new integration branch
  update, up        Pull-rebase and update submodules
  push, pr          Push a branch to remote
  agent             Install the loom skill for AI agents

Staging:
  add               Stage files using short IDs or paths [-p for interactive hunks]

Commits:
  commit, ci        Create a commit on a feature branch [-p for interactive hunks]
  fold              Amend, fixup, or move commits [-p for interactive hunks] [amend, am, fixup, mv, rub]
  absorb            Auto-distribute changes into originating commits
  split             Split a commit into two [-p for interactive hunks]
  swap              Swap two commits
  reword, rw        Reword a commit message or rename a branch
  drop, rm          Drop a change, commit, or branch

Branches:
  branch, br        Manage feature branches (create, merge, unmerge)
  switch, sw        Switch to any branch for testing (without weaving)

Inspection:
  status            Show the branch-aware status (default command)
  tui               Interactive status TUI (tree + diff, with actions)
  show, sh          Show commit details (like git show)
  diff, di          Show a diff using short IDs (like git diff)
  trace             Show the latest command trace

Recovery:
  continue, c       Resume a paused operation after resolving conflicts
  abort, a          Cancel a paused operation and restore original state

Options:
      --no-color       Disable colored output
      --agent          Machine-readable JSON status output for AI agents (see also LOOM_AGENT)
      --theme <THEME>  Color theme for graph output [default: auto] [possible values: auto, dark, light]
  -h, --help           Print help (see more with '--help')
  -V, --version        Print version
```

Running `git loom` with no command is equivalent to `git loom status`.

All commands that accept a target (commit, branch, or file) support [short IDs](status.md) — the compact identifiers shown in the status output. You can also use full git hashes, branch names, or partial hashes.

## Passing Options to Git

`show`, `diff`, `commit` and `add` each wrap a single git command. Anything
after a `--` separator is handed to that command untouched:

```bash
git loom show -- --stat
git loom diff osy..mqt -- --name-only
git loom commit -m "wip" -- --no-verify
git loom add zz -- -f
```

Before the separator loom parses strictly, so an option it doesn't define is an
error rather than a guess — the message tells you to move it after the `--`.
That also means a flag keeps loom's meaning on loom's side of the separator and
git's meaning on git's: `git loom diff -a` is loom's `--all`, while
`git loom diff -- -a` is git's `--text`.

Forwarded arguments land after the revisions loom resolved and before any
pathspec loom builds, so options stay options and paths stay paths. Because the
tokens are never inspected, values may be attached or detached — `-U5`,
`--unified=5` and `-S x` all reach git as written.

When you forward arguments to `add` or `commit`, loom steps back: the git
command runs uncaptured so its own output reaches you, and loom stops narrating
what it can no longer vouch for. (Under `--agent` those two stay captured —
there is nobody there to close an editor.) `show` and `diff` are always
uncaptured; displaying is all they do.

The other commands don't take a `--`: they either render their own output or
drive a rebase, where there is no single git command to forward to.
