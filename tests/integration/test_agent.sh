#!/usr/bin/env bash
# Integration tests for: agent mode (--agent / LOOM_AGENT) and loom agent init
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

# The JSON status is the last line of stdout. gl_capture merges the streams, so
# its assertions grep for JSON fragments; gl_capture_json keeps them apart for
# the ones about which stream a line lands on.

# ── agent init ────────────────────────────────────────────────────────────────

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

describe "agent mode: warns when an installed skill is out of date"
setup_repo_with_remote
# Point the home install at an empty directory so a skill installed on the
# developer's own machine cannot influence the assertions below.
OLD_HOME="$HOME"; OLD_USERPROFILE="${USERPROFILE:-}"
export HOME="$TMPROOT/fake-home" USERPROFILE="$TMPROOT/fake-home"
mkdir -p "$HOME"
PROJECT_SKILL="$WORK/.claude/skills/git-loom/SKILL.md"
mkdir -p "$(dirname "$PROJECT_SKILL")"
echo "skill from an older loom" > "$PROJECT_SKILL"

gl_capture status --agent
assert_exit_ok "$CODE" "skill_outdated_exit"
assert_contains "$OUT" '"status":"ok"' "skill_outdated_still_ok"
assert_contains "$OUT" "differs from the one this loom ships" "skill_outdated_warned"
assert_not_contains "$OUT" "! The Claude git-loom skill" "skill_outdated_not_printed"
assert_contains "$OUT" "git-loom agent init --project" "skill_outdated_hint"
# Advisory only: the stale file is never rewritten behind the user's back.
grep -q "skill from an older loom" "$PROJECT_SKILL" || fail "skill_outdated_rewritten"

gl agent init --project > /dev/null 2>&1
gl_capture status --agent
assert_exit_ok "$CODE" "skill_current_exit"
assert_not_contains "$OUT" "differs from the one this loom ships" "skill_current_no_warning"
rm -rf "$WORK/.claude"
export HOME="$OLD_HOME"; export USERPROFILE="$OLD_USERPROFILE"

# ── needs_input: commit without a branch ──────────────────────────────────────

describe "agent mode: commit without -b lists woven branches, changes nothing"
setup_repo_with_remote
write_file "a.txt" "content a"
gl commit --agent -b feature-a -m "A1" zz > /dev/null 2>&1
write_file "b.txt" "content b"
gl commit --agent -b feature-b -m "B1" zz > /dev/null 2>&1

write_file "c.txt" "content c"
gl_capture_json commit --agent -m "C1" zz
assert_eq "10" "$CODE" "commit_needs_input_exit"
# Every status reaches the machine stream, not just `ok`: assert the stream,
# not merged output, or a regression back onto stderr would pass unnoticed.
assert_contains "$(json_line)" '"status":"needs_input"' "commit_needs_input_status"
assert_not_contains "$STDERR" '"status":"needs_input"' "commit_needs_input_not_on_stderr"
assert_contains "$JSON" '"kind":"select"' "commit_needs_input_kind"
assert_contains "$JSON" "feature-a" "commit_needs_input_option_a"
assert_contains "$JSON" "feature-b" "commit_needs_input_option_b"
assert_contains "$JSON" '"allow_other":true' "commit_needs_input_allow_other"
assert_contains "$JSON" '"hint":' "commit_needs_input_hint"
assert_contains "$JSON" "or -i for the integration branch itself" \
    "commit_needs_input_hint_mentions_integration"
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

# ── needs_input: missing -m (editor guard) ────────────────────────────────────

describe "agent mode: commit without -m never opens an editor"
write_file "e.txt" "content e"
gl_capture commit --agent -b feature-a zz
assert_eq "10" "$CODE" "no_message_exit"
assert_contains "$OUT" '"status":"needs_input"' "no_message_status"
assert_contains "$OUT" '"kind":"text"' "no_message_kind"
gl drop zz -y > /dev/null 2>&1

# ── needs_confirmation: drop without -y ───────────────────────────────────────

describe "agent mode: drop a file without -y asks for confirmation"
write_file "a.txt" "modified content"
gl_capture_json drop --agent a.txt
assert_eq "10" "$CODE" "drop_confirm_exit"
assert_contains "$(json_line)" '"status":"needs_confirmation"' "drop_confirm_status"
assert_not_contains "$STDERR" '"status"' "drop_confirm_not_on_stderr"
assert_contains "$JSON" "loom drop <target> -y" "drop_confirm_hint"
assert_file_content "a.txt" "modified content" "drop_confirm_untouched"

gl_capture drop --agent a.txt -y
assert_exit_ok "$CODE" "drop_yes_exit"
assert_contains "$OUT" '"status":"ok"' "drop_yes_status"
assert_file_content "a.txt" "content a" "drop_yes_restored"

# ── completions: never in agent mode ──────────────────────────────────────────

describe "agent mode: completions prints its script alone"
gl_capture_json completions powershell --agent
assert_exit_ok "$CODE" "completions_exit"
assert_contains "$JSON" "Register-ArgumentCompleter" "completions_script"
assert_not_contains "$JSON" '"status"' "completions_no_json"

LOOM_AGENT=1 gl_capture_json completions notashell
assert_eq "1" "$CODE" "completions_bad_shell_exit"
assert_not_contains "$JSON$STDERR" '"status"' "completions_bad_shell_no_json"

# ── -p answers with a listing, never a TUI ───────────────────────────────────

describe "agent mode: -p with nothing to stage says so"
gl_capture add --agent -p
assert_eq "1" "$CODE" "patch_nothing_exit"
assert_contains "$OUT" '"status":"error"' "patch_nothing_status"
assert_contains "$OUT" "No changes to stage" "patch_nothing_msg"

# ── error: tui is rejected ────────────────────────────────────────────────────

describe "agent mode: tui is rejected with a structured error"
gl_capture tui --agent
assert_eq "1" "$CODE" "tui_rejected_exit"
assert_contains "$OUT" '"status":"error"' "tui_rejected_status"
assert_contains "$OUT" "the TUI is interactive" "tui_rejected_msg"

# ── error: normal failures still end with a JSON status ───────────────────────

describe "agent mode: a failing command reports status error"
gl_capture_json drop --agent no-such-target-xyz -y
assert_eq "1" "$CODE" "error_exit"
assert_contains "$(json_line)" '"status":"error"' "error_status"
assert_not_contains "$STDERR" '"status"' "error_not_on_stderr"

# ── paused: a conflicting update ──────────────────────────────────────────────

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

gl_capture_json update --agent -y
assert_exit_ok "$CODE" "paused_exit"
assert_contains "$(json_line)" '"status":"paused"' "paused_status"
assert_not_contains "$STDERR" '"status":"paused"' "paused_not_on_stderr"
assert_contains "$JSON" "loom continue" "paused_hint"
assert_contains "$STDERR" "-> origin/" "update_fetch_summary_on_stderr"
assert_not_contains "$JSON" "-> origin/" "update_fetch_summary_not_on_stdout"
assert_state_file "paused_state_file"

# A blocked command while paused still ends with a JSON error status
gl_capture_json status --agent
assert_eq "1" "$CODE" "blocked_exit"
assert_contains "$(json_line)" '"status":"error"' "blocked_status"
assert_not_contains "$STDERR" '"status":"error"' "blocked_not_on_stderr"

# `loom add` is blocked too, which is why the skill tells the agent to stage
# conflict resolutions with raw `git add`
gl_capture add --agent zz
assert_eq "1" "$CODE" "add_blocked_while_paused_exit"
assert_contains "$OUT" '"status":"error"' "add_blocked_while_paused_status"

echo "resolved content" > "$WORK/conflict.txt"
git -C "$WORK" add conflict.txt

gl_capture_json continue --agent
assert_exit_ok "$CODE" "continue_exit"
assert_contains "$(json_line)" '"status":"ok"' "continue_status"
assert_no_state_file "continue_state_cleared"

# ── status: the graph as JSON ─────────────────────────────────────────────────

describe "agent mode: status puts the whole graph on stdout as one JSON line"
setup_repo_with_remote
create_feature_branch feature-api
git -C "$WORK" checkout -q feature-api
echo api > "$WORK/api.txt"
git -C "$WORK" add api.txt
git -C "$WORK" commit -q -m "feat(api): add the endpoint"
git -C "$WORK" checkout -q integration
weave_branch feature-api
write_file dirty.txt "local edit"

