# Moving Files Between Commits

Sometimes a commit touches files that belong in different commits. Use the `commit:index` syntax shown by `git loom status -f` to move a single file.

First, check which files are in each commit:

```bash
$ git loom status -f
```

> [!NOTE]
> `-f` without arguments shows files for all commits. You can pass specific short IDs (e.g. `git loom status -f mqt`) to limit the output.

```
│╭─ fd [feature-dashboard]
│●    rsv  147aa31 add dashboard layout
│┊      rsv:0 A  src/dashboard.rs
│┊      rsv:1 A  templates/dashboard.html
├╯
│
│╭─ fa [feature-auth]
│●    mqt  c32bc09 add login form
│┊      mqt:0 M  src/auth.rs
│┊      mqt:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

You realize `templates/login.html` (index `mqt:1`) would be better off in the dashboard commit. Move it:

```bash
$ git loom fold mqt:1 rsv
```

The file's changes are removed from `mqt` and applied to `rsv`:

```
│╭─ fd [feature-dashboard]
│●    rsv  a3b2ef8 add dashboard layout
│┊      rsv:0 A  src/dashboard.rs
│┊      rsv:1 A  templates/dashboard.html
│┊      rsv:2 A  templates/login.html
├╯
│
│╭─ fa [feature-auth]
│●    mqt  cb15064 add login form
│┊      mqt:0 M  src/auth.rs
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

See also: [fold reference](../commands/fold.md)
