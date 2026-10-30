# push

Push a feature branch, with the branches it is stacked on, and open or update its pull request.

**Alias:** `pr`

## Usage

```
git loom push [<branch>] [--no-pr] [-f] [--title <title>] [--draft]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<branch>` | Branch name or short ID. A picker opens if omitted. |

### Options

| Option | Description |
|--------|-------------|
| `--no-pr` | Push only: no pull request or Gerrit review |
| `-f, --force` | Force the push, even if the branch changed on the remote |
| `--title <title>` | Title of the branch's new pull request (see [PR Title and Description](#pr-title-and-description)) |
| `--draft` (alias `--wip`) | Open the branch's pull request as a draft, or its Gerrit review as work in progress (see [Draft Pull Requests](#draft-pull-requests)) |

## What It Does

Pushes the branch safely: the push is refused if the remote branch has commits you haven't seen. Then, depending on the forge:

| Forge | Detected by | Pull request | Stacked branch | Needs |
|-------|-------------|--------------|----------------|-------|
| GitHub | `github.com` in the remote URL | Created or updated | One PR per branch, linked into a GitHub stack | [`gh`](https://cli.github.com) |
| GitLab | `gitlab` in the remote URL | Merge request created or updated | One MR per branch | — |
| Azure DevOps | `dev.azure.com` in the remote URL | Created or updated | Refused (use `--no-pr`) | [`az`](https://learn.microsoft.com/en-us/cli/azure/) |
| Gerrit | An `ssh://` URL on port 29418, or Gerrit's `commit-msg` hook | Review pushed to `refs/for/<upstream>` | Gerrit chains the reviews | — |
| Plain Git | Your choice | — | Branches pushed | — |

When the forge isn't detected (a GitLab server whose URL lacks `gitlab`, for instance), loom asks once and saves your answer in [`loom.remoteType`](../configuration.md#loomremotetype). Forgejo, Gitea and Bitbucket work as plain Git; the link their server prints after a push is shown.

```bash
git loom push feature-a
# ✓ Pushed `feature-a` to `origin`
# ✓ PR created: https://github.com/owner/repo/pull/42
```

Without `gh` or `az`, the branch is still pushed, and loom prints a link to create the pull request in the browser. A stacked branch on GitHub needs `gh`, and `--draft` needs `gh` or `az`, which is what marks the pull request.

## Stacked Branches

A branch built on top of another one is *stacked* on it — what `git loom status` draws with `├─`. Pushing it pushes what its review depends on. With `d` on `c` on `b` on `a`:

```bash
git loom push b
# ✓ Pushed `a`, `b` to `origin`
#   Re-pushed above `b`: `c`
# ! Not pushed above `b`: `d`
#   Run `loom push d` to publish them
```

- `a` and `b`, the branch and everything below it, are pushed, each with its own pull request targeting the branch below. The bottom one targets your upstream branch.
- `c` was pushed before and has changed since, so it is pushed again to keep the remote consistent.
- `d` was never pushed, so it is left for you to push.

On GitHub the pull requests are linked into a [stack](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs), and a PR whose base is wrong is retargeted. In a fork, every PR targets the upstream branch instead, since GitHub can't stack across forks. When the bottom PR merges, `git loom update` drops its commits and the stack shrinks.

To build a stack, start a branch on another with [`branch b --on a`](branch.md), or split a branch in two with `branch part-1 --on <commit>`.

[Hidden branches](../configuration.md#loomhidebranchpattern) are never pushed, nor is a branch stacked on one.

## PR Title and Description

A new pull request is titled and described from its branch's commits:

- **One commit**: its subject and body.
- **Several commits**: loom asks for a title, and the description lists every commit message. `--title` answers in advance.

`--title` applies to the branch you named, not to the branches below it. It works on GitHub, Azure DevOps and GitLab.

Change-Id trailers are left out of the description. On GitLab the server writes the description, so they stay there.

## Draft Pull Requests

`--draft` opens the branch's pull request as a draft, or converts the one it already has:

```bash
git loom push feature-a --draft
# ✓ Pushed `feature-a` to `origin`
# ✓ Draft PR created: https://github.com/owner/repo/pull/42
```

On GitHub, GitLab and Azure DevOps the pull request is a draft. On GitHub and Azure DevOps that takes `gh` or `az`: without it the push is refused rather than leaving the pull request ready for review. On Gerrit the review is pushed as work in progress (`refs/for/main%wip`), and so are the reviews it is chained on. Like `--title`, it applies to the branch you named, not to the branches below it.

Loom never takes a pull request out of draft: mark it ready on the forge when it is.

## Forcing a Push

A forge that rebased your branch on the server (landing the pull request below it, for example) makes the next push fail. Loom suggests the way past it:

```
✗ git push failed
  › If `feature-a` has diverged on the remote, push again with `loom push feature-a -f`
```

Check what is on the remote first: `-f` overwrites it, for every branch of the stack. A Gerrit review push never forces.

## Push Remote

Loom pushes to the remote your integration branch tracks. On GitHub, if that remote is `upstream` and an `origin` exists, it pushes to `origin`, your fork. Set [`loom.pushRemote`](../configuration.md#loompushremote) for any other layout, and when your integration branch tracks a local branch, which is on no remote.

With several GitHub accounts logged in to `gh`, loom uses the first one that can push to the push remote: the account that owns your fork, or one that is a collaborator on a shared repository. Setting `GH_TOKEN` or `GITHUB_TOKEN` overrides that. This needs `gh` 2.81 or later; Linux distributions often ship an older one, so install it from [cli.github.com](https://cli.github.com).

## Pushing Without a Pull Request

`--no-pr` pushes the branch and its stack (on Gerrit, the branch alone) without creating anything else — to back up work in progress, for example.

On Gerrit it pushes a real branch, which only an admin can delete. Unless the branch name starts with `wip/`, loom offers to push it as `wip/<branch>` instead:

```
? Branch `feature-a` is not prefixed with `wip/` — a Gerrit admin will be needed to delete the remote branch later
> Push as `feature-a` (admin required to delete it later)
  Push as `wip/feature-a` instead
  Cancel
```

## Agent Mode

With [`--agent`](agent.md#agent-mode), the JSON `ok` line carries a `push` report:

```json
{"status":"ok","messages":["…"],"push":{"remote":"origin","forge":"github","pushed":["a","b"],
 "prs":[{"branch":"a","base":"main","state":"created","url":"https://github.com/owner/repo/pull/41"},
        {"branch":"b","base":"a","state":"not_created","reason":"it has several commits and no `--title`",
         "create_url":"https://github.com/owner/repo/compare/a...b?expand=1","hint":"Re-run with `loom push b --title <title>`"}]}}
```

Each PR's `state` is `created`, `updated` or `not_created`. A branch of several commits can't be asked for a title, so its PR is `not_created` unless you pass `--title`. `draft` is set when `--draft` made it one. `republished`, `not_pushed`, `links` and the GitHub `stack` number appear when they apply.

## Examples

```bash
git loom push feature-a                      # push and open or update the PR
git loom push feature-b --title "Login page" # several commits: give the PR a title
git loom push feature-a --draft              # open the PR as a draft
git loom push feature-a --no-pr              # push only
git loom push                                # pick the branch
```

## Prerequisites

- On an integration branch, with the branch woven into it
