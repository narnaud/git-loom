# Fixing Up a Commit

"add password validation" (`pkz`) is really part of "add login form" (`mqt`):

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
git loom fold pkz mqt
```

```
│╭─ fa [feature-auth]
│●    mqt  9dff7bd add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

`pkz` is gone and its changes are in `mqt`, which keeps its message. The source must be newer than the target. To fix up only some of `pkz`'s hunks, use `git loom fold -p pkz mqt`.

See also: [fold reference](../commands/fold.md)