gl_capture_json status --agent
assert_exit_ok "$CODE" "status_json_exit"
assert_eq "1" "$(wc -l <<< "$JSON" | tr -d ' ')" "status_json_single_line"
assert_contains "$JSON" '"status":"ok"' "status_json_status"
assert_contains "$JSON" '"graph":{"schema":1' "status_json_schema"
assert_contains "$JSON" '"integration_branch":"integration"' "status_json_branch"
assert_contains "$JSON" '"id":"zz"' "status_json_unstaged_id"
assert_contains "$JSON" '"name":"feature-api"' "status_json_branch_name"
assert_contains "$JSON" '"subject":"feat(api): add the endpoint"' "status_json_subject"
assert_contains "$JSON" "\"label\":\"origin/$BASE_BRANCH\"" "status_json_upstream"
assert_contains "$JSON" '"state":"untracked"' "status_json_file_state"
# The rendered tree is human output: it must not reach the machine stream.
assert_not_contains "$JSON" "local changes" "status_json_no_tree_on_stdout"

describe "agent mode: the rendered tree goes to stderr"
gl_capture_json status --agent
assert_contains "$STDERR" "[local changes]" "status_tree_on_stderr"
assert_contains "$STDERR" "feature-api" "status_tree_branch_on_stderr"
assert_not_contains "$STDERR" '"status":"ok"' "status_no_json_on_stderr"

describe "agent mode: status -f adds commit files with ids counting from 0"
gl_capture_json status --agent -f
assert_exit_ok "$CODE" "status_files_exit"
assert_contains "$JSON" '"path":"api.txt"' "status_files_path"
# The file id is the owning commit's short ID with `:0` appended, not just
# anything ending in `:0`.
COMMIT_SID=$(grep -o '"id":"[a-z0-9]*","hash":"[0-9a-f]*","oid":"[0-9a-f]*","subject":"feat(api): add the endpoint"' <<< "$JSON" | sed 's/"id":"//; s/".*//')
assert_contains "$JSON" "\"id\":\"$COMMIT_SID:0\"" "status_files_id_from_zero"

describe "agent mode: a stack names the branch below in stacked_on"
setup_repo_with_remote
create_feature_branch feature-api
git -C "$WORK" checkout -q feature-api
echo api > "$WORK/api.txt"
git -C "$WORK" add api.txt
git -C "$WORK" commit -q -m "api"
git -C "$WORK" checkout -q -b feature-ui
echo ui > "$WORK/ui.txt"
git -C "$WORK" add ui.txt
git -C "$WORK" commit -q -m "ui"
git -C "$WORK" checkout -q integration
weave_branch feature-ui

gl_capture_json status --agent
assert_exit_ok "$CODE" "stack_json_exit"
assert_contains "$JSON" '"stacked_on":"feature-api"' "stack_json_edge"
assert_contains "$JSON" '"stacked_on":null' "stack_json_bottom"

describe "agent mode: a command with no payload puts only the JSON on stdout"
write_file solo.txt "one line of stdout"
gl_capture_json commit --agent -b feature-api -m "feat(api): solo" solo.txt
assert_exit_ok "$CODE" "solo_stdout_exit"
assert_eq "1" "$(wc -l <<< "$JSON" | tr -d ' ')" "solo_stdout_single_line"
assert_contains "$JSON" '"status":"ok"' "solo_stdout_status"
# The success lines a person reads are on the other stream.
assert_contains "$STDERR" "feature-api" "solo_progress_on_stderr"

describe "agent mode: diff prints its patch before the JSON object"
# The patch has to exist for the ordering to mean anything.
write_file ui.txt "edited for the diff"
# `loom diff` keeps the user's diff config (Spec 016); these make the patch
# one whatever `diff.external` and `color.diff` say.
gl_capture_json diff --agent -- --no-ext-diff --no-color
assert_exit_ok "$CODE" "diff_json_exit"
assert_contains "$JSON" "diff --git" "diff_json_has_patch"
assert_contains "$(head -1 <<< "$JSON")" "diff --git" "diff_json_patch_first"
assert_contains "$(json_line)" '"status":"ok"' "diff_json_last_line"
# The patch is machine payload on stdout, so stderr carries no part of it.
assert_not_contains "$STDERR" "diff --git" "diff_patch_not_on_stderr"

describe "agent mode: absorb -n prints its plan before the JSON object"
gl_capture_json absorb --agent -n
assert_exit_ok "$CODE" "absorb_plan_exit"
assert_contains "$JSON" "ui.txt -> " "absorb_plan_on_stdout"
assert_contains "$JSON" "Dry run: would absorb" "absorb_plan_summary_on_stdout"
assert_contains "$(json_line)" '"status":"ok"' "absorb_plan_last_line"
assert_not_contains "$STDERR" "ui.txt -> " "absorb_plan_not_on_stderr"

describe "agent mode: trace prints its log before the JSON object"
gl_capture_json trace --agent
assert_exit_ok "$CODE" "trace_exit"
assert_contains "$JSON" "Log path:" "trace_log_on_stdout"
assert_contains "$(json_line)" '"status":"ok"' "trace_last_line"
assert_not_contains "$STDERR" "Log path:" "trace_log_not_on_stderr"

describe "agent mode: a branch stacked on a hidden one says so"
setup_repo_with_remote
create_feature_branch local-base
git -C "$WORK" checkout -q local-base
echo base > "$WORK/base.txt"
git -C "$WORK" add base.txt
git -C "$WORK" commit -q -m "base"
git -C "$WORK" checkout -q -b feature-top
echo top > "$WORK/top.txt"
git -C "$WORK" add top.txt
git -C "$WORK" commit -q -m "top"
git -C "$WORK" checkout -q integration
weave_branch feature-top

gl_capture_json status --agent
assert_exit_ok "$CODE" "hidden_stack_exit"
assert_not_contains "$JSON" '"name":"local-base"' "hidden_stack_base_hidden"
assert_contains "$JSON" '"stacked_on":null,"stacked_on_hidden":true' "hidden_stack_flagged"

gl_capture_json status --agent -a
assert_contains "$JSON" '"stacked_on":"local-base","stacked_on_hidden":true' "hidden_stack_named_with_all"

# ── hunk selection: -p answered as data ───────────────────────────────────────

# The fingerprint out of the JSON status (the last line of the captured output).
json_fingerprint() { tail -1 <<< "$OUT" | grep -o '"fingerprint":"[0-9a-f]*"' | cut -d'"' -f4; }

# Two edits far enough apart in one file to be two hunks.
setup_two_hunk_commit() {
    setup_repo_with_remote
    create_feature_branch feature-a
    switch_to feature-a
    seq 1 40 > "$WORK/multi.txt"
    git -C "$WORK" add multi.txt
    git -C "$WORK" commit -q -m "Add multi"
    switch_to integration
    weave_branch feature-a
    perl -pi -e 's/^3$/THREE/; s/^37$/THIRTYSEVEN/' "$WORK/multi.txt"
    git -C "$WORK" add multi.txt
    git -C "$WORK" commit -q -m "Change two regions"
}

describe "agent mode: split -p lists its hunks instead of rendering the picker"
setup_two_hunk_commit

gl_capture split HEAD -m "first region" -p --agent
assert_eq "10" "$CODE" "split_patch_listed_exit"
assert_contains "$OUT" '"status":"needs_input"' "split_patch_listed_status"
assert_contains "$OUT" '"kind":"multiselect"' "split_patch_listed_kind"
assert_contains "$OUT" '"options":["multi.txt:1","multi.txt:2"]' "split_patch_listed_options"
assert_contains "$OUT" '"diff":"@@ -1,6 +1,6 @@' "split_patch_listed_diff_header"
assert_contains "$OUT" "+THIRTYSEVEN" "split_patch_listed_diff"
assert_contains "$OUT" "--hunks <id> [--hunks <id>...] --hunks-from" "split_patch_listed_hint"
# Listing is pre-flight: nothing was split.
assert_head_msg "Change two regions" "split_patch_listed_no_mutation"

FP="$(json_fingerprint)"

describe "agent mode: split -p --hunks splits by the listed ids"
gl_capture split HEAD -m "first region" -p --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "split_by_hunks_exit"
assert_contains "$OUT" '"status":"ok"' "split_by_hunks_status"
assert_head_msg "Change two regions" "split_by_hunks_second"
assert_msg_at 1 "first region" "split_by_hunks_first"
assert_contains "$(show_patch HEAD~1)" "+THREE" "split_by_hunks_first_content"
assert_not_contains "$(show_patch HEAD~1)" "+THIRTYSEVEN" "split_by_hunks_first_only"
assert_contains "$(show_patch HEAD)" "+THIRTYSEVEN" "split_by_hunks_second_content"

