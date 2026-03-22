#!/usr/bin/env bash
# Integration tests for: gl update
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" update >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: detached HEAD"
setup_repo_with_remote
git -C "$WORK" checkout -q --detach HEAD
gl_capture update
assert_exit_fail "$CODE" "precond_detached_head"
assert_contains "$OUT" "detached" "precond_detached_msg"

describe "precond: branch with no upstream tracking"
setup_repo_with_remote
git -C "$WORK" branch --unset-upstream integration
gl_capture update
assert_exit_fail "$CODE" "precond_no_upstream"
assert_contains "$OUT" "no upstream" "precond_no_upstream_msg"

# ══════════════════════════════════════════════════════════════════════════════
# ALREADY UP TO DATE
# ══════════════════════════════════════════════════════════════════════════════

describe "already up-to-date: succeeds and reports updated branch"
setup_repo_with_remote
out=$(gl update 2>&1)
assert_exit_ok $? "already_up_to_date_ok"
assert_contains "$out" "Fetched latest changes"  "already_up_to_date_fetched"
assert_contains "$out" "Rebased onto upstream"   "already_up_to_date_rebased"
assert_contains "$out" "Updated branch"          "already_up_to_date_success_msg"

# ══════════════════════════════════════════════════════════════════════════════
# FETCH AND REBASE NEW UPSTREAM COMMITS
# ══════════════════════════════════════════════════════════════════════════════

describe "new upstream commit: integration rebased on top"
setup_repo_with_remote
OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Upstream feature" "upstream.txt"
git -C "$OTHER" push -q origin

old_head=$(head_hash)
out=$(gl update 2>&1)
assert_exit_ok $? "upstream_new_ok"
assert_contains "$out" "Fetched latest changes"  "upstream_new_fetched"
assert_contains "$out" "Rebased onto upstream"   "upstream_new_rebased"
assert_contains "$out" "Updated branch"          "upstream_new_success_msg"
# Integration HEAD moved forward (merged upstream)
new_head=$(head_hash)
assert_ne "$old_head" "$new_head" "upstream_new_head_advanced"
# The upstream commit is now in history
assert_log_contains "Upstream feature" "upstream_new_commit_in_log"

describe "new upstream commit: output includes upstream short hash"
# (upstream_info is appended to the success message)
assert_contains "$out" "origin/" "upstream_remote_name_in_msg"

# ══════════════════════════════════════════════════════════════════════════════
# LOCAL COMMITS PRESERVED
# ══════════════════════════════════════════════════════════════════════════════

describe "local commits on integration are preserved after rebase"
setup_repo_with_remote
commit_file "Local integration work" "local.txt"
local_msg=$(head_msg)

OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Remote upstream work" "remote.txt"
git -C "$OTHER" push -q origin

out=$(gl update 2>&1)
assert_exit_ok $? "local_commits_preserved_ok"
# Local commit message still in history
assert_log_contains "Local integration work" "local_commits_still_in_log"
# Remote commit also in history
assert_log_contains "Remote upstream work" "remote_commit_in_log"

# ══════════════════════════════════════════════════════════════════════════════
# DIRTY WORKING TREE (AUTOSTASH)
# ══════════════════════════════════════════════════════════════════════════════

describe "dirty working tree is preserved via autostash"
setup_repo_with_remote
# Commit a tracked file so we can modify it
commit_file "Tracked base" "tracked.txt"

OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Upstream while dirty" "upfile.txt"
git -C "$OTHER" push -q origin

# Create an uncommitted modification
write_file "tracked.txt" "dirty content"

out=$(gl update 2>&1)
assert_exit_ok $? "dirty_tree_ok"
assert_contains "$out" "Rebased onto upstream" "dirty_tree_rebased"
# The uncommitted change must survive
assert_file_content "tracked.txt" "dirty content" "dirty_tree_change_preserved"

# ══════════════════════════════════════════════════════════════════════════════
# WOVEN BRANCHES SURVIVE REBASE (--update-refs)
# ══════════════════════════════════════════════════════════════════════════════

