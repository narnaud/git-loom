# Spec 004: Internal Sequence Editor

## Overview

git-loom uses a hidden `internal-sequence-edit` subcommand to act as its own
`GIT_SEQUENCE_EDITOR` during interactive rebases. Instead of generating
platform-specific shell commands (PowerShell on Windows, `sed` on Unix),
git-loom invokes itself to rewrite the rebase todo file. This gives a single,
portable, testable code path for all platforms.

## Why a Self-Invocation Approach?

Interactive rebase requires a `GIT_SEQUENCE_EDITOR` — a program that rewrites
the todo file before the rebase runs. The previous implementation used shell
commands:

- **Windows**: Created a temporary PowerShell script with
  `-ExecutionPolicy Bypass`, introducing a security surface and temp file
  cleanup concerns
- **Unix**: Used `sed -i` with a hash interpolated into the command string,
  risking shell injection if inputs were ever improperly sanitized

Both approaches had downsides:

| Concern              | Shell approach                     | Self-invocation          |
|----------------------|------------------------------------|--------------------------|
| Cross-platform       | Two code paths, different behavior | Single code path         |
| Security             | Shell injection / execution policy | No shell involved        |
| Testability          | Hard to unit-test shell scripts    | Standard Rust function   |
| Temp file cleanup    | PowerShell script left on disk     | No temp files            |
| Reusability          | Copy-paste for each new command    | Shared subcommand        |

Self-invocation eliminates all of these by keeping the logic in Rust.

## CLI

```bash
git-loom internal-sequence-edit --actions-json <json> <todo_file>
```

This subcommand is **hidden** from `--help` output. It is not intended for
direct user invocation — git calls it automatically as the sequence editor.

**Arguments:**

- `--actions-json <json>`: JSON-encoded list of rebase actions to apply
- `<todo_file>`: Path to the rebase todo file provided by git (positional,
  appended by git after the `GIT_SEQUENCE_EDITOR` command)

**Exit codes:**

- `0`: Success (actions applied)
- `1`: Error reading/writing the todo file, parsing JSON, or hash not found

## Rebase Actions

The `RebaseAction` enum defines all supported todo file transformations:

### `Edit`

Replaces `pick <hash>` with `edit <hash>` to stop the rebase at that commit.

```json
{"Edit": {"short_hash": "abc1234"}}
```

Used by the `reword` command to stop at a commit for message editing.

### `Fixup`

Moves a source commit immediately after a target commit and changes it from
`pick` to `fixup`, effectively folding the source into the target.

```json
{"Fixup": {"source_hash": "abc1234", "target_hash": "def5678"}}
```

Used by the `fold` command to amend files into an earlier commit.

### `Move`

Moves a commit to the tip of a branch section. With `--rebase-merges
--update-refs`, each branch section ends with a block of `update-ref` and
`label` directives (possibly separated by blank lines). The `Move` action
extracts this entire block, categorizes the lines, and re-inserts them in the
correct order so that:

- The target branch's `label` and `update-ref` come **after** the inserted
  commit (so the merge includes the commit and the branch pointer advances).
