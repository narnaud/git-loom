---
name: write-integration-tests
description: Write or extend integration tests for a git-loom subcommand. Invoke as /write-integration-tests <command> (e.g. /write-integration-tests push).
---

# Write Integration Tests

Create or extend `tests/integration/test_<command>.sh`.

## Required reads and coverage

Before writing, read the matching spec (list `specs/` if needed), all of
`tests/integration/helpers.sh` (the only test utilities), canonical reference
`tests/integration/test_status.sh`, and the target test if it exists.

Map the spec to tests:

- Preconditions/errors go under `PRECONDITIONS`.
- Give each flag a section; cover every behavior, edge case, and `must not`.
- For each commit, branch, or entity identifier, test both a full Git hash and
  a loom short ID parsed from `gl status` or the shortid allocator. Pair
  equivalent hash and short-ID cases.

## File shape

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

## Non-negotiable conventions

| Concern | Rule |
|---|---|
| Isolation | Every test starts with `describe "..."`, then `setup_repo_with_remote`. |
| Branches | Feature names start with `g`-`z`, never `a`-`f` (hex-ID collision), e.g. `g-feat`, `h-fix`. |
| Success | `out=$(gl <command> ...)`, immediately followed by `assert_exit_ok $? "label"`. |
| Failure | `gl_capture <command> ...` (sets `$OUT`, `$CODE`), immediately followed by `assert_exit_fail "$CODE" "label"`. |
| Labels | Short snake_case, unique in the file. |
| Absence | Prefer `assert_not_contains`. |
| Verification | Use helpers (`head_msg`, `branch_oid`, `assert_file_content`, etc.); use `git -C "$WORK" ...` only if no helper exists. |
| Environment | `gl()` sets `NO_COLOR=1` and `GIT_TERMINAL_PROMPT=0`; repeat only for direct `$GL_BIN` calls such as CWD tests. |
| Counts | `grep -c "needle" <<< "$out"` plus `assert_eq`. |
| Short IDs | Example: `short_id=$(gl status \| grep "Commit message" \| grep -oE '[0-9a-z]{4,8}' \| head -1)`; pair with a full-hash test. |
| Prompts | Never pipe answers (`"y\n"`, `"n\n"`) to interactive confirmation. Test `--yes`; leave prompts untested because non-TTY detection is unpredictable. |

Use the exact skeleton banner with uppercase titles. Include shebang and helper
source only once.

## Output

If `tests/integration/test_<command>.sh` does not exist: output the full file.
If it already exists: read first, then write the full revision or only new `describe` blocks with insertion location. Remind the user to run
`cargo build && bash tests/integration/test_<command>.sh`.
