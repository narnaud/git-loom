# Spec 011: Push

## Overview

`git loom push` pushes one woven feature branch to the remote, together with
the branches it is stacked on. It detects the remote type (plain Git, GitHub,
GitLab, Azure DevOps, Gerrit) and uses the appropriate push strategy. It never
pushes the integration branch.

## Why Push?

When working with stacked/woven feature branches, pushing a branch to a remote
varies significantly depending on the hosting platform:

- **Plain Git** needs `--force-with-lease` because the branch is rebased
- **GitHub** benefits from opening a pull request after pushing
- **Gerrit** requires a special `refs/for/` refspec

`git loom push` unifies these under one command with automatic remote detection.

## CLI

```bash
git-loom push [branch] [--no-pr] [-f|--force]
```

**Arguments:**

- `branch` (optional): Branch name or short ID. If omitted, an interactive
  picker is shown.

**Flags:**

- `--no-pr`: Push without creating a PR or Gerrit review. For GitHub and Azure
  DevOps, skips the `gh pr create` / `az repos pr create` step. For Gerrit,
  pushes directly to the branch ref instead of `refs/for/` (see below).
- `-f`, `--force`: Push with `--force` instead of the default
  `--force-with-lease --force-if-includes`. Needed when the lease check refuses
  a push that is intentional, for example after the remote branch was updated
  from elsewhere. It applies to every branch the push contains, not only the
  one named: a stack goes out in one `git push`, so forcing it overwrites the
  downstack and the re-published upstack too. Has no effect on a Gerrit
  `refs/for/` review push, which never forces.

**Behavior:**

