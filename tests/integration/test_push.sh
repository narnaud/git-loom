#!/usr/bin/env bash
# Integration tests for: gl push
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

remote_has()    { git -C "$TMPROOT/remote.git" rev-parse --verify "refs/heads/$1" > /dev/null 2>&1; }
remote_oid()    { git -C "$TMPROOT/remote.git" rev-parse "refs/heads/$1"; }

# Install stdin as the bare remote's pre-receive hook. The global config may
# point core.hooksPath elsewhere; aim it back at the remote.
remote_pre_receive() {
    git -C "$TMPROOT/remote.git" config core.hooksPath "$TMPROOT/remote.git/hooks"
    cat > "$TMPROOT/remote.git/hooks/pre-receive"
    chmod +x "$TMPROOT/remote.git/hooks/pre-receive"
}

# b (B1) stacked on a (A1), woven as one merge with a's tip inside b's
# section — the shape loom itself produces; x (X1) independent and woven.
# Single-commit branches keep PR creation from prompting for a title.
build_stack() {
    setup_repo_with_remote
    create_feature_branch "b"
    switch_to b
    commit_file "A1" "a1.txt"
    git -C "$WORK" branch a
    commit_file "B1" "b1.txt"
    switch_to integration
    weave_branch "b"

    create_feature_branch "x"
    switch_to x
    commit_file "X1" "x1.txt"
    switch_to integration
    weave_branch "x"
    git -C "$WORK" config loom.remoteType plain
}

# ── PRECONDITIONS ─────────────────────────────────────────────────────────────

describe "precond: no woven branches"
setup_repo_with_remote
gl_capture push
assert_exit_fail "$CODE" "precond_no_branches"
assert_contains "$OUT" "No woven branches to push" "precond_no_branches_msg"

describe "precond: hidden branches are never pushed"
build_stack
git -C "$WORK" branch -m a local-a
gl_capture push local-a
assert_exit_fail "$CODE" "precond_hidden_self"
assert_contains "$OUT" "local-a is hidden" "precond_hidden_self_msg"
gl_capture push b
assert_exit_fail "$CODE" "precond_hidden_parent"
assert_contains "$OUT" "b is stacked on hidden branch local-a" "precond_hidden_parent_msg"
remote_has local-a && fail "hidden: local-a should not be on the remote"
remote_has b && fail "hidden: b should not be on the remote"
gl_capture push x
assert_exit_ok "$CODE" "precond_hidden_other_ok"

describe "precond: every branch hidden"
build_stack
for name in a b x; do
    git -C "$WORK" branch -m "$name" "local-$name"
done
gl_capture push
assert_exit_fail "$CODE" "precond_all_hidden"
assert_contains "$OUT" "Every woven branch is hidden" "precond_all_hidden_msg"

describe "precond: branch not woven"
build_stack
git -C "$WORK" branch stray
gl_capture push stray
assert_exit_fail "$CODE" "precond_not_woven"
assert_contains "$OUT" "not woven" "precond_not_woven_msg"

# ── PLAIN REMOTE ──────────────────────────────────────────────────────────────

describe "a lone branch is pushed on its own"
build_stack
gl_capture push x
assert_exit_ok "$CODE" "lone_ok"
assert_contains "$OUT" 'Pushed x to origin' "lone_msg"
remote_has x || fail "lone: x should be on the remote"
remote_has a && fail "lone: a should not be on the remote"
remote_has b && fail "lone: b should not be on the remote"

describe "a stacked branch is pushed with its downstack"
build_stack
gl_capture push b
assert_exit_ok "$CODE" "downstack_ok"
assert_contains "$OUT" 'Pushed a, b to origin' "downstack_msg"
assert_eq "$(remote_oid a)" "$(branch_oid a)" "downstack_a_oid"
assert_eq "$(remote_oid b)" "$(branch_oid b)" "downstack_b_oid"
remote_has x && fail "downstack: x should not be on the remote"
assert_not_contains "$OUT" "Not pushed" "downstack_no_hint"

describe "a never-pushed upper branch is only hinted"
build_stack
gl_capture push a
assert_exit_ok "$CODE" "hint_ok"
assert_contains "$OUT" 'Pushed a to origin' "hint_msg"
assert_contains "$OUT" 'Not pushed above a: b' "hint_unpublished"
assert_contains "$OUT" "loom push b" "hint_command"
remote_has b && fail "hint: b should not be on the remote"

describe "a stale published upper branch is re-pushed with a lower one"
build_stack
gl push b > /dev/null
b_before="$(remote_oid b)"
a1_sid="$(commit_sid_from_status "A1")"
gl reword "$a1_sid" -m "A1 reworded" > /dev/null
assert_ne "$(branch_oid b)" "$b_before" "republish_b_rewritten_locally"
gl_capture push a
assert_exit_ok "$CODE" "republish_ok"
assert_contains "$OUT" 'Pushed a to origin' "republish_msg"
assert_contains "$OUT" 'Re-pushed above a: b' "republish_hint"
assert_eq "$(remote_oid a)" "$(branch_oid a)" "republish_a_oid"
assert_eq "$(remote_oid b)" "$(branch_oid b)" "republish_b_oid"

