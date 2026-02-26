# Commands Overview

```
Usage: git-loom [OPTIONS] [COMMAND]

Commands:
  status       Show the branch-aware status (default)
  init         Initialize a new integration branch tracking a remote
  branch       Create a new feature branch
  commit       Create a commit on a feature branch without leaving integration
  reword       Reword a commit message or rename a branch
  fold         Fold source(s) into a target (amend files, fixup commits, move commits)
  drop         Drop a commit or a branch from history
  update       Pull-rebase the integration branch and update submodules
  push         Push a feature branch to remote
  completions  Generate shell completions (powershell, clink)

Options:
      --no-color  Disable colored output
  -h, --help      Print help
```

Running `git-loom` with no command is equivalent to `git loom status`.

All commands that accept a target (commit, branch, or file) support [short IDs](status.md) — the compact identifiers shown in the status output. You can also use full git hashes, branch names, or partial hashes.
