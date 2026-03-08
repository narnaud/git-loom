# Moving a Commit Between Branches

You committed a logging helper to `feature-auth` by mistake — it belongs in `feature-dashboard`.

```
│╭─ fd [feature-dashboard]
│●   e1 add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●   a3 add logging helper
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Move it with fold:

```bash
$ git loom fold a3 fd
```

Commit `a3` is removed from `feature-auth` and appended to `feature-dashboard`:

```
│╭─ fd [feature-dashboard]
│●   a3 add logging helper
│●   e1 add dashboard layout
├╯
│
│╭─ fa [feature-auth]
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

You can also move a commit into a **new** branch in one step with `--create`:

```bash
$ git loom fold -c a3 feature-logging
```

See also: [fold reference](../commands/fold.md)
