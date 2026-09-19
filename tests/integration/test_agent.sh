#!/usr/bin/env bash
# Integration tests for: agent mode (--agent / LOOM_AGENT) and loom agent init
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

# The JSON status is the last line of stderr; gl_capture merges stdout+stderr,
# so assertions grep for the JSON fragments rather than compare whole output.

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
gl_capture commit --agent -m "C1" zz
assert_eq "10" "$CODE" "commit_needs_input_exit"
assert_contains "$OUT" '"status":"needs_input"' "commit_needs_input_status"
assert_contains "$OUT" '"kind":"select"' "commit_needs_input_kind"
assert_contains "$OUT" "feature-a" "commit_needs_input_option_a"
assert_contains "$OUT" "feature-b" "commit_needs_input_option_b"
assert_contains "$OUT" '"allow_other":true' "commit_needs_input_allow_other"
assert_contains "$OUT" '"hint":' "commit_needs_input_hint"
assert_contains "$OUT" "or -i for the integration branch itself" \
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
gl_capture drop --agent a.txt
assert_eq "10" "$CODE" "drop_confirm_exit"
assert_contains "$OUT" '"status":"needs_confirmation"' "drop_confirm_status"
assert_contains "$OUT" "loom drop <target> -y" "drop_confirm_hint"
assert_file_content "a.txt" "modified content" "drop_confirm_untouched"

gl_capture drop --agent a.txt -y
assert_exit_ok "$CODE" "drop_yes_exit"
assert_contains "$OUT" '"status":"ok"' "drop_yes_status"
assert_file_content "a.txt" "content a" "drop_yes_restored"

# ── completions: dispatched early, but still ends with a JSON status ──────────

describe "agent mode: completions still ends with a JSON status"
gl_capture completions powershell --agent
assert_exit_ok "$CODE" "completions_exit"
assert_contains "$OUT" '"status":"ok"' "completions_status"

gl_capture completions notashell --agent
assert_eq "1" "$CODE" "completions_bad_shell_exit"
assert_contains "$OUT" '"status":"error"' "completions_bad_shell_status"

# ── error: -p is rejected ─────────────────────────────────────────────────────

describe "agent mode: -p/--patch is rejected with a structured error"
gl_capture add --agent -p
assert_eq "1" "$CODE" "patch_rejected_exit"
assert_contains "$OUT" '"status":"error"' "patch_rejected_status"
assert_contains "$OUT" "--patch is interactive" "patch_rejected_msg"

# ── error: tui is rejected ────────────────────────────────────────────────────

describe "agent mode: tui is rejected with a structured error"
gl_capture tui --agent
assert_eq "1" "$CODE" "tui_rejected_exit"
assert_contains "$OUT" '"status":"error"' "tui_rejected_status"
assert_contains "$OUT" "the TUI is interactive" "tui_rejected_msg"

# ── error: normal failures still end with a JSON status ───────────────────────

describe "agent mode: a failing command reports status error"
gl_capture drop --agent no-such-target-xyz -y
assert_eq "1" "$CODE" "error_exit"
assert_contains "$OUT" '"status":"error"' "error_status"

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
assert_contains "$(git -C "$WORK" show HEAD~1)" "+THREE" "split_by_hunks_first_content"
assert_not_contains "$(git -C "$WORK" show HEAD~1)" "+THIRTYSEVEN" "split_by_hunks_first_only"
assert_contains "$(git -C "$WORK" show HEAD)" "+THIRTYSEVEN" "split_by_hunks_second_content"

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
assert_contains "$(git -C "$WORK" show "$(branch_oid feature-a)")" "+THIRTYSEVEN" "fold_by_hunks_moved"
assert_contains "$(git -C "$WORK" show HEAD)" "+THREE" "fold_by_hunks_kept"
assert_not_contains "$(git -C "$WORK" show HEAD)" "+THIRTYSEVEN" "fold_by_hunks_removed"

describe "agent mode: fold -p <commit> zz uncommits listed hunks"
setup_two_hunk_commit
SRC="$(commit_sid_from_status "Change two regions")"
gl_capture fold -p "$SRC" zz --agent
assert_eq "10" "$CODE" "fold_zz_listed_exit"
FP="$(json_fingerprint)"

gl_capture fold -p "$SRC" zz --hunks multi.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "fold_zz_exit"
assert_contains "$(git -C "$WORK" diff)" "+THREE" "fold_zz_unstaged"
assert_contains "$(git -C "$WORK" show HEAD)" "+THIRTYSEVEN" "fold_zz_kept"

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
assert_contains "$(git -C "$WORK" show HEAD)" "+changed" "fold_deletion_kept_hunk"
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
assert_contains "$(git -C "$WORK" show HEAD~1)" "a,b.txt" "split_comma_mixed_took_comma_path"
assert_contains "$(git -C "$WORK" show HEAD~1)" "multi.txt" "split_comma_mixed_took_plain"

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
chmod +x "$WORK/mode.txt"
git -C "$WORK" add -A
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
assert_contains "$(git -C "$WORK" show "$source_sha")" "+THREE" "moved_target_source_intact"
assert_not_contains "$(git -C "$WORK" show HEAD~2)" "+THREE" "moved_target_not_amended"

