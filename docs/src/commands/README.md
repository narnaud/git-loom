# Commands Overview

```
Usage: git-loom.exe [OPTIONS] [COMMAND]

Workflow:
  init              Initialize a new integration branch
  update, up        Pull-rebase and update submodules
  push, pr          Push a branch to remote

Commits:
  commit, ci        Create a commit on a feature branch
  fold              Amend, fixup, or move commits [amend, am, fixup, mv, rub]
  absorb            Auto-distribute changes into originating commits
  split             Split a commit into two
  reword, rw        Reword a commit message or rename a branch
  drop, rm          Drop a change, commit, or branch

Branches:
  branch, br        Manage feature branches (create, merge, unmerge)

Inspection:
  status            Show the branch-aware status (default command)
  show, sh          Show commit details (like git show)
  trace             Show the latest command trace

Options:
      --no-color       Disable colored output
      --theme <THEME>  Color theme for graph output [default: auto] [possible values: auto, dark, light]
  -h, --help           Print help (see more with '--help')
  -V, --version        Print version
```

Running `git-loom` with no command is equivalent to `git loom status`.

All commands that accept a target (commit, branch, or file) support [short IDs](status.md) — the compact identifiers shown in the status output. You can also use full git hashes, branch names, or partial hashes.
