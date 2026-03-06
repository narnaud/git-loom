# Commands Overview

```
Usage: git-loom.exe [OPTIONS] [CONTEXT] [COMMAND]

Commands:
  init    Initialize a new integration branch tracking a remote
  status  Show the branch-aware status
  commit  Create a commit on a feature branch without leaving integration
  fold    Fold source(s) into a target (amend files, fixup commits, move commits)
  reword  Reword a commit message or rename a branch
  drop    Drop a commit or a branch from history
  show    Show the diff and metadata for a commit (like git show)
  split   Split a commit into two sequential commits
  absorb  Absorb working tree changes into the commits that introduced them
  branch  Create a new feature branch, or a stacked branch
  push    Push a feature branch to remote
  update  Pull-rebase the integration branch and update submodules
  trace   Show the latest command trace
  help    Print this message or the help of the given subcommand(s)

Arguments:
  [CONTEXT]  Number of context commits to show before the base [default: 1]

Options:
      --no-color  Disable colored output
  -f, --files     Show files changed in each commit
  -h, --help      Print help
  -V, --version   Print version
```

Running `git-loom` with no command is equivalent to `git loom status`.

All commands that accept a target (commit, branch, or file) support [short IDs](status.md) — the compact identifiers shown in the status output. You can also use full git hashes, branch names, or partial hashes.