describe "an undetected remote type is asked about, saving nothing unanswered"
build_stack
git -C "$WORK" config --unset loom.remoteType
gl_capture --agent push x
assert_eq "10" "$CODE" "remote_type_menu_exit"
assert_contains "$OUT" '"status":"needs_input"' "remote_type_menu_status"
assert_contains "$OUT" "Which kind of remote is" "remote_type_menu_prompt"
for forge in GitHub GitLab "Azure DevOps" Gerrit "Plain Git"; do
    assert_contains "$OUT" "$forge" "remote_type_menu_lists_$forge"
done
assert_contains "$OUT" "git config loom.remoteType" "remote_type_menu_hint"
remote_has x && fail "remote_type_menu: x should not be on the remote"
git -C "$WORK" config --get loom.remoteType && fail "remote_type_menu: nothing should be saved"

describe "a detected remote type is saved"
build_stack
git -C "$WORK" config --unset loom.remoteType
git -C "$WORK" remote set-url origin "https://github.com/owner/repo.git"
git -C "$WORK" remote set-url --push origin "$TMPROOT/remote.git"
gl_capture push x --no-pr
assert_exit_ok "$CODE" "remote_type_saved_ok"
assert_eq "github" "$(git -C "$WORK" config --get loom.remoteType)" "remote_type_saved"

describe "agent mode reports a plain push as JSON"
build_stack
gl_capture_json --agent push a
assert_exit_ok "$CODE" "agent_plain_ok"
assert_contains "$(json_line)" '"push":{"remote":"origin","forge":"plain","pushed":["a"],"not_pushed":["b"]}' "agent_plain_json"

describe "--title is refused where no PR takes it"
build_stack
gl_capture push x --title "T"
assert_exit_fail "$CODE" "title_plain_fails"
assert_contains "$OUT" "--title names a pull request" "title_plain_msg"
remote_has x && fail "title_plain: x should not be on the remote"
gl_capture push x --no-pr --title "T"
assert_eq "2" "$CODE" "title_no_pr_conflicts"

describe "--draft is refused on a plain remote, which has nothing to mark"
gl_capture push x --draft
assert_exit_fail "$CODE" "draft_plain_fails"
assert_contains "$OUT" "--draft marks a pull request" "draft_plain_msg"
remote_has x && fail "draft_plain: x should not be on the remote"
gl_capture push x --no-pr --draft
assert_eq "2" "$CODE" "draft_no_pr_conflicts"

describe "--no-pr pushes the downstack too"
build_stack
gl_capture push b --no-pr
assert_exit_ok "$CODE" "nopr_ok"
assert_eq "$(remote_oid a)" "$(branch_oid a)" "nopr_a_oid"
assert_eq "$(remote_oid b)" "$(branch_oid b)" "nopr_b_oid"

describe "a remote whose name holds a / is pushed to"
build_stack
git -C "$WORK" remote rename origin team/fork
gl_capture push x
assert_exit_ok "$CODE" "slash_remote_ok"
assert_contains "$OUT" 'Pushed x to team/fork' "slash_remote_msg"
remote_has x || fail "slash_remote: x should be on the remote"

describe "a local upstream pushes to loom.pushRemote, which it needs"
build_stack
git -C "$WORK" branch -q -u "$BASE_BRANCH" integration
gl_capture_json --agent push
assert_eq "1" "$CODE" "local_upstream_refused_before_picking"
assert_contains "$(json_line)" "loom.pushRemote" "local_upstream_refused_before_picking_msg"
gl_capture push x
assert_exit_fail "$CODE" "local_upstream_refused"
assert_contains "$OUT" "loom.pushRemote" "local_upstream_refused_msg"
remote_has x && fail "local_upstream: x should not be on the remote yet"
git -C "$WORK" config loom.pushRemote origin
gl_capture push x
assert_exit_ok "$CODE" "local_upstream_ok"
assert_contains "$OUT" 'Pushed x to origin' "local_upstream_msg"
remote_has x || fail "local_upstream: x should be on the remote"

# ── GITLAB (push options checked in the trace of the push) ────────────────────

# Let the bare remote accept push options; the trace of the push records the
# exact `git push` line loom ran, options included.
gitlab_origin() {
    git -C "$TMPROOT/remote.git" config receive.advertisePushOptions true
    git -C "$WORK" config loom.remoteType gitlab
}

describe "gitlab: each layer is pushed with its own merge request target"
build_stack
gitlab_origin
gl_capture push b
assert_exit_ok "$CODE" "gitlab_stack_ok"
assert_contains "$OUT" 'Pushed a, b to origin' "gitlab_stack_pushed"
trace="$(gl trace)"
assert_contains "$trace" "-o merge_request.create -o merge_request.target=$BASE_BRANCH -u origin a" "gitlab_stack_opts_a"
assert_contains "$trace" "-o merge_request.create -o merge_request.target=a -u origin b" "gitlab_stack_opts_b"

describe "gitlab: a re-published upper branch gets no new merge request"
a1_sid="$(commit_sid_from_status "A1")"
gl reword "$a1_sid" -m "A1 reworded" > /dev/null
gl_capture push a
assert_exit_ok "$CODE" "gitlab_republish_ok"
assert_contains "$OUT" 'Pushed a to origin' "gitlab_republish_a"
assert_contains "$OUT" 'Re-pushed above a: b' "gitlab_republish_b"
assert_eq "$(remote_oid b)" "$(branch_oid b)" "gitlab_republish_b_oid"
trace="$(gl trace)"
assert_contains "$trace" "-o merge_request.create -o merge_request.target=$BASE_BRANCH -u origin a" "gitlab_republish_opts_a"
assert_contains "$trace" "-o merge_request.target=a -u origin b" "gitlab_republish_opts_b"
assert_not_contains "$trace" "merge_request.create -o merge_request.target=a -u origin b" "gitlab_republish_no_create"

