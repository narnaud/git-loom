# Uncommitting Changes

Sometimes you want to undo a commit or pull a file out of one — maybe to re-split changes differently. Here's the starting point:

```bash
$ git loom status -f
```

```
│╭─ fa [feature-auth]
│●    pkz  add password validation 92b8427
│┊      pkz:0 M  src/auth.rs
│●    mqt  add login form bae0b72
│┊      mqt:0 A  src/auth.rs
│┊      mqt:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

## Uncommitting a Commit

You decide `pkz` ("add password validation") was premature — you want its changes back in the working tree. Fold it into `zz` (the working directory):

```bash
$ git loom fold pkz zz
```

The commit is removed from history and its changes appear as unstaged modifications:

```
╭─ zz [local changes]
│    M src/auth.rs
│
│╭─ fa [feature-auth]
│●    mqt  add login form bae0b72
│┊      mqt:0 A  src/auth.rs
│┊      mqt:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

## Uncommitting a File

Instead of removing the whole commit, you just want to extract `templates/login.html` (index `mqt:1`) from `mqt`:

```bash
$ git loom fold mqt:1 zz
```

The file is removed from the commit and appears as an untracked file in the working directory, leaving the rest of `mqt` intact:

```
╭─ zz [local changes]
│    ⁕ templates/login.html
│
│╭─ fa [feature-auth]
│●    pkz  add password validation 2a660a7
│┊      pkz:0 M  src/auth.rs
│●    mqt  add login form 4afdd0c
│┊      mqt:0 A  src/auth.rs
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

See also: [fold reference](../commands/fold.md)
