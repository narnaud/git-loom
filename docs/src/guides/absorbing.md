# Auto-absorbing Changes

You've made small fixes across several files: a typo in `src/auth.rs`, a layout tweak in `templates/dashboard.html`. Each belongs in a different commit. Instead of folding them one by one, let loom work out where each goes:

```bash
git loom absorb -n
#   src/auth.rs -> d0472f9 "add login form" (feature-auth)
#   templates/dashboard.html -> 2ee61e1 "add dashboard layout" (feature-dashboard)
#   src/shared.rs [hunk 1/2] -> d0472f9 "add login form" (feature-auth)
#   src/shared.rs [hunk 2/2] -- skipped (pure addition)
# Dry run: would absorb 3 hunk(s) from 3 file(s) into 2 commit(s)
```

`-n` shows the plan without changing anything. Each hunk goes to the commit that last changed the lines it touches, so one file can feed several commits. When the plan looks right, run it:

```bash
git loom absorb
```

Hunks without a clear target stay in the working tree. Newly added lines, in particular, have no history to trace: [fold](amending.md) them yourself. To absorb only some files, name them: `git loom absorb src/auth.rs`.

See also: [absorb reference](../commands/absorb.md)