# Make the bare remote answer like GitLab: an MR link per pushed branch, only
# its create form for one named in $TMPROOT/form-only, and a rejection for one
# named in $TMPROOT/reject.
gitlab_hook() {
    remote_pre_receive <<'HOOK'
#!/usr/bin/env bash
root="$(dirname "$PWD")"
while read -r _ _ ref; do
    branch="${ref#refs/heads/}"
    [[ "$branch" == "$(cat "$root/reject" 2>/dev/null)" ]] && exit 1
    if [[ "$branch" == "$(cat "$root/form-only" 2>/dev/null)" ]]; then
        echo "https://gitlab.example.com/g/r/-/merge_requests/new?merge_request%5Bsource_branch%5D=$branch"
    else
        echo "https://gitlab.example.com/g/r/-/merge_requests/$branch"
    fi
done
HOOK
}

describe "gitlab: a branch new on the remote reads created, a known one updated"
build_stack
gitlab_origin
gitlab_hook
gl_capture push b
assert_exit_ok "$CODE" "gitlab_created_ok"
assert_contains "$OUT" "PR created: https://gitlab.example.com/g/r/-/merge_requests/a" "gitlab_created_a"
assert_contains "$OUT" "PR created: https://gitlab.example.com/g/r/-/merge_requests/b" "gitlab_created_b"
a1_sid="$(commit_sid_from_status "A1")"
gl reword "$a1_sid" -m "A1 reworded" > /dev/null
gl_capture push b
assert_exit_ok "$CODE" "gitlab_updated_ok"
assert_contains "$OUT" "PR updated: https://gitlab.example.com/g/r/-/merge_requests/a" "gitlab_updated_a"
assert_contains "$OUT" "PR updated: https://gitlab.example.com/g/r/-/merge_requests/b" "gitlab_updated_b"

describe "gitlab: a branch GitLab opened no MR for gets its create form"
build_stack
gitlab_origin
gitlab_hook
echo x > "$TMPROOT/form-only"
gl_capture push x
assert_exit_ok "$CODE" "gitlab_form_ok"
assert_contains "$OUT" "PR not created for x: GitLab did not create it" "gitlab_form_reason"
assert_contains "$OUT" "Create it at https://gitlab.example.com/g/r/-/merge_requests/new" "gitlab_form_link"

describe "gitlab: a push failing on an upper layer reports the MRs below it"
build_stack
gitlab_origin
gitlab_hook
echo b > "$TMPROOT/reject"
gl_capture push b
assert_exit_fail "$CODE" "gitlab_partial_fails"
assert_contains "$OUT" "Pushed a to origin" "gitlab_partial_pushed"
assert_contains "$OUT" "PR created: https://gitlab.example.com/g/r/-/merge_requests/a" "gitlab_partial_mr"
remote_has b && fail "gitlab_partial: b should not be on the remote"

describe "gitlab: agent mode reports the push and its MRs as JSON, --title on the named branch"
build_stack
gitlab_origin
gitlab_hook
gl_capture_json --agent push b --title "Add B"
assert_exit_ok "$CODE" "gitlab_agent_ok"
assert_contains "$(json_line)" '"push":{"remote":"origin","forge":"gitlab","pushed":["a","b"],"prs":[{"branch":"a","base":"'"$BASE_BRANCH"'","state":"created","url":"https://gitlab.example.com/g/r/-/merge_requests/a"},{"branch":"b","base":"a","state":"created","url":"https://gitlab.example.com/g/r/-/merge_requests/b"}]}' "gitlab_agent_json"
trace="$(gl trace)"
assert_contains "$trace" "-o merge_request.title=Add B -o merge_request.target=a -u origin b" "gitlab_title_on_b"
assert_not_contains "$trace" "merge_request.title=Add B -o merge_request.target=$BASE_BRANCH" "gitlab_title_not_on_a"

describe "gitlab: --draft marks the named branch's merge request a draft, new or not"
build_stack
gitlab_origin
gitlab_hook
gl_capture push b --draft
assert_exit_ok "$CODE" "gitlab_draft_ok"
assert_contains "$OUT" "PR created: https://gitlab.example.com/g/r/-/merge_requests/a" "gitlab_draft_a_plain"
assert_contains "$OUT" "Draft PR created: https://gitlab.example.com/g/r/-/merge_requests/b" "gitlab_draft_b_draft"
assert_not_contains "$OUT" "Draft PR created: https://gitlab.example.com/g/r/-/merge_requests/a" "gitlab_draft_a_not_draft"
trace="$(gl trace)"
assert_contains "$trace" "-o merge_request.create -o merge_request.draft -o merge_request.target=a -u origin b" "gitlab_draft_on_b"
assert_not_contains "$trace" "merge_request.draft -o merge_request.target=$BASE_BRANCH" "gitlab_draft_not_on_a"
b1_sid="$(commit_sid_from_status "B1")"
gl reword "$b1_sid" -m "B1 reworded" > /dev/null
gl_capture_json --agent push b --draft
assert_exit_ok "$CODE" "gitlab_draft_again_ok"
assert_contains "$(json_line)" '{"branch":"b","base":"a","state":"updated","url":"https://gitlab.example.com/g/r/-/merge_requests/b","draft":true}' "gitlab_draft_agent_json"
assert_contains "$(json_line)" "Marked as draft" "gitlab_draft_marked"

