# push

Push a feature branch to the remote, together with the branches it is stacked on. Automatically detects the remote type and uses the appropriate push strategy.

## Usage

```
git loom push [branch] [--no-pr] [-f|--force]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[branch]` | Branch name or short ID (optional; interactive picker if omitted) |

### Flags

| Flag | Description |
|------|-------------|
| `--no-pr` | Push without creating a PR or Gerrit review (see below) |
| `-f`, `--force` | Push with `--force` instead of `--force-with-lease --force-if-includes` |

## Forges at a Glance

| Forge | Detected by | CLI | Branch of its own | Stacked branch | Without the CLI | `--no-pr` |
|-------|-------------|-----|-------------------|----------------|-----------------|-----------|
| GitHub | `github.com` in the remote URL | `gh` | PR created or updated | One PR per layer, each targeting the one below, linked into a GitHub stack | Link to create the PR; a stacked branch is refused | Pushes the branch and its stack, no PR |
| GitLab | `gitlab` in the remote URL | none (push options) | MR created or updated | One MR per layer, each targeting the one below | Not needed | Pushes without the MR push options |
| Azure DevOps | `dev.azure.com` in the remote URL | `az` | PR created or updated | Refused: Azure has no stacked PRs | Link to create the PR | Pushes the branch and its stack, no PR |
| Gerrit | Gerrit `commit-msg` hook | none (`refs/for/`) | Review created or updated | Relation chain, built by Gerrit | Not needed | Pushes a branch, asking first unless it starts with `wip/` |
| Plain Git | your pick in the menu | none | Push only | Push only | Not needed | Same as without |

When nothing is detected, for example on a self-hosted GitLab, `git loom push` asks which forge it is. Set `git config loom.remote-type` (`github`, `gitlab`, `azure`, `gerrit`, `plain`) when detection guesses wrong. Forgejo, Gitea and Bitbucket are pushed as plain Git; the link their server prints after the push is shown.

GitHub, GitLab and Azure DevOps report a push the same way:

```bash
git loom push b                 # b stacked on a
# ✓ Pushed `a`, `b` to `origin`
# ✓ PR updated: https://github.com/owner/repo/pull/41
# ✓ PR created: https://github.com/owner/repo/pull/42
```

A pushed branch left without a PR gets one warning, whatever the reason: the CLI is missing, agent mode, or the CLI call failed. It links the forge's own page to create the PR when one can be built. A stacked branch the forge cannot handle is refused before anything is pushed:

```bash
# ✗ Cannot create stacked PRs: `b` is stacked on `a`
#   › Azure DevOps has no stacked pull requests
#   › Land `a` first, or push without PRs (`--no-pr`)
```

## Stacked Branches

A branch is *stacked* on another when it is built on top of it — its oldest commit's parent is the other branch's tip. That is exactly what `git loom status` draws with `│├─` between two branches. Stacks can be several branches deep.

Pushing a stacked branch pushes what its review depends on. With `d` on `c` on `b` on `a`:

```bash
git loom push b
# ✓ Pushed `a`, `b` to `origin`
#   Re-pushed above `b`: `c`
# ! Not pushed above `b`: `d`
#   Run `loom push d` to publish them
```

- **`a`, `b`** — the branch and everything below it, always.
- **`c`** — a branch above `b` whose remote is no longer at its tip, whether the rebase that rewrote `b` rewrote it too or it carries commits of its own. Either way the server no longer matches the stack, so it is re-pushed. No PR is created for it.
- **`d`** — a branch above `b` that was never pushed is only mentioned. It has no remote ref to keep in step, and creating one is your call.

Everything goes out in one atomic `git push` (one lease check, and a refused branch leaves the others untouched). Branches hidden by `loom.hideBranchPattern` are never pushed: pushing one, or a branch stacked on one, is refused. Gerrit needs none of this: `refs/for/` already uploads the whole chain of changes.

### Pull requests per layer

Each pushed branch gets a PR that targets the branch below it; the bottom one targets your upstream branch (`main`):

