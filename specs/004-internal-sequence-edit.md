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
git-loom internal-sequence-edit <todo_file> --actions-json <json>
```

This subcommand is **hidden** from `--help` output. It is not intended for
direct user invocation — git calls it automatically as the sequence editor.

**Arguments:**

- `<todo_file>`: Path to the rebase todo file provided by git (positional argument)
- `--actions-json <json>`: JSON-encoded list of rebase actions to apply

**Exit codes:**

- `0`: Success (actions applied, or hashes not found with warning)
- `1`: Error reading/writing the todo file or parsing JSON

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
3. For each action, uses regex-based parsing to identify matching lines
4. Applies the specified modifications (e.g., replace `pick` with `edit`)
5. Writes the modified file back
6. If hashes are not found, emits warnings to stderr (non-fatal)

After the edit, the todo file becomes:

```
edit abc1234 First commit
pick def5678 Second commit
pick 9876543 Third commit
```

Git then stops at that commit, allowing git-loom to amend the message and
continue the rebase.

## Integration with Reword

The `reword` command (see **Spec 003**) constructs the sequence editor string
using JSON-serialized actions:

```
reword_commit()
    ↓
Build RebaseAction::Edit { short_hash }
    ↓
serde_json::to_string(&actions) → JSON
    ↓
format!("\"{}\" internal-sequence-edit --actions-json {}", exe, escaped_json)
    ↓
passed as GIT_SEQUENCE_EDITOR to git rebase --interactive
    ↓
git invokes: git-loom internal-sequence-edit --actions-json '[...]' <todo_file>
    ↓
serde_json::from_str() parses actions
    ↓
apply_actions_to_todo() reads/rewrites the todo file using structured parsing
```

This JSON-based approach makes it easy to add future actions (Drop, Reorder,
Squash, Fixup) without modifying the CLI interface. New operations simply add
variants to the `RebaseAction` enum.

## Binary Path Resolution

During normal execution, `std::env::current_exe()` returns the git-loom binary
path. During `cargo test`, however, `current_exe()` returns the test harness
binary in `target/<profile>/deps/`, which cannot serve as a sequence editor.

The `loom_exe_path()` helper detects this situation:

1. Get the current executable path
2. If the parent directory is named `deps`, look one level up for the actual
   `git-loom` binary
3. If found, return that path; otherwise fall back to `current_exe()`

This means `cargo build && cargo test` is required when running tests after
source changes, to ensure the binary at `target/<profile>/git-loom` is
up to date.

## Design Decisions

### JSON-Encoded Actions for Extensibility

Actions are passed as JSON rather than individual CLI flags (e.g., `--edit hash1
--edit hash2`). This design choice provides:

**Extensibility:**
- Easy to add new action types (Drop, Reorder, Squash, Fixup)
- Complex actions can include multiple parameters without CLI flag proliferation
- Future operations like "move commit X after Y" work naturally

**Type Safety:**
- Serde handles serialization/deserialization with compile-time validation
- The `RebaseAction` enum documents all possible operations
- Invalid JSON fails fast with clear error messages

**Simplicity:**
- One `--actions-json` parameter instead of many action-specific flags
- Internal API is cleaner: functions take `Vec<RebaseAction>`
- Testing is easier: construct actions as Rust values, not CLI strings

**Example:**
```json
[
  {"Edit": {"short_hash": "abc1234"}},
  {"Edit": {"short_hash": "def5678"}}
]
```

Future actions might look like:
```json
[
  {"Drop": {"short_hash": "abc1234"}},
  {"Reorder": {"from_index": 2, "to_index": 5}},
  {"Squash": {"target": "abc1234", "into": "def5678"}}
]
```

### Hidden Subcommand over Environment Variable

An alternative would be a standalone helper binary or a mode triggered by an
environment variable. A hidden clap subcommand was chosen because:

- **No extra binaries** to build, distribute, or keep in sync
- **Discoverable** in the source via the `Command` enum
- **Type-safe arguments** validated by clap's derive parser
- **Standard dispatch** through the existing `main()` match

### Warning Instead of Error on Missing Hash

If the target hash isn't found in the todo file, the handler warns on stderr
but exits successfully. This avoids aborting the rebase for edge cases where
git may format the todo differently than expected. The rebase will proceed, and
the caller (`reword_commit`) will detect that the commit wasn't stopped at and
report the failure at a higher level.

### Multiple Actions Support

The JSON format naturally supports multiple actions in a single invocation:

- Multiple `Edit` actions can mark several commits for editing
- Each commit hash appears at most once in a rebase todo
- Actions are processed in a single pass through the file
- Hash prefix matching stops at the first match per action

This design allows operations like:
```bash
# Mark multiple commits for editing in one rebase
git-loom reword abc123 def456 ghi789
```

Future commands can combine different action types:
```bash
# Hypothetical: edit one commit, drop another, reorder a third
git-loom amend-batch --edit abc123 --drop def456 --move ghi789:after:abc123
```

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