# ── GERRIT (the refspec checked on the bare remote) ───────────────────────────

describe "gerrit: --wip, the alias of --draft, uploads the review as work in progress"
build_stack
git -C "$WORK" config loom.remoteType gerrit
gl_capture push x
assert_exit_ok "$CODE" "gerrit_ready_ok"
assert_contains "$OUT" "Pushed x to origin (Gerrit: refs/for/$BASE_BRANCH)" "gerrit_ready_msg"
assert_eq "$(git -C "$TMPROOT/remote.git" rev-parse "refs/for/$BASE_BRANCH")" "$(branch_oid x)" "gerrit_ready_ref"
gl_capture push x --wip
assert_exit_ok "$CODE" "gerrit_wip_ok"
assert_contains "$OUT" "Pushed x to origin (Gerrit: refs/for/$BASE_BRANCH%wip)" "gerrit_wip_msg"
assert_eq "$(git -C "$TMPROOT/remote.git" rev-parse "refs/for/$BASE_BRANCH%wip")" "$(branch_oid x)" "gerrit_wip_ref"

# ── GITHUB (gh shim) ──────────────────────────────────────────────────────────
# The shim is a shell script on PATH; Rust's Command cannot spawn one on
# Windows, so these scenarios only run elsewhere.

if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
    skipped "github scenarios (Rust cannot spawn the shell gh shim on Windows)"
    pass
fi

# A token in the environment would stop loom from picking a gh account.
unset GH_TOKEN GITHUB_TOKEN

# Fake `gh`: records every invocation in $GH_LOG and keeps PR/stack state in
# $GH_STATE so successive pushes see the PRs earlier ones created.
install_gh_shim() {
    GH_SHIM="$TMPROOT/shim"
    GH_LOG="$TMPROOT/gh.log"
    GH_STATE="$TMPROOT/gh-state"
    mkdir -p "$GH_SHIM" "$GH_STATE"
    : > "$GH_LOG"
    cat > "$GH_SHIM/gh" <<'SHIM'
#!/usr/bin/env bash
set -euo pipefail
echo "$*${GH_TOKEN:+ [token $GH_TOKEN]}" >> "$GH_LOG"
arg_after() { local key="$1"; shift; while [[ $# -gt 1 ]]; do [[ "$1" == "$key" ]] && { echo "$2"; return; }; shift; done; echo ""; }
repo="$(arg_after --repo "$@")"
case "$1 ${2:-}" in
    "--version ")
        echo "gh version 9.9.9" ;;
    "auth status")
        # Logged-in accounts are $GH_STATE/account_<user>, holding the token;
        # $GH_STATE/active names the active one.
        accounts=""
        for f in "$GH_STATE"/account_*; do
            [[ -f "$f" ]] || continue
            user="${f##*/account_}"
            active=false; [[ "$(cat "$GH_STATE/active" 2>/dev/null)" == "$user" ]] && active=true
            accounts="$accounts${accounts:+,}{\"state\":\"success\",\"active\":$active,\"login\":\"$user\",\"token\":\"$(cat "$f")\"}"
        done
        echo "{\"hosts\":{\"github.com\":[$accounts]}}" ;;
    "pr list")
        # State files hold "<number> <base> <head owner> [<draft>]"; --head
        # matches the branch name whatever fork it lives in, like GitHub.
        head="$(arg_after --head "$@")"
        if [[ -f "$GH_STATE/pr_$head" ]]; then
            read -r number base owner draft < "$GH_STATE/pr_$head"
            echo "[{\"number\":$number,\"url\":\"https://github.com/$repo/pull/$number\",\"baseRefName\":\"$base\",\"isDraft\":${draft:-false},\"headRepositoryOwner\":{\"login\":\"${owner:-owner}\"}}]"
        else
            echo "[]"
        fi ;;
    "pr create")
        head="$(arg_after --head "$@")"
        base="$(arg_after --base "$@")"
        owner="${repo%%/*}"
        [[ "$head" == *:* ]] && { owner="${head%%:*}"; head="${head#*:}"; }
        draft=false; for arg in "$@"; do [[ "$arg" == --draft ]] && draft=true; done
        number=$(( $(ls "$GH_STATE" | grep -c '^pr_' || true) + 1 ))
        echo "$number $base $owner $draft" > "$GH_STATE/pr_$head"
        echo "https://github.com/$repo/pull/$number" ;;
    "pr edit")
        # $GH_STATE/no_edit makes the retarget fail, like a base GitHub lacks.
        [[ -f "$GH_STATE/no_edit" ]] && { echo "HTTP 422" >&2; exit 1; }
        number="$3"
        base="$(arg_after --base "$@")"
        for f in "$GH_STATE"/pr_*; do
            read -r n _ owner draft < "$f"
            [[ "$n" == "$number" ]] && echo "$number $base ${owner:-owner} ${draft:-false}" > "$f"
        done ;;
    "pr ready")
        # gh pr ready --undo <n> --repo <repo>: back to draft.
        [[ "$3" == "--undo" ]] || { echo "gh shim: only --undo is expected" >&2; exit 1; }
        number="$4"
        for f in "$GH_STATE"/pr_*; do
            read -r n base owner _ < "$f"
            [[ "$n" == "$number" ]] && echo "$number $base ${owner:-owner} true" > "$f"
        done ;;
    "api --hostname")
        # gh api --hostname github.com repos/<repo> --jq .permissions.push:
        # $GH_STATE/push_<token> lists the repos that token can push to, and
        # $GH_STATE/hidden_<token> the ones it cannot see (a 404).
        repo="${4#repos/}"
        grep -qxF "$repo" "$GH_STATE/hidden_${GH_TOKEN:-}" 2>/dev/null && { echo "HTTP 404" >&2; exit 1; }
        grep -qxF "$repo" "$GH_STATE/push_${GH_TOKEN:-}" 2>/dev/null && echo true || echo false ;;
    "api repos"*)
        # gh api repos/<repo>/pulls/<n> --jq .stack
        number="${2##*/}"
        if [[ -f "$GH_STATE/stack_$number" ]]; then
            echo "{\"number\":$(cat "$GH_STATE/stack_$number"),\"size\":2}"
        else
            echo "null"
        fi ;;
    "api --method")
        # gh api --method POST repos/<repo>/stacks[/<n>/add] --input -
        path="$4"
        repo="${path#repos/}"; repo="${repo%%/stacks*}"
        body="$(cat)"
        echo "STDIN: $body" >> "$GH_LOG"
        if [[ "$path" == */add ]]; then
            stack="${path%/add}"; stack="${stack##*/}"
        else
            stack=7
        fi
        for n in $(echo "$body" | grep -oE '[0-9]+'); do echo "$stack" > "$GH_STATE/stack_$n"; done
        # The Stacks API answers with the API url only, no html_url.
        echo "{\"number\":$stack,\"url\":\"https://api.github.com/repos/$repo/stacks/$stack\",\"base\":{\"ref\":\"main\"},\"open\":true}" ;;
    *)
        echo "gh shim: unexpected arguments: $*" >&2
        exit 1 ;;
