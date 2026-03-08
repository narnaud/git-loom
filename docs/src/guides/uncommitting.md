# Uncommitting Changes

Sometimes you want to undo a commit or pull a file out of one — maybe to re-split changes differently. Here's the starting point:

```bash
$ git loom status -f
```

```
│╭─ fa [feature-auth]
│●    c2 add password validation
│┊      c2:0 M  src/auth.rs
│●    d0 add login form
│┊      d0:0 A  src/auth.rs
│┊      d0:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

## Uncommitting a Commit

You decide `c2` ("add password validation") was premature — you want its changes back in the working tree. Fold it into `zz` (the working directory):

```bash
$ git loom fold c2 zz
```

The commit is removed from history and its changes appear as unstaged modifications:

```
╭─ zz [local changes]
│    M src/auth.rs
│
│╭─ fa [feature-auth]
│●    d0 add login form
│┊      d0:0 A  src/auth.rs
│┊      d0:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

## Uncommitting a File

Instead of removing the whole commit, you just want to extract `templates/login.html` (index `d0:1`) from `d0`:

```bash
$ git loom fold d0:1 zz
```

The file is removed from the commit and appears as an untracked file in the working directory, leaving the rest of `d0` intact:

```
╭─ zz [local changes]
│    ⁕ templates/login.html
│
│╭─ fa [feature-auth]
│●    c2 add password validation
│┊      c2:0 M  src/auth.rs
│●    d0 add login form
│┊      d0:0 A  src/auth.rs
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

See also: [fold reference](../commands/fold.md)
