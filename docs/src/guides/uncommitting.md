# Uncommitting Changes

To rework a commit, or take a file out of one, fold it into `zz`, your local changes:

```bash
git loom status -f
```

```
│╭─ fa [feature-auth]
│●    pkz  92b8427 add password validation
│┊      pkz:0 M  src/auth.rs
│●    mqt  bae0b72 add login form
│┊      mqt:0 A  src/auth.rs
│┊      mqt:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

## A Whole Commit

"add password validation" (`pkz`) was premature:

```bash
git loom fold pkz zz
```

```
╭─ zz [local changes]
│   au  M src/auth.rs
│
│╭─ fa [feature-auth]
│●    mqt  bae0b72 add login form
│┊      mqt:0 A  src/auth.rs
│┊      mqt:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The commit is gone and its changes are back in the working tree, unstaged.

## A Single File

Instead, take only `templates/login.html` (`mqt:1`) out of `mqt`:

```bash
git loom fold mqt:1 zz
```

```
╭─ zz [local changes]
│   lo  ⁕ templates/login.html
│
│╭─ fa [feature-auth]
│●    pkz  2a660a7 add password validation
│┊      pkz:0 M  src/auth.rs
│●    mqt  4afdd0c add login form
│┊      mqt:0 A  src/auth.rs
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The commit added the file, so it comes back untracked. For individual hunks, use `git loom fold -p mqt zz`.

See also: [fold reference](../commands/fold.md)
