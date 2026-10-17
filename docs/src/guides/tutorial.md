# Tutorial

This tutorial walks through a typical *git-loom* session: set up an integration branch, work on two features at once, and stay up to date with upstream.

## Getting Started

Your project has a remote `origin/main`. Create an integration branch from it:

```bash
git checkout main
git loom init
```

A fresh clone works the same. If it is a GitHub fork, `init` offers to track the original repository as `upstream` (see [init](../commands/init.md)).

Run `git loom status` (or just `git loom`) to see where you are:

```
╭─ zz [local changes]
│   no changes
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Nothing yet: no local changes, and the upstream commit your work will build on.

## Your First Commit

You add a login form: `src/auth.rs` and `templates/login.html`. They show up as local changes:

```
╭─ zz [local changes]
│   au  ⁕ src/auth.rs
│   lo  ⁕ templates/login.html
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Commit them straight to a feature branch:

```bash
git loom commit -b feature-auth -m "add login form" zz
```

`zz` means "all local changes". The `feature-auth` branch doesn't exist yet, so loom creates it and weaves it into the integration branch:

```
╭─ zz [local changes]
│   no changes
│
│╭─ fa [feature-auth]
│●    mqt  711d082 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

`fa` and `mqt` are short IDs for the branch and the commit; you type them in later commands. Add password validation and commit again, this time naming the branch by its short ID:

```bash
git loom commit -b fa -m "add password validation" zz
```

```
╭─ zz [local changes]
│   no changes
│
│╭─ fa [feature-auth]
│●    pkz  4f825d2 add password validation
│●    mqt  711d082 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

## Working on Multiple Features

Now you start a dashboard, `src/dashboard.rs` and `templates/dashboard.html`. No need to switch branches — commit to a new one:

```bash
git loom commit -b feature-dashboard -m "add dashboard layout" zz
```

```
╭─ zz [local changes]
│   no changes
│
│╭─ fd [feature-dashboard]
│●    rsv  7a5bb0b add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●    pkz  4f825d2 add password validation
│●    mqt  711d082 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Both branches are in your working tree, so you build and test them together, while each stays independent for review. That is the core of *git-loom*.

## Staying Up to Date

Your teammates pushed to `origin/main`. Fetch their work and rebase yours on top:

```bash
git loom update
```

```
╭─ zz [local changes]
│   no changes
│
│╭─ fd [feature-dashboard]
│●    rsv  8f74d1c add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●    pkz  660e970 add password validation
│●    mqt  885cc0c add login form
├╯
│
● b2c3d4e (upstream) [origin/main] Teammate's latest commit
```

Every branch moved onto the new upstream. The hashes changed, the short IDs did not. Your uncommitted changes are kept.

Once a branch you pushed is merged and deleted on the remote, `update` offers to remove your local copy:

```
#   › `feature-auth`: upstream gone
# ? Remove local branch? [y/N]
```

If the rebase hits a conflict, `update` pauses so you can resolve it — see [Resolving Conflicts](conflict-resolution.md).

## Next Steps

- Reshape your work: [amend](amending.md), [fix up](fixup.md), [split](splitting.md), [move commits](moving-commits.md) or [files](moving-files.md), [uncommit](uncommitting.md), [absorb](absorbing.md)
- Work hunk by hunk with [`-p`](hunk-selection.md)
- [Push for review](pushing.md)
- Let an AI agent do it: [Working with AI Agents](agents.md)