describe "woven feature branch ref is updated after rebase"
setup_repo_with_remote
create_feature_branch "g-woven"
switch_to g-woven
commit_file "Woven commit A" "woven-a.txt"
commit_file "Woven commit B" "woven-b.txt"
local_tip=$(head_hash)
switch_to integration
weave_branch "g-woven"

OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Upstream progress" "progress.txt"
git -C "$OTHER" push -q origin

out=$(gl update 2>&1)
assert_exit_ok $? "woven_survive_ok"
assert_contains "$out" "Rebased onto upstream" "woven_survive_rebased"
# The feature branch still exists
assert_branch_exists "g-woven" "woven_survive_branch_exists"
# The feature branch tip changed (was rebased)
new_tip=$(branch_oid "g-woven")
assert_ne "$local_tip" "$new_tip" "woven_survive_ref_updated"
# Its commits are still reachable from the branch
assert_contains "$(git -C "$WORK" log g-woven --oneline)" "Woven commit B" "woven_survive_tip_msg"

# ══════════════════════════════════════════════════════════════════════════════
# GONE UPSTREAM CLEANUP
# ══════════════════════════════════════════════════════════════════════════════

describe "--yes removes a local branch whose remote tracking branch was deleted"
setup_repo_with_remote
# Create and push a feature branch, then weave it so commits are merged
create_feature_branch "g-gone-branch"
switch_to g-gone-branch
commit_file "Gone branch commit" "gone.txt"
git -C "$WORK" push -q -u origin g-gone-branch >/dev/null
switch_to integration
weave_branch "g-gone-branch"
# Delete it on the remote
git -C "$WORK" push -q origin --delete g-gone-branch >/dev/null
# gl update with --yes should fetch (prune), detect gone, and remove
out=$(gl update --yes 2>&1)
assert_exit_ok $? "gone_yes_ok"
assert_contains "$out" "gone upstream" "gone_yes_warning"
assert_contains "$out" "g-gone-branch"  "gone_yes_branch_listed"
assert_contains "$out" "Removed branch" "gone_yes_branch_removed"
assert_branch_not_exists "g-gone-branch" "gone_yes_branch_deleted"


describe "--yes removes only gone branches, not live ones"
setup_repo_with_remote
# A live branch (has remote)
create_feature_branch "g-live-branch"
switch_to g-live-branch
commit_file "Live branch commit" "live.txt"
git -C "$WORK" push -q -u origin g-live-branch >/dev/null
switch_to integration
weave_branch "g-live-branch"
# A gone branch (also woven so safe_delete can remove it)
create_feature_branch "h-gone-only"
switch_to h-gone-only
commit_file "Gone only commit" "goneonly.txt"
git -C "$WORK" push -q -u origin h-gone-only >/dev/null
switch_to integration
weave_branch "h-gone-only"
git -C "$WORK" push -q origin --delete h-gone-only >/dev/null
out=$(gl update --yes 2>&1)
assert_exit_ok $? "gone_selective_ok"
assert_branch_not_exists "h-gone-only"  "gone_selective_gone_removed"
assert_branch_exists     "g-live-branch" "gone_selective_live_kept"

# ══════════════════════════════════════════════════════════════════════════════
# SUBMODULE UPDATE
# ══════════════════════════════════════════════════════════════════════════════

describe "submodule update runs when .gitmodules is present"
setup_repo_with_remote

# Create a submodule repo in the temp dir
SUB_REMOTE="$TMPROOT/sub-remote.git"
SUB_SEED="$TMPROOT/sub-seed"
git init -q "$SUB_SEED"
git -C "$SUB_SEED" config user.email "test@test.com"
git -C "$SUB_SEED" config user.name "Test"
git -C "$SUB_SEED" config core.autocrlf false
echo "submod content" > "$SUB_SEED/sub.txt"
git -C "$SUB_SEED" add sub.txt
git -C "$SUB_SEED" commit -q -m "Sub initial"
git clone -q --bare "$SUB_SEED" "$SUB_REMOTE"
rm -rf "$SUB_SEED"

