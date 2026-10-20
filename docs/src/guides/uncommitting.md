# Uncommitting Changes

To rework a commit, or take a file out of one, fold it into `zz`, your local changes:

```bash
git loom status -f
```

```
│╭─ fa [feature-auth]
│●    hug  92b8427 add password validation
│┊      hug:0 M  src/auth.rs
│●    pop  bae0b72 add login form
│┊      pop:0 A  src/auth.rs
│┊      pop:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

## A Whole Commit

"add password validation" (`hug`) was premature:

```bash
git loom fold hug zz
```

```
╭─ zz [local changes]
│   au  M src/auth.rs
│
│╭─ fa [feature-auth]
│●    pop  bae0b72 add login form
│┊      pop:0 A  src/auth.rs
│┊      pop:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The commit is gone and its changes are back in the working tree, unstaged.

## A Single File

Instead, take only `templates/login.html` (`pop:1`) out of `pop`:

```bash
git loom fold pop:1 zz
```

```
╭─ zz [local changes]
│   lo  ⁕ templates/login.html
│
│╭─ fa [feature-auth]
│●    hug  2a660a7 add password validation
│┊      hug:0 M  src/auth.rs
│●    pop  4afdd0c add login form
│┊      pop:0 A  src/auth.rs
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The commit added the file, so it comes back untracked. For individual hunks, use `git loom fold -p pop zz`.

See also: [fold reference](../commands/fold.md)
