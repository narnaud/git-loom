---
name: write-spec
description: Write a new spec or update an existing one for a git-loom command. Invoke as /write-spec <command> (e.g. /write-spec swap).
---

# Write or Update a git-loom Spec

The user has invoked `/write-spec <command>`. Your job is to produce or update
the spec file for that command, following the conventions established by all
existing specs.

## Step 1 — Gather context (read ALL of these before writing anything)

1. **Existing specs**: list `specs/` to find all specs and determine the next
   available number. Read at least two specs that are structurally similar to
   the command being documented — prefer specs for commands that involve rebases
   (`008-drop.md`, `007-fold.md`) or are simpler (`014-continue-abort.md`,
   `003-reword.md`).

2. **Implementation**: read the source file(s) for the command:
   - Primary module: `src/<command>.rs`
   - Weave mutations it uses: read the relevant methods in `src/weave.rs`
   - CLI definition: find the command's variant in `src/main.rs` to get the
     exact argument names, flags, aliases, and help text
   - Transaction wiring: check `src/transaction.rs` for `after_continue`
     registration (indicates the command supports conflict recovery)

3. **Tests**: skim `src/<command>_test.rs` and `src/weave_test.rs` for the
   command — the test names and scenarios reveal edge cases and error paths
   that must appear in the spec.

4. **Existing spec** (if updating): read the current `specs/NNN-<command>.md`
   in full before deciding what to change.

## Step 2 — Determine action

- **New spec**: use the next available number (max existing number + 1, zero-padded to 3 digits).
- **Update existing spec**: preserve all sections that are still accurate;
  add, remove, or revise only what has changed.

## Step 3 — Write the spec

### File naming

```
specs/NNN-<command>.md
```

### Required sections (in this order)

Every spec must contain these sections. Omit a section only if it genuinely
does not apply, and note why inline.

```markdown
# Spec NNN: <Title>

## Overview

One short paragraph: what the command does and why it exists at a high level.

## Why <CommandName>?

Explain the user pain this command relieves. Show what raw git would require
and why that is inconvenient. Keep it concrete.

## CLI

‍```bash
git-loom <command> [options] <args>
‍```

**Arguments:**

- `<arg>`: description, accepted forms (hash, short ID, branch name, …)

**Flags:**

- `--flag` / `-f`: description, defaults, constraints

(Omit Flags if the command has none beyond the global `--no-color`/`--theme`.)

## What Happens

One or more sub-sections covering every distinct behavior path, named after
the condition that triggers them (e.g. "When Target is a Commit", "When
Branches Are Adjacent").

Each sub-section must state:

**What changes:**
- bullet list of observable effects

**What stays the same:**
- bullet list of things explicitly preserved

Include special cases, delegations (e.g. "If this is the only commit on a
branch, delegates to X"), and any auto-cleanup behavior.

## Target Resolution

How arguments are resolved via `resolve_arg()`. State the `accept` list and
the priority order. Reference Spec 002 for the algorithm.

(Omit if the command takes no user-supplied identifiers.)

## Conflict Recovery

State whether the command supports resumable conflict handling (`loom continue`
/ `loom abort`) or uses hard-fail (auto-abort on conflict).

If resumable, document:
- What `LoomState.context` contains (the JSON fields)
- What `after_continue` does when recovery succeeds

(Omit if the command never runs a rebase.)

## Prerequisites

Bulleted list of hard requirements: git version, working tree, upstream
tracking, etc.

## Examples

One `### Title` sub-section per distinct use case. Each example must show:
1. A `git-loom status` excerpt (or relevant state) to set the scene
2. The exact command invocation
3. A comment describing what happened

Use realistic but minimal scenarios. Mirror the output format used in
`008-drop.md` and `007-fold.md`.

## Design Decisions

One `### Title` sub-section per non-obvious decision. Explain what was chosen,
what the alternative was, and why the chosen approach is better for users.
```

### Style conventions

- **Present tense throughout**: "The commit is removed", not "will be removed".
- **"git-loom" in code blocks**, plain "loom" in prose.
- **Short IDs**: mention them whenever an argument accepts them; they are a
  first-class input form.
- **Error messages**: quote exact error strings using backticks, e.g.
  `"Branch '<name>' is not woven into the integration branch"`.
- **"What changes / What stays the same"**: every behavior sub-section that
  modifies history must have these two bullet blocks.
- **No implementation details**: do not mention Rust types, function names,
  or internal struct names. The spec describes user-visible behavior only.
- **Cross-references**: link to other specs with "see Spec NNN" (e.g.,
  "see Spec 002 for the resolution algorithm").

## Step 4 — Output

- **New spec**: write the complete file to `specs/NNN-<command>.md`. Then
  add a row to the spec table in `CLAUDE.md` (under `### Specs`, in numeric
  order): `| \`specs/NNN-<command>.md\` | <one-line description> |`
- **Update**: write the updated file. Summarize what changed and why in 2–3
  sentences after the file write.
- In both cases, confirm the spec number and file path at the end.