# Add the submodule; pass protocol.file.allow=always via -c so the internal
# git-clone spawned by submodule-add inherits the permission (file:// is
# disallowed by default since Git 2.38.1).
(cd "$WORK" && git -c protocol.file.allow=always submodule -q add "$SUB_REMOTE" mysubmod)
git -C "$WORK" commit -q -m "Add submodule"

out=$(gl update 2>&1)
assert_exit_ok $? "submodule_ok"
assert_contains "$out" "Updating submodules" "submodule_spinner_start"
assert_contains "$out" "Updated submodules"  "submodule_spinner_stop"

# ══════════════════════════════════════════════════════════════════════════════
# CHERRY-PICKED UPSTREAM COMMITS ARE FILTERED
# ══════════════════════════════════════════════════════════════════════════════

describe "cherry-picked feature commit is filtered from rebase"
setup_repo_with_remote
# Enable histogram diff algorithm — this triggers different hunk merging
# between `git log -p` (respects config) and `git diff-tree -p` (ignores it,
# always uses Myers). The resulting patch-IDs differ, which was the root cause
# of the bug where cherry-picked commits weren't detected during update.
git -C "$WORK" config diff.algorithm histogram

# Push a doc file to upstream so the merge-base includes it.
# The doc has sections that produce different diff hunks under histogram
# vs Myers when rewritten — this is key to triggering the bug.
SETUP="$TMPROOT/setup"
git clone -q "$TMPROOT/remote.git" "$SETUP"
git -C "$SETUP" config user.email "test@test.com"
git -C "$SETUP" config user.name "Test"
git -C "$SETUP" config core.autocrlf false
cat > "$SETUP/doc.md" << 'ORIGINAL'
# Title

## Overview

Some overview text here.

## CLI

```bash
command [--yes]
```

**Arguments:**

- `--yes` / `-y`: Skip the prompt.

**Behavior:**

- Validates the current state
- Fetches all changes
- Rebases local commits
- Updates submodules
- On conflict, reports the error
- After success, proposes to remove branches

## What Happens

1. **Validation**:
   - HEAD must be on a branch
   - Must have upstream tracking

2. **Fetch**:
   - All changes are fetched
   - Tags are force-updated

## Conflict Handling

When conflicts occur:
- The operation pauses
- User resolves manually

## Prerequisites

- Git 2.38 or later
- Must be in a git repository
ORIGINAL
git -C "$SETUP" add doc.md
git -C "$SETUP" commit -q -m "Add doc"
git -C "$SETUP" push -q origin
rm -rf "$SETUP"
# Fetch so WORK sees the new upstream base
git -C "$WORK" fetch -q origin
git -C "$WORK" rebase -q "$(git -C "$WORK" rev-parse --abbrev-ref --symbolic-full-name @{u})"

create_feature_branch "cherry-feat"
switch_to cherry-feat
# Rewrite the doc (produces different hunks under histogram vs Myers)
cat > "$WORK/doc.md" << 'UPDATED'
# Title

## Overview

Some overview text here.

## CLI

```bash
command [--yes]
```

**Flags:**

- `--yes` / `-y`: Skip the prompt.

## What Happens

### Normal Update

**What changes:**

1. **Validation**:
   - HEAD must be on a branch
   - Must have upstream tracking

2. **Fetch**:
   - All changes are fetched
   - Tags are force-updated
   - Deleted remote branches are pruned

**What stays the same:**
- Feature branch refs are kept in sync
- Merge topology is preserved

## Conflict Recovery

When conflicts occur:
- State is saved
- User resolves manually
- Continue or abort

## Prerequisites

- Git 2.38 or later
- Must be in a git repository
UPDATED
git -C "$WORK" add doc.md
git -C "$WORK" commit -q -m "Rewrite doc"
switch_to integration
weave_branch "cherry-feat"

# Upstream recreates the same change (simulates cherry-pick with different OID)
OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "upstream@test.com"
git -C "$OTHER" config user.name "Upstream"
git -C "$OTHER" config core.autocrlf false
base_branch=$(git -C "$OTHER" rev-parse --abbrev-ref HEAD)
cat > "$OTHER/doc.md" << 'UPDATED'
# Title

