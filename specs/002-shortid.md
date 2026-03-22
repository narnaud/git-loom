# Spec 002: Short IDs

## Overview

Every entity displayed in the status output receives a unique **short ID**:
a compact, human-friendly identifier inspired by jujutsu and Git Butler CLI.
Short IDs let users refer to branches, commits, and files by typing just a
couple of characters, enabling future interactive commands such as
`git-loom amend <id>` or `git-loom goto <id>`.

## Entity Types

| Entity | Candidate source | Default ID | Example |
|--------|-----------------|------------|---------|
| Local changes | `zz` (hardcoded) | `zz` | always `zz` |
| Branch | Word-based candidates | First letter of each word | `feature-a` → `fa` |
| Commit | Full hex hash (40 chars) | First 2 hex chars | `d0472f9…` → `d0` |
| File | Word-based candidates from stem | First letter of each word | `new_file.txt` → `nf` |

Files use the **file stem** (filename without extension) as source so that
IDs are based on meaningful parts, not file extensions. The full filename
(not the full path) is used so that IDs remain stable across renames within
different directories.

### Candidate generation

For branches and files, the algorithm generates an ordered list of 2-char
candidate IDs, trying to stay at 2 characters as long as possible:

**Multi-word names** (split on `-`, `_`, `/`): generate all combinations of
one character from the first word and one character from the second word.
The first candidate is the first letter of each word (the "initials"):

```
feature-alpha    → candidates: fa, fe, ft, fu, fr, ea, ee, …
feature-awesome  → candidates: fa, fe, fs, fo, fm, ea, ee, …
```

**Single-word names**: generate all character pairs (i, j) where i < j:

```
main        → candidates: ma, mi, mn, ai, an, in, …
master      → candidates: ma, ms, mt, me, mr, as, at, …
```

**Single-character names** (e.g. a file named `a`): the character is doubled
to meet the 2-char minimum: `a` → `aa`.

**Commits**: successive hex prefixes (2, 3, 4, … chars). These are the only
entities likely to require 3+ character IDs.

### 3-char fallback

If all 2-char candidates are exhausted (unlikely for branches and files),
the algorithm falls back to 3+ character prefixes.

## Allocation Order

Entities are allocated IDs in priority order, not insertion order:

1. **Unstaged** (always `zz`)
2. **Commits** — allocated first because they have the most constrained
   candidate set (hex prefixes only). This ensures commits keep their
   natural 2-char hex prefix unless two commits collide.
3. **Branches and Files** — allocated last. Their rich word-based candidate
   sets make it easy to find alternatives around already-assigned commit IDs.

Within each priority group, the original entity order is preserved (stable
sort) for determinism.

## Collision Resolution

IDs are assigned greedily in priority order. Each entity receives its
first candidate that is not already taken by another entity. Only **exact
ID matches** are considered collisions — sharing a first letter is fine as
long as the full 2-character IDs are distinct.

```
feature-a        →  fa  ✓ (first candidate, not taken)
feature-b        →  fb  ✓ (first candidate 'fb' ≠ 'fa', not taken)
```

When two entities share the same first candidate, the second entity moves
to its next candidate:

```
feature-alpha    →  fa  ✓ (first candidate, not taken)
feature-awesome  →  fe  ✓ ('fa' taken, next candidate 'fe' is available)
```

Single-word names work the same way:

```
main             →  ma  ✓ (not taken)
mainstream       →  mi  ✓ ('ma' taken, next candidate 'mi' is available)
master           →  ms  ✓ ('ma' taken, next candidate 'ms' is available)
```

**Fallback:** If all candidates are exhausted (e.g. two entities with
identical source text), a numeric suffix is appended: `ab`, `ab1`, `ab2`, etc.

Collision resolution operates **globally** across all entity types. A branch
named `main` and a file named `main.rs` both generate `ma` as their first
candidate; the first gets `ma`, the second gets `mi`.

## Display Format

Short IDs are rendered in **blue with underline** (`COLOR_SHORTID`). Their
position varies by entity type:

### Local changes header

The short ID `zz` appears between the graph connector and the label:

```
╭─ zz [local changes]
│   no changes
│
```

### Files (in the local section)

The short ID appears before the status character:

```
╭─ zz [local changes]
│   ma M src/main.rs
│   nf A new_file.txt
│
```

### Branches

The short ID appears before the branch name brackets:

```
│╭─ fa [feature-a]
│●   d072f9 Fix bug
├╯
```

### Commits

The short ID **replaces** the beginning of the displayed hash. The first N
characters (matching the short ID length) are shown in blue with underline;
the remaining characters of the abbreviated hash are dimmed:

```
│●   d072f9 Fix bug
      ^^^^^^
      ││││└─ dimmed (rest of short hash)
      │└──── dimmed (rest of short hash)
      └───── blue + underline (short ID = "d0")
```

In plain text (ANSI stripped), the full abbreviated hash is preserved:

```
│●   d072f9 Fix bug
```

### Upstream marker

The upstream / common base line does **not** receive a short ID since it is
not an actionable entity.

## Complete Example

