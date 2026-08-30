#!/usr/bin/env bash
# Integration tests for: agent mode (--agent / LOOM_AGENT) and loom agent init
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# The JSON status is the last line of stderr; gl_capture merges stdout+stderr,
# so assertions grep for the JSON fragments rather than compare whole output.

# ══════════════════════════════════════════════════════════════════════════════
# agent init
# ══════════════════════════════════════════════════════════════════════════════

describe "agent init: installs, is idempotent, refreshes stale content"
setup_repo_with_remote
SKILLS_BASE="$TMPROOT/agent-base"
SKILL_FILE="$SKILLS_BASE/skills/git-loom/SKILL.md"

gl_capture agent init --dir "$SKILLS_BASE"
assert_exit_ok "$CODE" "agent_init_ok"
assert_contains "$OUT" "Installed Claude skill" "agent_init_installed"
[[ -f "$SKILL_FILE" ]] || fail "agent_init_file_missing"
grep -q "name: git-loom" "$SKILL_FILE" || fail "agent_init_frontmatter"

gl_capture agent init --dir "$SKILLS_BASE"
assert_exit_ok "$CODE" "agent_init_idempotent"
assert_contains "$OUT" "already up to date" "agent_init_up_to_date"

echo "stale" > "$SKILL_FILE"
gl_capture agent init --dir "$SKILLS_BASE"
assert_exit_ok "$CODE" "agent_init_refresh"
assert_contains "$OUT" "Updated Claude skill" "agent_init_updated"
grep -q "name: git-loom" "$SKILL_FILE" || fail "agent_init_refreshed_content"

describe "agent init: works while an operation is paused"
mkdir -p "$WORK/.git/loom"
echo '{"command":"update","rollback":{},"context":null}' > "$WORK/.git/loom/state.json"
gl_capture agent init --dir "$SKILLS_BASE"
assert_exit_ok "$CODE" "agent_init_while_paused"
rm -f "$WORK/.git/loom/state.json"

# ══════════════════════════════════════════════════════════════════════════════
# needs_input: commit without a branch
# ══════════════════════════════════════════════════════════════════════════════

describe "agent mode: commit without -b lists woven branches, changes nothing"
setup_repo_with_remote
write_file "a.txt" "content a"
gl commit --agent -b feature-a -m "A1" zz > /dev/null 2>&1
write_file "b.txt" "content b"
gl commit --agent -b feature-b -m "B1" zz > /dev/null 2>&1

write_file "c.txt" "content c"
gl_capture commit --agent -m "C1" zz
assert_eq "10" "$CODE" "commit_needs_input_exit"
assert_contains "$OUT" '"status":"needs_input"' "commit_needs_input_status"
assert_contains "$OUT" '"kind":"select"' "commit_needs_input_kind"
assert_contains "$OUT" "feature-a" "commit_needs_input_option_a"
assert_contains "$OUT" "feature-b" "commit_needs_input_option_b"
assert_contains "$OUT" '"allow_other":true' "commit_needs_input_allow_other"
assert_contains "$OUT" '"hint":' "commit_needs_input_hint"
# Nothing was committed
assert_log_not_contains "C1" "commit_needs_input_no_commit"

describe "agent mode: answering the hint commits and reports ok"
gl_capture commit --agent -b feature-a -m "C1" zz
assert_exit_ok "$CODE" "commit_ok_exit"
assert_contains "$OUT" '"status":"ok"' "commit_ok_status"
assert_contains "$OUT" '"messages":' "commit_ok_messages"
assert_log_contains "C1" "commit_ok_in_log"

describe "agent mode: LOOM_AGENT env var behaves like --agent"
write_file "d.txt" "content d"
OUT=$( (cd "$WORK" && NO_COLOR=1 LOOM_AGENT=1 "$GL_BIN" commit -m "D1" zz) 2>&1) && CODE=$? || CODE=$?
assert_eq "10" "$CODE" "env_var_exit"
assert_contains "$OUT" '"status":"needs_input"' "env_var_status"
gl drop zz -y > /dev/null 2>&1  # clean the leftover staged change

# ══════════════════════════════════════════════════════════════════════════════
# needs_input: missing -m (editor guard)
# ══════════════════════════════════════════════════════════════════════════════

describe "agent mode: commit without -m never opens an editor"
write_file "e.txt" "content e"
gl_capture commit --agent -b feature-a zz
assert_eq "10" "$CODE" "no_message_exit"
assert_contains "$OUT" '"status":"needs_input"' "no_message_status"
assert_contains "$OUT" '"kind":"text"' "no_message_kind"
gl drop zz -y > /dev/null 2>&1

