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
  fold              Amend, fixup, or move commits [-p for interactive hunks] [amend, fixup, mv, rub]
  absorb            Auto-distribute changes into originating commits
  split             Split a commit into two [-p for interactive hunks]
  swap              Swap two commits
  reword, rw        Reword a commit message or rename a branch
  drop, rm          Drop a change, commit, or branch

Branches:
  branch, br        Create a feature branch
  merge             Merge an existing branch into integration
  unmerge           Remove a branch from integration (keeps the ref)
  switch, sw        Switch to any branch for testing (without merging)
  worktree, wt      Manage worktrees, each with its own integration branch (list by default, new, drop, path)

Inspection:
  status            Show the branch-aware status (default command)
  tui               Interactive status TUI (tree + diff, with actions)
  show, sh          Show commit details (like git show)
  diff, di          Show a diff using short IDs (like git diff)
  trace             Show the latest command trace

Recovery:
  continue, cont    Resume a paused operation after resolving conflicts
  abort             Cancel a paused operation and restore original state

Options:
      --no-color       Disable colored output
      --agent          Machine-readable JSON status output for AI agents (see also LOOM_AGENT)
      --theme <THEME>  Color theme for graph output [default: auto] [possible values: auto, dark, light]
  -h, --help           Print help (see more with '--help')
  -V, --version        Print version
```

`git loom` with no command runs `git loom status`.

Wherever a command takes a commit, branch or file, you can give its [short ID](status.md#short-ids) from `git loom status`, as well as a hash, branch name or path.

## Passing Options to Git

`show`, `diff`, `add` and `commit` each wrap one git command. Anything after a `--` separator goes to it untouched:

```bash
git loom show -- --stat
git loom diff pop..hug -- --name-only
git loom commit -m "wip" -- --no-verify
git loom add zz -- -f
```

`fold`, `split`, `absorb` and `reword` accept them too, for the commits they create. The other commands take none.

Before the `--`, loom accepts only its own options: an unknown one is an error that tells you to move it after the separator. A flag means loom's option on one side and git's on the other: `git loom diff -a` is loom's `--all`, `git loom diff -- -a` is git's `--text`.

> [!WARNING]
> `--` is expert mode: you can shoot yourself in the foot. Loom passes the options to git as written and checks nothing. An option that changes what git commits or how — `-a`, a pathspec, `-m`, `--amend`, `--dry-run` — does exactly that, even in the middle of a loom command that makes several commits, and loom carries on with whatever git did. Use it only when you know what the option does to that git command.
