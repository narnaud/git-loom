---
name: update-docs
description: Create or update the user-facing documentation for a git-loom command. Invoke as /update-docs <command> (e.g. /update-docs swap). Reads the spec and implementation, then writes docs/src/commands/<command>.md and updates SUMMARY.md if needed.
---

# Create or Update Command Documentation

The user has invoked `/update-docs <command>`. Your job is to produce or update
the command's page in `docs/src/commands/` following the conventions of all
existing pages.

## Step 1 — Gather context (read ALL of these before writing anything)

1. **Spec**: find and read `specs/0NN-<command>.md`. If the number is unknown,
   list `specs/` to find it. This is the primary source of truth for behavior.

2. **Implementation**: read `src/<command>.rs` and find the command's variant
   in `src/main.rs` — extract the exact argument names, flags, aliases, and
   help text as they appear in the CLI.

3. **Existing doc page**: if `docs/src/commands/<command>.md` already exists,
   read it in full before deciding what to change.

4. **Format reference**: read two existing command pages that are structurally
   similar — prefer `docs/src/commands/drop.md` (a command with multiple
   target types and conflict recovery) and one simpler page such as
   `docs/src/commands/reword.md` or `docs/src/commands/show.md`.

5. **SUMMARY.md**: read `docs/src/SUMMARY.md` to check whether the command is
   already listed, and to find the right insertion point if not.

## Step 2 — Determine action

- **New page**: the command has no existing doc page — create
  `docs/src/commands/<command>.md` and add it to `SUMMARY.md` under
  `# Commands`, in alphabetical order among the other command entries.
- **Update existing page**: preserve sections that are still accurate; revise
  only what has changed.

## Step 3 — Write the doc page

### File path

```
docs/src/commands/<command>.md
```

### Required sections (in this order)

```markdown
# <command>

<One sentence: what the command does.>

## Usage

‍```
git-loom <command> [options] <args>
‍```

<One or two sentences if the argument interpretation isn't obvious
(e.g. last arg is target, or single-arg mode differs from multi-arg mode).>

### Arguments

| Argument | Description |
|----------|-------------|
| `<arg>` | description and accepted forms |

### Options

| Option | Description |
|--------|-------------|
| `-f, --flag` | description |

(Omit "Options" if the command has no flags beyond global ones.)

## What It Does

One subsection per distinct behavior path, named after the triggering condition
("When Target is a Commit", "When Arguments Are Branches", etc.).

Each subsection: 1–3 short sentences. No "What changes / What stays the same"
blocks — those belong in specs, not user docs.

(For commands with a dispatch table like `fold`, use "## Type Dispatch" with
the dispatch table instead, followed by "## Actions" with one subsection per
action type.)

## Target Resolution

Numbered list of resolution priority:
1. **Type name** — how it resolves
2. …

(Omit if the command takes no user-supplied identifiers.)

## Examples

One `### Title` subsection per distinct use case. Each example:
- A bash code block with the invocation
- A `# comment` on the line after explaining what happened

Mirror the style of `docs/src/commands/drop.md` exactly.

## Conflicts

If the command supports conflict recovery:
- Show the conflict pause output (as a bash code block with "# !" warning comments)
- Show the continue invocation
- Note which sub-operations do NOT support pause/resume (auto-abort)
- End with a cross-link: See [continue](continue.md) and [abort](abort.md) for details.

(Omit entirely if the command never runs a rebase.)

## Prerequisites

Bullet list: hard requirements only (git version, working tree, upstream
tracking config, etc.).
```

### Style conventions

- **Terse and practical** — user docs are a quick reference, not a spec. One
  sentence where specs use a paragraph. Tables where specs use bullet lists.
- **Present tense**: "Removes the commit", not "will remove".
- **"git-loom" in code blocks**, plain "loom" in prose.
- **Short IDs**: mention them in argument descriptions — they are first-class.
- **Exact CLI flags**: copy flag names, short aliases, and default values
  verbatim from `src/main.rs`.
- **No design rationale**: that belongs in specs. Docs answer "what does it
  do?" not "why does it work this way?".
- **Conflict output format**: use "# !" for warning lines, "# ✓" for success
  lines in bash code blocks, matching the existing style in `drop.md`.
- **Cross-links**: link to related commands using relative markdown links:
  `[continue](continue.md)`.

## Step 4 — Update SUMMARY.md (new pages only)

Add the new command to the `# Commands` section of `docs/src/SUMMARY.md` in
alphabetical order:

```markdown
- [<command>](commands/<command>.md)
```

## Step 5 — Output

- Write the doc file (create or overwrite).
- If SUMMARY.md was updated, write that file too.
- Confirm the file path(s) written.
- Remind the user that the built HTML under `docs/book/` is not regenerated
  automatically — run `mdbook build docs` to rebuild if needed.
