# Splitting a Commit

"add login form" (`pop`) also brought in validation helpers that deserve their own commit:

```bash
git loom status -f pop
```

```
│╭─ fa [feature-auth]
│●    pop  291658f add login form
│┊      pop:0 A  src/auth.rs
│┊      pop:1 A  src/validation.rs
│┊      pop:2 A  templates/login.html
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

Split it, picking the files for the first commit:

```bash
git loom split pop -m "add validation helpers"
# ? Select files for the first commit
# > [x] src/validation.rs
#   [ ] src/auth.rs
#   [ ] templates/login.html
```

```
│╭─ fa [feature-auth]
│●    pop  5e1a9c2 add login form
│●    cup  8c2b2fa add validation helpers
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The files you picked form the first commit, with the new message. The rest stay in the second, which keeps the original message and its short ID `pop`.

To skip the picker, name the files: `git loom split pop -m "add validation helpers" src/validation.rs`. To split within a file, or a commit that touches a single file, pick hunks with `git loom split -p pop`. Without `-m`, your editor opens for the first commit's message.

See also: [split reference](../commands/split.md)