# ══════════════════════════════════════════════════════════════════════════════
# needs_confirmation: drop without -y
# ══════════════════════════════════════════════════════════════════════════════

describe "agent mode: drop a file without -y asks for confirmation"
write_file "a.txt" "modified content"
gl_capture drop --agent a.txt
assert_eq "10" "$CODE" "drop_confirm_exit"
assert_contains "$OUT" '"status":"needs_confirmation"' "drop_confirm_status"
assert_contains "$OUT" "loom drop <target> -y" "drop_confirm_hint"
assert_file_content "a.txt" "modified content" "drop_confirm_untouched"

gl_capture drop --agent a.txt -y
assert_exit_ok "$CODE" "drop_yes_exit"
assert_contains "$OUT" '"status":"ok"' "drop_yes_status"
assert_file_content "a.txt" "content a" "drop_yes_restored"

# ══════════════════════════════════════════════════════════════════════════════
# completions: dispatched early, but still ends with a JSON status
# ══════════════════════════════════════════════════════════════════════════════

describe "agent mode: completions still ends with a JSON status"
gl_capture completions powershell --agent
assert_exit_ok "$CODE" "completions_exit"
assert_contains "$OUT" '"status":"ok"' "completions_status"

gl_capture completions notashell --agent
assert_eq "1" "$CODE" "completions_bad_shell_exit"
assert_contains "$OUT" '"status":"error"' "completions_bad_shell_status"

# ══════════════════════════════════════════════════════════════════════════════
# error: -p is rejected
# ══════════════════════════════════════════════════════════════════════════════

describe "agent mode: -p/--patch is rejected with a structured error"
gl_capture add --agent -p
assert_eq "1" "$CODE" "patch_rejected_exit"
assert_contains "$OUT" '"status":"error"' "patch_rejected_status"
assert_contains "$OUT" "--patch is interactive" "patch_rejected_msg"

# ══════════════════════════════════════════════════════════════════════════════
# error: normal failures still end with a JSON status
# ══════════════════════════════════════════════════════════════════════════════

describe "agent mode: a failing command reports status error"
gl_capture drop --agent no-such-target-xyz -y
assert_eq "1" "$CODE" "error_exit"
assert_contains "$OUT" '"status":"error"' "error_status"

# ══════════════════════════════════════════════════════════════════════════════
# paused: a conflicting update
# ══════════════════════════════════════════════════════════════════════════════

describe "agent mode: a conflicting update reports paused, continue reports ok"
setup_repo_with_remote

# Base version of conflict.txt, pushed upstream
commit_file "Base commit" "conflict.txt"
upstream_branch="$(git -C "$WORK" rev-parse --abbrev-ref --symbolic-full-name @{u} | sed 's|origin/||')"
git -C "$WORK" push -q origin "HEAD:$upstream_branch"

# Upstream: modify conflict.txt
OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
echo "upstream content" > "$OTHER/conflict.txt"
git -C "$OTHER" add conflict.txt
git -C "$OTHER" commit -q -m "Upstream change"
git -C "$OTHER" push -q origin

# Local: diverge on the same file
echo "local content" > "$WORK/conflict.txt"
git -C "$WORK" add conflict.txt
git -C "$WORK" commit -q -m "Local change"

gl_capture update --agent -y
assert_exit_ok "$CODE" "paused_exit"
assert_contains "$OUT" '"status":"paused"' "paused_status"
assert_contains "$OUT" "loom continue" "paused_hint"
assert_state_file "paused_state_file"

# A blocked command while paused still ends with a JSON error status
gl_capture status --agent
assert_eq "1" "$CODE" "blocked_exit"
assert_contains "$OUT" '"status":"error"' "blocked_status"

# `loom add` is blocked too, which is why the skill tells the agent to stage
# conflict resolutions with raw `git add`
gl_capture add --agent zz
assert_eq "1" "$CODE" "add_blocked_while_paused_exit"
assert_contains "$OUT" '"status":"error"' "add_blocked_while_paused_status"

echo "resolved content" > "$WORK/conflict.txt"
git -C "$WORK" add conflict.txt

gl_capture continue --agent
assert_exit_ok "$CODE" "continue_exit"
assert_contains "$OUT" '"status":"ok"' "continue_status"
assert_no_state_file "continue_state_cleared"

pass
