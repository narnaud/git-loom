# Amending a Past Commit

You realize the login form is missing a CSRF token. You fix `src/auth.rs` and check the status:

```
╭─ zz [local changes]
│    M src/auth.rs
│
│╭─ fa [feature-auth]
│●    pkz  81356bf add password validation
│●    mqt  a337eda add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

You want to amend this change into the original "add login form" commit (`mqt`), not create a new commit.

```bash
$ git loom fold src/auth.rs mqt
```

This stages `src/auth.rs` and amends it into commit `mqt`. The branch topology stays the same — the commit just gains the new changes.

If you've already staged the files you want to amend, you can use the single-argument form:

```bash
$ git add src/auth.rs
$ git loom fold mqt
```

This folds only the staged changes — any unstaged modifications to the same files are preserved.

To amend **all** working tree changes into a commit at once:

```bash
$ git loom fold zz mqt
```

> [!TIP]
> Use `git loom status -f mqt` to see which files are in a commit before and after amending.

See also: [fold reference](../commands/fold.md)
