#!/usr/bin/env bash
# Integration tests for: gl absorb
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

# ── PRECONDITIONS ─────────────────────────────────────────────────────────────

describe "precond: not in a git repository"
new_tmpdir TMP_NOGIT
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

# ── BASIC ABSORPTION ──────────────────────────────────────────────────────────

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

# ── --DRY-RUN ─────────────────────────────────────────────────────────────────

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

# ── FILE ARGUMENTS ────────────────────────────────────────────────────────────

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

# ── SKIPPED HUNKS AND FILES ───────────────────────────────────────────────────

describe "pure-addition hunk is skipped and reported"
setup_repo_with_remote
commit_file "Existing content" "append.txt"
# Append a new line — results in a pure-addition hunk (no `-` lines to blame)
echo "appended line" >> "$WORK/append.txt"
gl_capture absorb
assert_exit_fail "$CODE" "skip_pure_add_fail"
assert_contains "$OUT" "skipped"                  "skip_pure_add_skipped"
assert_contains "$OUT" "No files could be absorbed" "skip_pure_add_nofiles"

# ── WOVEN BRANCHES ────────────────────────────────────────────────────────────

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
assert_head_parent_count 2   "topo_abs_merge_preserved"
assert_branch_exists "g-topo-abs" "topo_abs_branch_exists"

# ── CONTINUE / ABORT ──────────────────────────────────────────────────────────
# Absorb attributes each hunk to the last commit that touched its lines, so a
# fixup conflicts only on a line adjacent to a later commit's change (see the
# last test here). The machinery itself is driven through a synthetic state
# file.

describe "absorb: a resolution that keeps a target unchanged skips its fixup, change back in worktree"
setup_repo_with_remote
printf 'a
b
c
d
e
f
g
h
' > "$WORK/f.txt"; echo x > "$WORK/g.txt"
git -C "$WORK" add f.txt g.txt; git -C "$WORK" commit -q -m "A: create f and g"
printf 'a2
b
c
d
e
f
g
h
' > "$WORK/f.txt"; echo x2 > "$WORK/g.txt"
git -C "$WORK" commit -q -am "B: tweak f line 1 and g"
# Line 2 belongs to A and sits under B's change to line 1: A's fixup conflicts on A.
printf 'a2
b2
c
d
e
f
g
h
' > "$WORK/f.txt"; echo x3 > "$WORK/g.txt"
gl_capture absorb
assert_state_file "absorb_empty_fixup_state"
git -C "$WORK" checkout HEAD -- f.txt
gl_capture continue
assert_exit_ok "$CODE" "absorb_empty_fixup_ok"
assert_no_state_file "absorb_empty_fixup_state_removed"
assert_contains "$OUT" "Absorbed into 1 of 2 commit(s)" "absorb_empty_fixup_msg"
assert_contains "$OUT" "back in your working tree" "absorb_empty_fixup_restored_msg"
assert_not_contains "$OUT" "Absorbed 2 hunk" "absorb_empty_fixup_no_success"
assert_eq "$(git -C "$WORK" show HEAD~1:f.txt | sed -n 2p)" "b" "absorb_empty_fixup_a_unchanged"
assert_eq "$(git -C "$WORK" show HEAD:g.txt)" "x3" "absorb_empty_fixup_b_absorbed"
assert_eq "$(sed -n 2p "$WORK/f.txt")" "b2" "absorb_empty_fixup_worktree"

describe "absorb: a commit that keeps one of its two fixups still counts as absorbed into"
setup_repo_with_remote
printf 'a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n' > "$WORK/f.txt"; echo x > "$WORK/g.txt"; echo k > "$WORK/k.txt"
git -C "$WORK" add f.txt g.txt k.txt; git -C "$WORK" commit -q -m "A: create f, g and k"
printf 'a2\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl2\n' > "$WORK/f.txt"; echo x2 > "$WORK/g.txt"
git -C "$WORK" commit -q -am "B: tweak f lines 1 and 12, and g"
# A gets a whole-file fixup (k.txt) and a hunk fixup (f.txt line 2); only the second conflicts.
printf 'a2\nb2\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl3\n' > "$WORK/f.txt"; echo x3 > "$WORK/g.txt"; echo k2 > "$WORK/k.txt"
gl_capture absorb
assert_state_file "absorb_two_fixups_state"
git -C "$WORK" checkout HEAD -- f.txt
gl_capture continue
assert_exit_ok "$CODE" "absorb_two_fixups_ok"
assert_contains "$OUT" "Absorbed into 2 of 2 commit(s)" "absorb_two_fixups_msg"
assert_eq "$(git -C "$WORK" show HEAD~1:k.txt)" "k2" "absorb_two_fixups_a_absorbed"
assert_eq "$(sed -n 2p "$WORK/f.txt")" "b2" "absorb_two_fixups_worktree"

describe "absorb: continue with no active rebase dispatches after_continue"
setup_repo_with_remote
commit_file "Absorb base" "absorb-base.txt"
mkdir -p "$WORK/.git/loom"
printf '{"command":"absorb","rollback":{"saved_head":"","saved_refs":{},"delete_branches":[],"saved_staged_patch":"","saved_worktree_patch":""},"context":{"skipped_patch":null,"num_hunks":2,"num_files":1,"num_commits":1}}' \
    > "$WORK/.git/loom/state.json"
gl_capture continue
assert_exit_ok  "$CODE" "absorb_cont_ok"
assert_no_state_file   "absorb_cont_state_removed"
assert_contains "$OUT" "Absorbed" "absorb_cont_msg"

describe "absorb: abort restores original HEAD"
setup_repo_with_remote
commit_file "Absorb pre-state" "absorb-pre.txt"
old_head=$(head_hash)
mkdir -p "$WORK/.git/loom"
printf '{"command":"absorb","rollback":{"reset_hard_to":"%s","delete_branches":[],"saved_staged_patch":"","saved_worktree_patch":""},"context":{"skipped_patch":null,"num_hunks":1,"num_files":1,"num_commits":1}}' \
    "$old_head" > "$WORK/.git/loom/state.json"
commit_file "Simulated rebase progress" "progress.txt"
gl_capture abort
assert_exit_ok  "$CODE" "absorb_abort_ok"
assert_contains "$OUT" "Aborted" "absorb_abort_msg"
assert_no_state_file   "absorb_abort_state_removed"
assert_eq "$old_head" "$(head_hash)" "absorb_abort_head_restored"


# ── GIT ARGUMENT FORWARDING ───────────────────────────────────────────────────

describe "absorb: a pre-commit hook is skipped with -- --no-verify"
setup_repo_with_remote
commit_file "Hooked absorb" "hooked-absorb.txt"
commit_file "Above absorb" "above-absorb.txt"
install_failing_hook pre-commit
write_file "hooked-absorb.txt" "absorbed"
gl_capture absorb
assert_exit_fail "$CODE" "absorb_hook_blocks"
assert_eq "absorbed" "$(cat "$WORK/hooked-absorb.txt")" "absorb_hook_change_kept"
gl_capture absorb -- --no-verify
assert_exit_ok "$CODE" "absorb_no_verify_ok"
assert_eq "absorbed" "$(git -C "$WORK" show HEAD~1:hooked-absorb.txt)" "absorb_no_verify_content"
assert_eq "Above absorb" "$(head_msg)" "absorb_no_verify_keeps_the_commit_above"

pass
