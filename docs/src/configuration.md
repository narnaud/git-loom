# Configuration

## Git Config Settings

| Setting | Values | Default | Description |
|---------|--------|---------|-------------|
| `loom.remote-type` | `github`, `azure`, `gerrit` | Auto-detected | Override the remote type for `git loom push` |
| `loom.push-remote` | Any remote name | Auto-detected | Override which remote to push to (e.g., `personal` for fork workflows) |
| `loom.hideBranchPattern` | Any prefix string | `local-` | Prefix for branches hidden from `loom status` by default |

### `loom.remote-type`

By default, `git loom push` auto-detects the remote type:

- **GitHub** — if the remote URL contains `github.com`
- **Azure DevOps** — if the remote URL contains `dev.azure.com`
- **Gerrit** — if `.git/hooks/commit-msg` contains "gerrit"
- **Plain Git** — otherwise

You can override this with:

```bash
git config loom.remote-type github   # Force GitHub push (push + open PR)
git config loom.remote-type azure    # Force Azure DevOps push (push + open PR)
git config loom.remote-type gerrit   # Force Gerrit push (refs/for/<branch>)
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

Hidden branches remain fully accessible to all other loom commands (`fold`, `drop`, `commit`, `push`, etc.).

When creating or renaming a branch to a name that matches this prefix, *git-loom* prints a warning.

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
