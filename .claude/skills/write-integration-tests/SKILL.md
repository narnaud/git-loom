---
name: write-integration-tests
description: Write or extend integration tests for a git-loom subcommand. Invoke as /write-integration-tests <command> (e.g. /write-integration-tests push).
---

# Write Integration Tests for a git-loom subcommand

The user has invoked `/write-integration-tests <command>`. Your job is to produce a complete, well-structured `tests/integration/test_<command>.sh` file (or extend an existing one).

## Step 1 — Gather context (read all three before writing a line)

1. **Spec**: find and read `specs/0NN-<command>.md`. If the number is unknown, list `specs/` to find it.
2. **Helpers**: read `tests/integration/helpers.sh` — these are the only utilities available in tests.
3. **Style reference**: read `tests/integration/test_status.sh` as the canonical style example.

## Step 2 — Plan coverage

From the spec, extract:
- All preconditions / error paths (→ PRECONDITIONS section)
- Every flag and option (→ one section per flag)
- All described behaviors and edge cases (→ sections matching the spec's structure)
- Any "must not" / negative behaviors
- For every argument that accepts a commit, branch, or entity identifier: plan tests using **both** a full git hash **and** a loom short ID (captured from `gl status` output or via the `shortid` allocator). Short ID support is a first-class feature and must be explicitly covered.

## Step 3 — Write the test file

### File skeleton

```bash
#!/usr/bin/env bash
# Integration tests for: gl <command>
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

# ... precondition tests ...

# ══════════════════════════════════════════════════════════════════════════════
# <SECTION NAME FROM SPEC>
# ══════════════════════════════════════════════════════════════════════════════

# ... tests ...

pass
```

### Conventions (non-negotiable)

- **Every test** starts with `describe "..."` then `setup_repo_with_remote` (fresh repo per test).
- **Branch names** for feature branches must start with a letter **g–z** (never a–f) to avoid collisions with hex short IDs. Use descriptive names: `g-feat`, `h-fix`, etc.
- **Capturing output**: use `out=$(gl <command> ...)` for commands expected to succeed; use `gl_capture <command> ...` (sets `$OUT` and `$CODE`) when testing for failure.
- **Exit assertions**: always `assert_exit_ok $? "label"` or `assert_exit_fail "$CODE" "label"` immediately after the invocation.
- **Assertion labels**: short snake_case, unique within the file (used in failure messages).
- **Negative assertions**: prefer `assert_not_contains` over absence-by-implication.
- **No raw `git` for verification**: use the helpers (`head_msg`, `branch_oid`, `assert_file_content`, etc.). Fall back to `git -C "$WORK" ...` only when no helper exists.
- **`NO_COLOR=1` and `GIT_TERMINAL_PROMPT=0`** are set by the `gl()` helper automatically — do not repeat them unless invoking `$GL_BIN` directly (e.g. for CWD tests).
- **Counting occurrences**: use `grep -c "needle" <<< "$out"` + `assert_eq`.
- **Short IDs**: capture them by running `gl status` (or the relevant listing command) and parsing with e.g. `short_id=$(gl status | grep "Commit message" | grep -oE '[0-9a-z]{4,8}' | head -1)`. Always pair a short-ID test with an equivalent full-hash test to prove both input forms work.
- **No interactive prompt testing**: never write tests that drive an interactive confirmation prompt (e.g. piping `"y\n"` or `"n\n"` to stdin). The binary detects non-TTY contexts unpredictably. Instead, test the `--yes` flag (auto-confirm) for the affirmative path, and leave the interactive prompt untested at the integration level.
- Do **not** add a shebang comment or `source helpers.sh` line more than once.

### Section headers

Use the exact banner style from the skeleton (═══ lines, ALL CAPS title).

## Step 4 — Output

- If `tests/integration/test_<command>.sh` does not exist: output the full file.
- If it already exists: read it first, then output only the new `describe` blocks to append (with a comment indicating where to insert them), or output the full updated file if restructuring is needed.
- After writing the file, remind the user to run `cargo build && bash tests/integration/test_<command>.sh` to verify.
