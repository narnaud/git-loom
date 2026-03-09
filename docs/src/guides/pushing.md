# Pushing for Review

Your `feature-auth` branch is ready. Push it to the remote:

```bash
$ git loom push fa
```

git-loom detects your remote type automatically and runs the appropriate commands:

- **GitHub** — pushes the branch, then checks if a PR exists. If a PR already exists, prints its URL (`PR updated: https://...`). Otherwise opens the PR creation page in your browser via the [`gh` CLI](https://cli.github.com/).
- **Azure DevOps** — pushes the branch, then checks if a PR exists. If a PR already exists, prints its URL. Otherwise opens the PR creation page via the [`az` CLI](https://learn.microsoft.com/en-us/cli/azure/).
- **Gerrit** — pushes to `refs/for/<target>` (where `<target>` is your upstream branch, e.g. `main` or `master`) with the branch name as topic. Review URLs from the Gerrit remote are displayed after the push.
- **Plain Git** — pushes with `--force-with-lease`.

If `gh` or `az` are not installed, the push still succeeds — you just won't get the automatic PR creation.

If you just want to push without creating a PR (e.g. to back up your work):

```bash
$ git loom push fa --no-pr
```

When you omit the branch argument, git-loom shows an interactive picker:

```bash
$ git loom push
# ? Select branch to push
# > feature-auth
#   feature-dashboard
```

See also: [push reference](../commands/push.md)