- Resolves the target branch (explicit argument or interactive picker)
- Validates the branch is woven into the integration branch
- Works out the push plan: the branch, the branches below it in its stack,
  and the published branches above it that need re-pushing (see
  [Stacked Branches](#stacked-branches))
- Detects the remote type
- Pushes using the appropriate strategy

## Stacked Branches

A feature branch is **stacked** on another when its oldest commit's parent is
the other branch's tip — the same tip adjacency the status graph uses to draw
`│├─` between branches (spec 001). Stacks can be several branches deep.

A stack is **linear**: the weave replays the feature branches one after
another, so a branch built on another branch's tip either continues that stack
or is rebased off it (spec 004). Nothing ever sits beside a layer, and every
branch has at most one branch directly above it.

### What a push contains

Given `d` on `c` on `b` on `a` on `main`, `git loom push b`:

| Branches | Rule |
|----------|------|
| `a`, `b` | **Downstack**: the requested branch and everything below it. A stacked branch cannot be reviewed without the branches it is built on, and their PRs need the lower branches to exist on the remote. |
| `c` (published, out of step) | **Re-published upstack**: an upstack branch whose remote tracking ref is not at its tip (`Different`) — rewritten by the same rebase that rewrote `b`, carrying commits of its own, or both. Either way what is on the server no longer matches the stack, so it is pushed too, but never gets a PR created. |
| `d` (never pushed) | **Hint only**: `Not pushed above 'b': 'd'` with `Run `loom push d` to publish them`. It has no remote ref to keep in step, and creating one is the user's call. |
| upstack `Synced` / `Gone` | Skipped: nothing to update, or its PR was already merged or closed. |
| downstack `Gone` | Skipped: its PR was merged or closed and the branch deleted on the remote, so pushing it would resurrect it. The layer above takes its place and targets what it was stacked on, or `main` when it becomes the bottom. The requested branch is pushed whatever its remote status. |

The hint names the topmost branch left alone: the stack is linear, so pushing
that one publishes every branch below it.

All these branches go out in a single `git push --atomic` (one lease check,
one round trip, and a lease refused on any branch leaves every branch
untouched on the server), except on GitLab where each branch is pushed
separately because the merge-request push options differ per branch. Gerrit is unaffected: a
`refs/for/` push of the requested branch already uploads its whole ancestry as
a relation chain.

```bash
git-loom push b
# ✓ Pushed `a`, `b` to `origin`
#   Re-pushed above `b`: `c`
# ! Not pushed above `b`: `d`
#   Run `loom push d` to publish them
```

### PR base per layer

Each pushed branch is a **layer** whose PR targets the branch below it; the
bottom layer targets the upstream target branch (`main`). A branch this push
leaves off the remote — merged, or never pushed — is no target: the layer
above it takes the nearest branch below that the push does leave there, and
`main` when there is none:

| Remote | Base handling |
|--------|---------------|
| GitHub | `--base <below>` on creation; an existing PR whose base differs is retargeted with `gh pr edit --base` (`PR retargeted to `a`: <url>`). |
| GitLab | `-o merge_request.target=<below>` on each branch's push. GitLab renders dependent MRs and retargets them on merge itself. A re-published layer is pushed without `merge_request.create`: the target option updates its existing MR, and none is created. |
| Azure DevOps | Refused — see below. |
| Gerrit | Unchanged: `refs/for/<upstream>`; Gerrit builds the relation chain. |

**Azure DevOps.** Azure has no stacked pull requests, and `az repos pr update`
cannot retarget an existing one either, so a stack would land as PRs whose base
says nothing a reviewer can rely on. Pushing a stacked branch is refused before
anything reaches the remote:

```
✗ `b` is stacked on `a` — Azure DevOps has no stacked pull requests
  Land the branches below it first, or push without a PR (`--no-pr`)
```

`--no-pr` still pushes the whole plan, PRs being what Azure cannot express.

When a lower branch has landed upstream and `loom update` has rebased the
integration branch, the stack simply shrinks: the next branch becomes the
bottom and its PR is retargeted to `main` (GitHub normally does this by itself
when the lower PR merges). It shrinks the same way before `loom update` runs:
a lower branch whose remote ref is already gone is dropped from the push.

### GitHub stack registration

GitHub links stacked PRs through a *Stack* object on top of the base-branch
chain. After the PRs exist, loom reconciles it through `gh api`, reading each
PR's `stack` field (`GET repos/{owner}/{repo}/pulls/{n}`, bottom to top):

| Memberships (bottom → top) | Action |
|----------------------------|--------|
| none in a stack | `POST repos/{owner}/{repo}/stacks` with `{"pull_requests":[bottom, …, top]}` |
| bottom ones in stack `S`, rest in none | `POST repos/{owner}/{repo}/stacks/S/add` with the rest |
| all in the same stack | nothing (`Stack #S already links these N PRs`) |
| anything else | warn and leave the stacks alone |
| a membership lookup failed | warn (`Could not read the GitHub stack of these PRs`) and leave the stacks alone; a failed read is never taken for "not in a stack" |

Nothing is persisted locally: the stack is derived from the branch topology on
each push and GitHub's Stack object is the source of truth. Registration runs
whenever the push looks after more than one branch's PR — its downstack and
the re-published layers above together — on the contiguous run of layers (from
the bottom) that all have a PR and each target the layer below. A branch with
nothing below it still forms a chain when a re-published branch sits above it,
whose PR targets it. The
Stacks API requires every PR's base to be the previous PR's head, so a layer
skipped in between — an upstack branch that was already in sync — ends the
run: with `d` re-published above a synced `c`, only `a` and `b` are linked.
If the API call fails — for example on a
host without stacked pull requests — the PR bases are still in place and a
warning points at `gh extension install github/gh-stack` and `loom trace`.

**PR creation in a chain.** A push covering one branch keeps the
`gh pr create --web` flow (the browser opens on the PR form). In a chain,
missing PRs are created
directly with `gh pr create` (title and description as in
[PR Title and Description](#pr-title-and-description)) and their URLs printed
(`PR created: <url>`), because `--web` leaves the PR uncreated until the form
is submitted and the stack could never be registered in the same run.

**Forks.** GitHub does not support stacks across forks. In a fork setup
(pushing to `origin`, PRs on `upstream`) a warning is printed once,
`Stacked pull requests are not supported across forks — PRs target `main``,
every layer's PR targets the upstream branch, and no stack is registered.
Every layer still gets its PR: missing ones are created directly, as in any
stack, only the registration step is skipped.

**Agent mode.** PRs are never created behind an agent's back (spec 019): the
skipped layer is reported (`Skipped creating a PR for `b` (agent mode)`) and,
having no PR, ends the run of layers the stack registration links, like any
other layer without one.

```bash
git-loom push b          # b stacked on a, no PRs yet
# ✓ Pushed `a`, `b` to `origin`
# ✓ PR created: https://github.com/owner/repo/pull/41
# ✓ PR created: https://github.com/owner/repo/pull/42
# ✓ Stack #7 registered with 2 PRs
```

### Creating a stack locally

A stack is a topology, not a flag. The ways to get one:

- `git loom branch part-1 -t <commit inside feature>` — placing a branch on a
  commit inside another branch's section splits it in two stacked layers
  (spec 005).
- `git loom branch b -t a` then `git loom commit -b b` — a branch co-located
  with `a` becomes a stacked layer as soon as it receives its own commit
  (spec 004, co-located branch splitting).

Amending any layer works as usual (`fold` files into a commit, `absorb`,
`reword`): the weave rebase rewrites the layers above it too, and the next push
of any layer re-publishes them. **Known limitation:** `git loom commit -b a` and
`git loom fold <commit> a` do not yet accept a branch that is a *lower* layer of
a stack, because the weave resolves the target by section and lower layers only
exist as `update-ref`s inside the top section's. Commit to the top layer, or
fold into an existing commit, instead.

After the bottom PR merges, `git loom update` rebases the integration branch
and the stack shrinks from the bottom; with `loom.pruneGoneBranches` the merged
local branch is removed too.

## Remote Type Detection

Detection priority (first match wins):

1. **Explicit config**: `git config loom.remote-type` — values: `github`, `gitlab`, `azure`, `gerrit`, `plain`
2. **URL heuristics**: Remote URL contains `github.com` → GitHub
3. **URL heuristics**: Remote URL contains `gitlab` → GitLab
4. **URL heuristics**: Remote URL contains `dev.azure.com` → Azure DevOps
5. **Hook inspection**: `.git/hooks/commit-msg` contains "gerrit" (case-insensitive) → Gerrit
6. **Gerrit confirmation**: if nothing matched but the remote *looks* like
   Gerrit — the remote URL uses Gerrit's standard SSH port (`:29418/`), or one
   of the last 20 commits reachable from HEAD carries a `Change-Id:` trailer —
   the user is asked to confirm. The answer is saved as
   `git config loom.remote-type` (`gerrit` or `plain`) so the question is asked
   at most once per repository. This catches Gerrit repos where the hook check
   fails, e.g. when [pre-commit](https://pre-commit.com) manages the commit-msg
   hook and the generated wrapper never mentions "gerrit".
7. **Fallback**: Plain

Self-hosted GitLab instances whose hostname does not contain `gitlab` (e.g.
`invent.kde.org`) are not auto-detected — set `git config loom.remote-type
gitlab` for those. Even without detection, a plain push still surfaces the MR
link the server prints (see [Plain](#plain-default)).

The Gerrit confirmation prompt only runs in `git loom push` itself. The
detection helper is also used non-interactively (e.g. by `git loom update` to
resolve the fork push remote), where it silently falls back to Plain.

## Push Remote Selection

Detection priority (first match wins):

1. **Explicit config**: `git config loom.push-remote <remote>` — specify the remote name to push to
2. **GitHub fork convention**: if the integration remote is named `upstream` and `origin` exists, push to `origin`
3. **Fallback**: integration branch's remote

This allows fork workflows where the integration branch tracks the upstream repository but branches are pushed to a personal fork. For non-standard remote names, set `loom.push-remote` explicitly.

## Push Strategies

### Plain (default)

```bash
# a lone branch
git push --force-with-lease --force-if-includes -u <remote> <branch>
# a stack: every branch of the plan in one push
git push --force-with-lease --force-if-includes --atomic -u <remote> <branch>...
```

Uses `--force-with-lease` because woven branches are frequently rebased and
need force pushing. `--force-if-includes` adds an extra safety check that the
local ref includes the remote ref. Which branches a push carries, and the
`--atomic` guarantee over them, are covered in
[Stacked Branches](#stacked-branches) above.

After pushing, any `remote:` lines containing an `http(s)` URL are surfaced as
indented continuation lines below the success message. This shows the MR/PR
creation link that servers like GitLab print on push, even when the remote type
was not detected as GitLab.

### GitHub

```bash
# the plain push above: one branch, or the whole plan atomically
git push --force-with-lease --force-if-includes [--atomic] -u <remote> <branch>...
# If PR exists:
#   Pushed `feature-a` to `origin`
#   PR updated: https://github.com/owner/repo/pull/42
# If no PR, a lone branch opens the PR form in the browser
gh pr create --web --head <head> --base <target> --repo <owner/repo>
# in a stack every PR is created directly, targeting the branch below
gh pr create --head <head> --base <branch below> --repo <owner/repo>
# and an existing PR whose base is wrong is retargeted
gh pr edit <n> --repo <owner/repo> --base <branch below>
```

Pushes the branch with `--force-with-lease` (same safety as plain), then
checks whether a PR already exists for the branch using `gh pr list`. Its
`--head` filter matches the branch name in every fork of the repository, so
the result is narrowed to the PR whose head lives in the push remote's
repository (`headRepositoryOwner`), so a stranger's PR from a same-named
branch is not reported, retargeted, or taken for ours. That owner comes from
the push remote's URL; in the rare setup where it cannot be read, the first
match stands in and the narrowing does not apply. If a PR exists, prints its
URL without opening the browser. If no PR exists,
creates the PR via the `gh` CLI with an auto-generated title and
description (see [PR Title and Description](#pr-title-and-description)
below). If `gh` is not installed, prints a helpful message with a link to
install it.

**Fork workflow:** When the integration branch tracks `upstream/main` (a fork
setup), feature branches are pushed to `origin` (the user's fork) instead.
The `--head` argument is prefixed with the fork owner (e.g. `user:branch`)
and `--repo` points to the upstream repository so the PR targets the correct
repo.

**Upstream branch skip:** If the branch being pushed is the upstream target
branch itself (e.g. pushing `main` when tracking `origin/main`), PR creation
is skipped and the push falls back to the plain force-with-lease strategy.

### GitLab

```bash
# one push per layer, because the MR options differ per branch
git push --force-with-lease --force-if-includes \
    -o merge_request.create -o merge_request.target=<branch below> -u <remote> <branch>
# a re-published layer keeps its MR: the target option without merge_request.create
git push --force-with-lease --force-if-includes \
    -o merge_request.target=<branch below> -u <remote> <branch>
# Pushed `feature-a` to `origin`
#   https://gitlab.com/group/repo/-/merge_requests/42
```

Uses GitLab [push options](https://docs.gitlab.com/ee/user/project/push_options.html)
so the server creates a merge request (or points to the existing one) as part of
the push. The MR URL GitLab prints in the push output is surfaced below the
success message, reusing the same `remote:` URL extraction as the plain and
Gerrit strategies. No extra CLI tool is required.

If the branch being pushed is the upstream target branch itself, the MR push
options are skipped and it falls back to a plain push.

### Azure DevOps

```bash
# the plain push above: one branch, or the whole plan atomically
git push --force-with-lease --force-if-includes [--atomic] -u <remote> <branch>...
# If PR exists:
#   Pushed `feature-a` to `origin`
#   PR updated: https://dev.azure.com/org/project/_git/repo/pullrequest/42
# If no PR:
# always for one branch: a stacked branch never gets this far
az repos pr create --open --source-branch <branch> --target-branch <target> \
    --org <org-url> --project <project> --repository <repo>
```

Pushes the branch with `--force-with-lease` (same safety as plain), then checks
whether a PR already exists for the branch using `az repos pr list`. If a PR exists,
prints its URL without opening the browser. If no PR exists, creates the PR
via the `az` CLI with an auto-generated title and description (see
[PR Title and Description](#pr-title-and-description) below). The organization,
project and repository are read from the remote URL and passed explicitly,
because az stops auto-detecting the project and repository once `--org` is
given; `--detect` stands in for all three only when the URL cannot be parsed.
If `az` is not installed, prints a helpful message with a link to install it.

### Gerrit

```bash
git push <remote> <branch>:refs/for/<target>
```

Uses the Gerrit `refs/for/` refspec to create or update a change. No topic is
set — Gerrit keeps whatever topic the change already has.

After pushing, stderr from the git push command is captured and scanned for
review URLs. Lines starting with `remote:` that contain `http://` or `https://`
are extracted and displayed below the success message as indented continuation
lines.

## PR Title and Description

When creating a new PR (GitHub or Azure DevOps), git-loom auto-generates the
title and description from the commits the PR contains: those between its base
and the branch tip. For a layer stacked straight on its base — the usual case —
that is the branch's own commits, not those of the branches below it. When the
base is further down, because a fork's PRs target the trunk or because the
layer below was dropped as merged, the PR's diff spans the branches in between
and the description spans them too, so it always describes what a reviewer
sees:

- **Single commit**: the commit subject becomes the PR title and the commit
  body becomes the PR description.
- **Multiple commits**: the user is prompted for a PR title via an
  interactive input naming the branch (`PR title for `b``). The description is built by concatenating all commit
  messages (oldest to newest), separated by `---` dividers. Each entry
  includes the commit subject and body.
- **No commits** (empty branch): the branch name is used as the title with
  an empty description.

Merge commits in the branch are skipped when gathering commit messages.

## Branch Selection

- **Explicit argument**: Resolved via `resolve_arg()` with `accept = [Branch]` — see spec 002. Must be a woven branch.
- **Interactive picker**: Lists woven, non-hidden branches from `info.branches` via `cliclack::select`
- No "create new" option (unlike commit — we're pushing existing branches)

**Hidden branches** (`loom.hideBranchPattern`, spec 001) are never pushed.
`loom push local-x` is refused (`Branch `local-x` is hidden and is never
pushed`), and so is a branch whose downstack contains one (`Branch `b` is
stacked on hidden branch `local-a`, which is never pushed`), since pushing
`b` would publish `local-a` with it; both point at renaming the branch or
changing the pattern. Hidden branches above the requested one are neither
re-published nor hinted, as `loom status` never shows them.

## Error Cases

### No woven branches

```bash
git-loom push
# error: No woven branches to push. Create a branch with 'git loom branch' first.
```

### Branch not woven

```bash
git-loom push stray-branch
# error: Branch 'stray-branch' is not woven into the integration branch.
```

### Target is not a branch

```bash
git-loom push abc123
# error: Target must be a branch, not a commit.
```

### gh CLI not installed (GitHub remote)

```bash
git-loom push feature-a
# Pushed `feature-a` to `origin`
# Install 'gh' CLI to create pull requests: https://cli.github.com
```

### az CLI not installed (Azure DevOps remote)

```bash
git-loom push feature-a
# Pushed `feature-a` to `origin`
# Install 'az' CLI to create pull requests: https://learn.microsoft.com/cli/azure/install-azure-cli
```

### Push fails (e.g., no network)

```bash
git-loom push feature-a
# error: git push ... failed:
# fatal: Could not read from remote repository.
```

## Examples

### Push to a plain remote

```bash
git-loom push feature-a
# Pushed `feature-a` to `origin`
```

### Push to GitHub (with gh CLI)

```bash
git-loom push feature-a
# Pushed `feature-a` to `origin`
# (browser opens to PR creation page)
```

### Push to Azure DevOps (with az CLI)

```bash
git-loom push feature-a
# Pushed `feature-a` to `origin`
# (browser opens to PR creation page)
```

### Push to Gerrit

```bash
git-loom push feature-a
# Pushed `feature-a` to `origin` (Gerrit: `refs/for/main`)
#   › https://gerrit.example.com/c/project/+/12345
```

### Interactive branch selection

```bash
git-loom push
# ? Select branch to push
# > feature-a
#   feature-b
#   feature-c
# Pushed `feature-a` to `origin`
```

### Override remote type via config

```bash
git config loom.remote-type gerrit
git-loom push feature-a
# Pushed `feature-a` to `origin` (Gerrit: `refs/for/main`)
#   › https://gerrit.example.com/c/project/+/12345
```

## Gerrit --no-pr Behavior

When `--no-pr` is used with a Gerrit remote, the push goes directly to the
branch ref (not `refs/for/`), which creates or updates a remote branch rather
than a Gerrit change.

**If the branch starts with `wip/`:** pushed directly (no prompt) — Gerrit
projects typically allow users to delete their own `wip/` branches.

**If the branch does not start with `wip/`:** an interactive prompt is shown,
since creating a non-`wip/` remote branch in Gerrit requires a project admin
to delete it later:

```
? Branch `feature-a` is not prefixed with `wip/` — a Gerrit admin will be needed to delete the remote branch later
> Push as `feature-a` (admin required to delete it later)
  Push as `wip/feature-a` instead
  Cancel
```

- **Push as-is**: pushes `feature-a` to `remote/feature-a` with `--force-with-lease`
- **Push as `wip/<branch>`**: pushes with refspec `feature-a:wip/feature-a` — no admin needed to delete it
- **Cancel**: aborts with `Push cancelled`

## Design Decisions

### Force-with-lease for all pushes

Woven branches are rebased as part of normal loom operations (fold, drop,
commit). Force pushing is expected, but `--force-with-lease` prevents
accidentally overwriting changes pushed from another machine. This applies
to all remote types (plain, GitHub, and Gerrit's underlying push).

### gh CLI as optional dependency

The `gh` CLI is not required. When absent, the push still succeeds — only
the PR creation step is skipped with a helpful installation message.

### GitHub Fork Workflow

In a fork setup where the integration branch tracks `upstream/main`, the push
remote is automatically switched to `origin` (the user's fork). PR creation
targets the upstream repository by resolving the `upstream` remote URL. The
`--head` argument includes the fork owner prefix so GitHub can match the PR
source correctly.

### One branch and its dependencies

Each push names one branch. What goes out with it is not a choice but a
consequence of the stack: the branches below it, which its review depends on,
and the published branches above it that the same rebase rewrote. Independent
branches are never pushed together — that would be confusing and error-prone.

## Prerequisites

- Must be in a git repository with a working tree (not bare)
- Current branch must be an integration branch (has upstream tracking)
- At least one woven branch must exist
- Network access to the remote (for `git push`)
- Git 2.38 or later (checked globally at startup)
- `gh` CLI (optional, for GitHub PR creation)
- `az` CLI (optional, for Azure DevOps PR creation)
