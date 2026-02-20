# Spec 004: Weave

## Overview

The **Weave** module (`src/weave.rs`) is the heart of git-loom's history
rewriting. It provides a structured graph model of the integration branch
topology and a set of pure mutation methods. All commands that modify history
(branch, commit, drop, fold, reword) follow the same pattern:

1. **Build** a `Weave` from the repository state
2. **Mutate** the graph (drop a commit, move a commit, fixup, etc.)
3. **Serialize** the graph to a git rebase todo file
4. **Execute** a single interactive rebase with the pre-generated todo

This replaces the earlier approach of parsing and manipulating git's generated
todo file with string operations. Instead, git-loom generates the entire todo
from scratch based on the graph, making the process robust and predictable.

## Data Model

### `Weave`

The top-level struct representing the integration branch topology:

```rust
pub struct Weave {
    pub base_oid: Oid,                        // merge-base ("onto")
    pub branch_sections: Vec<BranchSection>,  // woven branches (dependency order)
    pub integration_line: Vec<IntegrationEntry>,  // first-parent line entries
}
```

### `BranchSection`

A woven branch section. Each section corresponds to a side branch that is
merged into the integration line via a merge commit.

```rust
pub struct BranchSection {
    pub reset_target: String,       // "onto" or another branch label
    pub commits: Vec<CommitEntry>,  // oldest first
    pub label: String,              // canonical name for the section
    pub branch_names: Vec<String>,  // all refs at this tip (co-located)
}
```

When multiple branches point to the same tip (co-located branches), they
share a single `BranchSection` with multiple entries in `branch_names`.

### `CommitEntry`

A single commit in the todo file:

```rust
pub struct CommitEntry {
    pub oid: Oid,
    pub short_hash: String,
    pub message: String,
    pub command: Command,
    pub update_refs: Vec<String>,  // non-woven branch names at this commit
}
```

### `Command`

The rebase todo command for a commit:

| Variant | Serializes to | Used by |
|---------|---------------|---------|
| `Pick` | `pick` | Default for all commits |
| `Edit` | `edit` | `reword` (pauses rebase at commit) |
| `Fixup` | `fixup` | `fold` (absorbs into previous commit) |

### `IntegrationEntry`

An entry on the integration (first-parent) line:

| Variant | Meaning |
|---------|---------|
| `Pick(CommitEntry)` | A regular commit on the integration line |
| `Merge { original_oid, label }` | A merge point referencing a branch section |

For existing merge commits, `original_oid` is `Some(oid)` and the serialized
line uses `merge -C <hash> <label>` to preserve the original merge message.
For newly created merges (e.g., from `weave_branch`), `original_oid` is
`None` and the line is `merge <label>`.

## Building the Graph

### `Weave::from_repo(repo)`

Constructs the graph from the current repository state by walking the
first-parent line from HEAD to the merge-base:

1. For each **merge commit**: identify the branch (second parent), walk
   backward to collect the branch's commits, match branch refs by tip OID,
   and create a `BranchSection` + `Merge` entry on the integration line.
2. For each **regular commit**: create a `Pick` entry on the integration
   line. If a branch ref points at this commit, add its name to `update_refs`.
3. Branches at the merge-base with no commits are skipped.

The walk uses two helpers:

- `walk_first_parent_line(repo, head, stop)` — walks from HEAD following
  only first parents, collecting both regular and merge commits in
  oldest-first order.
- `walk_branch_commits(repo, tip, stop)` — walks a side branch from its
  tip back to the merge-base, collecting non-merge commits.

### Prerequisites

`from_repo` requires an integration branch with upstream tracking configured
(it calls `gather_repo_info`). For non-integration repos, callers should
handle the error and fall back to a simpler approach (see `reword.rs`).

## Mutation Methods

All mutations are pure operations on the in-memory graph. They do not touch
the repository.

### `drop_commit(oid)`