describe "agent mode: a selection listed against a different diff is refused"
setup_two_hunk_commit
gl_capture split HEAD -m "nope" -p --hunks multi.txt:1 --hunks-from deadbeef --agent
assert_eq "1" "$CODE" "stale_fingerprint_exit"
assert_contains "$OUT" '"status":"error"' "stale_fingerprint_status"
assert_contains "$OUT" "hunks changed since" "stale_fingerprint_msg"
assert_head_msg "Change two regions" "stale_fingerprint_no_mutation"

gl_capture split HEAD -m "nope" -p --agent
FP="$(json_fingerprint)"
gl_capture split HEAD -m "nope" -p --hunks multi.txt:9 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "unknown_hunk_id_exit"
assert_contains "$OUT" "No hunk" "unknown_hunk_id_msg"

describe "agent mode: fold -p moves listed hunks between commits"
setup_two_hunk_commit
SRC="$(commit_sid_from_status "Change two regions")"
TGT="$(commit_sid_from_status "Add multi")"

gl_capture fold -p "$SRC" "$TGT" --agent
assert_eq "10" "$CODE" "fold_patch_listed_exit"
assert_contains "$OUT" '"status":"needs_input"' "fold_patch_listed_status"
FP="$(json_fingerprint)"

gl_capture fold -p "$SRC" "$TGT" --hunks multi.txt:2 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_by_hunks_exit"
assert_contains "$OUT" '"status":"ok"' "fold_by_hunks_status"
assert_contains "$(show_patch "$(branch_oid feature-a)")" "+THIRTYSEVEN" "fold_by_hunks_moved"
assert_contains "$(show_patch HEAD)" "+THREE" "fold_by_hunks_kept"
assert_not_contains "$(show_patch HEAD)" "+THIRTYSEVEN" "fold_by_hunks_removed"

describe "agent mode: fold -p <commit> zz uncommits listed hunks"
setup_two_hunk_commit
SRC="$(commit_sid_from_status "Change two regions")"
gl_capture fold -p "$SRC" zz --agent
assert_eq "10" "$CODE" "fold_zz_listed_exit"
FP="$(json_fingerprint)"

gl_capture fold -p "$SRC" zz --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_zz_exit"
assert_contains "$(diff_patch)" "+THREE" "fold_zz_unstaged"
assert_contains "$(show_patch HEAD)" "+THIRTYSEVEN" "fold_zz_kept"

# A deletion has no hunk of its own, so it moves as the commit's whole-file
# diff — the one form `git apply` can take for it.
setup_deleted_and_changed() {
    setup_repo_with_remote
    echo gone > "$WORK/gone.txt"
    git -C "$WORK" add gone.txt
    git -C "$WORK" commit -q -m "Add gone"
    echo kept > "$WORK/kept.txt"
    git -C "$WORK" add kept.txt
    git -C "$WORK" commit -q -m "Add kept"
    git -C "$WORK" rm -q gone.txt
    echo changed > "$WORK/kept.txt"
    git -C "$WORK" add kept.txt
    git -C "$WORK" commit -q -m "Delete one, change another"
}

describe "agent mode: fold -p moves a picked deletion whole"
setup_deleted_and_changed

gl_capture fold -p HEAD 'HEAD^' --agent
assert_eq "10" "$CODE" "fold_deletion_listed_exit"
assert_contains "$OUT" '"id":"gone.txt:1"' "fold_deletion_listed"
FP="$(json_fingerprint)"

gl_capture fold -p HEAD 'HEAD^' --hunks gone.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_deletion_exit"
assert_contains "$(git -C "$WORK" show --name-status 'HEAD^')" "D	gone.txt" "fold_deletion_moved"
assert_not_contains "$(git -C "$WORK" show --name-status HEAD)" "gone.txt" "fold_deletion_left_source"
assert_contains "$(show_patch HEAD)" "+changed" "fold_deletion_kept_hunk"
assert_eq "" "$(git -C "$WORK" status --porcelain)" "fold_deletion_clean"

describe "agent mode: fold -p <commit> zz uncommits a deletion"
setup_deleted_and_changed

gl_capture fold -p HEAD zz --agent
assert_eq "10" "$CODE" "fold_zz_deletion_listed_exit"
FP="$(json_fingerprint)"

gl_capture fold -p HEAD zz --hunks gone.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_zz_deletion_exit"
assert_not_contains "$(git -C "$WORK" show --name-status HEAD)" "gone.txt" "fold_zz_deletion_left_commit"
assert_contains "$(git -C "$WORK" status --porcelain)" " D gone.txt" "fold_zz_deletion_unstaged"

describe "agent mode: split -p keeps its <files> filter in the replay hint"
setup_two_hunk_commit
echo other > "$WORK/other.txt"
git -C "$WORK" add other.txt
git -C "$WORK" commit -q --amend --no-edit

gl_capture split HEAD -m "first region" -p multi.txt --agent
assert_eq "10" "$CODE" "split_filtered_listed_exit"
assert_contains "$OUT" "-p multi.txt --hunks" "split_filtered_hint_keeps_files"
assert_not_contains "$OUT" '"id":"other.txt:1"' "split_filtered_listing"
FP="$(json_fingerprint)"

# The hint has to round-trip: without the filter the re-run lists a different
# set and fails the fingerprint check instead of splitting.
gl_capture split HEAD -m "first region" -p multi.txt --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "split_filtered_replay_exit"
assert_msg_at 1 "first region" "split_filtered_replay_split"

# A path the agent must get back as one argument, not two.
describe "agent mode: split -p quotes a filtered path that has a space"
setup_two_hunk_commit
cp "$WORK/multi.txt" "$WORK/with space.txt"
git -C "$WORK" add "with space.txt"
git -C "$WORK" commit -q --amend --no-edit

gl_capture split HEAD -m "spaced" -p "with space.txt" --agent
assert_eq "10" "$CODE" "split_spaced_listed_exit"
assert_contains "$OUT" "-p 'with space.txt' --hunks" "split_spaced_hint_quoted"

# A revspec is shell syntax in zsh, so the hint has to come back quotable.
describe "agent mode: fold -p quotes both hint arguments"
setup_two_hunk_commit

# Two loose commits, so HEAD^ is a plain fold target rather than the merge.
seq 1 40 > "$WORK/later.txt"
git -C "$WORK" add later.txt
git -C "$WORK" commit -q -m "Later commit"

gl_capture fold -p HEAD 'HEAD^' --agent
assert_eq "10" "$CODE" "fold_hint_quoted_exit"
assert_contains "$OUT" "loom fold -p HEAD 'HEAD^' --hunks" "fold_hint_quoted_both"

gl_capture fold -p 'HEAD^{commit}' zz --agent
assert_eq "10" "$CODE" "fold_zz_hint_quoted_exit"
assert_contains "$OUT" "loom fold -p 'HEAD^{commit}' zz --hunks" "fold_zz_hint_quoted"

# Replaying the -m hint must run the same kind of split the agent asked for.
describe "agent mode: split without -m keeps -p and its files in the hint"
setup_two_hunk_commit
gl_capture split HEAD -p multi.txt --agent
assert_eq "10" "$CODE" "split_no_message_exit"
assert_contains "$OUT" '"kind":"text"' "split_no_message_kind"
assert_contains "$OUT" "loom split HEAD -m <message> -p multi.txt" "split_no_message_hint"

gl_capture split HEAD -p --agent
assert_contains "$OUT" "loom split HEAD -m <message> -p" "split_no_message_hint_bare"

# An id already chosen comes back in the form the listing hint advertises.
# The fingerprint has to be fetched with -m: without it the listing never runs.
gl_capture split HEAD -m tmp -p --agent
FP="$(json_fingerprint)"
gl_capture split HEAD -p --hunks multi.txt:1 --hunks multi.txt:2 --hunks-from "$FP" --agent
assert_contains "$OUT" "--hunks 'multi.txt:1' --hunks 'multi.txt:2' --hunks-from $FP" "split_no_message_hint_hunks"
assert_head_msg "Change two regions" "split_no_message_no_mutation"

