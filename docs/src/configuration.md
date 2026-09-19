# Configuration

## Git Config Settings

| Setting | Values | Default | Description |
|---------|--------|---------|-------------|
| `loom.remote-type` | `github`, `gitlab`, `azure`, `gerrit`, `plain` | Auto-detected | Override the remote type for `git loom push` |
| `loom.push-remote` | Any remote name | Auto-detected | Override which remote to push to (e.g., `personal` for fork workflows) |
| `loom.hideBranchPattern` | Any prefix string | `local-` | Prefix for branches hidden from `loom status` by default |
| `loom.statusContext` | Integer ≥ 1 | `1` | Context commits shown at and before the base by `loom status` and `loom tui` |
| `loom.pruneGoneBranches` | `true`, `false` | `false` | Let `git loom update` remove local branches whose remote branch is gone |
| `loom.changeId` | `true`, `false` | `true` | Add a Gerrit-style `Change-Id` trailer to every commit loom creates |

### `loom.remote-type`

By default, `git loom push` auto-detects the remote type:

- **GitHub** — if the remote URL contains `github.com`
- **GitLab** — if the remote URL contains `gitlab`
- **Azure DevOps** — if the remote URL contains `dev.azure.com`
- **Gerrit** — if `.git/hooks/commit-msg` contains "gerrit", or you confirm the prompt when the remote looks like Gerrit
- **Plain Git** — otherwise

You can override this with:

```bash
git config loom.remote-type github   # Force GitHub push (push + PR, stacked PRs)
git config loom.remote-type gitlab   # Force GitLab push (merge request push options)
git config loom.remote-type azure    # Force Azure DevOps push (push + open PR)
git config loom.remote-type gerrit   # Force Gerrit push (refs/for/<branch>)
git config loom.remote-type plain    # Force a plain force-with-lease push
```

### `loom.push-remote`

By default, `git loom push` uses the integration branch's remote for pushing. One exception: if the integration branch tracks a remote named `upstream` and a remote named `origin` also exists, pushes go to `origin` automatically (the standard GitHub fork convention).

For non-standard fork setups where your remotes have different names, set this explicitly:

```bash
git config loom.push-remote personal
```

For example, with remotes:

- `origin` → upstream read-only repository
- `personal` → your fork (where you push)

Now `git loom push` will push to `personal` regardless of remote names.

### `loom.hideBranchPattern`

Branches whose names start with this prefix are hidden from `loom status` by default — both the branch section and its commits are suppressed. Pass `--all` to show them.

```bash
git config loom.hideBranchPattern "local-"   # default: hide local-* branches
git config loom.hideBranchPattern "secret-"  # hide secret-* branches instead
git config loom.hideBranchPattern ""         # disable hiding entirely
```

Hidden branches remain fully accessible to the other loom commands (`fold`, `drop`, `commit`, etc.), except `push`, which never publishes a hidden branch nor a branch stacked on one.

When creating or renaming a branch to a name that matches this prefix, *git-loom* prints a warning.

### `loom.statusContext`

How much history to show at and before the base: `1` (the default) shows the base alone, `3` adds the two commits before it as dimmed context lines.

```bash
git config loom.statusContext 5
```

The positional argument overrides it for one run (`git loom status 1`), and in `loom tui` the `+` and `-` keys change the depth live.

### `loom.changeId`

Every commit that `git loom commit`, `split`, or `reword` creates ends with a
`Change-Id: I<40 hex>` trailer, in the form Gerrit's `commit-msg` hook
writes. Git rebases copy messages verbatim, so the trailer gives a commit one
identity that survives every `update`, `fold`, `swap`, or `split`.

```bash
git config loom.changeId false   # create commits without a Change-Id
```

Gerrit's own settings are honored too: `gerrit.createChangeId false` also
disables generation, and with `gerrit.reviewUrl` set the trailer is written in
Gerrit's `Link: <url>/id/I<hex>` form. Commits that already carry a Change-Id
are never stamped twice, and a `commit-msg` hook, if installed, still runs:
with `-m` it sees the trailer loom added; on the editor path it runs first, and
a Change-Id it adds is kept. Without `-m`, loom adds a missing trailer by
amending the message once after the editor, so `post-commit` and
`post-rewrite` hooks see that amend too. That amend follows your git config
rather than the options you passed after `--`: a commit signed through
`commit.gpgsign` stays signed, one signed only by a forwarded `-S` loses its
signature. Set the config, or pass `-m`.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `NO_COLOR` | Disable colored output when set (follows the [NO_COLOR](https://no-color.org/) standard) |
| `TERM` | Colors are automatically disabled when `TERM=dumb` |

## CLI Flags

| Flag | Description |
|------|-------------|
| `--no-color` | Disable colored output |
| `--theme <auto\|dark\|light>` | Set the graph color theme (default: `auto`) |

### `--theme`

Controls the color palette used for graph output.

| Value | Behavior |
|-------|----------|
| `auto` | Detect the terminal background and choose dark or light automatically. Falls back to dark if detection fails or output is not a TTY. |
| `dark` | Always use the dark theme (optimized for dark terminal backgrounds). |
| `light` | Always use the light theme (optimized for light terminal backgrounds). |

```bash
git loom --theme light
git loom --theme dark status
```
