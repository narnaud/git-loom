#!/usr/bin/env bash
# Integration tests for: gl split
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# NOTE: The split command uses an interactive file picker (inquire::MultiSelect)
# that runs before any message handling.  Success paths cannot be exercised via
# the CLI in a non-TTY context; they are covered by unit tests in split_test.rs.
# These integration tests cover preconditions and pre-picker validation only.

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" split HEAD -m "msg" >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: unknown full hash is rejected"
setup_repo_with_remote
gl_capture split "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef" -m "msg"
assert_exit_fail "$CODE" "precond_unknown_hash"

describe "precond: unknown short string is rejected"
setup_repo_with_remote
gl_capture split "zzzz9999" -m "msg"
assert_exit_fail "$CODE" "precond_unknown_short"

# ══════════════════════════════════════════════════════════════════════════════
# VALIDATION — SINGLE-FILE COMMIT
# (error is raised before the interactive picker; testable in non-TTY context)
# ══════════════════════════════════════════════════════════════════════════════

describe "single-file HEAD commit is rejected with clear message"
setup_repo_with_remote
commit_file "Single file commit" "only.txt"
gl_capture split HEAD -m "First part"
assert_exit_fail "$CODE" "single_file_head_fail"
assert_contains "$OUT" "only one file" "single_file_head_msg"

describe "single-file non-HEAD commit is rejected (full hash)"
setup_repo_with_remote
commit_file "Single file target" "only.txt"
target_hash="$(head_hash)"
commit_file "Later commit" "later.txt"
gl_capture split "$target_hash" -m "First part"
assert_exit_fail "$CODE" "single_file_nonhead_fail"
assert_contains "$OUT" "only one file" "single_file_nonhead_msg"

describe "single-file commit is rejected via short ID"
# Also proves short-ID resolution works for split targets
setup_repo_with_remote
commit_file "Short ID target" "only.txt"
commit_file "Later for stack" "later2.txt"
short_id=$(commit_sid_from_status "Short ID target")
gl_capture split "$short_id" -m "First part"
assert_exit_fail "$CODE" "single_file_shortid_fail"
assert_contains "$OUT" "only one file" "single_file_shortid_msg"

pass
