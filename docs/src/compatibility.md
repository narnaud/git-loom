# Compatibility

From 1.0, git-loom follows [semantic versioning](https://semver.org/). What this page calls stable changes only in a major release; minor releases add features, and patch releases fix bugs.

## Stable

| What | Promise |
|------|---------|
| Commands and options | Every command, alias and option that `--help` or these docs show keeps its spelling and meaning. A renamed one keeps its old spelling, hidden, until the next major release. |
| [Configuration](configuration.md) | The `loom.*` settings and the values they accept, and the `LOOM_AGENT` variable. |
| Commit identity | The `Change-Id` trailer loom writes (`Change-Id: I<40 hex>`, or Gerrit's `Link:` form), and the [persistent ID](commands/status.md#short-ids) spelled from it: a commit keeps the same letters across releases. |
| Requirements | Git 2.43 or later. A minor release can raise the minimum to a later Git 2.x; requiring Git 3 waits for a major release, git-loom 2.0 at the earliest. |
| [Agent mode](commands/agent.md#agent-mode) | As described below. |

## Agent Mode

Agent mode is read by tools, so it is stable like the CLI, except the status graph, which has its own version number.

Stable like the CLI:

- `--agent` and `LOOM_AGENT` turn it on.
- stdout ends with one JSON object, on its last line.
- The `status` values (`ok`, `needs_input`, `needs_confirmation`, `paused`, `error`), their fields, and the exit codes 0, 1, 2 and 10.
- The hunk listing that `-p` answers with (`items`, `fingerprint`), and re-running with `--hunks` and `--hunks-from`.
- The `push` report.
- How `messages` names a commit that a command created or rewrote: `` `pop` (1a2b3c4) `` and `` `pop` (was: 1a2b3c4, now: 5d6e7f8) ``, or `` `1a2b3c4` `` and `` `1a2b3c4` (now `5d6e7f8`) `` for a commit without a Change-Id. The rest of a message's wording can change.

Versioned by `schema`:

- The `graph` object of `git loom status --agent`. Its `schema` number goes up when a field is removed or renamed, or a value changes meaning. That can happen in a minor release, and the release notes say so. Read `schema` and stop on a number you don't know.

In both:

- New fields can appear in any release: ignore the ones you don't know.
- A new `status` value or exit code comes only in a major release.

## Not Covered

These can change in any release:

- Human output: the status tree, messages, prompts, colors, and the TUI's layout and keys.
- Which short ID a branch, a file or a commit without a Change-Id gets, and how many of a persistent ID's letters `status` shows.
- In agent mode, everything on stderr.
- The output of `git loom trace`, and that of `show` and `diff`, which is git's.
- The [agent skill](commands/agent.md): `git loom agent install` rewrites it for each release.
- A paused operation: finish it with `git loom continue` or `git loom abort` before upgrading.
- Options passed to git after `--`, which loom hands on unchecked.
- The Rust version needed to build from source.