| Remote | What loom does |
|--------|----------------|
| GitHub | Creates missing PRs with `--base <branch below>`, retargets an existing PR whose base is wrong (`PR updated: …` with `Retargeted to 'a'`), then links the PRs into a [GitHub stack](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs) |
| GitLab | Pushes each branch with `merge_request.target=<branch below>`; GitLab shows the dependency and retargets on merge. A re-pushed upper branch keeps its existing MR and gets no new one |
| Azure DevOps | Not supported — pushing a stacked branch is refused (see [Azure DevOps](#azure-devops)) |
| Gerrit | Unchanged |

On GitHub, stacked PRs need the `gh` CLI. Without it, pushing a stacked branch is refused before anything is pushed, with the same message as on Azure DevOps; `--no-pr` still pushes the whole stack.

On GitHub the stack shows a stack map on every PR, reviewers see one layer at a time, and when the bottom PR merges GitHub rebases and retargets the ones above by itself. Nothing is stored locally: loom reads each PR's stack membership back through `gh api` on every push and creates or extends the stack as needed. When a push creates several PRs, all their URLs are printed; the stack map on each PR leads to the others.

```bash
git loom push b
# ✓ Pushed `a`, `b` to `origin`
# ✓ PR created: https://github.com/owner/repo/pull/41
# ✓ PR created: https://github.com/owner/repo/pull/42
# ✓ Stack #7 registered with 2 PRs
```

GitHub does not support stacks across forks: in a fork workflow every layer still gets its PR, but they all target the upstream branch and a warning says so. If the stack API is unavailable on your host, or a PR's stack membership cannot be read, the PR bases are still set and loom points you at the `gh-stack` extension.

### Creating a stack

A stack is just a shape of your branches:

- `git loom branch part-1 -t <commit inside feature>` splits a branch into two stacked layers.
- `git loom branch b -t a` followed by `git loom commit -b b` puts b's first commit on top of a.

Amending any layer (`fold`, `absorb`, `reword`) rewrites the layers above it as well; the next `push` of any layer re-publishes them. Adding a commit to a *lower* layer with `commit -b` or `fold <commit> <branch>` is not supported yet — commit to the top layer or fold into an existing commit instead.

When the bottom PR merges, `git loom update` rebases your integration branch and the stack shrinks from the bottom.

## Remote Type Detection

The remote type is settled once per repository (first match wins):

1. **Explicit config** — `git config loom.remote-type` set to `github`, `gitlab`, `azure`, `gerrit`, or `plain`
2. **Auto-detection** — the result is saved as `loom.remote-type`:
   - remote URL contains `github.com` → GitHub
   - remote URL contains `gitlab` → GitLab
   - remote URL contains `dev.azure.com` → Azure DevOps
   - `.git/hooks/commit-msg` contains "gerrit" → Gerrit
3. **Menu** — when nothing is detected, you pick the forge from a menu listing every type (in `loom tui` too); your pick is saved as `loom.remote-type`

```bash
git loom push feature-a
# ? Which kind of remote is `origin`? (saved as `loom.remote-type`)
# > GitHub
#   GitLab
#   Azure DevOps
#   Gerrit
#   Plain Git
```

Self-hosted GitLab whose hostname does not contain `gitlab` (e.g. `invent.kde.org`) is not auto-detected, so pick GitLab in the menu. Since the type is saved, change `loom.remote-type` if the remote later moves to another forge.

## Push Remote Selection

Detection priority (first match wins):

1. **Explicit config** — `git config loom.push-remote <remote>`
2. **GitHub fork convention** — if the integration remote is named `upstream` and `origin` exists, push to `origin`
3. **Fallback** — integration branch's remote

For non-standard fork setups (e.g., integration branch tracks `origin` but you push to `personal`), set:

```bash
git config loom.push-remote personal
```

## Push Strategies

### Plain Git (default)

```bash
git push --force-with-lease --force-if-includes -u <remote> <branch>
```

Uses `--force-with-lease` because woven branches are frequently rebased. `--force-if-includes` adds extra safety.

#### When the remote rewrote your branch

Landing the pull request below a stacked branch makes the forge rebase that branch for you, server-side. The tip on the remote is then a commit your clone never held, and `--force-if-includes` refuses every later push of that branch — with a hint to `git pull` that does not help, because merging it back only adds content you already have.

loom names the flag that does:

```
✗ git push failed
  › If `feature-a` has diverged on the remote, push again with `loom push feature-a -f`
```

That is the push you ran, forced — `--no-pr` and the branch you named both survive it. loom does not check whether the remote really diverged, or whether what is there is yours: a colleague's commit and a forge's rebase look the same from your clone, and only you can tell them apart. Fetch and look if you are unsure. Typing the flag is what makes it your call. Only a push git refused gets this hint: a failure over credentials, the network, a hook, or a server that rejected the change itself is not one a force gets past.

Pass `-f` / `--force` to push with plain `--force` instead, for when the lease check refuses a push you know is correct. It applies to every remote type except a Gerrit `refs/for/` review push, which never forces. It also applies to every branch the push contains, not only the one named: a stack goes out in a single `git push`, so forcing it overwrites the downstack and re-published upstack branches too.

Any `remote:` lines containing an `http(s)` URL are shown below the success message, so the MR/PR creation link that servers like GitLab print on push is visible even when the remote type was not detected.

### GitHub

Pushes the branch with `--force-with-lease`, then checks whether a PR already exists for the branch:

- **PR exists** — prints the PR URL (`PR updated: https://github.com/owner/repo/pull/42`)
- **No PR** — creates the PR via `gh pr create` with an auto-generated title and description (see [PR Title and Description](#pr-title-and-description) below) and prints its URL (`PR created: …`)

No browser is opened in either case.

For a stacked branch, see [Stacked Branches](#stacked-branches). If `gh` is not installed, see [Without the forge CLI](#without-the-forge-cli).

In a **fork workflow** (tracking `upstream/main`), pushes go to `origin` (your fork) and the PR targets the upstream repository automatically.

With several GitHub accounts logged in to `gh`, loom runs `gh` as the account that owns the push remote (e.g. `you` for `git@github.com:you/repo`), so the active `gh` account does not matter. A `GH_TOKEN` you set yourself takes precedence.

If the branch being pushed is the upstream target branch itself, PR creation is skipped.

### GitLab

```bash
git push --force-with-lease --force-if-includes \
    -o merge_request.create -o merge_request.target=<target> -u <remote> <branch>
```

Uses GitLab [push options](https://docs.gitlab.com/ee/user/project/push_options.html) so the server creates a merge request (or points to the existing one) during the push. The MR GitLab reports is printed as `PR created: …` when the branch is new on the remote, and `PR updated: …` otherwise, like the other forges. No extra CLI tool is required. If the branch being pushed is the upstream target branch itself, the MR push options are skipped.

### Azure DevOps

Azure has no stacked pull requests, and `az repos pr update` cannot retarget an existing one either, so a stack would land as PRs whose base says nothing a reviewer can rely on. Pushing a stacked branch is refused before anything reaches the remote:

```
✗ Cannot create stacked PRs: `b` is stacked on `a`
  › Azure DevOps has no stacked pull requests
  › Land `a` first, or push without PRs (`--no-pr`)
```

`--no-pr` still pushes the whole stack; it is the pull requests Azure cannot express.

For a branch of its own, loom pushes it with `--force-with-lease`, then checks whether a PR already exists:

- **PR exists** — prints the PR URL (`PR updated: https://dev.azure.com/...`)
- **No PR** — creates the PR via `az repos pr create` with an auto-generated title and description (see [PR Title and Description](#pr-title-and-description) below) and prints its URL (`PR created: …`)

No browser is opened in either case.

The organization, project and repository are read from the remote URL and passed explicitly; `--detect` is only used when the URL cannot be parsed. If `az` is not installed, see [Without the forge CLI](#without-the-forge-cli).

Legacy `https://<org>.visualstudio.com/...` remotes are not supported: they are not auto-detected, and even with `loom.remote-type azure` the `--detect` fallback fails on them. Point the remote at its `dev.azure.com` URL instead.

### Without the forge CLI

If `gh` or `az` is not installed, a branch of its own is still pushed, and *git-loom* prints a link to the forge's new pull request page for it, with a hint to install the CLI:

```bash
git loom push feature-a
# ✓ Pushed `feature-a` to `origin`
# ! PR not created for `feature-a`: `gh` is not installed
#   › Create it at https://github.com/owner/repo/compare/main...feature-a?expand=1
#   › Install `gh` to have loom create it: https://cli.github.com
```

The page shows the open PR instead when there is one. The same warning appears whenever a pushed branch is left without a PR, for example in agent mode or when the CLI call fails. A stacked branch is refused without `gh`, as described in [Pull requests per layer](#pull-requests-per-layer).

### Gerrit

```bash
git push <remote> <branch>:refs/for/<target>
```

Uses the `refs/for/` refspec. No topic is set. After pushing, any review URLs returned by Gerrit are extracted from the remote output and displayed below the success message.

## PR Title and Description

When creating a new PR (GitHub or Azure DevOps), *git-loom* auto-generates the title and description from the commits the PR contains — those between its base and the branch tip. A stacked PR includes only its branch's own commits. When a PR targets the trunk instead (in a fork, or because its lower layer was dropped as merged), it also includes commits from the lower layers, so the description always matches the diff a reviewer sees:

- **Single commit** — the commit subject becomes the PR title and the commit body becomes the description.
- **Multiple commits** — you are prompted for a PR title (the prompt names the branch). The description is built by concatenating all commit messages (oldest to newest), separated by `---` dividers.
- **Empty branch** — the branch name is used as the title with an empty description.

## Pushing Without a PR or Review

Use `--no-pr` when you want to push a branch to the remote without triggering PR or review creation — for example, to back up a branch, share work-in-progress, or push to a staging ref.

| Remote type | `--no-pr` behavior |
|-------------|-------------------|
| Plain | Same as normal (force-with-lease push) |
| GitHub | Skips `gh pr create` |
| GitLab | Plain push without `merge_request.create` push options |
| Azure DevOps | Skips `az repos pr create` |
| Gerrit | Plain push to branch ref instead of `refs/for/` (see below) |

### Gerrit: `wip/` prefix warning

In Gerrit, pushing directly to a branch ref (not `refs/for/`) creates a remote branch that requires a **project admin** to delete. To protect against accidental non-deletable branches, `--no-pr` on Gerrit prompts when the branch name doesn't start with `wip/`:

```
? Branch `feature-a` is not prefixed with `wip/` — a Gerrit admin will be needed to delete the remote branch later
> Push as `feature-a` (admin required to delete it later)
  Push as `wip/feature-a` instead
  Cancel
```

- **Push as-is** — pushes to `remote/feature-a`; an admin is needed to delete it later
- **Push as `wip/<branch>`** — pushes with refspec `feature-a:wip/feature-a`; your local branch name is unchanged
- **Cancel** — aborts the push

If the branch already starts with `wip/`, no prompt is shown.

## Examples

### Push to a plain remote

```bash
git loom push feature-a
# Pushed 'feature-a' to origin
```

### Push to GitHub (new PR)

```bash
git loom push feature-a
# Pushed 'feature-a' to origin
# PR created: https://github.com/owner/repo/pull/42
```

### Push to GitHub (PR already exists)

```bash
git loom push feature-a
# Pushed 'feature-a' to origin
# PR updated: https://github.com/owner/repo/pull/42
```

### Push a stacked branch to GitHub

```bash
git loom push feature-b        # feature-b is stacked on feature-a
# Pushed `feature-a`, `feature-b` to `origin`
# PR updated: https://github.com/owner/repo/pull/41
# PR created: https://github.com/owner/repo/pull/42
# Stack #7 registered with 2 PRs
```

### Push to Azure DevOps (new PR)

```bash
git loom push feature-a
# Pushed 'feature-a' to origin
# PR created: https://dev.azure.com/org/project/_git/repo/pullrequest/42
```

### Push to Azure DevOps (PR already exists)

```bash
git loom push feature-a
# Pushed 'feature-a' to origin
# PR updated: https://dev.azure.com/org/project/_git/repo/pullrequest/42
```

### Push to Gerrit

```bash
git loom push feature-a
# Pushed 'feature-a' to origin (Gerrit: refs/for/main)
#   › https://gerrit.example.com/c/project/+/12345
```

### Interactive selection

```bash
git loom push
# ? Select branch to push
# > feature-a
#   feature-b
# Pushed 'feature-a' to origin
```

### Push without opening a PR (GitHub)

```bash
git loom push feature-a --no-pr
# Pushed 'feature-a' to origin
```

### Push without a review, renaming to wip/ (Gerrit)

```bash
git loom push feature-a --no-pr
# ? Branch `feature-a` is not prefixed with `wip/`...
# > Push as `wip/feature-a` instead
# Pushed 'feature-a' to origin as 'wip/feature-a'
```

### Override remote type

```bash
git config loom.remote-type gerrit
git loom push feature-a
# Pushed 'feature-a' to origin (Gerrit: refs/for/main)
#   › https://gerrit.example.com/c/project/+/12345
```

## Prerequisites

- Must be on an integration branch with upstream tracking
- The target branch must be woven into the integration branch
- Network access to the remote
- `gh` CLI (for GitHub PR creation; required to push a stacked branch with PRs)
- `az` CLI (optional, for Azure DevOps PR creation)