## Overview

Some overview text here.

## CLI

```bash
command [--yes]
```

**Flags:**

- `--yes` / `-y`: Skip the prompt.

## What Happens

### Normal Update

**What changes:**

1. **Validation**:
   - HEAD must be on a branch
   - Must have upstream tracking

2. **Fetch**:
   - All changes are fetched
   - Tags are force-updated
   - Deleted remote branches are pruned

**What stays the same:**
- Feature branch refs are kept in sync
- Merge topology is preserved

## Conflict Recovery

When conflicts occur:
- State is saved
- User resolves manually
- Continue or abort

## Prerequisites

- Git 2.38 or later
- Must be in a git repository
UPDATED
git -C "$OTHER" add doc.md
git -C "$OTHER" commit -q -m "Rewrite doc"
git -C "$OTHER" push -q origin "$base_branch"

out=$(gl update 2>&1)
assert_exit_ok $? "cherry_pick_filter_ok"
assert_contains "$out" "Rebased onto upstream" "cherry_pick_rebased"
# The feature commit message should still be in history (from the upstream copy)
assert_log_contains "Rewrite doc" "cherry_pick_commit_in_log"

describe "partially cherry-picked branch keeps remaining commits"
setup_repo_with_remote
create_feature_branch "partial-cherry"
switch_to partial-cherry
commit_file "Partial F1" "pf1.txt"
commit_file "Partial F2" "pf2.txt"
commit_file "Partial F3" "pf3.txt"
switch_to integration
weave_branch "partial-cherry"

# Upstream cherry-picks only F1
OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "upstream@test.com"
git -C "$OTHER" config user.name "Upstream"
git -C "$OTHER" config core.autocrlf false
# Recreate F1's change on upstream (simulates cherry-pick with different OID)
base_branch=$(git -C "$OTHER" rev-parse --abbrev-ref HEAD)
echo "Partial F1" > "$OTHER/pf1.txt"
git -C "$OTHER" add pf1.txt
git -C "$OTHER" commit -q -m "Partial F1"
git -C "$OTHER" push -q origin "$base_branch"

out=$(gl update 2>&1)
assert_exit_ok $? "partial_cherry_ok"
assert_contains "$out" "Rebased onto upstream" "partial_cherry_rebased"
# F2 and F3 should still be on the branch
assert_contains "$(git -C "$WORK" log partial-cherry --oneline)" "Partial F2" "partial_cherry_f2_kept"
assert_contains "$(git -C "$WORK" log partial-cherry --oneline)" "Partial F3" "partial_cherry_f3_kept"

describe "fully cherry-picked branch is handled gracefully"
setup_repo_with_remote
create_feature_branch "full-cherry"
switch_to full-cherry
commit_file "Full F1" "ff1.txt"
commit_file "Full F2" "ff2.txt"
switch_to integration
weave_branch "full-cherry"

# Upstream cherry-picks both commits
OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "upstream@test.com"
git -C "$OTHER" config user.name "Upstream"
git -C "$OTHER" config core.autocrlf false
# Recreate both changes on upstream (simulates cherry-pick with different OIDs)
base_branch=$(git -C "$OTHER" rev-parse --abbrev-ref HEAD)
echo "Full F1" > "$OTHER/ff1.txt"
git -C "$OTHER" add ff1.txt
git -C "$OTHER" commit -q -m "Full F1"
echo "Full F2" > "$OTHER/ff2.txt"
git -C "$OTHER" add ff2.txt
git -C "$OTHER" commit -q -m "Full F2"
git -C "$OTHER" push -q origin "$base_branch"

out=$(gl update 2>&1)
assert_exit_ok $? "full_cherry_ok"
assert_contains "$out" "Rebased onto upstream" "full_cherry_rebased"
assert_log_contains "Full F1" "full_cherry_f1_in_log"
assert_log_contains "Full F2" "full_cherry_f2_in_log"

pass
