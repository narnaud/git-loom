# Tutorial

This tutorial walks you through a typical *git-loom* workflow — from initializing an integration branch to working on multiple features simultaneously.

## Getting Started

You have a project tracked by Git with a remote `origin/main`. Let's set up *git-loom*.

```bash
git checkout main
git loom init
```

*git-loom* creates an integration branch that tracks `origin/main`. Run `git loom status` (or just `git loom`) to see the baseline:

```
╭─ zz [local changes]
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

A clean slate — the `zz` local changes section is empty, and you can see the upstream marker. Time to start working.

## Your First Commit

You add a login form to your project — create `src/auth.rs` and `templates/login.html`. Check the status:

```
╭─ zz [local changes]
│    ⁕ src/auth.rs
│    ⁕ templates/login.html
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Your new files show up as untracked in the local changes section. Instead of the usual `git add` / `git commit` dance, you use *git-loom* to commit directly to a feature branch:

```bash
git loom commit -b feature-auth -m "add login form" zz
```

This single command:

1. Stages all your changes (`zz` means "everything")
2. Creates the `feature-auth` branch (it didn't exist yet)
3. Weaves it into the integration topology
4. Creates the commit on that branch

Check the status:

```
│╭─ fa [feature-auth]
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Your commit sits on `feature-auth`, shown as a side branch off the integration line. The short IDs `fa` (branch) and `d0` (commit) are what you'll type in subsequent commands.

You keep working on the same feature — add password validation and commit again:

```bash
git loom commit -b fa -m "add password validation" zz
```

Notice you can use the short ID `fa` instead of the full branch name. The status now shows:

```
│╭─ fa [feature-auth]
│●   c2 add password validation
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

## Working on Multiple Features

While `feature-auth` is in progress, you want to start on a dashboard. You create `src/dashboard.rs` and `templates/dashboard.html`:

```
╭─ zz [local changes]
│    ⁕ src/dashboard.rs
│    ⁕ templates/dashboard.html
│
│╭─ fa [feature-auth]
│●   c2 add password validation
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

No need to switch branches — just commit to a new one:

```bash
git loom commit -b feature-dashboard -m "add dashboard layout" zz
```

```
│╭─ fd [feature-dashboard]
│●   e1 add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●   c2 add password validation
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Two independent feature branches, both woven into the integration branch. You can build and test everything together while keeping the branches separate. This is the core of *git-loom*: **work on multiple features simultaneously without branch switching.**

## Staying Up to Date

Your teammates have been pushing to `origin/main`. Time to pull their changes and rebase your work on top:

```bash
git loom update
```

This fetches upstream changes, rebases your integration branch (including all woven feature branches) on top, and updates submodules if any. Your working tree changes are automatically preserved.

```
│╭─ fd [feature-dashboard]
│●   e1 add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●   c2 add password validation
│●   d0 add login form
├╯
│
● b2c3d4e (upstream) [origin/main] Teammate's latest commit
· a1b2c3d 2026-03-07 Latest upstream commit
```

The upstream marker moved forward — your branches are now rebased on top of the latest changes.

If any of your pushed feature branches have been merged and deleted on the remote, `update` will notice and offer to clean up the local branches:

```
# ! 1 local branch with a gone upstream:
#   · feature-auth
# ? Remove them? [y/N]
```

> [!NOTE]
> If a rebase conflict occurs, `update` aborts automatically and tells you the full `git rebase` command to re-run and resolve conflicts manually.

See also: [update reference](../commands/update.md)

Now that you know the basics, check out the recipe guides for common operations:

- [Amending a Past Commit](amending.md)
- [Fixing Up a Commit](fixup.md)
- [Splitting a Commit](splitting.md)
- [Moving a Commit Between Branches](moving-commits.md)
- [Moving Files Between Commits](moving-files.md)
- [Uncommitting Changes](uncommitting.md)
- [Auto-absorbing Changes](absorbing.md)
- [Pushing for Review](pushing.md)
