# Moving a Commit Between Branches

You committed a logging helper to `feature-auth` by mistake. It belongs in `feature-dashboard`:

```
│╭─ fd [feature-dashboard]
│●    dog  24a86e6 add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●    sun  6395f01 add logging helper
│●    pop  64518a4 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Fold the commit into the branch:

```bash
git loom fold sun fd
```

```
│╭─ fd [feature-dashboard]
│●    sun  db04256 add logging helper
│●    dog  24a86e6 add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●    pop  64518a4 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The commit lands on top of the branch. Some variations:

```bash
git loom fold sun pop fd            # move several commits at once
git loom fold sun --below dog       # land right under dog instead of on top
git loom fold -c sun feature-logging   # move it into a new branch
```

See also: [fold reference](../commands/fold.md)
