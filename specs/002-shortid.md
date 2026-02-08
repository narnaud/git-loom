# Spec 002: Short IDs

## Overview

Every entity displayed in the status output receives a unique **short ID**:
a compact, human-friendly identifier inspired by jujutsu and Git Butler CLI.
Short IDs let users refer to branches, commits, and files by typing just a
couple of characters, enabling future interactive commands such as
`git-loom amend <id>` or `git-loom goto <id>`.

## Entity Types

| Entity | Source string | Default ID | Example |
|--------|-------------|------------|---------|
| Unstaged changes | `zz` (hardcoded) | `zz` | always `zz` |
| Branch | Full branch name | First 2 chars of name | `feature-a` → `fe` |
| Commit | Full hex hash (40 chars) | First 2 hex chars | `d0472f9…` → `d0` |
| File | Filename (last path component) | First 2 chars of filename | `src/main.rs` → `ma` |

Files use the **filename** (not the full path) as source string so that
the short ID is more likely to remain stable across renames within different
directories.

## Collision Resolution

IDs start at 2 characters. When two or more entities share the same ID, all
colliding IDs are extended by one additional character from their source
string. This process repeats until every ID is unique.

```
feature-a  →  fe  (collision with feature-b)
feature-b  →  fe  (collision with feature-a)
             ↓ extend to 3 chars
feature-a  →  fea
feature-b  →  fea  (still collides)
             ↓ extend to 4 chars
feature-a  →  feat
feature-b  →  feat  (still collides)
             ↓ extend to 5 chars
feature-a  →  featu
feature-b  →  featu  (still collides)
             ↓ extend to 6 chars
feature-a  →  featur
feature-b  →  featur  (still collides)
             ↓ extend to 7 chars
feature-a  →  feature
feature-b  →  feature  (still collides)
             ↓ extend to 8 chars
feature-a  →  feature-
feature-b  →  feature-  (still collides)
             ↓ extend to 9 chars
feature-a  →  feature-a
feature-b  →  feature-b  ✓ unique
```

**Fallback:** if source strings are fully exhausted and collisions remain
(e.g. two entities with identical source text), a numeric suffix is appended:
`ab`, `ab1`, `ab2`, etc. The algorithm is bounded to at most 200 extension
rounds to guarantee termination.

Collision resolution operates **globally** across all entity types. A branch
named `main` and a file named `main.rs` both start as `ma` and will be
extended until they differ.

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
│   ne A new_file.txt
│
```

### Branches

The short ID appears before the branch name brackets:

```
│╭─ fe [feature-a]
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
│   ne A new_file.txt
│
│╭─ fe [feature-b]
│●   d0472f9 Fix bug in feature B
│●   7a067a9 Start feature B
├╯
│
│╭─ fea [feature-a]
│●   2ee61e1 Add feature A
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

In this example, `feature-b` and `feature-a` collide at 2 characters (`fe`),
so `feature-a` is extended to `fea` while `feature-b` keeps `fe` (after
extension to `fea` vs `feb`, they diverge at 3 characters — the first to
become unique at a given length keeps that length).

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
- **Minimal length:** IDs are as short as possible (starting at 2 characters)
  while remaining unique within a single status invocation.

## Design Decisions

- **Global collision resolution:** all entity types share one ID namespace.
  This avoids ambiguity when a future command receives an ID without an
  explicit type qualifier.
- **Filename over full path:** using just the filename gives shorter,
  more memorable IDs and better stability when files move between directories.
- **No persistence:** the repository state changes constantly; recomputing
  is fast and avoids stale-mapping bugs.
- **Reusable module:** `shortid.rs` is independent of rendering so that
  future commands can resolve user-provided IDs to entities using the same
  allocator.
- **Unstaged is always `zz`:** a fixed, memorable ID for the working tree
  section. The letters `zz` are unlikely to collide with branch/file/commit
  prefixes in practice.