describe "agent mode: split -p handles a path containing a comma"
setup_two_hunk_commit
cp "$WORK/multi.txt" "$WORK/a,b.txt"
git -C "$WORK" add "a,b.txt"
git -C "$WORK" commit -q --amend --no-edit

gl_capture split HEAD -m comma -p "a,b.txt" --agent
assert_eq "10" "$CODE" "split_comma_listed_exit"
assert_contains "$OUT" '"options":["a,b.txt:1"]' "split_comma_listed_option"
FP="$(json_fingerprint)"

gl_capture split HEAD -m comma -p "a,b.txt" --hunks "a,b.txt:1" --hunks-from "$FP" --agent
assert_contains "$OUT" "at least one hunk for the second commit" "split_comma_id_resolved"
assert_not_contains "$OUT" "Invalid hunk id" "split_comma_not_split"

# One flag per id, so a comma path sits next to another id without ambiguity.
gl_capture split HEAD -m comma -p --agent
FP="$(json_fingerprint)"
gl_capture split HEAD -m comma -p --hunks "a,b.txt:1" --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "split_comma_mixed_exit"
assert_contains "$(show_patch HEAD~1)" "a,b.txt" "split_comma_mixed_took_comma_path"
assert_contains "$(show_patch HEAD~1)" "multi.txt" "split_comma_mixed_took_plain"

# A comma list is the natural wrong guess; the error has to name the fix.
# The split above moved HEAD, so the listing has to be taken again.
setup_two_hunk_commit
cp "$WORK/multi.txt" "$WORK/a,b.txt"
git -C "$WORK" add "a,b.txt"
git -C "$WORK" commit -q --amend --no-edit
gl_capture split HEAD -m comma -p --agent
FP="$(json_fingerprint)"
gl_capture split HEAD -m comma -p --hunks "a,b.txt:1,multi.txt:1" --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "split_comma_list_exit"
assert_contains "$OUT" "one \`--hunks\` per id" "split_comma_list_msg"

describe "agent mode: a commit with nothing pickable is not reported as cancelled"
setup_repo_with_remote
echo x > "$WORK/mode.txt"
git -C "$WORK" add mode.txt
git -C "$WORK" commit -q -m "Add mode.txt"
# Stage the mode bit through the index: `chmod +x` alone stages nothing
# where core.filemode is false (Windows), leaving nothing to commit.
git -C "$WORK" update-index --chmod=+x mode.txt
git -C "$WORK" commit -q -m "Make it executable"

gl_capture split HEAD -m nope -p --agent
assert_eq "1" "$CODE" "nothing_pickable_exit"
assert_contains "$OUT" "No hunks to select in" "nothing_pickable_msg"
assert_not_contains "$OUT" "Cancelled" "nothing_pickable_not_cancelled"

describe "agent mode: split -p must leave a hunk for the second commit"
setup_two_hunk_commit
gl_capture split HEAD -m all -p --agent
FP="$(json_fingerprint)"
gl_capture split HEAD -m all -p --hunks multi.txt:1 --hunks multi.txt:2 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "split_all_hunks_exit"
assert_contains "$OUT" "at least one hunk for the second commit" "split_all_hunks_msg"
assert_head_msg "Change two regions" "split_all_hunks_no_mutation"

# The target is re-resolved on the replay, so a revspec that has come to name a
# different commit must be refused, not amended into whatever now sits there.
describe "agent mode: a target that moved between the two calls is refused"
setup_repo_with_remote
seq 1 40 > "$WORK/f.txt"
git -C "$WORK" add f.txt
git -C "$WORK" commit -q -m "A"
commit_file "B" b.txt
perl -pi -e 's/^3$/THREE/' "$WORK/f.txt"
git -C "$WORK" add f.txt
git -C "$WORK" commit -q -m "C"
source_sha="$(head_hash)"

gl_capture fold -p "$source_sha" 'HEAD~2' --agent
assert_eq "10" "$CODE" "moved_target_listed_exit"
FP="$(json_fingerprint)"

commit_file "D" d.txt   # HEAD~2 now names B, not A

gl_capture fold -p "$source_sha" 'HEAD~2' --hunks f.txt:1 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "moved_target_exit"
assert_contains "$OUT" "hunks changed since" "moved_target_msg"
assert_contains "$(show_patch "$source_sha")" "+THREE" "moved_target_source_intact"
assert_not_contains "$(show_patch HEAD~2)" "+THREE" "moved_target_not_amended"

# `--hunks` is not agent-only, and the non-agent path has its own messages.
describe "--hunks works without --agent"
setup_two_hunk_commit
gl_capture split HEAD -m "first region" -p --agent
FP="$(json_fingerprint)"

gl_capture split HEAD -m "first region" -p --hunks multi.txt:1 --hunks-from "$FP"
assert_exit_ok "$CODE" "no_agent_hunks_exit"
assert_msg_at 1 "first region" "no_agent_hunks_split"
assert_contains "$(show_patch HEAD~1)" "+THREE" "no_agent_hunks_content"

describe "--hunks without --agent refuses a stale fingerprint"
setup_two_hunk_commit
gl_capture split HEAD -m nope -p --hunks multi.txt:1 --hunks-from deadbeefcafe
assert_eq "1" "$CODE" "no_agent_stale_exit"
assert_contains "$OUT" "hunks changed since" "no_agent_stale_msg"
assert_head_msg "Change two regions" "no_agent_stale_no_mutation"

# Nobody opened a picker, so `Cancelled` would be the wrong answer here.
describe "--hunks without --agent on a commit with nothing to pick"
setup_repo_with_remote
echo x > "$WORK/mode.txt"
git -C "$WORK" add mode.txt
git -C "$WORK" commit -q -m "Add mode.txt"
git -C "$WORK" update-index --chmod=+x mode.txt
git -C "$WORK" commit -q -m "Make it executable"

gl_capture split HEAD -m nope -p --hunks mode.txt:1 --hunks-from deadbeefcafe
assert_eq "1" "$CODE" "no_agent_empty_exit"
assert_contains "$OUT" "No hunks to select in" "no_agent_empty_msg"
assert_not_contains "$OUT" "Cancelled" "no_agent_empty_not_cancelled"

describe "agent mode: split takes binary and deleted files whole"
setup_repo_with_remote
printf '\x00\x01old\x00' > "$WORK/blob.bin"
echo doomed > "$WORK/gone.txt"
seq 1 10 > "$WORK/plain.txt"
git -C "$WORK" add blob.bin gone.txt plain.txt
git -C "$WORK" commit -q -m "Add three files"
printf '\x00\x01new\x00' > "$WORK/blob.bin"
rm "$WORK/gone.txt"
perl -pi -e 's/^5$/FIVE/' "$WORK/plain.txt"
git -C "$WORK" add -A
git -C "$WORK" commit -q -m "Touch three files"

gl_capture split HEAD -m "binary and deletion" -p --agent
assert_eq "10" "$CODE" "split_whole_listed_exit"
assert_contains "$OUT" '"options":["blob.bin:1","gone.txt:1","plain.txt:1"]' "split_whole_all_selectable"
FP="$(json_fingerprint)"

gl_capture split HEAD -m "binary and deletion" -p --hunks blob.bin:1 --hunks gone.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "split_whole_exit"
assert_msg_at 1 "binary and deletion" "split_whole_first"
assert_contains "$(git -C "$WORK" show --stat HEAD~1)" "blob.bin" "split_whole_first_binary"
assert_contains "$(git -C "$WORK" show --stat HEAD~1)" "gone.txt" "split_whole_first_deletion"
assert_contains "$(git -C "$WORK" show --stat HEAD)" "plain.txt" "split_whole_second_text"
assert_not_contains "$(git -C "$WORK" show --stat HEAD)" "blob.bin" "split_whole_second_clean"

describe "agent mode: fold -p uncommits a binary file whole"
setup_repo_with_remote
printf '\x00\x01\x02binary\x00' > "$WORK/blob.bin"
seq 1 10 > "$WORK/plain.txt"
git -C "$WORK" add blob.bin plain.txt
git -C "$WORK" commit -q -m "Add binary and text"

gl_capture fold -p HEAD zz --agent
assert_eq "10" "$CODE" "binary_listed_exit"
assert_contains "$OUT" '"options":["blob.bin:1","plain.txt:1"]' "binary_listed_options"
FP="$(json_fingerprint)"

