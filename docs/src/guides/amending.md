# Amending a Past Commit

The login form is missing a CSRF token. You fix `src/auth.rs`:

```
╭─ zz [local changes]
│   au  M src/auth.rs
│
│╭─ fa [feature-auth]
│●    hug  81356bf add password validation
│●    pop  a337eda add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

The fix belongs in "add login form" (`pop`), not in a new commit. Fold it in:

```bash
git loom fold src/auth.rs pop
```

`pop` now includes the fix; `hug` is replayed on top of it. Other ways to say what to fold:

```bash
git loom fold au pop                     # the file by short ID
git loom fold zz pop                     # all local changes
git add src/auth.rs && git loom fold pop # only what is staged
```

To fold only some of a file's changes, use [`fold -p`](hunk-selection.md). To let loom find the right commit for each change, use [`absorb`](absorbing.md).

> [!TIP]
> `git loom status -f pop` lists the files in a commit.

See also: [fold reference](../commands/fold.md)
