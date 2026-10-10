# git-loom completions for bash
# Add to ~/.bashrc: eval "$(git-loom completions bash)"

# Every subcommand, in the order `git-loom --help` groups them. Aliases that
# are a distinct word (fixup, rm, ...) are listed next to the command they
# stand for; the ones that merely abbreviate it (ci, sh, ...) still work but
# are left out to keep the list readable.
__git_loom_commands="init update push pr agent
add
commit fold amend fixup mv rub absorb split swap reword drop rm
branch merge unmerge switch worktree
status tui show diff trace
continue abort"

# Prints the words that may complete $1, given the arguments typed before it
# ($2...). Prints nothing where a path is expected, so that bash falls back to
# file completion.
__git_loom_candidates() {
    local cur=$1
    shift
    local -a args=("$@")
    local n=${#args[@]} i sub= subi=-1 nested=

    # Bash splits `--theme=dark` into `--theme`, `=`, `dark`.
    if (( n > 0 )) && [[ ${args[n-1]} == --theme ||
        ( ${args[n-1]} == = && n > 1 && ${args[n-2]} == --theme ) ]]; then
        echo "auto dark light"
        return
    fi

    for (( i = 0; i < n; i++ )); do
        case ${args[i]} in
            --theme) (( i++ )); [[ ${args[i]-} == = ]] && (( i++ )) ;;
            -*) ;;
            *) sub=${args[i]}; subi=$i; break ;;
        esac
    done

    if [[ -z $sub ]]; then
        if [[ $cur == -* ]]; then
            echo "--no-color --theme --version -h --help"
        else
            echo "$__git_loom_commands"
        fi
        return
    fi

    case $sub in
        up) sub=update ;;
        pr) sub=push ;;
        ci) sub=commit ;;
        amend | fixup | mv | rub) sub=fold ;;
        rw) sub=reword ;;
        rm) sub=drop ;;
        br) sub=branch ;;
        sw) sub=switch ;;
        wt) sub=worktree ;;
        sh) sub=show ;;
        di) sub=diff ;;
        c) sub=continue ;;
        a) sub=abort ;;
    esac

    # The nested subcommand is the first non-flag word after the subcommand.
    for (( i = subi + 1; i < n; i++ )); do
        case ${args[i]} in
            -*) ;;
            *) nested=${args[i]}; break ;;
        esac
    done

    local flags=
    case $sub in
        update) flags="-y --yes" ;;
        push) flags="--no-pr -f --force --title" ;;
        add) flags="-p --patch --hunks --hunks-from" ;;
        commit) flags="-b --branch -i --integration -m --message -p --patch --hunks --hunks-from" ;;
        fold) flags="-c --create -p --patch --above --below --hunks --hunks-from" ;;
        absorb) flags="-n --dry-run" ;;
        split) flags="-m --message -p --patch --hunks --hunks-from" ;;
        reword) flags="-m --message" ;;
        drop) flags="-y --yes" ;;
        status) flags="-f --files -n --context -a --all" ;;
        # Git's own options go after a `--`, so only loom's are listed.
        diff) flags="--staged --cached -a --all" ;;
        agent)
            if [[ -z $nested && $cur != -* ]]; then
                echo "install"
                return
            elif [[ $nested == install ]]; then
                [[ $cur != -* ]] && echo "claude"
                flags="--project"
            fi
            ;;
        branch) flags="-t --target" ;;
        merge) flags="-a --all" ;;
        worktree)
            if [[ -z $nested && $cur != -* ]]; then
                echo "new list ls drop rm path"
                return
            fi
            ;;
    esac

    # Only offer flags once the user commits to one, so that plain words fall
    # back to file completion (commit, add, fold, ... take paths).
    [[ $cur == -* ]] && echo "$flags -h --help"
}

_git_loom_bash() {
    local cur=${COMP_WORDS[COMP_CWORD]} n=$((COMP_CWORD - 1))
    local IFS=$' \t\n'
    # Right after `--theme=` the current word is the `=` itself, but readline
    # replaces only what follows it.
    if [[ $cur == = ]]; then
        cur=
        (( n++ ))
    fi
    COMPREPLY=($(compgen -W "$(__git_loom_candidates "$cur" "${COMP_WORDS[@]:1:n}")" -- "$cur"))
}
# Bash completes by the typed name, never through an alias. LOOM_COMMANDS:
# space-separated names (aliases) that get completion too, default "loom".
# Set it before this script loads, e.g. LOOM_COMMANDS="loom gl".
# shellcheck disable=SC2086 # split into names on purpose
complete -o bashdefault -o default -F _git_loom_bash git-loom ${LOOM_COMMANDS:-loom}

# `git loom <Tab>`: git's own completion calls `_git_<command>`, with its
# `words`, `cword` and `cur` in scope.
_git_loom() {
    local i=1
    while (( i < cword )) && [[ ${words[i]} != loom ]]; do (( i++ )); done
    __gitcomp "$(__git_loom_candidates "$cur" "${words[@]:i+1:cword-i-1}")"
}
