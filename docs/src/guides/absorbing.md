# Auto-absorbing Changes

You've been tweaking code across several files — fixing a typo in `src/auth.rs`, adjusting a layout in `templates/dashboard.html`. Each change was last touched by a different commit. Instead of manually folding each one, let git-loom figure it out:

```bash
$ git loom absorb
```

For each changed file, absorb splits the diff into hunks, blames each hunk's original lines to find the originating commit, and folds each hunk into the right place — even when a single file has changes belonging to different commits.

Run a **dry run** first to see what would happen:

```bash
$ git loom absorb -n  # or --dry-run
#   src/auth.rs -> d0 "add login form"
#   templates/dashboard.html -> e1 "add dashboard layout"
#   src/shared.rs [hunk 1/2] -> d0 "add login form"
#   src/shared.rs [hunk 2/2] -- skipped (pure addition)
# Dry run: would absorb 3 hunk(s) from 3 file(s) into 2 commit(s)
```

When a file has hunks going to different commits, each hunk is absorbed independently. Hunks that can't be attributed are left in the working tree.

> [!NOTE]
> Absorb works by blaming existing lines to find their originating commit. It handles modified and deleted lines well, but **newly added lines** (insertions that don't replace existing code) cannot be traced to a commit and will be skipped. Use `fold` for those.

You can also restrict absorption to specific files:

```bash
$ git loom absorb src/auth.rs
```

See also: [absorb reference](../commands/absorb.md)
