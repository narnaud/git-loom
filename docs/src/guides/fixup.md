# Fixing Up a Commit

You have two commits on `feature-auth` and realize that `pkz` ("add password validation") should really be part of `mqt` ("add login form") — they're logically the same change.

```
│╭─ fa [feature-auth]
│●    pkz  2b48f49 add password validation
│●    mqt  bf7e5af add login form
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
│●    mqt  9dff7bd add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The source commit must be newer than the target. The target keeps its message.

See also: [fold reference](../commands/fold.md)
