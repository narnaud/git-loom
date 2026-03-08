# Moving Files Between Commits

Sometimes a commit touches files that belong in different commits. Use the `commit:index` syntax shown by `git loom status -f` to move a single file.

First, check which files are in each commit:

```bash
$ git loom status -f
```

> [!NOTE]
> `-f` without arguments shows files for all commits. You can pass specific short IDs (e.g. `git loom status -f d0`) to limit the output.

```
│╭─ fd [feature-dashboard]
│●    e1 add dashboard layout
│┊      e1:0 A  src/dashboard.rs
│┊      e1:1 A  templates/dashboard.html
├╯
│
│╭─ fa [feature-auth]
│●    d0 add login form
│┊      d0:0 M  src/auth.rs
│┊      d0:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

You realize `templates/login.html` (index `d0:1`) would be better off in the dashboard commit. Move it:

```bash
$ git loom fold d0:1 e1
```

The file's changes are removed from `d0` and applied to `e1`:

```
│╭─ fd [feature-dashboard]
│●    e1 add dashboard layout
│┊      e1:0 A  src/dashboard.rs
│┊      e1:1 A  templates/dashboard.html
│┊      e1:2 A  templates/login.html
├╯
│
│╭─ fa [feature-auth]
│●    d0 add login form
│┊      d0:0 M  src/auth.rs
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

See also: [fold reference](../commands/fold.md)