esac
SHIM
    chmod +x "$GH_SHIM/gh"
    export GH_LOG GH_STATE
}

# Make origin look like GitHub for `gh --repo` while still pushing to the
# local bare remote.
github_origin() {
    git -C "$WORK" remote set-url origin "https://github.com/owner/repo.git"
    git -C "$WORK" remote set-url --push origin "$TMPROOT/remote.git"
    git -C "$WORK" config loom.remoteType github
}

gl_gh() { PATH="$GH_SHIM:$PATH" gl "$@"; }
gl_gh_capture() { OUT=$(gl_gh "$@" 2>&1) && CODE=$? || CODE=$?; }

describe "github: a stack creates chained PRs and registers the stack"
build_stack
install_gh_shim
github_origin
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_stack_ok"
assert_contains "$OUT" 'Pushed a, b to origin' "gh_stack_pushed"
assert_contains "$OUT" "PR created: https://github.com/owner/repo/pull/1" "gh_stack_pr_a"
assert_contains "$OUT" "PR created: https://github.com/owner/repo/pull/2" "gh_stack_pr_b"
assert_contains "$OUT" "Stack #7 registered with 2 PRs" "gh_stack_registered"
assert_not_contains "$OUT" "api.github.com" "gh_stack_no_api_url"
log="$(cat "$GH_LOG")"
assert_contains "$log" "pr create --head a --base $BASE_BRANCH --repo owner/repo" "gh_stack_create_a"
assert_contains "$log" "pr create --head b --base a --repo owner/repo" "gh_stack_create_b"
assert_contains "$log" "api repos/owner/repo/pulls/1 --jq .stack" "gh_stack_membership"
assert_contains "$log" "api --method POST repos/owner/repo/stacks --input -" "gh_stack_post"
assert_contains "$log" 'STDIN: {"pull_requests":[1,2]}' "gh_stack_body"
assert_not_contains "$log" "pr view" "gh_stack_no_browser"

describe "github: pushing again leaves the complete stack alone"
: > "$GH_LOG"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_again_ok"
assert_contains "$OUT" "PR updated: https://github.com/owner/repo/pull/1" "gh_again_a"
assert_contains "$OUT" "PR updated: https://github.com/owner/repo/pull/2" "gh_again_b"
assert_contains "$OUT" "Stack #7 already links these 2 PRs" "gh_again_complete"
assert_not_contains "$(cat "$GH_LOG")" "api --method POST" "gh_again_no_post"

describe "github: a PR with the wrong base is retargeted"
echo "2 $BASE_BRANCH" > "$GH_STATE/pr_b"
: > "$GH_LOG"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_retarget_ok"
assert_contains "$OUT" 'PR updated: https://github.com/owner/repo/pull/2' "gh_retarget_msg"
assert_contains "$OUT" 'Retargeted to a' "gh_retarget_base"
assert_contains "$(cat "$GH_LOG")" "pr edit 2 --repo owner/repo --base a" "gh_retarget_edit"

describe "github: a new layer is added to the existing stack"
git -C "$WORK" branch c b
switch_to c
commit_file "C1" "c1.txt"
switch_to integration
weave_branch "c"
: > "$GH_LOG"
gl_gh_capture push c
assert_exit_ok "$CODE" "gh_extend_ok"
assert_contains "$OUT" 'Pushed a, b, c to origin' "gh_extend_pushed"
assert_contains "$OUT" "PR created: https://github.com/owner/repo/pull/3" "gh_extend_pr_c"
assert_contains "$OUT" "Stack #7 extended with 3 PRs" "gh_extend_msg"
log="$(cat "$GH_LOG")"
assert_contains "$log" "pr create --head c --base b --repo owner/repo" "gh_extend_create_c"
assert_contains "$log" "api --method POST repos/owner/repo/stacks/7/add --input -" "gh_extend_post"
assert_contains "$log" 'STDIN: {"pull_requests":[3]}' "gh_extend_body"
assert_not_contains "$log" "pr view" "gh_extend_no_browser"

