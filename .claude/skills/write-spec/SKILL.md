---
name: write-spec
description: Write a new spec or update an existing one for a git-loom command. Invoke as /write-spec <command> (e.g. /write-spec swap).
---

# Write or Update a Spec

For `/write-spec <command>`, follow established spec conventions. Specs are
AI context, not user documentation: minimize tokens while preserving every
normative behavior, invariant, edge case, exact error, and CLI contract.

## Concision

- Prefer dense tables and bullets; state each rule once.
- Omit background, repeated summaries, decorative output, and rationale that
   does not constrain implementation.
- Include only the smallest example that adds information not already stated.
- Cross-reference another spec instead of restating its rules.
- Expand only for new or previously undocumented normative behavior.

## Required reads

Read all before editing:

1. List `specs/`; read at least two structurally similar specs. Prefer rebase
   specs `008-drop.md` and `007-fold.md`, or simpler `014-continue-abort.md`
   and `003-reword.md`.
2. Read `src/<command>.rs`, relevant mutation methods in `src/core/weave.rs`, the
   command variant in `src/main.rs` (exact args, flags, aliases, help), and
   `src/core/transaction.rs` (`after_continue` registration means resumable conflict
   recovery).
3. Skim `src/<command>_test.rs` and relevant `src/weave_test.rs` scenarios for
   edge and error cases.
4. When updating, read `specs/NNN-<command>.md` completely.

New specs use `max(existing number) + 1`, zero-padded to three digits. Existing
specs retain accurate sections and change only what behavior requires.

## File and sections

Write `specs/NNN-<command>.md`. Include sections in this order; omit only when
genuinely inapplicable and say why inline.

| Section | Requirement |
|---|---|
| `# Spec NNN: <Title>` | Exact number and title, then a one-line `> **Normative.**` scope statement. |
| `## CLI` | Exact bash usage, then Arguments and (unless only global options) Flags with forms, defaults, and constraints. |
| `## Resolution` | Accepted target kinds and priority; refer to Spec 002. Omit without identifiers. Merge into a `## Resolution and dispatch` section when argument shape selects the operation. |
| Behavior sections | One per distinct target or mode, named for it (`## Commit target`, `## Branch targets`, `## Patch mode (-p)`). Compact tables or condition-named bullets covering every path, preservation rule, special case, delegation, and cleanup. Do not repeat generic invariants. |
| Diagnostics | A `Condition` / `Exact error` table, inside the behavior section it belongs to or standalone. Quote errors verbatim from the implementation. |
| Conflict recovery | For rebase commands, state resumable `loom continue`/`loom abort` or hard-fail auto-abort. If resumable, document the persisted recovery-context JSON fields and successful post-continue behavior. |
| `## Prerequisites` | Hard requirements only. May be folded into a trailing invariants section. |
| `## Examples` | Optional; include only cases that clarify behavior the rules cannot express as compactly. |

Section names above are the default shape, not a fixed list: name behavior
sections after what they govern and follow the reference specs you read.

Write present-tense user-visible behavior. Use `git-loom` in code and loom in
prose. Mention short IDs for every accepting argument. Quote exact errors in
backticks, verbatim from the implementation. Name an internal identifier only
when it is the contract itself — a persisted JSON field or `op` value, or a
named shared step the spec must pin down; never to describe implementation.
Cross-reference as `see Spec NNN`.

## Output

- New: write the full spec, then add its numeric-order row under the Specs table
  in `CLAUDE.md`:
  `| \`specs/NNN-<command>.md\` | <one-line description> |`
- Update: write the revised spec, then summarize what changed and why in 2-3
  sentences.
- Always confirm the spec number and path.
