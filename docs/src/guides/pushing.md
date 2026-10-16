# Pushing for Review

`feature-auth` is ready for review. Push it:

```bash
git loom push fa
# ✓ Pushed `feature-auth` to `origin`
# ✓ PR created: https://github.com/owner/repo/pull/42
```

Loom detects the forge and opens the review there: a pull request on GitHub (with [`gh`](https://cli.github.com/)) or Azure DevOps (with [`az`](https://learn.microsoft.com/en-us/cli/azure/)), a merge request on GitLab, a review on Gerrit. Push again after changing the branch and the same review is updated. Without `gh` or `az`, loom prints a link to open the pull request yourself.

On GitHub and Azure DevOps, a branch with one commit takes its title from that commit. For a branch with several, loom asks for a title, or pass it with `--title`.

If `feature-auth` is stacked on another branch, the branch below is pushed too, and each gets its own review targeting the one below it. A stack needs `gh` on GitHub, and Azure DevOps has no stacked pull requests: both refuse unless you pass `--no-pr`.

To push without opening a review, for a backup or work in progress:

```bash
git loom push fa --no-pr
```

Without a branch argument, loom asks which one to push.

See also: [push reference](../commands/push.md)
