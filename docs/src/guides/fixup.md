# Fixing Up a Commit

You have two commits on `feature-auth` and realize that `c2` ("add password validation") should really be part of `d0` ("add login form") — they're logically the same change.

```
│╭─ fa [feature-auth]
│●   c2 add password validation
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Fold the newer commit into the older one:

```bash
$ git loom fold c2 d0
```

Commit `c2` disappears from history and its changes are absorbed into `d0`:

```
│╭─ fa [feature-auth]
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The source commit must be newer than the target. The target keeps its message.

See also: [fold reference](../commands/fold.md)
