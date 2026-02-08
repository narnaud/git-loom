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
| Unstaged changes | `zz` (hardcoded) | `zz` | always `zz` |
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

**Multi-word names** (split on `-`, `_`, `/`): pick one character from each
of two words. The first candidate is the first letter of each word (the
"initials"). On collision, alternative character positions are tried:

```
feature-alpha    → candidates: fa, fl, fp, fh, ea, el, …
feature-awesome  → candidates: fa, fw, fe, fs, ea, ew, …
```

**Single-word names**: pick character pairs (i, j) where i < j:

```
main        → candidates: ma, mi, mn, ai, an, in, …
master      → candidates: ma, ms, mt, me, mr, as, at, …
```

**Commits**: successive hex prefixes (2, 3, 4, … chars). These are the only
entities likely to require 3+ character IDs.

### 3-char fallback

If all 2-char candidates are exhausted (unlikely for branches and files),
the algorithm falls back to 3+ character prefixes built by interleaving
characters from each word round-robin.

## Collision Resolution

IDs are assigned greedily in entity order. Each entity receives its
highest-priority available candidate — the first one not already taken by
a previous entity.

```
feature-a        →  fa  ✓ (first candidate, available)
feature-b        →  fb  ✓ (first candidate, available)
```

When initials collide, the second entity picks an alternative 2-char ID:

```
feature-alpha    →  fa  ✓ (first candidate, available)
feature-awesome  →  fw  ✓ (fa taken → next candidate "fw")
```

Single-word collisions work the same way:

```
main             →  ma  ✓
master           →  ms  ✓ (ma taken → next candidate "ms")
maintenance      →  mi  ✓ (ma taken → next candidate "mi")
```

**Fallback:** if all candidates are exhausted (e.g. two entities with
identical source text), a numeric suffix is appended: `ab`, `ab1`, `ab2`,
etc.

Collision resolution operates **globally** across all entity types. A branch
named `main` and a file named `main.rs` both start as `ma`; the second one
picks an alternative 2-char ID like `mi`.

## Display Format

Short IDs are rendered in **blue with underline** (`COLOR_SHORTID`). Their
position varies by entity type:

### Unstaged changes header

The short ID `zz` appears between the graph connector and the label:

```
╭─ zz [unstaged changes]
│   no changes
│
```

### Files (in the unstaged section)

The short ID appears before the status character:

```
╭─ zz [unstaged changes]
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
╭─ zz [unstaged changes]
│   ma M src/main.rs
│   nf A new_file.txt
│
│╭─ fb [feature-b]
│●   d0472f9 Fix bug in feature B
│●   7a067a9 Start feature B
├╯
│
│╭─ fa [feature-a]
│●   2ee61e1 Add feature A
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

In this example, `feature-a` and `feature-b` resolve immediately to `fa`
and `fb` (first letter of each word: `f`+`a` vs `f`+`b`).

## Architecture

### Module: `shortid.rs`

The short ID system lives in a dedicated `shortid` module, separate from
rendering logic. This enables reuse by future commands.

**Public types:**

- `Entity` — enum with variants `Unstaged`, `Branch(String)`,
  `Commit(git2::Oid)`, `File(String)`.
- `IdAllocator` — computes and stores the mapping from entities to short IDs.

**Public API:**

```rust
impl IdAllocator {
    pub fn new(entities: Vec<Entity>) -> Self;
    pub fn get_unstaged(&self) -> &str;
    pub fn get_branch(&self, name: &str) -> &str;
    pub fn get_commit(&self, oid: git2::Oid) -> &str;
    pub fn get_file(&self, path: &str) -> &str;
}
```

### Integration with rendering (`graph.rs`)

During rendering, all entities are collected from the built sections and
passed to `IdAllocator::new()`. The allocator is then threaded through each
`render_*` function, which calls the appropriate getter to display the ID.

```
RepoInfo → build_sections() → Vec<Section>
    ↓
collect entities from sections → Vec<Entity>
    ↓
IdAllocator::new(entities) → collision-free mapping
    ↓
render_*() functions look up IDs and format output
```

### Color constant

```rust
const COLOR_SHORTID: Color = Color::Blue;
```

Added to the color palette in `graph.rs`. Short IDs are always rendered with
`.color(COLOR_SHORTID).underline()`.

## Properties

- **Deterministic:** same repository state produces the same short IDs.
- **No persistence:** IDs are recomputed on every invocation. No temp files
  or caches are needed.
- **Stable across changes:** file IDs use the filename (not path), so moving
  a file between directories preserves its ID. Branch and commit IDs are
  derived from stable sources (name and hash respectively).
- **Minimal length:** IDs target 2 characters for branches and files,
  using word structure and greedy candidate selection to avoid unnecessary
  extension. Only commits (hex hashes) may require 3+ characters.

## Design Decisions

- **Global collision resolution:** all entity types share one ID namespace.
  This avoids ambiguity when a future command receives an ID without an
  explicit type qualifier.
- **Word-based candidates:** splitting on `-`, `_`, `/` and generating
  character combinations from different words produces many 2-char options,
  keeping IDs short even when names share long prefixes.
- **Greedy assignment:** entities are processed in order, each receiving
  the first available candidate. This is simple, deterministic, and fast.
- **File stem over full filename:** using the stem (without extension)
  means IDs reflect meaningful name parts, not `.rs` or `.txt` suffixes.
- **Filename over full path:** gives shorter, more memorable IDs and
  better stability when files move between directories.
- **No persistence:** the repository state changes constantly; recomputing
  is fast and avoids stale-mapping bugs.
- **Reusable module:** `shortid.rs` is independent of rendering so that
  future commands can resolve user-provided IDs to entities using the same
  allocator.
- **Unstaged is always `zz`:** a fixed, memorable ID for the working tree
  section. The letters `zz` are unlikely to collide with branch/file/commit
  prefixes in practice.
