# Configuration

## Git Config Settings

Set them with `git config <setting> <value>`.

| Setting | Values | Default | Description |
|---------|--------|---------|-------------|
| `loom.remote-type` | `github`, `gitlab`, `azure`, `gerrit`, `plain` | Detected, else asked | How `git loom push` pushes |
| `loom.push-remote` | A remote name | Detected | The remote `git loom push` pushes to |
| `loom.hideBranchPattern` | A prefix | `local-` | Branches hidden from `git loom status` |
| `loom.statusContext` | An integer ≥ 1 | `1` | Commits shown at and before the base by `status` and `tui` |
| `loom.pruneGoneBranches` | `true`, `false` | `false` | Let `git loom update` remove merged and gone branches without asking |
| `loom.changeId` | `true`, `false` | `true` | Add a `Change-Id` trailer to the commits loom creates |

### `loom.remote-type`

`git loom push` detects the forge from the remote URL (`github.com`, `gitlab`, `dev.azure.com`, an `ssh://` URL on Gerrit's port 29418) or a Gerrit `commit-msg` hook. When it can't tell, it asks. Either way the result is saved here; set it yourself if detection guessed wrong:

```bash
git config loom.remote-type gitlab
```

See [push](commands/push.md) for what each type does.

### `loom.push-remote`

By default, `git loom push` pushes to the remote your integration branch tracks. On GitHub, if that remote is `upstream` and an `origin` exists, it pushes to `origin`, your fork. Set this for any other layout:

```bash
git config loom.push-remote personal
```

### `loom.hideBranchPattern`

Branches whose names start with this prefix are hidden from `git loom status`, commits included, unless you pass `--all`. Use it for local-only work such as personal configuration. Every other command still accepts them, but `push` never publishes them.

```bash
git config loom.hideBranchPattern "secret-"   # hide secret-* instead of local-*
git config loom.hideBranchPattern ""          # hide nothing
```

### `loom.statusContext`

How much history to show at and before the base: `1` shows the base alone, `3` adds the two commits before it, dimmed. `git loom status -n` overrides it for one run, and `+`/`-` change it live in `git loom tui`.

### `loom.pruneGoneBranches`

When `true`, `git loom update` removes branches that were fully merged upstream, or whose remote branch was deleted, without asking — as `update -y` does.

### `loom.changeId`

Commits created by `commit`, `split` and `reword` get a Gerrit-style `Change-Id` trailer, which gives them [short IDs](commands/status.md#short-ids) that survive rewrites. Set it to `false` to create commits without one.

Gerrit's own settings are honored: `gerrit.createChangeId false` also turns it off, and `gerrit.reviewUrl` switches to Gerrit's `Link:` form. A commit that already has a Change-Id, from a `commit-msg` hook for example, keeps it.

> [!NOTE]
> When you write the message in your editor, loom adds the trailer by amending the commit afterwards. That amend uses your git config, not options passed after `--`: a commit signed with a forwarded `-S` loses its signature. Use `commit.gpgsign`, or `-m`.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `NO_COLOR` | Disable colored output when set (see [no-color.org](https://no-color.org/)) |
| `TERM` | `dumb` disables colored output |
| `LOOM_AGENT` | Any non-empty value other than `0` turns on [agent mode](commands/agent.md#agent-mode), like `--agent` |

## Global Flags

These go before or after the command.

| Flag | Description |
|------|-------------|
| `--no-color` | Disable colored output |
| `--theme <auto\|dark\|light>` | Colors for a dark or light terminal background. `auto` (the default) detects it, and falls back to dark. |
| `--agent` | Machine-readable output for AI agents and scripts — see [agent mode](commands/agent.md#agent-mode) |
