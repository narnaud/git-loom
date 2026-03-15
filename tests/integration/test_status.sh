#!/usr/bin/env bash
# Integration tests for: gl status
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" status >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: detached HEAD"
setup_repo_with_remote
git -C "$WORK" checkout -q --detach HEAD
gl_capture status
assert_exit_fail "$CODE" "precond_detached_head"

describe "precond: branch with no upstream tracking"
setup_repo_with_remote
git -C "$WORK" branch --unset-upstream integration
gl_capture status
assert_exit_fail "$CODE" "precond_no_upstream"

# ══════════════════════════════════════════════════════════════════════════════
# EMPTY INTEGRATION BRANCH
# ══════════════════════════════════════════════════════════════════════════════

describe "empty branch shows upstream marker and no changes"
setup_repo_with_remote
out=$(gl status)
assert_exit_ok $? "empty_ok"
assert_contains "$out" "(upstream)"  "empty_upstream_label"
assert_contains "$out" "Initial"     "empty_upstream_msg"
assert_contains "$out" "origin/"     "empty_upstream_remote"
assert_contains "$out" "no changes"  "empty_shows_no_changes"

# ══════════════════════════════════════════════════════════════════════════════
# WORKING CHANGES
# ══════════════════════════════════════════════════════════════════════════════

describe "staged file appears in local changes section"
setup_repo_with_remote
write_file "staged.txt" "staged content"
git -C "$WORK" add staged.txt
out=$(gl status)
assert_exit_ok $? "staged_ok"
assert_contains "$out" "local changes" "staged_section_header"
assert_contains "$out" "staged.txt"    "staged_filename"
assert_contains "$out" "A "            "staged_index_char"

describe "unstaged modified file appears in local changes section"
setup_repo_with_remote
commit_file "Tracked file" "tracked.txt"
write_file "tracked.txt" "modified"
out=$(gl status)
assert_exit_ok $? "unstaged_ok"
assert_contains "$out" "local changes" "unstaged_section_header"
assert_contains "$out" "tracked.txt"   "unstaged_filename"
assert_contains "$out" " M"            "unstaged_worktree_char"

describe "untracked file shows with ⁕ marker"
setup_repo_with_remote
write_file "untracked.txt" "untracked"
out=$(gl status)
assert_exit_ok $? "untracked_ok"
assert_contains "$out" "local changes"  "untracked_section_header"
assert_contains "$out" "untracked.txt"  "untracked_filename"
assert_contains "$out" "⁕"              "untracked_marker"

describe "clean working tree shows no changes"
setup_repo_with_remote
out=$(gl status)
assert_contains "$out" "no changes" "clean_shows_no_changes"

# ══════════════════════════════════════════════════════════════════════════════
# SINGLE WOVEN BRANCH
# ══════════════════════════════════════════════════════════════════════════════

# Branch names starting with g–z avoid short-ID collisions with hex digits a–f.
describe "single branch: name, commit message, and graph symbols"
setup_repo_with_remote
create_feature_branch "g-alpha"
switch_to g-alpha
commit_file "Alpha commit" "alpha.txt"
switch_to integration
weave_branch "g-alpha"
out=$(gl status)
assert_exit_ok $? "single_branch_ok"
assert_contains "$out" "g-alpha"      "single_branch_name"
assert_contains "$out" "Alpha commit" "single_branch_msg"
assert_contains "$out" "│╭─"          "single_branch_open"
assert_contains "$out" "│●"           "single_branch_commit"
assert_contains "$out" "├╯"           "single_branch_close"
assert_contains "$out" "(upstream)"   "single_branch_upstream"

# ══════════════════════════════════════════════════════════════════════════════
# MULTIPLE INDEPENDENT WOVEN BRANCHES
# ══════════════════════════════════════════════════════════════════════════════

describe "two independent branches both appear with their commits"
setup_repo_with_remote
create_feature_branch "g-alpha"
switch_to g-alpha
commit_file "Alpha commit" "alpha.txt"
switch_to integration

create_feature_branch "h-beta"
switch_to h-beta
commit_file "Beta commit" "beta.txt"
switch_to integration

