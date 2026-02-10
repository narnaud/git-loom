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
git-loom internal-sequence-edit <short_hash> <todo_file>
```

This subcommand is **hidden** from `--help` output. It is not intended for
direct user invocation — git calls it automatically as the sequence editor.

**Arguments:**

- `<short_hash>`: The 7-character (or shorter) commit hash prefix to mark for
  editing
- `<todo_file>`: Path to the rebase todo file provided by git

**Exit codes:**

- `0`: Success (hash found and replaced, or hash not found with warning)
- `1`: I/O error reading or writing the todo file

## What Happens

When git starts an interactive rebase, it writes a todo file like:

```
pick abc1234 First commit
pick def5678 Second commit
pick 9876543 Third commit
```

git-loom sets `GIT_SEQUENCE_EDITOR` to invoke itself:

```
"<path-to-git-loom>" internal-sequence-edit abc1234
```

Git calls this command with the todo file path appended as the final argument.
The handler:

1. Reads the todo file
2. Finds the first line starting with `pick <short_hash>`
3. Replaces `pick` with `edit` on that line
4. Writes the modified file back
5. If no matching line is found, emits a warning to stderr (non-fatal)

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
in `reword_commit()`:

```
reword_commit()
    ↓
loom_exe_path() → resolved binary path
    ↓
format!("\"{}\" internal-sequence-edit {}", exe, short_hash)
    ↓
passed as GIT_SEQUENCE_EDITOR to git rebase --interactive
    ↓
git invokes: git-loom internal-sequence-edit <hash> <todo_file>
    ↓
handle_sequence_edit() reads/rewrites the todo file
```

Future commands that need to manipulate the rebase todo (commit reordering,
commit splitting, etc.) can reuse the same subcommand pattern, either directly
or by extending it with additional actions.

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

### First Match Only

Only the first `pick <hash>` line is replaced. This is intentional:

- A commit hash appears at most once in a rebase todo
- Stopping early avoids accidental edits if a hash prefix collides with
  content in comment lines
- Future multi-commit operations can extend the subcommand with additional
  arguments rather than changing this behavior