# `--hunks` is not agent-only, and the non-agent path has its own messages.
describe "--hunks works without --agent"
setup_two_hunk_commit
gl_capture split HEAD -m "first region" -p --agent
FP="$(json_fingerprint)"

gl_capture split HEAD -m "first region" -p --hunks multi.txt:1 --hunks-from "$FP"
assert_exit_ok "$CODE" "no_agent_hunks_exit"
assert_msg_at 1 "first region" "no_agent_hunks_split"
assert_contains "$(git -C "$WORK" show HEAD~1)" "+THREE" "no_agent_hunks_content"

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
chmod +x "$WORK/mode.txt"
git -C "$WORK" add -A
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
assert_not_contains "$OUT" '"selectable":false' "split_whole_none_rejected"
FP="$(json_fingerprint)"

gl_capture split HEAD -m "binary and deletion" -p --hunks blob.bin:1 --hunks gone.txt:1 --hunks-from "$FP" --agent
assert_exit_ok "$CODE" "split_whole_exit"
assert_msg_at 1 "binary and deletion" "split_whole_first"
assert_contains "$(git -C "$WORK" show --stat HEAD~1)" "blob.bin" "split_whole_first_binary"
assert_contains "$(git -C "$WORK" show --stat HEAD~1)" "gone.txt" "split_whole_first_deletion"
assert_contains "$(git -C "$WORK" show --stat HEAD)" "plain.txt" "split_whole_second_text"
assert_not_contains "$(git -C "$WORK" show --stat HEAD)" "blob.bin" "split_whole_second_clean"

describe "agent mode: fold cannot take binary entries"
setup_repo_with_remote
printf '\x00\x01\x02binary\x00' > "$WORK/blob.bin"
seq 1 10 > "$WORK/plain.txt"
git -C "$WORK" add blob.bin plain.txt
git -C "$WORK" commit -q -m "Add binary and text"

gl_capture fold -p HEAD zz --agent
assert_eq "10" "$CODE" "binary_listed_exit"
assert_contains "$OUT" '"id":"blob.bin:1"' "binary_listed_item"
assert_contains "$OUT" '"selectable":false' "binary_listed_unselectable"
assert_contains "$OUT" '"options":["plain.txt:1"]' "binary_listed_options"
FP="$(json_fingerprint)"

gl_capture fold -p HEAD zz --hunks blob.bin:1 --hunks-from "$FP" --agent
assert_eq "1" "$CODE" "binary_picked_exit"
assert_contains "$OUT" "cannot move \`blob.bin:1\`" "binary_picked_msg"

# A listing whose every id would be refused is a prompt with no answer.
describe "agent mode: a commit fold cannot touch at all is not listed"
setup_repo_with_remote
printf '\x00\x01old\x00' > "$WORK/only.bin"
git -C "$WORK" add only.bin
git -C "$WORK" commit -q -m "Add binary"
printf '\x00\x01new\x00' > "$WORK/only.bin"
git -C "$WORK" add only.bin
git -C "$WORK" commit -q -m "Change binary"

gl_capture fold -p HEAD zz --agent
assert_eq "1" "$CODE" "unanswerable_exit"
assert_contains "$OUT" "only binary files" "unanswerable_msg"
assert_not_contains "$OUT" "needs_input" "unanswerable_not_a_prompt"

describe "agent mode: -p over working-tree changes is still rejected"
setup_two_hunk_commit
write_file multi.txt "dirty"

gl_capture fold -p HEAD --agent
assert_eq "1" "$CODE" "fold_worktree_patch_exit"
assert_contains "$OUT" '"status":"error"' "fold_worktree_patch_status"
assert_contains "$OUT" "unavailable in agent mode" "fold_worktree_patch_msg"

gl_capture fold -p HEAD --hunks multi.txt:1 --hunks-from deadbeef --agent
assert_eq "1" "$CODE" "fold_worktree_hunks_exit"
assert_contains "$OUT" "only applies to a commit source" "fold_worktree_hunks_msg"

# Validating the arguments first is what makes this message the one about
# unsupported sources rather than the working-tree-patch one below it.
describe "agent mode: fold -p with a branch target reports the source rule"
gl_capture fold -p HEAD feature-a --agent
assert_eq "1" "$CODE" "fold_branch_target_exit"
assert_contains "$OUT" "does not support commit or branch sources" "fold_branch_target_msg"

pass