weave_branch "g-alpha"
weave_branch "h-beta"

out=$(gl status)
assert_contains "$out" "g-alpha"      "two_branches_first_name"
assert_contains "$out" "h-beta"       "two_branches_second_name"
assert_contains "$out" "Alpha commit" "two_branches_first_msg"
assert_contains "$out" "Beta commit"  "two_branches_second_msg"

# ══════════════════════════════════════════════════════════════════════════════
# STACKED BRANCHES (h-top built on top of g-base)
# ══════════════════════════════════════════════════════════════════════════════

describe "stacked branches show │├─ and ││ connectors"
setup_repo_with_remote
create_feature_branch "g-base"
switch_to g-base
commit_file "Base A1" "base-a1.txt"
commit_file "Base A2" "base-a2.txt"
switch_to integration
weave_branch "g-base"

# h-top branches from g-base's tip, making it stacked on top of g-base
git -C "$WORK" branch h-top g-base
switch_to h-top
commit_file "Top B1" "top-b1.txt"
switch_to integration
weave_branch "h-top"

out=$(gl status)
assert_contains "$out" "g-base"  "stacked_base_name"
assert_contains "$out" "h-top"   "stacked_top_name"
assert_contains "$out" "Base A1" "stacked_base_msg_a1"
assert_contains "$out" "Base A2" "stacked_base_msg_a2"
assert_contains "$out" "Top B1"  "stacked_top_msg_b1"
# Lower branch uses │├─ and the gap between stacked branches uses ││
assert_contains "$out" "│├─"     "stacked_lower_connector"
assert_contains "$out" "││"      "stacked_between_connector"

# ══════════════════════════════════════════════════════════════════════════════
# CO-LOCATED BRANCHES (two refs pointing to the same tip commit)
# ══════════════════════════════════════════════════════════════════════════════

describe "co-located branches: both headers appear above shared commits"
setup_repo_with_remote
create_feature_branch "g-coloc-a"
switch_to g-coloc-a
commit_file "Colocated commit" "coloc.txt"
switch_to integration

# h-coloc-b points to the exact same tip as g-coloc-a
git -C "$WORK" branch h-coloc-b g-coloc-a
weave_branch "g-coloc-a"

out=$(gl status)
assert_contains "$out" "g-coloc-a"       "coloc_first_name"
assert_contains "$out" "h-coloc-b"       "coloc_second_name"
assert_contains "$out" "Colocated commit" "coloc_commit_msg"
# First branch uses │╭─, co-located branch uses │├─ (no ││ gap between them)
assert_contains "$out" "│╭─"             "coloc_first_header"
assert_contains "$out" "│├─"             "coloc_second_header"
# Shared commit appears exactly once
count=$(grep -c "Colocated commit" <<< "$out")
assert_eq "$count" "1" "coloc_commit_not_duplicated"

# ══════════════════════════════════════════════════════════════════════════════
# EMPTY BRANCH (at upstream base, no commits in range)
# ══════════════════════════════════════════════════════════════════════════════

describe "branch at merge-base shows as header+close with no commits"
setup_repo_with_remote
create_feature_branch "g-empty"
# g-empty sits at the upstream OID; no commits have been added to it

out=$(gl status)
assert_contains     "$out" "g-empty" "empty_branch_name"
assert_contains     "$out" "│╭─"     "empty_branch_open"
assert_contains     "$out" "├╯"      "empty_branch_close"
assert_not_contains "$out" "│●"      "empty_branch_no_commits"

# ══════════════════════════════════════════════════════════════════════════════
# LOOSE COMMITS (on integration line, not owned by any feature branch)
# ══════════════════════════════════════════════════════════════════════════════

describe "loose commit on integration line renders with bare ●"
setup_repo_with_remote
commit_file "Loose commit" "loose.txt"
out=$(gl status)
assert_contains     "$out" "Loose commit" "loose_commit_msg"
# Loose commits use bare ● (no │ prefix), unlike branch commits │●
assert_not_contains "$out" "│●"           "loose_no_branch_marker"

