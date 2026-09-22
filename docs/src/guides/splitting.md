# Splitting a Commit

A commit touches multiple files that should really be separate commits. Let's look at the current state:

```bash
git loom status -f mqt
```

```
│╭─ fa [feature-auth]
│●    mqt  291658f add login form
│┊      mqt:0 A  src/auth.rs
│┊      mqt:1 A  src/validation.rs
│┊      mqt:2 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

You want `src/validation.rs` in its own commit. Split the commit:

```bash
$ git loom split mqt -m "add validation helpers"
# ? Select files for the first commit
# > [x] src/validation.rs
#   [ ] src/auth.rs
#   [ ] templates/login.html
```

Select the files for the **first** commit — the remaining files stay in the **second** commit, which keeps the original message. The result:

```
│╭─ fa [feature-auth]
│●    mqt  5e1a9c2 add login form
│●    wsl  8c2b2fa add validation helpers
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The commit must have at least two files — otherwise there's nothing to split. Both sides must get at least one file.

> [!TIP]
> If you omit `-m`, *git-loom* opens your editor for the first commit's message.

See also: [split reference](../commands/split.md)
