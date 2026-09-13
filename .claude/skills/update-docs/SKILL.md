---
name: update-docs
description: Create or update the user-facing documentation for a git-loom command. Invoke as /update-docs <command> (e.g. /update-docs swap). Reads the spec and implementation, then writes docs/src/commands/<command>.md and updates SUMMARY.md if needed.
---

# Update Command Documentation

For `/update-docs <command>`, follow existing command-page conventions.

## Required reads

Read all before editing:

1. `specs/0NN-<command>.md` (list `specs/` if needed): behavioral authority.
2. `src/<command>.rs` and its `src/main.rs` variant: exact arguments, flags,
   aliases, help, and `GROUPED_COMMANDS`.
3. Existing `docs/src/commands/<command>.md`, if any.
4. Two structurally similar pages: prefer `drop.md` plus `reword.md` or `show.md`.
5. `docs/src/SUMMARY.md` for presence and insertion point.
6. The `## Commands` block in `README.md` and opening block in
   `docs/src/commands/README.md`.

## Page action and shape

- New: create `docs/src/commands/<command>.md`; add it alphabetically under
  `# Commands` in `docs/src/SUMMARY.md` as
  `- [<command>](commands/<command>.md)`.
- Existing: preserve accurate sections; change only stale content.

Use these sections in order; omit only where stated:

| Section | Requirement |
|---|---|
| `# <command>` | One-sentence purpose. |
| `## Usage` | Exact `git-loom <command> [options] <args>` block; add 1-2 sentences only for non-obvious argument modes. |
| `### Arguments` | Table of argument and accepted forms. |
| `### Options` | Exact flags; omit if only global flags exist. |
| `## What It Does` | One condition-named subsection per behavior, 1-3 sentences. No spec-style change/preservation blocks. For dispatch commands such as fold, use `## Type Dispatch` table then `## Actions`. |
| `## Target Resolution` | Numbered priority list; omit without identifiers. |
| `## Examples` | One titled subsection per use case: bash invocation, then a `# comment` explaining the result; match `drop.md`. |
| `## Conflicts` | Only for rebase commands: paused output with `# !` warning comments, continue command, non-resumable auto-abort operations, and final link to [continue](continue.md) and [abort](abort.md). |
| `## Prerequisites` | Hard requirements only. |

Style: terse, practical, present tense; `git-loom` in code and loom in prose.
Treat short IDs as first-class in argument descriptions. Copy CLI names,
aliases, and defaults verbatim from `src/main.rs`. Exclude design rationale.
Use `# !` warnings and `# ✓` successes in conflict blocks. Use relative links.

## Command lists (always)

Regenerate both from `GROUPED_COMMANDS`, even when updating an existing page:

- In `README.md`, replace the complete code block below `## Commands` with the
  grouped list, preserving category headings/spacing. Exclude `Usage:` and
  `Options:`.
- In `docs/src/commands/README.md`, replace only the opening code block with:

```text
Usage: git-loom [OPTIONS] [COMMAND]

<categories and commands from GROUPED_COMMANDS>

Options:
      --no-color       Disable colored output
      --theme <THEME>  Color theme for graph output [default: auto] [possible values: auto, dark, light]
  -h, --help           Print help (see more with '--help')
  -V, --version        Print version
```

Leave subsequent prose unchanged.

## Output

Write the command page and both command-list files; write `SUMMARY.md` only for
a new listing. Confirm every written path. Remind the user that `docs/book/` is
not regenerated automatically and `mdbook build docs` rebuilds it.