# ══════════════════════════════════════════════════════════════════════════════
# UPSTREAM-TRACKING BRANCH EXCLUSION
# ══════════════════════════════════════════════════════════════════════════════

describe "branch tracking same upstream as integration is excluded"
setup_repo_with_remote
upstream_ref=$(git -C "$WORK" rev-parse --abbrev-ref integration@{upstream})
git -C "$WORK" checkout -q -b g-mirror-upstream
git -C "$WORK" branch --set-upstream-to="$upstream_ref" g-mirror-upstream >/dev/null
switch_to integration
out=$(gl status)
assert_not_contains "$out" "g-mirror-upstream" "same_upstream_excluded"

# ══════════════════════════════════════════════════════════════════════════════
# REMOTE TRACKING INDICATORS
# ══════════════════════════════════════════════════════════════════════════════

describe "local-only branch (never pushed) has no remote indicator"
setup_repo_with_remote
create_feature_branch "g-local-only"
switch_to g-local-only
commit_file "Local only commit" "localonly.txt"
switch_to integration
weave_branch "g-local-only"
out=$(gl status)
assert_not_contains "$out" "✓" "local_only_no_synced"
assert_not_contains "$out" "↑" "local_only_no_ahead"
assert_not_contains "$out" "✗" "local_only_no_gone"

describe "branch in sync with remote shows ✓"
setup_repo_with_remote
create_feature_branch "g-synced"
switch_to g-synced
commit_file "Synced commit" "synced.txt"
switch_to integration
weave_branch "g-synced"
git -C "$WORK" push -q -u origin g-synced >/dev/null
out=$(gl status)
assert_contains "$out" "✓" "synced_checkmark"

describe "branch ahead of remote shows ↑"
setup_repo_with_remote
create_feature_branch "g-ahead"
switch_to g-ahead
commit_file "First pushed commit" "first.txt"
git -C "$WORK" push -q -u origin g-ahead >/dev/null
# Add an unpushed commit
commit_file "Unpushed commit" "second.txt"
switch_to integration
weave_branch "g-ahead"
out=$(gl status)
assert_contains "$out" "↑" "ahead_arrow"

describe "remote branch deleted (after fetch --prune) shows ✗"
setup_repo_with_remote
create_feature_branch "g-pruned"
switch_to g-pruned
commit_file "Will be pruned" "pruned.txt"
switch_to integration
weave_branch "g-pruned"
git -C "$WORK" push -q -u origin g-pruned >/dev/null
git -C "$WORK" push -q origin --delete g-pruned >/dev/null
git -C "$WORK" fetch -q --prune origin
out=$(gl status)
assert_contains "$out" "✗" "pruned_cross"

# ══════════════════════════════════════════════════════════════════════════════
# HIDDEN BRANCHES (loom.hideBranchPattern)
# ══════════════════════════════════════════════════════════════════════════════

describe "branch matching hide prefix is invisible (name and commits)"
setup_repo_with_remote
git -C "$WORK" config loom.hideBranchPattern "local-"
create_feature_branch "local-secret"
switch_to local-secret
commit_file "Secret commit" "secret.txt"
switch_to integration
weave_branch "local-secret"
out=$(gl status)
assert_not_contains "$out" "local-secret"  "hidden_name_absent"
assert_not_contains "$out" "Secret commit" "hidden_commit_absent"

describe "--all reveals hidden branch and its commits"
out=$(gl status --all)
assert_contains "$out" "local-secret"  "hidden_name_with_all"
assert_contains "$out" "Secret commit" "hidden_commit_with_all"

describe "custom hideBranchPattern hides matching, shows non-matching"
setup_repo_with_remote
git -C "$WORK" config loom.hideBranchPattern "g-priv-"
create_feature_branch "g-priv-stuff"
switch_to g-priv-stuff
commit_file "Private commit" "private.txt"
switch_to integration
weave_branch "g-priv-stuff"

create_feature_branch "h-public"
switch_to h-public
commit_file "Public commit" "public.txt"
switch_to integration
weave_branch "h-public"

