# Amending a Past Commit

You realize the login form is missing a CSRF token. You fix `src/auth.rs` and check the status:

```
╭─ zz [local changes]
│    M src/auth.rs
│
│╭─ fa [feature-auth]
│●   c2 add password validation
│●   d0 add login form
├╯
│
● a1b2c3d (upstream) [origin/main] Latest upstream commit
```

You want to amend this change into the original "add login form" commit (`d0`), not create a new commit.

```bash
$ git loom fold src/auth.rs d0
```

This stages `src/auth.rs` and amends it into commit `d0`. The branch topology stays the same — the commit just gains the new changes.

If you've already staged the files you want to amend, you can use the single-argument form:

```bash
$ git add src/auth.rs
$ git loom fold d0
```

This folds only the staged changes — any unstaged modifications to the same files are preserved.

To amend **all** working tree changes into a commit at once:

```bash
$ git loom fold zz d0
```

> [!TIP]
> Use `git loom status -f d0` to see which files are in a commit before and after amending.

See also: [fold reference](../commands/fold.md)