describe "github: an upstack layer already in sync ends the stack run"
# d on c on b on a, all published without PRs; PRs exist but no stack yet.
# Only d is then rewritten, so `push b` re-publishes d while c stays in
# sync. d targets c, so the stack can only link a and b.
build_stack
install_gh_shim
github_origin
git -C "$WORK" branch c b
switch_to c
commit_file "C1" "c1.txt"
git -C "$WORK" branch d
switch_to d
commit_file "D1" "d1.txt"
switch_to integration
weave_branch "d"
gl_gh push d --no-pr > /dev/null
echo "1 $BASE_BRANCH owner" > "$GH_STATE/pr_a"
echo "2 a owner" > "$GH_STATE/pr_b"
echo "3 b owner" > "$GH_STATE/pr_c"
echo "4 c owner" > "$GH_STATE/pr_d"
d1_sid="$(commit_sid_from_status "D1")"
gl reword "$d1_sid" -m "D1 reworded" > /dev/null
: > "$GH_LOG"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_gap_ok"
assert_contains "$OUT" 'Re-pushed above b: d' "gh_gap_republished"
assert_contains "$OUT" "Stack #7 registered with 2 PRs" "gh_gap_registered"
assert_contains "$(cat "$GH_LOG")" 'STDIN: {"pull_requests":[1,2]}' "gh_gap_body"

describe "github: a lone branch gets its PR created without a browser"
: > "$GH_LOG"
gl_gh_capture push x
assert_exit_ok "$CODE" "gh_lone_ok"
assert_contains "$OUT" "PR created: https://github.com/owner/repo/pull/5" "gh_lone_created"
assert_contains "$(cat "$GH_LOG")" "pr create --head x --base $BASE_BRANCH --repo owner/repo" "gh_lone_create"
assert_not_contains "$(cat "$GH_LOG")" "pr view" "gh_lone_no_browser"
assert_not_contains "$(cat "$GH_LOG")" "pr create --web" "gh_lone_no_form"
assert_not_contains "$(cat "$GH_LOG")" "stacks" "gh_lone_no_stack"

describe "github: --draft converts an existing PR to a draft once"
: > "$GH_LOG"
gl_gh_capture push x --draft
assert_exit_ok "$CODE" "gh_draft_existing_ok"
assert_contains "$OUT" "PR updated: https://github.com/owner/repo/pull/5" "gh_draft_existing_updated"
assert_contains "$OUT" "Marked as draft" "gh_draft_existing_marked"
assert_contains "$(cat "$GH_LOG")" "pr ready --undo 5 --repo owner/repo" "gh_draft_existing_ready_undo"
: > "$GH_LOG"
gl_gh_capture push x --draft
assert_exit_ok "$CODE" "gh_draft_already_ok"
assert_not_contains "$OUT" "Marked as draft" "gh_draft_already_quiet"
assert_not_contains "$(cat "$GH_LOG")" "pr ready" "gh_draft_already_no_call"

describe "github: --draft creates the named branch's PR as a draft"
create_feature_branch "y"
switch_to y
commit_file "Y1" "y1.txt"
switch_to integration
weave_branch "y"
: > "$GH_LOG"
gl_gh_capture --agent push y --draft
assert_exit_ok "$CODE" "gh_draft_create_ok"
assert_contains "$OUT" "Draft PR created: https://github.com/owner/repo/pull/6" "gh_draft_create_msg"
assert_contains "$OUT" '{"branch":"y","base":"'"$BASE_BRANCH"'","state":"created","url":"https://github.com/owner/repo/pull/6","draft":true}' "gh_draft_create_json"
assert_contains "$(cat "$GH_LOG")" "pr create --head y --base $BASE_BRANCH --repo owner/repo --title Y1 --body  --draft" "gh_draft_create_flag"

describe "github: --draft marks the draft whatever became of the base"
# y's PR 6, undrafted and on the wrong base, is retargeted and marked.
echo "6 a owner false" > "$GH_STATE/pr_y"
: > "$GH_LOG"
gl_gh_capture push y --draft
assert_exit_ok "$CODE" "gh_draft_retarget_ok"
assert_contains "$OUT" "PR updated: https://github.com/owner/repo/pull/6" "gh_draft_retarget_updated"
assert_contains "$OUT" "Retargeted to $BASE_BRANCH" "gh_draft_retarget_base"
assert_contains "$OUT" "Marked as draft" "gh_draft_retarget_marked"
assert_contains "$(cat "$GH_LOG")" "pr ready --undo 6 --repo owner/repo" "gh_draft_retarget_ready_undo"
# The retarget failing does not take the draft with it.
echo "6 a owner false" > "$GH_STATE/pr_y"
touch "$GH_STATE/no_edit"
: > "$GH_LOG"
gl_gh_capture --agent push y --draft
assert_exit_ok "$CODE" "gh_draft_retarget_failed_ok"
assert_contains "$OUT" "Could not retarget https://github.com/owner/repo/pull/6 to" "gh_draft_retarget_failed_warned"
assert_contains "$OUT" "Marked as draft" "gh_draft_retarget_failed_marked"
assert_contains "$OUT" '{"branch":"y","base":"a","state":"updated","url":"https://github.com/owner/repo/pull/6","draft":true,"reason":"Could not retarget' "gh_draft_retarget_failed_json"
assert_contains "$(cat "$GH_LOG")" "pr ready --undo 6 --repo owner/repo" "gh_draft_retarget_failed_ready_undo"
rm "$GH_STATE/no_edit"