gl_capture fold -p HEAD zz --hunks blob.bin:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "binary_uncommit_exit"
assert_not_contains "$(git -C "$WORK" show --stat HEAD)" "blob.bin" "binary_uncommit_left_commit"
assert_contains "$(git -C "$WORK" show --stat HEAD)" "plain.txt" "binary_uncommit_text_stays"
assert_contains "$(git -C "$WORK" status --porcelain)" "?? blob.bin" "binary_uncommit_untracked"

describe "agent mode: fold -p moves a binary file between commits"
setup_repo_with_remote
printf '\x00\x01old\x00' > "$WORK/only.bin"
git -C "$WORK" add only.bin
git -C "$WORK" commit -q -m "Add binary"
echo target > "$WORK/t.txt"
git -C "$WORK" add t.txt
git -C "$WORK" commit -q -m "Target"
printf '\x00\x01new\x00' > "$WORK/only.bin"
echo source > "$WORK/s.txt"
git -C "$WORK" add only.bin s.txt
git -C "$WORK" commit -q -m "Change binary"

gl_capture fold -p HEAD HEAD~1 --agent
assert_eq "10" "$CODE" "binary_move_listed_exit"
FP="$(json_fingerprint)"

gl_capture fold -p HEAD HEAD~1 --hunks only.bin:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "binary_move_exit"
assert_not_contains "$(git -C "$WORK" show --stat HEAD)" "only.bin" "binary_move_left_source"
assert_contains "$(git -C "$WORK" show --stat HEAD~1)" "only.bin" "binary_move_reached_target"
assert_eq "$(printf '\x00\x01new\x00' | git hash-object --stdin)" \
    "$(git -C "$WORK" rev-parse HEAD:only.bin)" "binary_move_content"
assert_eq "" "$(git -C "$WORK" status --porcelain)" "binary_move_clean"

# The case `-p` exists for: two logical changes in one file, no TUI available.
describe "agent mode: -p works over working-tree changes"
setup_two_hunk_commit
perl -pi -e 's/^5$/FIVE/; s/^35$/THIRTYFIVE/' "$WORK/multi.txt"

gl_capture fold -p multi.txt HEAD --agent
assert_eq "10" "$CODE" "worktree_patch_listed_exit"
assert_contains "$OUT" '"options":["multi.txt:1","multi.txt:2"]' "worktree_patch_listed_options"
assert_contains "$OUT" "+FIVE" "worktree_patch_listed_diff"
FP="$(json_fingerprint)"

gl_capture fold -p multi.txt HEAD --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "worktree_patch_folded_exit"
assert_contains "$(show_patch HEAD)" "+FIVE" "worktree_patch_folded_hunk"
assert_contains "$(diff_patch)" "+THIRTYFIVE" "worktree_patch_left_the_rest"

# The listing names a binary without its content, so only the fingerprint can
# tell that the file changed on disk since; `git add` would stage the new one.
describe "agent mode: a binary edited since its listing is refused"
setup_repo_with_remote
printf '\x00\x01first' > "$WORK/logo.bin"
gl_capture add -p logo.bin --agent
assert_contains "$OUT" '"diff":"(binary file)"' "worktree_binary_listed_label"
FP="$(json_fingerprint)"
printf '\x00\x01second' > "$WORK/logo.bin"

gl_capture add -p logo.bin --hunks logo.bin:1 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "worktree_binary_stale_exit"
assert_contains "$OUT" "hunks changed since" "worktree_binary_stale_msg"
assert_eq "" "$(git -C "$WORK" diff --cached --name-only)" "worktree_binary_stale_nothing_staged"

# A new file is taken whole, so the listing names its size rather than carrying
# a copy of it — both before and after it is staged.
describe "agent mode: a new file is listed by size, not by content"
setup_repo_with_remote
echo "UNIQUEMARKER" > "$WORK/new.txt"
seq 1 200 | sed 's/^/line /' >> "$WORK/new.txt"

gl_capture add -p new.txt --agent
assert_eq "10" "$CODE" "untracked_listed_exit"
assert_contains "$OUT" '"diff":"(new file, 201 line(s))"' "untracked_listed_summary"
assert_not_contains "$OUT" "UNIQUEMARKER" "untracked_listed_no_content"
FP="$(json_fingerprint)"

# Once the file is in the index its text is git's diff of the indexed content,
# which a filter can make something else than the file on disk, so there is no
# summary from there on.
git -C "$WORK" add -N new.txt
gl_capture add -p new.txt --agent
assert_eq "10" "$CODE" "intent_to_add_listed_exit"
assert_not_contains "$OUT" "new file, " "intent_to_add_no_summary"
assert_contains "$OUT" "UNIQUEMARKER" "intent_to_add_verbatim"
git -C "$WORK" reset -q

gl_capture add -p new.txt --hunks new.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "untracked_staged_exit"
assert_contains "$(git -C "$WORK" diff --cached --name-only)" "new.txt" "untracked_staged"

# Listing it again now that it is staged: verbatim, for the same reason.
gl_capture add -p new.txt --agent
assert_eq "10" "$CODE" "staged_new_listed_exit"
assert_not_contains "$OUT" "new file, " "staged_new_listed_no_summary"
assert_contains "$OUT" "UNIQUEMARKER" "staged_new_listed_verbatim"

# Edited again in the worktree, the file on disk is no longer the staged entry:
# UNIQUEMARKER is staged and gone from disk, so summarizing it would point at a
# file that has lost it. Both hunks stay verbatim.
perl -pi -e 's/^UNIQUEMARKER$/REWRITTEN/' "$WORK/new.txt"
echo "TAILMARKER" >> "$WORK/new.txt"
gl_capture add -p new.txt --agent
assert_eq "10" "$CODE" "staged_new_edited_exit"
assert_not_contains "$OUT" "new file, " "staged_new_edited_no_summary"
assert_contains "$OUT" "UNIQUEMARKER" "staged_new_edited_staged_verbatim"
assert_contains "$OUT" "TAILMARKER" "staged_new_edited_hunk_verbatim"

# Deleted from the worktree, the staged content is all the agent has left to
# decide on, so it stays in the listing too.
rm -f "$WORK/new.txt"
gl_capture add -p new.txt --agent
assert_eq "10" "$CODE" "staged_new_gone_exit"
assert_not_contains "$OUT" "new file, " "staged_new_gone_no_summary"
assert_contains "$OUT" "UNIQUEMARKER" "staged_new_gone_verbatim"
assert_contains "$OUT" "(file deleted)" "staged_new_gone_deletion_entry"
git -C "$WORK" reset -q

# Back to untracked, an edit the summary cannot see still invalidates the ids:
# the file is rebuilt at the same 201 lines with one of them changed, so only
# the fingerprint notices. The reset above is what makes this the untracked
# listing's fingerprint again.
echo "UNIQUEMARKER" > "$WORK/new.txt"
seq 1 200 | sed 's/^/line /' >> "$WORK/new.txt"
perl -pi -e 's/^line 100$/line ONEHUNDRED/' "$WORK/new.txt"
assert_eq "201" "$(wc -l < "$WORK/new.txt")" "untracked_edited_same_line_count"
gl_capture add -p new.txt --hunks new.txt:1 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "untracked_edited_exit"
assert_contains "$OUT" "The hunks changed since the listing" "untracked_edited_msg"

# A clean filter makes the indexed content something else than the file on
# disk. Summarizing it would send the agent to a 200-line file to read what the
# listing called one line.
describe "agent mode: a filtered new file is never summarized"
setup_repo_with_remote
git -C "$WORK" config filter.fake.clean 'sh -c "cat >/dev/null; echo POINTER"'
echo "big.bin filter=fake" > "$WORK/.gitattributes"
git -C "$WORK" add .gitattributes
git -C "$WORK" commit -q -m "add filter attrs"
seq 1 200 | sed 's/^/line /' > "$WORK/big.bin"

# Untracked, loom reads the bytes off disk, so the summary is the real file.
gl_capture add -p big.bin --agent
assert_contains "$OUT" '"diff":"(new file, 200 line(s))"' "filtered_untracked_summary"

# Staged, the entry is the one-line pointer and the disk holds 200 lines.
git -C "$WORK" add big.bin
assert_eq "POINTER" "$(git -C "$WORK" cat-file -p :big.bin)" "filtered_blob_is_a_pointer"
gl_capture add -p big.bin --agent
assert_eq "10" "$CODE" "filtered_staged_exit"
assert_not_contains "$OUT" "new file, " "filtered_staged_no_summary"
assert_contains "$OUT" "POINTER" "filtered_staged_verbatim"