Remove a commit from the graph. Searches branch sections first, then the
integration line. If the removed commit was the last one in a branch section,
the section and its merge entry are also removed.

Used by: `drop` (commit drop)

### `drop_branch(branch_name)`

Remove an entire branch section and its merge entry. Matches by branch name
in `branch_names` or by section `label`.

Used by: `drop` (branch drop)

### `move_commit(oid, to_branch)`

Move a commit to the tip of a target branch section. The commit is removed
from its current location (branch section or integration line) and appended
to the target section.

**Co-located branch handling:** When the target branch shares a section with
other branches (co-located), the section is split. The original section keeps
the remaining branches and all existing commits. A new stacked section is
created for the target branch, containing only the moved commit, with its
`reset_target` pointing to the original section's label. The integration
line's merge entry is updated to reference the outermost (stacked) section.

Used by: `fold` (commit to branch), `commit` (move to feature branch)

### `fixup_commit(source_oid, target_oid)`

Remove the source commit from its current location, change its command to
`Fixup`, and insert it immediately after the target commit. The source's
changes are absorbed into the target during rebase.

Used by: `fold` (commit into commit, file into non-HEAD commit)

### `edit_commit(oid)`

Change a commit's command to `Edit`, causing the rebase to pause at that
commit. Used with a subsequent `git commit --amend` and `git rebase
--continue`.

Used by: `reword` (pause at commit for message editing)

### `add_branch_section(label, branch_names, commits, reset_target)`

Add a new branch section to the graph. Used when creating merge topology for
a branch that has no section yet (e.g., a newly created empty branch).

Used by: `commit` (empty branch path)

### `add_merge(label, original_oid, position)`

Add a merge entry on the integration line. If `position` is `None`, appends
at the end. If `Some(idx)`, inserts at that index.

Used by: `commit` (empty branch path)

### `weave_branch(branch_name)`

Convert a non-woven branch (commits sitting on the integration line with an
`update_ref`) into a woven branch. Collects all integration line picks from
the start up to and including the branch tip, moves them into a new branch
section, and adds a merge entry.

Used by: `branch` (weave on creation)

### `reassign_branch(drop_branch, keep_branch)`

Reassign a branch section from one branch to another. Renames the section's
label and merge entry, removes the dropped branch from `branch_names`, and
ensures the keep branch is present. Used when dropping a co-located woven
branch.

Used by: `drop` (co-located woven branch)

## Serialization

### `Weave::to_todo()`

Serializes the graph to a git rebase `--rebase-merges` todo file:

```
label onto

reset <reset_target>
pick <hash> <message>
update-ref refs/heads/<non-woven-branch>
label <section-label>
update-ref refs/heads/<branch-name>

reset onto
pick <hash> <message>
update-ref refs/heads/<non-woven-branch>
merge -C <hash> <label> # Merge branch '<label>'
merge <label> # Merge branch '<label>'
```

Key serialization rules:

- Branch sections are emitted first, in dependency order
- Each section starts with `reset <target>` and ends with `label <name>`
  followed by `update-ref` lines for all `branch_names`
- The integration line follows, starting with `reset onto`
- Existing merges use `merge -C <oid> <label>` to preserve the message
- New merges use `merge <label>` (git generates a default message)
- Non-woven branches at specific commits emit `update-ref` lines after
  their pick line

## Execution

### `run_rebase(workdir, upstream, todo_content)`

Executes the pre-generated todo via interactive rebase:

1. Write `todo_content` to a temporary file
2. Set `GIT_SEQUENCE_EDITOR` to
   `git-loom internal-write-todo --source <temp_file>`
3. Set `GIT_EDITOR=true` to suppress editor prompts for new merge commits
4. Run `git rebase` with flags:
   - `--interactive` — enables the sequence editor
   - `--autostash` — stashes dirty working tree changes
   - `--keep-empty` — preserves empty commits
   - `--no-autosquash` — disables fixup!/squash! auto-reordering
   - `--rebase-merges` — preserves/creates merge topology
   - `--update-refs` — keeps branch refs up to date
