# Moving a Commit Between Branches

You committed a logging helper to `feature-auth` by mistake — it belongs in `feature-dashboard`.

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

Move it with fold:

```bash
$ git loom fold tqn fd
```

Commit `tqn` is removed from `feature-auth` and appended to `feature-dashboard`:

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

You can also move a commit into a **new** branch in one step with `--create`:

```bash
$ git loom fold -c tqn feature-logging
```

See also: [fold reference](../commands/fold.md)
