# Splitting a Commit

A commit touches multiple files that should really be separate commits. Let's look at the current state:

```bash
$ git loom status -f d0
```

```
│╭─ fa [feature-auth]
│●    d0 add login form
│┊      d0:0 A  src/auth.rs
│┊      d0:1 A  src/validation.rs
│┊      d0:2 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

You want `src/validation.rs` in its own commit. Split the commit:

```bash
$ git loom split d0 -m "add validation helpers"
# ? Select files for the first commit
# > [x] src/validation.rs
#   [ ] src/auth.rs
#   [ ] templates/login.html
```

Select the files for the **first** commit — the remaining files stay in the **second** commit, which keeps the original message. The result:

```
│╭─ fa [feature-auth]
│●   d1 add login form
│●   d0 add validation helpers
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The commit must have at least two files — otherwise there's nothing to split. Both sides must get at least one file.

> [!TIP]
> If you omit `-m`, git-loom opens your editor for the first commit's message.

See also: [split reference](../commands/split.md)
