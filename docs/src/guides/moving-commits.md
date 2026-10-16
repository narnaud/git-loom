# Moving a Commit Between Branches

You committed a logging helper to `feature-auth` by mistake. It belongs in `feature-dashboard`:

```
│╭─ fd [feature-dashboard]
│●    rsv  24a86e6 add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●    tqn  6395f01 add logging helper
│●    mqt  64518a4 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Fold the commit into the branch:

```bash
git loom fold tqn fd
```

```
│╭─ fd [feature-dashboard]
│●    tqn  db04256 add logging helper
│●    rsv  24a86e6 add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●    mqt  64518a4 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The commit lands on top of the branch. Some variations:

```bash
git loom fold tqn mqt fd            # move several commits at once
git loom fold tqn --below rsv       # land right under rsv instead of on top
git loom fold -c tqn feature-logging   # move it into a new branch
```

See also: [fold reference](../commands/fold.md)