out=$(gl status)
assert_not_contains "$out" "g-priv-stuff"   "custom_pattern_hides_name"
assert_not_contains "$out" "Private commit"  "custom_pattern_hides_commits"
assert_contains     "$out" "h-public"       "custom_pattern_visible_name"
assert_contains     "$out" "Public commit"   "custom_pattern_visible_commit"

describe "empty hideBranchPattern disables hiding"
setup_repo_with_remote
git -C "$WORK" config loom.hideBranchPattern ""
create_feature_branch "local-visible"
switch_to local-visible
commit_file "Visible commit" "visible.txt"
switch_to integration
weave_branch "local-visible"
out=$(gl status)
assert_contains "$out" "local-visible"  "empty_pattern_shows_name"
assert_contains "$out" "Visible commit" "empty_pattern_shows_commit"

# ══════════════════════════════════════════════════════════════════════════════
# UPSTREAM WITH NEW COMMITS (⏫)
# ══════════════════════════════════════════════════════════════════════════════

describe "upstream moved ahead shows ⏫ indicator with common base label"
setup_repo_with_remote
OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Remote ahead commit" "remote.txt"
git -C "$OTHER" push -q origin
git -C "$WORK" fetch -q origin
out=$(gl status)
assert_contains "$out" "⏫"          "upstream_ahead_indicator"
assert_contains "$out" "new commit"  "upstream_ahead_count_text"
assert_contains "$out" "common base" "upstream_common_base_label"

# ══════════════════════════════════════════════════════════════════════════════
# CONTEXT COMMITS (gl status N)
# ══════════════════════════════════════════════════════════════════════════════

describe "gl status 2 shows 1 context commit (·) before the merge-base"
setup_repo_with_remote
OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "New upstream base" "newbase.txt"
git -C "$OTHER" push -q origin
# Advance integration to the new upstream (fast-forward rebase)
remote_branch=$(git -C "$WORK" rev-parse --abbrev-ref integration@{upstream})
git -C "$WORK" fetch -q origin
git -C "$WORK" rebase -q "$remote_branch"
# "Initial" is now a context commit before the new merge-base
out=$(gl status 2)
assert_contains "$out" "·"       "context_dot_marker"
assert_contains "$out" "Initial" "context_shows_history"

# ══════════════════════════════════════════════════════════════════════════════
# CWD-RELATIVE FILE PATHS
# ══════════════════════════════════════════════════════════════════════════════

describe "file paths are CWD-relative when run from a subdirectory"
setup_repo_with_remote
mkdir -p "$WORK/src"
write_file "src/widget.rs" "pub struct Widget;"
git -C "$WORK" add src/widget.rs
# gl() always runs from $WORK root, so invoke the binary directly from the subdir
out=$(cd "$WORK/src" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" status)
assert_contains     "$out" "widget.rs"     "cwd_relative_basename"
assert_not_contains "$out" "src/widget.rs" "cwd_relative_no_prefix"

# ══════════════════════════════════════════════════════════════════════════════
# FILES FLAG (-f / --files)
# ══════════════════════════════════════════════════════════════════════════════

describe "-f shows changed files beneath each commit"
setup_repo_with_remote
create_feature_branch "g-files"
switch_to g-files
commit_file "Files commit A" "myfile-a.txt"
commit_file "Files commit B" "myfile-b.txt"
switch_to integration
weave_branch "g-files"
out=$(gl status -f)
assert_exit_ok $? "files_flag_ok"
assert_contains "$out" "myfile-a.txt" "files_flag_shows_file_a"
assert_contains "$out" "myfile-b.txt" "files_flag_shows_file_b"

describe "-f <hash> shows files only for the specified commit"
setup_repo_with_remote
create_feature_branch "g-files2"
switch_to g-files2
commit_file "Files commit X" "myfile-x.txt"
oid_x=$(head_hash)
commit_file "Files commit Y" "myfile-y.txt"
switch_to integration
weave_branch "g-files2"
out=$(gl status -f "$oid_x")
assert_exit_ok $? "files_single_commit_ok"
assert_contains     "$out" "myfile-x.txt" "files_single_shows_x"
assert_not_contains "$out" "myfile-y.txt" "files_single_hides_y"

pass