5. On failure, automatically runs `git rebase --abort` before returning

The `upstream` parameter controls the rebase range:
- `Some(oid)` — rebases from `<oid>` (exclusive) to HEAD
- `None` — rebases from `--root`

### `InternalWriteTodo` Subcommand

```bash
git-loom internal-write-todo --source <path> <todo_file>
```

A hidden subcommand that copies the contents of `--source` to `<todo_file>`.
Git calls this as the `GIT_SEQUENCE_EDITOR` and appends the todo file path
as the final positional argument.

This is a simple file copy — no parsing, no transformation. The entire
intelligence is in the graph model and serializer; this subcommand is just
the delivery mechanism.

### Binary Path Resolution

During normal execution, `std::env::current_exe()` returns the git-loom
binary path. During `cargo test`, `current_exe()` returns the test harness
binary in `target/<profile>/deps/`.

The `loom_exe_path()` helper detects this: if the parent directory is named
`deps`, it looks one level up for the actual `git-loom` binary. This means
`cargo build && cargo test` is required after source changes.

## Integration with Commands

| Command | Weave mutations used |
|---------|---------------------|
| `branch` (Spec 005) | `weave_branch` |
| `commit` (Spec 006) | `add_branch_section` + `add_merge` (empty branch), `move_commit` |
| `drop` (Spec 008) | `drop_commit`, `drop_branch`, `reassign_branch` |
| `fold` (Spec 007) | `fixup_commit`, `move_commit` |
| `reword` (Spec 003) | `edit_commit` |

Commands that don't modify history (`status`, `init`, `update`, `reword`
for branch rename) do not use the Weave.

## Design Decisions

### Generate Todo From Scratch

Rather than parsing git's generated todo file and applying text-level surgery
(find a `pick` line, move it, insert `fixup`, remove sections), git-loom
generates the entire todo from the repository's commit graph. This eliminates
dependence on git's exact output format and makes operations composable —
multiple mutations can be applied to the graph before a single serialization.

### Always Rebase from Merge-Base

All Weave-based operations scope the rebase from the merge-base commit. This
means the full integration history is replayed. The trade-off is slightly
slower for large branches, but dramatically simpler — one graph covers the
entire topology.

### File-Based Todo Transfer

The generated todo is written to a temporary file, and `internal-write-todo`
copies it to git's todo file. This avoids command-line length limits and
encoding issues with large todo files.

### `GIT_EDITOR=true` for New Merges

New merge commits (those without `-C` in the todo) would normally prompt for
a merge message. Setting `GIT_EDITOR=true` suppresses this by using a no-op
command that leaves the default "Merge branch '...'" message intact. This
only affects the rebase process — not the user's shell when rebase pauses
at an `edit` command.

### Co-Located Branch Splitting

When moving a commit to a co-located branch (one that shares a section with
other branches), the section is split into a stacked topology. This ensures
the moved commit appears only on the target branch, not on all co-located
branches. The resulting topology has the original section as the base and a
new section stacked on top for the target branch.

### Automatic Abort on Failure

If the rebase fails, `run_rebase` automatically calls `git rebase --abort`
before returning the error. This ensures atomic operations: either the
rebase succeeds completely or the repository is left in its original state.

### Unix Shell Escaping on All Platforms

The `GIT_SEQUENCE_EDITOR` command string uses Unix-style shell escaping
(`shell_escape::unix::escape`) even on Windows. Git for Windows uses
MSYS2/bash to execute the sequence editor, not `cmd.exe` or PowerShell.
Binary paths are normalized to forward slashes for Git compatibility.

### Fallback for Non-Integration Repos

`reword` is the only command that can operate outside an integration branch
context (rewording commits on any branch). When `Weave::from_repo()` fails,
`reword` falls back to building a minimal linear todo by walking the
first-parent line from HEAD to the target commit's parent. This fallback
does not use the Weave data model.
