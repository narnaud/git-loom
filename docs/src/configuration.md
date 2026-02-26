# Configuration

## Git Config Settings

| Setting | Values | Default | Description |
|---------|--------|---------|-------------|
| `loom.remote-type` | `github`, `gerrit` | Auto-detected | Override the remote type for `git loom push` |

### `loom.remote-type`

By default, `git loom push` auto-detects the remote type:

- **GitHub** — if the remote URL contains `github.com`
- **Gerrit** — if `.git/hooks/commit-msg` contains "gerrit"
- **Plain Git** — otherwise

You can override this with:

```bash
git config loom.remote-type github   # Force GitHub push (push + open PR)
git config loom.remote-type gerrit   # Force Gerrit push (refs/for/<branch>)
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `NO_COLOR` | Disable colored output when set (follows the [NO_COLOR](https://no-color.org/) standard) |
| `TERM` | Colors are automatically disabled when `TERM=dumb` |

## CLI Flags

| Flag | Description |
|------|-------------|
| `--no-color` | Disable colored output |
