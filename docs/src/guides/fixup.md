# Fixing Up a Commit

You have two commits on `feature-auth` and realize that `pkz` ("add password validation") should really be part of `mqt` ("add login form") — they're logically the same change.

```
│╭─ fa [feature-auth]
│●    pkz  add password validation 2b48f49
│●    mqt  add login form bf7e5af
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Fold the newer commit into the older one:

```bash
$ git loom fold pkz mqt
```

Commit `pkz` disappears from history and its changes are absorbed into `mqt`:

```
│╭─ fa [feature-auth]
│●    mqt  add login form 9dff7bd
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The source commit must be newer than the target. The target keeps its message.

See also: [fold reference](../commands/fold.md)