# A `gh` whose `--version` fails counts as not installed.
install_broken_gh() {
    install_gh_shim
    printf '#!/usr/bin/env bash\nexit 1\n' > "$GH_SHIM/gh"
}

describe "github without gh: a lone branch is pushed with a link to create its PR"
build_stack
install_broken_gh
github_origin
printf '#!/usr/bin/env bash\necho https://github.com/owner/repo/pull/new/x\n' | remote_pre_receive
gl_gh_capture push x
assert_exit_ok "$CODE" "nogh_lone_ok"
assert_not_contains "$OUT" "/pull/new/" "nogh_lone_no_server_hint"
assert_contains "$OUT" 'Pushed x to origin' "nogh_lone_pushed"
assert_contains "$OUT" "PR not created for x: gh is not installed" "nogh_lone_reason"
assert_contains "$OUT" "Create it at https://github.com/owner/repo/compare/$BASE_BRANCH...x?expand=1" "nogh_lone_link"
assert_contains "$OUT" "Install gh to have loom create it: https://cli.github.com" "nogh_lone_hint"

describe "github without gh: an Enterprise host keeps the server's link"
build_stack
install_broken_gh
github_origin
git -C "$WORK" remote set-url origin "git@ghe.example.com:owner/repo.git"
printf '#!/usr/bin/env bash\necho https://ghe.example.com/owner/repo/pull/new/x\n' | remote_pre_receive
gl_gh_capture push x
assert_exit_ok "$CODE" "nogh_server_link_ok"
assert_contains "$OUT" "https://ghe.example.com/owner/repo/pull/new/x" "nogh_server_link"
assert_contains "$OUT" "PR not created for x: gh is not installed" "nogh_server_link_reason"
assert_not_contains "$OUT" "Create it at" "nogh_server_link_no_form"

describe "github without gh: a stack is refused before the push"
build_stack
install_broken_gh
github_origin
gl_gh_capture push b
assert_exit_fail "$CODE" "nogh_stack_fails"
assert_contains "$OUT" "Cannot create stacked PRs: b is stacked on a" "nogh_stack_msg"
assert_contains "$OUT" "Stacked pull requests need gh" "nogh_stack_why"
remote_has a && fail "nogh_stack: a should not have been pushed"
remote_has b && fail "nogh_stack: b should not have been pushed"

describe "github without gh: --no-pr still pushes the stack"
gl_gh_capture push b --no-pr
assert_exit_ok "$CODE" "nogh_nopr_ok"
assert_contains "$OUT" 'Pushed a, b to origin' "nogh_nopr_pushed"
assert_not_contains "$OUT" "PR not created" "nogh_nopr_no_warning"

describe "github: agent mode creates PRs, leaving a multi-commit one to --title"
build_stack
install_gh_shim
github_origin
write_file "b2.txt" "B2"
gl commit -b b -m "B2" zz > /dev/null
gl_gh_capture --agent push b
assert_exit_ok "$CODE" "gh_agent_ok"
assert_contains "$OUT" '"push":{"remote":"origin","forge":"github","pushed":["a","b"]' "gh_agent_json_pushed"
assert_contains "$OUT" '{"branch":"a","base":"'"$BASE_BRANCH"'","state":"created","url":"https://github.com/owner/repo/pull/1"}' "gh_agent_json_a"
assert_contains "$OUT" '"branch":"b","base":"a","state":"not_created"' "gh_agent_json_b"
assert_contains "$OUT" "PR not created for b: it has several commits and no --title" "gh_agent_untitled"
assert_contains "$OUT" "Re-run with loom push b --title <title>" "gh_agent_title_hint"
assert_not_contains "$(cat "$GH_LOG")" "stacks" "gh_agent_no_stack_yet"

describe "github: --title creates the PR the agent was told to re-run for"
gl_gh_capture --agent push b --title "Add B"
assert_exit_ok "$CODE" "gh_title_ok"
assert_contains "$(cat "$GH_LOG")" "--base a --repo owner/repo --title Add B" "gh_title_passed"
assert_contains "$OUT" '{"branch":"b","base":"a","state":"created","url":"https://github.com/owner/repo/pull/2"}' "gh_title_json"
assert_contains "$OUT" '"stack":7' "gh_title_json_stack"