# A staged change with no hunk to show — a mode-only one — is not in the
# listing either, so it is not the picker's to fold.
describe "agent mode: fold -p leaves a staged change its picker could not show"
setup_two_hunk_commit
printf '#!/bin/sh\necho hi\n' > "$WORK/m.sh"
git -C "$WORK" add m.sh
git -C "$WORK" commit -q -m "add a script"
chmod +x "$WORK/m.sh"
git -C "$WORK" add m.sh
perl -pi -e 's/^5$/FIVE/' "$WORK/multi.txt"

gl_capture fold -p zz HEAD --agent
assert_not_contains "$OUT" "m.sh" "fold_modeonly_not_listed"
FP="$(json_fingerprint)"

gl_capture fold -p zz HEAD --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_modeonly_exit"
assert_eq "100644" "$(git -C "$WORK" ls-tree HEAD m.sh | awk '{print $1}')" "fold_modeonly_not_folded"
assert_contains "$(git -C "$WORK" diff --cached --name-only)" "m.sh" "fold_modeonly_still_staged"

# The picker filters to <files>, so a file staged outside it was never listed
# and nobody picked it: it stays staged and out of the commit.
describe "agent mode: fold -p folds only the files its picker listed"
setup_two_hunk_commit
echo "unrelated work" > "$WORK/other.txt"
git -C "$WORK" add other.txt
perl -pi -e 's/^5$/FIVE/' "$WORK/multi.txt"

gl_capture fold -p multi.txt HEAD --agent
assert_not_contains "$OUT" "other.txt" "fold_scope_not_listed"
FP="$(json_fingerprint)"

gl_capture fold -p multi.txt HEAD --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_scope_exit"
assert_contains "$(show_patch HEAD)" "+FIVE" "fold_scope_folded_the_pick"
assert_not_contains "$(show_patch HEAD)" "unrelated work" "fold_scope_no_leak"
assert_contains "$(git -C "$WORK" diff --cached --name-only)" "other.txt" "fold_scope_kept_staged"

# `--hunks` is the whole selection, so a staged hunk left out comes back out.
describe "agent mode: add -p --hunks replaces the whole selection"
setup_repo_with_remote
seq 1 40 > "$WORK/f.txt"
git -C "$WORK" add f.txt
git -C "$WORK" commit -q -m base
perl -pi -e 's/^3$/THREE/; s/^37$/THIRTYSEVEN/' "$WORK/f.txt"

gl_capture add -p f.txt --agent
FP="$(json_fingerprint)"
gl_capture add -p f.txt --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "add_patch_staged_exit"
assert_contains "$(diff_patch --cached)" "+THREE" "add_patch_staged_first"

gl_capture add -p f.txt --agent
assert_contains "$OUT" '"staged":true' "add_patch_marks_staged"
FP="$(json_fingerprint)"

gl_capture add -p f.txt --hunks f.txt:2 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "add_patch_replace_exit"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN" "add_patch_replace_staged"
assert_not_contains "$(diff_patch --cached)" "+THREE" "add_patch_replace_unstaged"

# Unstaging reverse-applies to the index alone, and INDEXONLY is in the index
# and nowhere else once the working tree changed it again.
describe "agent mode: add -p refuses to unstage what only the index holds"
setup_repo_with_remote
seq 1 40 > "$WORK/f.txt"
git -C "$WORK" add f.txt
git -C "$WORK" commit -q -m base
perl -pi -e 's/^3$/INDEXONLY/' "$WORK/f.txt"
git -C "$WORK" add f.txt
perl -pi -e 's/^INDEXONLY$/WORKTREE/; s/^37$/THIRTYSEVEN/' "$WORK/f.txt"

gl_capture add -p f.txt --agent
FP="$(json_fingerprint)"
gl_capture add -p f.txt --hunks f.txt:3 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "index_only_refused_exit"
assert_contains "$OUT" 'Keep `f.txt:1`' "index_only_refused_msg"
assert_contains "$(diff_patch --cached)" "+INDEXONLY" "index_only_kept"

gl_capture add -p f.txt --hunks f.txt:1 --hunks f.txt:3 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "index_only_kept_exit"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN" "index_only_staged_the_pick"
assert_contains "$(diff_patch --cached)" "+INDEXONLY" "index_only_still_staged"

# The stage patch is diffed against the index before the unstaging, whose
# change sits in its context: refused, and the unstaging before it undone.
describe "agent mode: add -p refuses to swap two nearby hunks"
setup_repo_with_remote
seq 1 10 > "$WORK/f.txt"
git -C "$WORK" add f.txt
git -C "$WORK" commit -q -m base
perl -pi -e 's/^3$/THREE/' "$WORK/f.txt"
git -C "$WORK" add f.txt
perl -pi -e 's/^5$/FIVE/' "$WORK/f.txt"

gl_capture add -p f.txt --agent
FP="$(json_fingerprint)"
gl_capture add -p f.txt --hunks f.txt:2 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "swap_exit"
assert_contains "$OUT" "no longer apply" "swap_msg"
assert_contains "$(diff_patch --cached)" "+THREE" "swap_kept_index"
assert_not_contains "$(diff_patch --cached)" "+FIVE" "swap_staged_nothing"
assert_eq "" "$(find "$WORK/.git" -maxdepth 1 -name 'index.lock')" "swap_no_lock_left"

# A looser match would find `bar;` in fn a and stage `baz;` there.
describe "agent mode: add -p never places a hunk by a looser match"
setup_repo_with_remote
printf 'fn a() {\n    foo;\n    bar;\n}\nfn b() {\n    old;\n    bar;\n}\n' > "$WORK/f.rs"
git -C "$WORK" add f.rs
git -C "$WORK" commit -q -m base
perl -pi -e 's/old;/foo;/' "$WORK/f.rs"
git -C "$WORK" add f.rs
perl -pi -e 's/bar;/baz;/ if $. == 7' "$WORK/f.rs"

gl_capture add -p f.rs --agent
FP="$(json_fingerprint)"
gl_capture add -p f.rs --hunks f.rs:2 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "fuzz_refused_exit"
assert_not_contains "$(git -C "$WORK" show :f.rs)" "baz" "fuzz_nothing_misplaced"
assert_contains "$(diff_patch --cached)" "+    foo;" "fuzz_kept_index"

# Kept staged, the left-out hunk travels as a blob, so the index-only refusal
# `add -p` needs would only stand in the way here.
describe "agent mode: commit -p may leave out what only the index holds"
setup_repo_with_remote
seq 1 40 > "$WORK/f.txt"
echo g > "$WORK/g.txt"
git -C "$WORK" add f.txt g.txt
git -C "$WORK" commit -q -m base
perl -pi -e 's/^5$/INDEXONLY/' "$WORK/f.txt"
git -C "$WORK" add f.txt
perl -pi -e 's/^INDEXONLY$/WORKTREE/' "$WORK/f.txt"
echo gg > "$WORK/g.txt"

gl_capture commit -i -m msg -p --agent
FP="$(json_fingerprint)"
gl_capture commit -i -m msg -p --hunks g.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "commit_index_only_exit"
assert_contains "$(show_patch HEAD)" "+gg" "commit_index_only_committed_pick"
assert_not_contains "$(show_patch HEAD)" "INDEXONLY" "commit_index_only_not_committed"
assert_contains "$(diff_patch --cached)" "+INDEXONLY" "commit_index_only_still_staged"

# Unstaged whole, a new file's index blob goes, and the worktree hunks were
# diffed against it.
describe "agent mode: commit -p says why a hunk of a file it unstages cannot apply"
setup_repo_with_remote
seq 1 10 > "$WORK/n.txt"
git -C "$WORK" add n.txt
perl -pi -e 's/^5$/FIVE/' "$WORK/n.txt"

gl_capture commit -i -m msg -p n.txt --agent
FP="$(json_fingerprint)"
gl_capture commit -i -m msg -p n.txt --hunks n.txt:2 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "unstaged_whole_exit"
assert_contains "$OUT" "no longer apply" "unstaged_whole_msg"
assert_contains "$(git -C "$WORK" diff --cached --name-status)" "A	n.txt" "unstaged_whole_kept_index"

