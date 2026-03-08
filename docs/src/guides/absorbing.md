# Auto-absorbing Changes

You've been tweaking code across several files — fixing a typo in `src/auth.rs`, adjusting a layout in `templates/dashboard.html`. Each file was last touched by a different commit. Instead of manually folding each file, let git-loom figure it out:

```bash
$ git loom absorb
```

For each changed file, absorb blames the original lines to find which commit last touched them. If all modified lines trace back to the same commit, the file is automatically folded into it.

Run a **dry run** first to see what would happen:

```bash
$ git loom absorb -n  # or --dry-run
# Would absorb:
#   src/auth.rs → d0 "add login form"
#   templates/dashboard.html → e1 "add dashboard layout"
# Skipped:
#   src/main.rs — modified lines span multiple commits
```

Files that span multiple commits are skipped — you'll need to handle those manually with `fold`.

> [!NOTE]
> Absorb works by blaming existing lines to find their originating commit. It handles modified and deleted lines well, but **newly added lines** (insertions that don't replace existing code) cannot be traced to a commit and will be skipped. Use `fold` for those.

You can also restrict absorption to specific files:

```bash
$ git loom absorb src/auth.rs
```

See also: [absorb reference](../commands/absorb.md)
