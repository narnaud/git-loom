# Moving Files Between Commits

A commit can hold a file that belongs in another commit. `git loom status -f` gives each file of each commit an ID such as `mqt:1`:

```bash
git loom status -f
```

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

`templates/login.html` (`mqt:1`) belongs with the dashboard. Move it:

```bash
git loom fold mqt:1 rsv
```

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

Pass commit IDs to `-f` (`git loom status -f mqt rsv`) to list only their files.

See also: [fold reference](../commands/fold.md)