# A stash pop that conflicts leaves unmerged paths with no operation to
# finish, and `git write-tree` refuses such an index.
describe "agent mode: add -p unstages beside an unmerged path"
setup_repo_with_remote
seq 1 40 > "$WORK/f.txt"
echo a > "$WORK/c.txt"
git -C "$WORK" add f.txt c.txt
git -C "$WORK" commit -q -m base
echo s > "$WORK/c.txt"
git -C "$WORK" stash -q
echo t > "$WORK/c.txt"
git -C "$WORK" commit -q -am t
git -C "$WORK" stash pop -q >/dev/null 2>&1 || true
perl -pi -e 's/^3$/THREE/; s/^37$/THIRTYSEVEN/' "$WORK/f.txt"
git -C "$WORK" add f.txt

gl_capture add -p f.txt --agent
FP="$(json_fingerprint)"
gl_capture add -p f.txt --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "unmerged_beside_exit"
assert_not_contains "$(diff_patch --cached f.txt)" "THIRTYSEVEN" "unmerged_beside_unstaged"

# The listing is pre-flight, so a file staged before it must still be staged
# after it — including content that exists only in the index.
describe "agent mode: commit -p leaves the index it was handed"
setup_repo_with_remote
seq 1 40 > "$WORK/f.txt"
echo "committed" > "$WORK/other.txt"
git -C "$WORK" add f.txt other.txt
git -C "$WORK" commit -q -m base
git -C "$WORK" push -q origin "integration:$BASE_BRANCH"
git -C "$WORK" fetch -q origin
echo "index only" > "$WORK/other.txt"
git -C "$WORK" add other.txt
echo "committed" > "$WORK/other.txt"
perl -pi -e 's/^3$/THREE/; s/^37$/THIRTYSEVEN/' "$WORK/f.txt"

gl_capture commit -b feat -m msg -p f.txt --agent
assert_eq "10" "$CODE" "commit_patch_listed_exit"
assert_contains "$(diff_patch --cached)" "index only" "commit_patch_listing_kept_index"
FP="$(json_fingerprint)"

gl_capture commit -b feat -m msg -p f.txt --hunks f.txt:99 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "commit_patch_bad_id_exit"
assert_contains "$(diff_patch --cached)" "index only" "commit_patch_bad_id_kept_index"

gl_capture commit -b feat -m msg -p f.txt --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "commit_patch_exit"
assert_contains "$(show_patch feat)" "+THREE" "commit_patch_took_the_hunk"
assert_not_contains "$(show_patch feat)" "THIRTYSEVEN" "commit_patch_left_the_rest"
assert_not_contains "$(git -C "$WORK" show feat --name-only --pretty=)" "other.txt" "commit_patch_no_leak"
assert_contains "$(diff_patch --cached)" "index only" "commit_patch_kept_index"

# With neither -b nor -i the branch prompt comes first, keeping -p in its hint:
# answered without it, the re-run stages whole files and undoes the picking.
describe "agent mode: commit -p asks for the branch before it stages"
gl_capture commit -m msg -p f.txt --agent
assert_eq "10" "$CODE" "commit_patch_branch_first_exit"
assert_contains "$OUT" '"prompt":"Select target branch"' "commit_patch_branch_first_prompt"
assert_contains "$OUT" 'loom commit -b <branch> -m msg -p f.txt' "commit_patch_branch_first_hint"
assert_not_contains "$(git -C "$WORK" diff --cached --name-only)" "f.txt" "commit_patch_branch_first_staged_nothing"

gl_capture commit -i -m msg -p f.txt --agent
assert_eq "10" "$CODE" "commit_patch_integration_exit"
assert_contains "$OUT" '"prompt":"Select hunks"' "commit_patch_integration_lists"

# The fingerprint does not cover the branch, so a replay can carry a bad one.
describe "agent mode: commit -p refuses a bad -b before it stages"
git -C "$WORK" branch lone "$(git -C "$WORK" commit-tree 'HEAD^{tree}' -m lone)"
gl_capture commit -i -m msg -p f.txt --agent
FP="$(json_fingerprint)"
gl_capture commit -b lone -m msg -p f.txt --hunks f.txt:1 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "commit_patch_bad_branch_exit"
assert_contains "$OUT" "not woven into the integration branch" "commit_patch_bad_branch_msg"
assert_not_contains "$(git -C "$WORK" diff --cached --name-only)" "f.txt" "commit_patch_bad_branch_staged_nothing"

describe "agent mode: commit -p hints keep -p and the forwarded git args"
gl_capture commit -p f.txt --agent -- --no-verify
assert_eq "10" "$CODE" "commit_patch_message_exit"
assert_contains "$OUT" "-m <message> [-b <branch> | -i] -p f.txt -- --no-verify" "commit_patch_message_hint"

gl_capture commit -m msg -p f.txt --agent -- --no-verify
assert_contains "$OUT" "-m msg -p f.txt -- --no-verify (a new name" "commit_patch_branch_hint_git_args"

gl_capture commit -i -m msg -p f.txt --agent -- --no-verify
assert_contains "$OUT" "--hunks-from $(json_fingerprint) -- --no-verify" "commit_patch_listing_hint_git_args"
assert_contains "$OUT" "loom commit -i -m msg -p f.txt --hunks" "commit_patch_listing_hint_raw_message"

# For commit and fold, a staged id left out means "not in this one", not
# "unstage it": it stays staged, as every other staged file does.
setup_left_out_repo() {
    setup_repo_with_remote
    seq 1 40 > "$WORK/f.txt"
    git -C "$WORK" add f.txt
    git -C "$WORK" commit -q -m base
    git -C "$WORK" push -q origin "integration:$BASE_BRANCH"
    git -C "$WORK" fetch -q origin
    perl -pi -e 's/^3$/THREE/; s/^37$/THIRTYSEVEN/' "$WORK/f.txt"
    echo new > "$WORK/new.txt"
    git -C "$WORK" add f.txt new.txt
}

describe "agent mode: commit -i -p keeps a staged hunk it left out staged"
setup_left_out_repo
gl_capture commit -i -m msg -p --agent
FP="$(json_fingerprint)"
gl_capture commit -i -m msg -p --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "loose_left_out_exit"
assert_contains "$(show_patch HEAD)" "+THREE" "loose_left_out_committed_pick"
assert_not_contains "$(show_patch HEAD)" "THIRTYSEVEN" "loose_left_out_not_committed"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN" "loose_left_out_still_staged"
assert_contains "$(git -C "$WORK" diff --cached --name-status)" "A	new.txt" "loose_left_out_file_still_staged"

describe "agent mode: commit -b -p keeps a staged hunk it left out staged"
setup_left_out_repo
gl_capture commit -b feat -m msg -p --agent
FP="$(json_fingerprint)"
gl_capture commit -b feat -m msg -p --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "branch_left_out_exit"
assert_contains "$(show_patch feat)" "+THREE" "branch_left_out_committed_pick"
assert_not_contains "$(show_patch feat)" "THIRTYSEVEN" "branch_left_out_not_committed"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN" "branch_left_out_still_staged"
assert_contains "$(git -C "$WORK" diff --cached --name-status)" "A	new.txt" "branch_left_out_file_still_staged"

# The target, then a commit on top of it, then two staged hunks on f.txt.
setup_left_out_fold() {
    setup_left_out_repo
    git -C "$WORK" commit -q -m target
    echo other > "$WORK/other.txt"
    git -C "$WORK" add other.txt
    git -C "$WORK" commit -q -m on-top
    perl -pi -e 's/^THREE$/THREE-C/; s/^THIRTYSEVEN$/THIRTYSEVEN-C/' "$WORK/f.txt"
    git -C "$WORK" add f.txt
}

describe "agent mode: fold -p into HEAD keeps a staged hunk it left out staged"
setup_left_out_fold
gl_capture fold -p zz HEAD --agent
FP="$(json_fingerprint)"
gl_capture fold -p zz HEAD --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_head_left_out_exit"
assert_contains "$(show_patch HEAD)" "+THREE-C" "fold_head_left_out_folded_pick"
assert_not_contains "$(show_patch HEAD)" "THIRTYSEVEN-C" "fold_head_left_out_not_folded"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN-C" "fold_head_left_out_still_staged"

describe "agent mode: fold -p into an older commit keeps a staged hunk it left out staged"
setup_left_out_fold
gl_capture fold -p zz HEAD~1 --agent
FP="$(json_fingerprint)"
gl_capture fold -p zz HEAD~1 --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_older_left_out_exit"
assert_contains "$(git -C "$WORK" show HEAD~1:f.txt)" "THREE-C" "fold_older_left_out_folded_pick"
assert_not_contains "$(git -C "$WORK" show HEAD~1:f.txt)" "THIRTYSEVEN-C" "fold_older_left_out_not_folded"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN-C" "fold_older_left_out_still_staged"

