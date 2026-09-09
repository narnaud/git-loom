#!/usr/bin/env bash
# Integration tests for: gl push
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

remote_has()    { git -C "$TMPROOT/remote.git" rev-parse --verify "refs/heads/$1" > /dev/null 2>&1; }
remote_oid()    { git -C "$TMPROOT/remote.git" rev-parse "refs/heads/$1"; }

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
}

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

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

# ══════════════════════════════════════════════════════════════════════════════
# PLAIN REMOTE
# ══════════════════════════════════════════════════════════════════════════════

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

describe "--no-pr pushes the downstack too"
build_stack
gl_capture push b --no-pr
assert_exit_ok "$CODE" "nopr_ok"
assert_eq "$(remote_oid a)" "$(branch_oid a)" "nopr_a_oid"
assert_eq "$(remote_oid b)" "$(branch_oid b)" "nopr_b_oid"

# ══════════════════════════════════════════════════════════════════════════════
# GITLAB (push options checked in the trace of the push)
# ══════════════════════════════════════════════════════════════════════════════

# Let the bare remote accept push options; the trace of the push records the
# exact `git push` line loom ran, options included.
gitlab_origin() {
    git -C "$TMPROOT/remote.git" config receive.advertisePushOptions true
    git -C "$WORK" config loom.remote-type gitlab
}

describe "gitlab: each layer is pushed with its own merge request target"
build_stack
gitlab_origin
gl_capture push b
assert_exit_ok "$CODE" "gitlab_stack_ok"
assert_contains "$OUT" 'Pushed a to origin' "gitlab_stack_a"
assert_contains "$OUT" 'Pushed b to origin' "gitlab_stack_b"
trace="$(gl trace)"
assert_contains "$trace" "-o merge_request.create -o merge_request.target=$BASE_BRANCH -u origin a" "gitlab_stack_opts_a"
assert_contains "$trace" "-o merge_request.create -o merge_request.target=a -u origin b" "gitlab_stack_opts_b"

describe "gitlab: a re-published upper branch gets no new merge request"
a1_sid="$(commit_sid_from_status "A1")"
gl reword "$a1_sid" -m "A1 reworded" > /dev/null
gl_capture push a
assert_exit_ok "$CODE" "gitlab_republish_ok"
assert_contains "$OUT" 'Pushed a to origin' "gitlab_republish_a"
assert_contains "$OUT" 'Pushed b to origin' "gitlab_republish_b"
assert_eq "$(remote_oid b)" "$(branch_oid b)" "gitlab_republish_b_oid"
trace="$(gl trace)"
assert_contains "$trace" "-o merge_request.create -o merge_request.target=$BASE_BRANCH -u origin a" "gitlab_republish_opts_a"
assert_contains "$trace" "-o merge_request.target=a -u origin b" "gitlab_republish_opts_b"
assert_not_contains "$trace" "merge_request.create -o merge_request.target=a -u origin b" "gitlab_republish_no_create"

# ══════════════════════════════════════════════════════════════════════════════
# GITHUB (gh shim)
# ══════════════════════════════════════════════════════════════════════════════
# The shim is a shell script on PATH; Rust's Command cannot spawn one on
# Windows, so these scenarios only run elsewhere.

if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
    skipped "github scenarios (Rust cannot spawn the shell gh shim on Windows)"
    pass
fi

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
echo "$*" >> "$GH_LOG"
arg_after() { local key="$1"; shift; while [[ $# -gt 1 ]]; do [[ "$1" == "$key" ]] && { echo "$2"; return; }; shift; done; echo ""; }
repo="$(arg_after --repo "$@")"
case "$1 ${2:-}" in
    "--version ")
        echo "gh version 9.9.9" ;;
    "pr list")
        # State files hold "<number> <base> <head owner>"; --head matches the
        # branch name whatever fork it lives in, like GitHub.
        head="$(arg_after --head "$@")"
        if [[ -f "$GH_STATE/pr_$head" ]]; then
            read -r number base owner < "$GH_STATE/pr_$head"
            echo "[{\"number\":$number,\"url\":\"https://github.com/$repo/pull/$number\",\"baseRefName\":\"$base\",\"headRepositoryOwner\":{\"login\":\"${owner:-owner}\"}}]"
        else
            echo "[]"
        fi ;;
    "pr create")
        head="$(arg_after --head "$@")"
        base="$(arg_after --base "$@")"
        owner="${repo%%/*}"
        [[ "$head" == *:* ]] && { owner="${head%%:*}"; head="${head#*:}"; }
        number=$(( $(ls "$GH_STATE" | grep -c '^pr_' || true) + 1 ))
        echo "$number $base $owner" > "$GH_STATE/pr_$head"
        echo "https://github.com/$repo/pull/$number" ;;
    "pr edit")
        number="$3"
        base="$(arg_after --base "$@")"
        for f in "$GH_STATE"/pr_*; do
            read -r n _ < "$f"
            [[ "$n" == "$number" ]] && echo "$number $base" > "$f"
        done ;;
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
    git -C "$WORK" config loom.remote-type github
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
assert_not_contains "$log" "--web" "gh_stack_no_browser"

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
assert_contains "$OUT" 'PR retargeted to a: https://github.com/owner/repo/pull/2' "gh_retarget_msg"
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

describe "github: a lone branch keeps the browser flow"
: > "$GH_LOG"
gl_gh_capture push x
assert_exit_ok "$CODE" "gh_lone_ok"
assert_contains "$(cat "$GH_LOG")" "pr create --web --head x --base $BASE_BRANCH --repo owner/repo" "gh_lone_web"
assert_not_contains "$(cat "$GH_LOG")" "stacks" "gh_lone_no_stack"

describe "github: agent mode never creates PRs and skips the stack"
build_stack
install_gh_shim
github_origin
gl_gh_capture --agent push b
assert_exit_ok "$CODE" "gh_agent_ok"
assert_contains "$OUT" "Skipped creating a PR" "gh_agent_skipped"
assert_not_contains "$(cat "$GH_LOG")" "pr create" "gh_agent_no_create"
assert_not_contains "$(cat "$GH_LOG")" "stacks" "gh_agent_no_stack"

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
git -C "$WORK" config loom.remote-type github
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
assert_not_contains "$log" "--web" "gh_fork_no_browser"
assert_not_contains "$log" "stacks" "gh_fork_no_stack"

describe "github: a stranger's PR with the same branch name is left alone"
# Someone else's fork has a branch `a` with an open PR against upstream.
echo "9 $BASE_BRANCH stranger" > "$GH_STATE/pr_a"
: > "$GH_LOG"
gl_gh_capture push b
assert_exit_ok "$CODE" "gh_stranger_ok"
assert_contains "$OUT" "PR created: https://github.com/owner/repo/pull/3" "gh_stranger_created_ours"
assert_contains "$OUT" "PR updated: https://github.com/owner/repo/pull/1" "gh_stranger_kept_b"
assert_not_contains "$(cat "$GH_LOG")" "pr edit" "gh_stranger_not_edited"

pass
