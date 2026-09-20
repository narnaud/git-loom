# Moving a Commit Between Branches

You committed a logging helper to `feature-auth` by mistake — it belongs in `feature-dashboard`.

```
│╭─ fd [feature-dashboard]
│●    rsv  add dashboard layout 24a86e6
├╯
│
│╭─ fa [feature-auth]
│●    tqn  add logging helper 6395f01
│●    mqt  add login form 64518a4
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
│●    tqn  add logging helper db04256
│●    rsv  add dashboard layout 24a86e6
├╯
│
│╭─ fa [feature-auth]
│●    mqt  add login form 64518a4
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

You can also move a commit into a **new** branch in one step with `--create`:

```bash
$ git loom fold -c tqn feature-logging
```

See also: [fold reference](../commands/fold.md)
