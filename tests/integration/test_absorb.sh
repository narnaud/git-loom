#!/usr/bin/env bash
# Integration tests for: gl absorb
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" absorb >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: no in-scope commits (integration at merge-base)"
setup_repo_with_remote
# in_scope check fires before changed-files check — no commits needed
gl_capture absorb
assert_exit_fail "$CODE" "precond_no_scope_fail"
assert_contains "$OUT" "No commits in scope" "precond_no_scope_msg"

describe "precond: no uncommitted changes"
setup_repo_with_remote
commit_file "Base commit" "base.txt"
gl_capture absorb
assert_exit_fail "$CODE" "precond_no_changes_fail"
assert_contains "$OUT" "Nothing to absorb" "precond_no_changes_msg"

describe "precond: all files skipped yields clear error"
# A newly-staged file has no blame in HEAD, so absorb skips it
setup_repo_with_remote
commit_file "Anchor" "anchor.txt"
write_file "brand-new.txt" "brand new content"
git -C "$WORK" add brand-new.txt
gl_capture absorb
assert_exit_fail "$CODE" "precond_all_skipped_fail"
assert_contains "$OUT" "skipped" "precond_all_skipped_reason"
assert_contains "$OUT" "No files could be absorbed" "precond_all_skipped_msg"

# ══════════════════════════════════════════════════════════════════════════════
# BASIC ABSORPTION
# ══════════════════════════════════════════════════════════════════════════════

describe "single file absorbed into its originating commit"
setup_repo_with_remote
commit_file "Feature work" "feature.txt"
old_hash="$(head_hash)"
write_file "feature.txt" "improved feature content"
out=$(gl absorb)
assert_exit_ok $? "absorb_single_ok"
assert_contains "$out" "feature.txt"                        "absorb_single_file"
assert_contains "$out" " -> "                               "absorb_single_arrow"
assert_contains "$out" "Feature work"                       "absorb_single_commit_msg"
assert_contains "$out" "1 hunk(s) from 1 file(s) into 1 commit(s)" "absorb_single_summary"
assert_ne "$old_hash" "$(head_hash)"                        "absorb_single_rewritten"
assert_head_msg "Feature work"                              "absorb_single_msg_preserved"
assert_contains "$(gl status)" "no changes"                 "absorb_single_clean_wt"

describe "multiple files absorbed into their respective commits"
setup_repo_with_remote
commit_file "Commit Alpha" "file-alpha.txt"
commit_file "Commit Beta"  "file-beta.txt"
write_file "file-alpha.txt" "modified alpha"
write_file "file-beta.txt"  "modified beta"
out=$(gl absorb)
assert_exit_ok $? "absorb_multi_ok"
assert_contains "$out" "file-alpha.txt"                              "absorb_multi_file_a"
assert_contains "$out" "file-beta.txt"                               "absorb_multi_file_b"
assert_contains "$out" "2 hunk(s) from 2 file(s) into 2 commit(s)"  "absorb_multi_summary"
assert_contains "$(gl status)" "no changes"                          "absorb_multi_clean_wt"

# ══════════════════════════════════════════════════════════════════════════════
# --DRY-RUN
# ══════════════════════════════════════════════════════════════════════════════

describe "--dry-run prints the plan without modifying the repository"
setup_repo_with_remote
commit_file "Dry target" "dry.txt"
saved_hash="$(head_hash)"
write_file "dry.txt" "dry modified"
out=$(gl absorb --dry-run)
assert_exit_ok $? "dryrun_ok"
assert_contains "$out" "dry.txt"       "dryrun_file_listed"
assert_contains "$out" "Dry run:"      "dryrun_prefix"
assert_contains "$out" "would absorb"  "dryrun_would_absorb"
# Commit and working tree must be unchanged
assert_eq "$saved_hash" "$(head_hash)"         "dryrun_commit_unchanged"
assert_file_content "dry.txt" "dry modified"   "dryrun_wt_unchanged"

describe "-n short flag behaves identically to --dry-run"
setup_repo_with_remote
commit_file "Short flag target" "sf.txt"
write_file "sf.txt" "sf modified"
out=$(gl absorb -n)
assert_exit_ok $? "dryrun_short_ok"
assert_contains "$out" "Dry run:"                      "dryrun_short_prefix"
assert_file_content "sf.txt" "sf modified"             "dryrun_short_wt_unchanged"

# ══════════════════════════════════════════════════════════════════════════════
# FILE ARGUMENTS
# ══════════════════════════════════════════════════════════════════════════════

describe "file argument restricts absorption to the named file only"
setup_repo_with_remote
commit_file "Commit P" "restrict-p.txt"
commit_file "Commit Q" "restrict-q.txt"
write_file "restrict-p.txt" "modified P"
write_file "restrict-q.txt" "modified Q"
out=$(gl absorb restrict-p.txt)
assert_exit_ok $? "restrict_ok"
assert_contains "$out" "restrict-p.txt"  "restrict_p_listed"
# restrict-q.txt must still be dirty (not absorbed)
status_out=$(gl status)
assert_contains     "$status_out" "restrict-q.txt"  "restrict_q_still_dirty"
assert_not_contains "$status_out" "restrict-p.txt"  "restrict_p_now_clean"

describe "unknown file argument is rejected"
setup_repo_with_remote
commit_file "Base" "base.txt"
gl_capture absorb totally-nonexistent-xyz.txt
assert_exit_fail "$CODE" "restrict_unknown_fail"

# ══════════════════════════════════════════════════════════════════════════════
# SKIPPED HUNKS AND FILES
# ══════════════════════════════════════════════════════════════════════════════

describe "pure-addition hunk is skipped and reported"
setup_repo_with_remote
commit_file "Existing content" "append.txt"
# Append a new line — results in a pure-addition hunk (no `-` lines to blame)
echo "appended line" >> "$WORK/append.txt"
gl_capture absorb
assert_exit_fail "$CODE" "skip_pure_add_fail"
assert_contains "$OUT" "skipped"                  "skip_pure_add_skipped"
assert_contains "$OUT" "No files could be absorbed" "skip_pure_add_nofiles"

# ══════════════════════════════════════════════════════════════════════════════
# WOVEN BRANCHES
# ══════════════════════════════════════════════════════════════════════════════

describe "absorb into a woven branch commit shows branch name in output"
setup_repo_with_remote
create_feature_branch "g-woven-abs"
switch_to g-woven-abs
commit_file "Branch feature" "branch-feat.txt"
switch_to integration
weave_branch "g-woven-abs"
write_file "branch-feat.txt" "improved branch feature"
out=$(gl absorb)
assert_exit_ok $? "woven_abs_ok"
assert_contains "$out" "branch-feat.txt"   "woven_abs_file"
assert_contains "$out" "(g-woven-abs)"     "woven_abs_branch_label"
assert_contains "$out" "Absorbed"          "woven_abs_summary"

describe "absorb preserves woven merge topology"
setup_repo_with_remote
create_feature_branch "g-topo-abs"
switch_to g-topo-abs
commit_file "Topology commit" "topo.txt"
switch_to integration
weave_branch "g-topo-abs"
# HEAD is now a merge commit; absorb into the branch commit
write_file "topo.txt" "improved topo"
out=$(gl absorb)
assert_exit_ok $? "topo_abs_ok"
# Merge topology must survive the rebase
assert_head_parent_count 2   "topo_abs_merge_preserved"
assert_branch_exists "g-topo-abs" "topo_abs_branch_exists"

pass
