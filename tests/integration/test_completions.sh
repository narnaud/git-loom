#!/usr/bin/env bash
# Integration tests for: gl completions (bash, zsh, fish)
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_repo_with_remote
SCRIPTS="$TMPROOT/completions"
mkdir -p "$SCRIPTS"

describe "each Unix shell gets its script"
for sh in bash zsh fish; do
    gl_capture completions "$sh"
    assert_exit_ok "$CODE" "${sh}_exit"
    assert_contains "$OUT" "loom" "${sh}_script"
    gl completions "$sh" > "$SCRIPTS/c.$sh"
    assert_eq "$(tr -cd '\r' < "$SCRIPTS/c.$sh" | wc -c | tr -d ' ')" "0" "${sh}_no_cr"
done

describe "bash completes subcommands, nested subcommands and flags"
bash -n "$SCRIPTS/c.bash" || fail "bash syntax"
complete_bash() {
    bash -c 'source "$1"; shift; COMP_WORDS=("$@"); COMP_CWORD=$(($# - 1));
        _git_loom_bash; echo "${COMPREPLY[*]}"' _ "$SCRIPTS/c.bash" "$@"
}
assert_eq "$(complete_bash loom tr)" "trace" "bash_command"
assert_eq "$(complete_bash loom wt '')" "new list ls drop rm path" "bash_worktree_subs"
assert_eq "$(complete_bash loom branch '')" "new create merge unmerge" "bash_branch_subs"
# Clap takes `branch -t HEAD merge` as creating a branch named merge.
assert_eq "$(complete_bash loom branch -t HEAD '')" "" "bash_no_subs_after_target"
assert_eq "$(complete_bash loom branch -t '')" "" "bash_no_subs_as_target"
assert_eq "$(complete_bash loom --theme '')" "auto dark light" "bash_theme"
# Bash splits `--theme=dark` into three words.
assert_eq "$(complete_bash loom --theme =)" "auto dark light" "bash_theme_eq"
assert_eq "$(complete_bash loom --theme = d)" "dark" "bash_theme_eq_prefix"
assert_eq "$(complete_bash loom --theme = dark tr)" "trace" "bash_theme_eq_value"
assert_eq "$(complete_bash loom absorb -)" "-n --dry-run -h --help" "bash_flags"
assert_eq "$(complete_bash loom commit x)" "" "bash_paths_left_to_bash"

describe "bash completes the names in LOOM_COMMANDS, loom by default"
completed_names() {
    bash -c 'source "$1"; complete -p | sed -n "s/.*_git_loom_bash //p" | sort | xargs' _ "$SCRIPTS/c.bash"
}
assert_eq "$(completed_names)" "git-loom loom" "bash_default_names"
assert_eq "$(LOOM_COMMANDS="gl loom" completed_names)" "git-loom gl loom" "bash_loom_commands"

if command -v zsh >/dev/null; then
    describe "zsh script parses and completes"
    zsh -n "$SCRIPTS/c.zsh" || fail "zsh syntax"
    # Stubs print the candidates `_git-loom` hands to zsh's completion system.
    complete_zsh() {
        zsh -fc 'source "$1"; shift
            _describe() { local a=${@[-1]}; print -r -- ${${(P)a}%%:*} }
            _files() { print -r -- _files }
            words=("$@"); CURRENT=$#; _git-loom' _ "$SCRIPTS/c.zsh" "$@"
    }
    assert_eq "$(complete_zsh loom branch '')" "new create merge unmerge" "zsh_branch_subs"
    assert_eq "$(complete_zsh loom branch -t HEAD '')" "_files" "zsh_no_subs_after_target"
    assert_eq "$(complete_zsh loom absorb -)" "-n --dry-run -h --help" "zsh_flags"
    # Git's zsh wrapper passes the whole line, 0-based indexes, ksh options.
    complete_git_zsh() {
        zsh -fc 'source "$1"; shift
            _describe() { local a=${@[-1]}; print -r -- ${${(P)a}%%:*} }
            words=("$@"); CURRENT=$(( $# - 2 )); cword=$(( $# - 1 )); __git_cmd_idx=2
            emulate ksh -c _git_loom' _ "$SCRIPTS/c.zsh" "$@"
    }
    assert_eq "$(complete_git_zsh git -p loom absorb -)" "-n --dry-run -h --help" "zsh_git_wrapper"
else
    skipped "zsh not installed"
fi

if command -v fish >/dev/null; then
    describe "fish completes subcommands and flags"
    fish -n "$SCRIPTS/c.fish" || fail "fish syntax"
    complete_fish() {
        fish -c 'source $argv[1]; complete -C $argv[2] | string replace -r "\t.*" "" | string join " "' \
            "$SCRIPTS/c.fish" "$1"
    }
    assert_eq "$(complete_fish 'git-loom tr')" "trace" "fish_command"
    assert_eq "$(complete_fish 'git-loom branch merge --')" "--all --help" "fish_nested_flags"
    EMPTY="$TMPROOT/empty"
    mkdir -p "$EMPTY"
    for b in branch br; do
        assert_eq "$(cd "$EMPTY" && complete_fish "git-loom $b -t HEAD ")" "" "fish_${b}_no_subs_after_target"
    done
else
    skipped "fish not installed"
fi

pass