describe "github: a fork stack gets a PR per layer targeting the base and no stack"
build_stack
install_gh_shim
# Integration tracks the upstream remote (the PR repo); branches go to origin (the fork).
git clone -q --bare "$TMPROOT/remote.git" "$TMPROOT/fork.git"
git -C "$WORK" remote rename origin upstream
git -C "$WORK" remote set-url upstream "https://github.com/owner/repo.git"
git -C "$WORK" remote set-url --push upstream "$TMPROOT/remote.git"
git -C "$WORK" remote add origin "https://github.com/forker/repo.git"
git -C "$WORK" remote set-url --push origin "$TMPROOT/fork.git"
git -C "$WORK" config loom.remoteType github
# b's PR targets the base, so it spans A1 and B1 and creating it would prompt
# for a title. It already exists; only a's PR is created here.
echo "1 $BASE_BRANCH forker" > "$GH_STATE/pr_b"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_fork_ok"
assert_contains "$OUT" 'Pushed a, b to origin' "gh_fork_pushed"
assert_contains "$OUT" "Stacked pull requests are not supported across forks" "gh_fork_warning"
assert_contains "$OUT" "PR created: https://github.com/owner/repo/pull/2" "gh_fork_pr_a"
assert_contains "$OUT" "PR updated: https://github.com/owner/repo/pull/1" "gh_fork_pr_b"
git -C "$TMPROOT/fork.git" rev-parse --verify refs/heads/b > /dev/null 2>&1 || fail "fork: b should be on the fork"
git -C "$TMPROOT/remote.git" rev-parse --verify refs/heads/b > /dev/null 2>&1 && fail "fork: b should not be on upstream"
log="$(cat "$GH_LOG")"
assert_contains "$log" "pr create --head forker:a --base $BASE_BRANCH --repo owner/repo" "gh_fork_create_a"
# b already targets the base, so the fork rule leaves it alone instead of
# retargeting it onto a.
assert_not_contains "$log" "pr edit" "gh_fork_no_retarget"
assert_not_contains "$log" "pr view" "gh_fork_no_browser"
assert_not_contains "$log" "stacks" "gh_fork_no_stack"
assert_not_contains "$log" "[token" "gh_fork_no_account_no_token"

describe "github: a stranger's PR with the same branch name is left alone"
# Someone else's fork has a branch `a` with an open PR against upstream.
echo "9 $BASE_BRANCH stranger" > "$GH_STATE/pr_a"
: > "$GH_LOG"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_stranger_ok"
assert_contains "$OUT" "PR created: https://github.com/owner/repo/pull/3" "gh_stranger_created_ours"
assert_contains "$OUT" "PR updated: https://github.com/owner/repo/pull/1" "gh_stranger_kept_b"
assert_not_contains "$(cat "$GH_LOG")" "pr edit" "gh_stranger_not_edited"

describe "github: gh acts as the fork owner when logged in to that account"
echo "forker-token" > "$GH_STATE/account_forker"
echo "other-token" > "$GH_STATE/account_other"
echo other > "$GH_STATE/active"
echo forker/repo > "$GH_STATE/push_forker-token"
: > "$GH_LOG"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_account_ok"
log="$(cat "$GH_LOG")"
assert_contains "$log" "api --hostname github.com repos/forker/repo --jq .permissions.push [token forker-token]" "gh_account_owner_probed"
assert_not_contains "$log" "[token other-token]" "gh_account_owner_first"
assert_contains "$log" "pr list --head a --repo owner/repo --json number,url,baseRefName,isDraft,headRepositoryOwner --limit 30 [token forker-token]" "gh_account_used"
assert_not_contains "$(gl trace)" "forker-token" "gh_account_not_traced"

describe "github: a GH_TOKEN set by the user wins over the fork owner"
: > "$GH_LOG"
OUT=$(GH_TOKEN=user-token gl_gh push b 2>&1) && CODE=$? || CODE=$?
assert_exit_ok "$CODE" "gh_user_token_ok"
log="$(cat "$GH_LOG")"
assert_not_contains "$log" "auth " "gh_user_token_no_lookup"
assert_not_contains "$log" ".permissions.push" "gh_user_token_no_probe"
assert_contains "$log" "[token user-token]" "gh_user_token_used"

# Same-repo push to an org repo: no account is named after the owner.
describe "github: gh acts as the account that can push to a shared repo"
build_stack
install_gh_shim
github_origin
echo "dfaure-token" > "$GH_STATE/account_dfaure"
echo "kdab-token" > "$GH_STATE/account_dfaure-kdab"
echo dfaure > "$GH_STATE/active"
echo owner/repo > "$GH_STATE/hidden_dfaure-token"
echo owner/repo > "$GH_STATE/push_kdab-token"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_shared_ok"
log="$(cat "$GH_LOG")"
assert_contains "$log" "api --hostname github.com repos/owner/repo --jq .permissions.push [token dfaure-token]" "gh_shared_hidden_probed"
assert_contains "$log" "api --hostname github.com repos/owner/repo --jq .permissions.push [token kdab-token]" "gh_shared_next_probed"
assert_contains "$log" "pr create --head a --base $BASE_BRANCH --repo owner/repo --title A1 --body  [token kdab-token]" "gh_shared_create_a"

describe "github: with no account able to push, gh keeps its active one"
rm -f "$GH_STATE/push_kdab-token" "$GH_STATE"/pr_*
: > "$GH_LOG"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_shared_none_ok"
log="$(cat "$GH_LOG")"
assert_contains "$log" "api --hostname github.com repos/owner/repo --jq .permissions.push [token kdab-token]" "gh_shared_none_probed"
assert_not_contains "$(grep -v permissions.push <<< "$log")" "[token" "gh_shared_none_no_token"

describe "github: with a single account, nothing is probed"
rm -f "$GH_STATE/account_dfaure-kdab"
: > "$GH_LOG"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_single_ok"
log="$(cat "$GH_LOG")"
assert_not_contains "$log" ".permissions.push" "gh_single_no_probe"
assert_not_contains "$log" "[token" "gh_single_no_token"

pass