- Co-located branches' `update-ref` lines stay **before** the inserted commit
  (so they don't accidentally advance past it).

```json
{"Move": {"commit_hash": "abc1234", "before_label": "feature-a"}}
```

Used by the `fold` and `commit` commands to move a commit between branches.

### `Drop`

Removes a `pick <hash>` line entirely from the todo, dropping the commit from
history.

```json
{"Drop": {"short_hash": "abc1234"}}
```

Used by the `drop` command to remove individual commits.

### `DropBranch`

Removes an entire woven branch section and its merge line from the todo. With
`--rebase-merges --update-refs`, a woven branch section looks like:

```text
reset onto
pick <hash> commit message
pick <hash> another commit
label <branch>
update-ref refs/heads/<branch>
...
merge -C <hash> <branch>
```

This action removes: the `reset` line that opens the section, all `pick` lines
in the section, the `label <branch>` line, the `update-ref refs/heads/<branch>`
line, and the `merge ... <branch>` line.

```json
{"DropBranch": {"branch_name": "feature-a"}}
```

If the branch label is not found, a warning is emitted to stderr and the action
is skipped (non-fatal). If the merge line is not found, a warning is emitted but
the section is still removed.

Used by the `drop` command to remove entire woven branches.

### `ReassignBranch`

Reassigns a woven branch section to a co-located branch. When dropping a woven
branch that shares its tip with another branch, the section's `label` and
`merge` lines are renamed to the surviving branch instead of being removed.
The dropped branch's `update-ref` line is removed while the surviving branch's
`update-ref` is preserved.

Transforms:
```text
reset onto
pick <hash> commit message
label <drop-branch>
update-ref refs/heads/<drop-branch>
update-ref refs/heads/<keep-branch>
...
merge -C <hash> <drop-branch>
```

Into:
```text
reset onto
pick <hash> commit message
label <keep-branch>
update-ref refs/heads/<keep-branch>
...
merge -C <hash> <keep-branch>
```

```json
{"ReassignBranch": {"drop_branch": "feature-a", "keep_branch": "feature-b"}}
```

Used by the `drop` command when dropping a woven branch that has co-located
siblings.

## What Happens

When git starts an interactive rebase, it writes a todo file like:

```
pick abc1234 First commit
pick def5678 Second commit
pick 9876543 Third commit
```

git-loom sets `GIT_SEQUENCE_EDITOR` to invoke itself with JSON-encoded actions:

```
"<path-to-git-loom>" internal-sequence-edit --actions-json '[{"Edit":{"short_hash":"abc1234"}}]'
```

Git calls this command and automatically appends the todo file path as the final
argument. The handler:

1. Parses the JSON to extract rebase actions
2. Reads the todo file
3. Validates that all commit hashes contain only hexadecimal characters
4. For each action, uses string splitting (`splitn`) to identify matching lines
   by hash prefix
5. Applies the specified modifications
6. Writes the modified file back

If a hash is not found, the action returns an error (except `DropBranch`, which
warns and continues).

## Rebase Builder

The `Rebase` struct provides a builder for running interactive rebases with
custom actions:

```rust
Rebase::new(workdir, RebaseTarget::Commit(hash))
    .action(RebaseAction::Edit { short_hash: "abc1234".into() })
    .action(RebaseAction::Drop { short_hash: "def5678".into() })
    .run()?;
```

`Rebase::run()` performs the following:

1. Resolves the git-loom binary path via `loom_exe_path()`
2. Validates all commit hashes (hex-only check)
3. Serializes actions to JSON
4. Builds the `GIT_SEQUENCE_EDITOR` string using Unix shell escaping (Git for
   Windows uses MSYS2/bash, so Unix escaping is correct on all platforms)
5. Runs `git rebase` with these flags:
   - `--interactive` — enables the sequence editor
   - `--autostash` — stashes dirty working tree changes
   - `--keep-empty` — preserves empty commits
   - `--no-autosquash` — disables fixup!/squash! auto-reordering
   - `--rebase-merges` — preserves merge topology
   - `--update-refs` — keeps branch refs in the rebased range up to date

### Rebase Targets

The `RebaseTarget` enum controls the rebase range:

- `Commit(hash)` — rebases from `<hash>^` (the parent of the commit)
- `Root` — rebases the entire history with `--root`

## Other Rebase Operations

### `rebase_onto(workdir, newbase, upstream)`

Runs `git rebase --onto <newbase> <upstream> --autostash --update-refs`. Used
to transplant a range of commits onto a new base.

### `continue_rebase(workdir)`

Runs `git rebase --continue`. If continuation fails, automatically aborts the
rebase before returning the error.

### `abort(workdir)`

Runs `git rebase --abort` to clean up a failed or in-progress rebase.

## Integration with Commands

Multiple git-loom commands use the rebase infrastructure:

- **`reword`** (Spec 003): Uses `Edit` to stop at a commit for message editing
- **`fold`** (Spec 007): Uses `Edit` to stop for amending, `Fixup` to fold
  commits, and `Move` to transfer commits between branches
- **`drop`** (Spec 008): Uses `Drop` to remove individual commits,
  `DropBranch` to remove entire woven branch sections, and
  `ReassignBranch` to hand off co-located woven branch sections
- **`commit`** (Spec 006): Uses `Move` to relocate commits to target branches

## Binary Path Resolution

During normal execution, `std::env::current_exe()` returns the git-loom binary
path. During `cargo test`, however, `current_exe()` returns the test harness
binary in `target/<profile>/deps/`, which cannot serve as a sequence editor.

The `loom_exe_path()` helper detects this situation:

1. Get the current executable path
2. If the parent directory is named `deps`, look one level up for the actual
   `git-loom` binary (or `git-loom.exe` on Windows)
3. If found, return that path; otherwise fall back to `current_exe()`

This means `cargo build && cargo test` is required when running tests after
source changes, to ensure the binary at `target/<profile>/git-loom` is
up to date.

## Design Decisions

### JSON-Encoded Actions for Extensibility

Actions are passed as JSON rather than individual CLI flags (e.g., `--edit hash1
--edit hash2`). This design choice provides:

**Extensibility:**
- Easy to add new action types
- Complex actions can include multiple parameters without CLI flag proliferation
- Operations like "move commit X to branch Y" work naturally

**Type Safety:**
- Serde handles serialization/deserialization with compile-time validation
- The `RebaseAction` enum documents all possible operations
- Invalid JSON fails fast with clear error messages

**Simplicity:**
- One `--actions-json` parameter instead of many action-specific flags
- Internal API is cleaner: functions take `Vec<RebaseAction>`
- Testing is easier: construct actions as Rust values, not CLI strings

**Example (multiple action types in one rebase):**
```json
[
  {"Edit": {"short_hash": "abc1234"}},
  {"Drop": {"short_hash": "def5678"}},
  {"Move": {"commit_hash": "111aaaa", "before_label": "feature-b"}},
  {"DropBranch": {"branch_name": "feature-a"}},
  {"ReassignBranch": {"drop_branch": "feature-b", "keep_branch": "feature-c"}}
]
```

### Hex Validation

Before building the rebase command, all commit hashes in actions are validated
to contain only hexadecimal characters. This prevents malformed input from
reaching git and provides clear error messages early. Branch names
(`DropBranch`, `ReassignBranch`) are not subject to hex validation.

### Hidden Subcommand over Environment Variable

An alternative would be a standalone helper binary or a mode triggered by an
environment variable. A hidden clap subcommand was chosen because:

- **No extra binaries** to build, distribute, or keep in sync
- **Discoverable** in the source via the `Command` enum
- **Type-safe arguments** validated by clap's derive parser
- **Standard dispatch** through the existing `main()` match

### Error vs Warning on Missing Hash

Most actions (`Edit`, `Fixup`, `Move`, `Drop`) return an error if the target
hash is not found in the todo file. This is the correct behavior since the
caller needs to know that the operation could not be applied.

`DropBranch` is the exception: if the branch label is not found, it emits a
warning to stderr and returns `Ok(())`. This handles cases where a branch may
not be woven into the integration branch. Similarly, a missing merge line
produces a warning but the section is still cleaned up.

### Multiple Actions Support

The JSON format naturally supports multiple actions in a single invocation:

- Actions are processed sequentially, each modifying the in-memory line buffer
- Order matters: earlier actions may remove or reorder lines that later actions
  reference
- Hash prefix matching stops at the first match per action

### Automatic Abort on Rebase Failure

The `git_commands::git_rebase` module provides automatic abort behavior to
ensure atomic operations:

- **`Rebase::run()`**: If the rebase fails to start (bad target, conflicts,
  etc.), automatically calls `git rebase --abort` before returning the error
- **`continue_rebase()`**: If continuation fails, automatically aborts the
  rebase before returning the error

This design choice means:

- Callers don't need to implement abort logic in every error path
- The repository is never left in a mid-rebase state requiring manual recovery
- Error handling is simplified: just use `?` and let the infrastructure clean up
- Future rebase-based commands get this safety automatically

### Unix Shell Escaping on All Platforms

The `GIT_SEQUENCE_EDITOR` command string uses Unix-style shell escaping
(`shell_escape::unix::escape`) even on Windows. This is because Git for Windows
uses MSYS2/bash to execute the sequence editor, not `cmd.exe` or PowerShell.
The binary path is also normalized to use forward slashes for Git compatibility.