```
╭─ zz [local changes]
│   ma M src/main.rs
│   nf A new_file.txt
│
│╭─ fa [feature-a]
│●   2ee61e1 Add feature A
├╯
│
│╭─ fb [feature-b]
│●   d0472f9 Fix bug in feature B
│●   7a067a9 Start feature B
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

In this example:
- `feature-a` gets `fa` (first letters: 'f' + 'a')
- `feature-b` gets `fb` (first letters: 'f' + 'b' — same first letter is fine)
- `main.rs` gets `ma` (first two letters)
- `new_file.txt` gets `nf` (first letters of words in stem: 'n' + 'f')

## Properties

- **Deterministic:** same repository state produces the same short IDs.
- **No persistence:** IDs are recomputed on every invocation. No temp files
  or caches are needed.
- **Stable across changes:** file IDs use the filename (not path), so moving
  a file between directories preserves its ID. Branch and commit IDs are
  derived from stable sources (name and hash respectively).
- **Minimal length:** IDs are always at least 2 characters. Branches and
  files target 2 characters using word structure and greedy candidate
  selection. Only commits (hex hashes) may require 3+ characters.

## Resolving Short IDs

Short IDs are designed for display **and** for user input. Commands that accept
entity identifiers (like `git-loom reword`) can resolve both git references and
short IDs using shared resolution logic.

### Resolution Strategy

Resolution tries multiple strategies in order:

**1. Git native references (tried first)**

Checks if the target is a valid git object:

- Full commit hashes: `abc123def456...`
- Partial hashes: `abc123` (minimum 4 characters)
- Symbolic refs: `HEAD`, `HEAD~2`, `main`, `origin/main`
- Any valid git revision syntax

If resolved, returns the target as a commit.

**2. Short ID lookup (tried second)**

If git resolution fails, the input is treated as a short ID. The full
commit graph is built using the same algorithm as status rendering, and
the short ID is searched in this order:

1. **Branches** - if match found, resolves to a branch
2. **Commits** - if match found, resolves to a commit
3. **Files** - if match found, resolves to a file

If no match is found, an error suggests the user run `git-loom status`
to see available IDs.

### Why This Order?

Git references are checked first because:

- **Speed**: `revparse_single` is instant; short ID resolution requires
  building the entire commit graph
- **Universality**: works in any repository state, not just when upstream
  is configured
- **Compatibility**: supports any git syntax users already know

Short IDs are a convenience layer on top of standard git operations, not a
replacement.

### Consistency Guarantees

The resolution system ensures that **what you see is what you type**:

- Both `git-loom status` and resolution use the same entity ordering
  and the same collision resolution algorithm
- A short ID visible in status output will always resolve to the same entity
- IDs are recomputed on every invocation (no stale caches)

### Prerequisites for Short ID Resolution

- Current branch must have upstream tracking configured
- Repository must be in a state where `gather_repo_info()` succeeds
- Target must match a short ID shown in `git-loom status`

Git reference resolution has no prerequisites (works in any repository).

### Argument resolution: `resolve_arg()`

All commands resolve user-provided arguments through `git::resolve_arg(repo, arg, accept)`.
The `accept` parameter is a `&[TargetKind]` slice specifying which kinds of targets the
command accepts and in what priority order. Resolution strategies are only attempted for
the listed kinds. Merge commits are automatically rejected when resolving `Commit` targets.
CWD-relative path conversion is handled internally for `File` targets.

Available `TargetKind` values:
- `File` — a working-tree file path (CWD-relative → repo-relative conversion applied)
- `Branch` — a local branch name
- `Commit` — a non-merge commit (hash, `HEAD`, etc.; branch names are excluded)
- `CommitFile` — a commit-file reference (e.g. `02:0`)
- `Unstaged` — the unstaged working directory (`zz`)

## Design Decisions

- **Global collision resolution:** all entity types share one ID namespace.
  This avoids ambiguity when a future command receives an ID without an
  explicit type qualifier.
- **Exact-match collision only:** collisions are checked against the full
  ID string, not just the first letter. This keeps IDs intuitive — for
  example, `fix-merge` → `fm` and `fix-update` → `fu` rather than forcing
  `fix-update` to a less obvious ID like `iu`. Shared first letters are
  acceptable because the full 2-character ID is still unique and easy to type.
- **Word-based candidates:** splitting on `-`, `_`, `/` and generating
  character combinations from different words produces many 2-char options,
  keeping IDs short even when names share long prefixes.
- **Commit-first allocation:** commits are allocated before branches and
  files because hex prefixes are the most constrained candidate set. This
  prevents branches from "stealing" a commit's natural 2-char prefix.
- **Greedy assignment with smart ordering:** within each priority group,
  entities are processed in order, each receiving the first available
  candidate. This is simple, deterministic, and fast.
- **File stem over full filename:** using the stem (without extension)
  means IDs reflect meaningful name parts, not `.rs` or `.txt` suffixes.
- **Filename over full path:** gives shorter, more memorable IDs and
  better stability when files move between directories.
- **No persistence:** the repository state changes constantly; recomputing
  is fast and avoids stale-mapping bugs.
- **Reusable module:** the short ID system is independent of rendering so
  that future commands can resolve user-provided IDs to entities using the
  same algorithm.
- **Unstaged is always `zz`:** a fixed, memorable ID for the working tree
  section. The letters `zz` are unlikely to collide with branch/file/commit
  prefixes in practice.