# The left-out hunk rides in the paused commit's state, so it must come back
# on `loom continue` and on `loom abort` alike. Conflict topology as in
# test_commit.sh: the relocated commit's base is not the feature branch's.
setup_left_out_conflict() {
    setup_repo_with_remote
    seq 1 40 > "$WORK/f.txt"
    git -C "$WORK" add f.txt
    git -C "$WORK" commit -q -m base
    git -C "$WORK" push -q origin "integration:$BASE_BRANCH"
    git -C "$WORK" fetch -q origin
    create_feature_branch "g-left-out"
    switch_to g-left-out
    printf "feature\n" > "$WORK/shared.txt"
    git -C "$WORK" add shared.txt
    git -C "$WORK" commit -q -m "Feature commit"
    switch_to integration
    weave_branch "g-left-out"
    printf "integration\n" > "$WORK/shared.txt"
    git -C "$WORK" add shared.txt
    git -C "$WORK" commit -q -m "Integration commit"
    printf "feature-v2\n" > "$WORK/shared.txt"
    perl -pi -e 's/^3$/THREE/; s/^37$/THIRTYSEVEN/' "$WORK/f.txt"
    git -C "$WORK" add shared.txt f.txt
}

describe "agent mode: commit -p keeps a left-out hunk staged across a paused rebase"
setup_left_out_conflict
gl_capture commit -b g-left-out -m v2 -p --agent
FP="$(json_fingerprint)"
gl_capture commit -b g-left-out -m v2 -p --hunks f.txt:1 --hunks shared.txt:1 --hunks-from "$FP" --agent
assert_state_file "left_out_pause_state"
printf "resolved\n" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
gl_capture continue --agent
printf "integration\n" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
gl_capture continue --agent
assert_exit_ok "$CODE" "left_out_continue_exit"
assert_no_state_file "left_out_continue_state_removed"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN" "left_out_continue_still_staged"
assert_not_contains "$(show_patch g-left-out)" "THIRTYSEVEN" "left_out_continue_not_committed"

describe "agent mode: commit -p abort puts back a left-out hunk staged"
setup_left_out_conflict
index_before="$(diff_patch --cached)"
gl_capture commit -b g-left-out -m v2 -p --agent
FP="$(json_fingerprint)"
gl_capture commit -b g-left-out -m v2 -p --hunks f.txt:1 --hunks shared.txt:1 --hunks-from "$FP" --agent
assert_state_file "left_out_abort_state"
gl_capture abort --agent
assert_exit_ok "$CODE" "left_out_abort_exit"
assert_eq "$index_before" "$(diff_patch --cached)" "left_out_abort_index_restored"

# The fixup is diffed against the commit on top, so moving it under that one
# conflicts on g.txt.
setup_left_out_fold_conflict() {
    setup_left_out_repo
    git -C "$WORK" commit -q -m base2
    echo t > "$WORK/g.txt"
    git -C "$WORK" add g.txt
    git -C "$WORK" commit -q -m target
    echo u > "$WORK/g.txt"
    git -C "$WORK" commit -q -am on-top
    echo v > "$WORK/g.txt"
    perl -pi -e 's/^THREE$/THREE-C/; s/^THIRTYSEVEN$/THIRTYSEVEN-C/' "$WORK/f.txt"
    git -C "$WORK" add g.txt f.txt
}

describe "agent mode: fold -p keeps a left-out hunk staged across a paused rebase"
setup_left_out_fold_conflict
gl_capture fold -p zz HEAD~1 --agent
FP="$(json_fingerprint)"
gl_capture fold -p zz HEAD~1 --hunks f.txt:1 --hunks g.txt:1 --hunks-from "$FP" --agent
assert_state_file "fold_left_out_pause_state"
while [ -f "$WORK/.git/loom/state.json" ]; do
    echo v > "$WORK/g.txt"
    git -C "$WORK" add g.txt
    gl_capture continue --agent
    [ "$CODE" = 0 ] || [ "$CODE" = 10 ] || [ -f "$WORK/.git/loom/state.json" ] || break
done
assert_exit_ok "$CODE" "fold_left_out_continue_exit"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN-C" "fold_left_out_continue_still_staged"
assert_not_contains "$(git -C "$WORK" show HEAD~1:f.txt)" "THIRTYSEVEN-C" "fold_left_out_continue_not_folded"

describe "agent mode: fold -p abort puts back a left-out hunk staged"
setup_left_out_fold_conflict
gl_capture fold -p zz HEAD~1 --agent
FP="$(json_fingerprint)"
gl_capture fold -p zz HEAD~1 --hunks f.txt:1 --hunks g.txt:1 --hunks-from "$FP" --agent
assert_state_file "fold_left_out_abort_state"
gl_capture abort --agent
assert_exit_ok "$CODE" "fold_left_out_abort_exit"
assert_contains "$(diff_patch --cached)" "+THIRTYSEVEN-C" "fold_left_out_abort_still_staged"
assert_contains "$(cat "$WORK/f.txt")" "THREE-C" "fold_left_out_abort_pick_kept"

# Unfiltered, so only what the picker returned keeps the rest out of the commit.
describe "agent mode: commit -p leaves out a staged change its picker could not show"
setup_repo_with_remote
seq 1 40 > "$WORK/f.txt"
printf '#!/bin/sh\necho hi\n' > "$WORK/m.sh"
git -C "$WORK" add f.txt m.sh
git -C "$WORK" commit -q -m base
chmod +x "$WORK/m.sh"
git -C "$WORK" add m.sh
perl -pi -e 's/^3$/THREE/' "$WORK/f.txt"

gl_capture commit -i -m msg -p --agent
assert_not_contains "$OUT" "m.sh" "commit_modeonly_not_listed"
FP="$(json_fingerprint)"

gl_capture commit -i -m msg -p --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "commit_modeonly_exit"
assert_contains "$(show_patch HEAD)" "+THREE" "commit_modeonly_took_the_hunk"
assert_eq "100644" "$(git -C "$WORK" ls-tree HEAD m.sh | awk '{print $1}')" "commit_modeonly_not_committed"
assert_contains "$(git -C "$WORK" diff --cached --name-only)" "m.sh" "commit_modeonly_still_staged"

describe "agent mode: commit -p lists a staged empty file"
setup_repo_with_remote
seq 1 40 > "$WORK/f.txt"
git -C "$WORK" add f.txt
git -C "$WORK" commit -q -m base
: > "$WORK/empty.py"
git -C "$WORK" add empty.py
perl -pi -e 's/^3$/THREE/' "$WORK/f.txt"

gl_capture commit -i -m msg -p --agent
assert_contains "$OUT" '"id":"empty.py:1"' "commit_empty_listed"
FP="$(json_fingerprint)"

gl_capture commit -i -m msg -p --hunks empty.py:1 --hunks f.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "commit_empty_exit"
assert_contains "$(git -C "$WORK" show HEAD --name-only --pretty=)" "empty.py" "commit_empty_committed"

# The target commit is part of the fingerprint, so a moved HEAD is refused
# rather than folded into whatever now sits there.
describe "agent mode: fold -p over the working tree fingerprints its target"
setup_two_hunk_commit
perl -pi -e 's/^5$/FIVE/' "$WORK/multi.txt"
gl_capture fold -p multi.txt HEAD --agent
FP="$(json_fingerprint)"
git -C "$WORK" commit -q --allow-empty -m "HEAD moved under the agent"

gl_capture fold -p multi.txt HEAD --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "worktree_patch_moved_target_exit"
assert_contains "$OUT" "The hunks changed since the listing" "worktree_patch_moved_target_msg"
assert_not_contains "$(show_patch HEAD)" "+FIVE" "worktree_patch_moved_target_untouched"

gl_capture fold -p multi.txt HEAD --agent -- --no-verify
assert_contains "$OUT" "--hunks-from $(json_fingerprint) -- --no-verify" "worktree_patch_hint_git_args"

describe "agent mode: fold -p with a branch target reports the source rule"
gl_capture fold -p HEAD feature-a --agent
assert_eq "1" "$CODE" "fold_branch_target_exit"
assert_contains "$OUT" "does not support commit or branch sources" "fold_branch_target_msg"

pass
