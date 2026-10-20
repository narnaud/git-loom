# Moving Files Between Commits

A commit can hold a file that belongs in another commit. `git loom status -f` gives each file of each commit an ID such as `pop:1`:

```bash
git loom status -f
```

```
│╭─ fd [feature-dashboard]
│●    dog  147aa31 add dashboard layout
│┊      dog:0 A  src/dashboard.rs
│┊      dog:1 A  templates/dashboard.html
├╯
│
│╭─ fa [feature-auth]
│●    pop  c32bc09 add login form
│┊      pop:0 M  src/auth.rs
│┊      pop:1 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

`templates/login.html` (`pop:1`) belongs with the dashboard. Move it:

```bash
git loom fold pop:1 dog
```

```
│╭─ fd [feature-dashboard]
│●    dog  a3b2ef8 add dashboard layout
│┊      dog:0 A  src/dashboard.rs
│┊      dog:1 A  templates/dashboard.html
│┊      dog:2 A  templates/login.html
├╯
│
│╭─ fa [feature-auth]
│●    pop  cb15064 add login form
│┊      pop:0 M  src/auth.rs
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Pass commit IDs to `-f` (`git loom status -f pop dog`) to list only their files.

See also: [fold reference](../commands/fold.md)
