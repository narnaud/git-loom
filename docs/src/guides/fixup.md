# Fixing Up a Commit

"add password validation" (`hug`) is really part of "add login form" (`pop`):

```
│╭─ fa [feature-auth]
│●    hug  2b48f49 add password validation
│●    pop  bf7e5af add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Fold the newer commit into the older one:

```bash
git loom fold hug pop
```

```
│╭─ fa [feature-auth]
│●    pop  9dff7bd add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

`hug` is gone and its changes are in `pop`, which keeps its message. The source must be newer than the target. To fix up only some of `hug`'s hunks, use `git loom fold -p hug pop`.

See also: [fold reference](../commands/fold.md)
