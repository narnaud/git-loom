# git-loom completions for zsh
# Add to ~/.zshrc, after compinit: eval "$(git-loom completions zsh)"

# Completes `git-loom` and `loom`, and `git loom` too: zsh's `_git` calls
# `_git-<command>` for a command it does not know. Either way `words[1]` is the
# command and the arguments follow it.
_git-loom() {
    # Every subcommand, in the order `git-loom --help` groups them. Aliases
    # that are a distinct word (fixup, rm, ...) are listed next to the command
    # they stand for; the ones that merely abbreviate it (ci, sh, ...) still
    # work but are left out to keep the list readable.
    local -a commands=(
        # Workflow
        'init:Initialize a new integration branch tracking a remote'
        'update:Pull-rebase the integration branch and update submodules'
        'push:Push a feature branch to remote'
        'pr:Alias of push'
        'agent:Install the loom skill for AI agents'
        # Staging
        'add:Stage files using short IDs, paths, or zz for all'
        # Commits
        'commit:Create a commit on a feature branch'
        'fold:Amend, fixup, or move commits'
        'amend:Alias of fold'
        'fixup:Alias of fold'
        'mv:Alias of fold'
        'rub:Alias of fold'
        'absorb:Absorb working tree changes into originating commits'
        'split:Split a commit into two sequential commits'
        'swap:Swap two commits within the same sequence'
        'reword:Reword a commit message or rename a branch'
        'drop:Drop a change, a commit, or a branch from history'
        'rm:Alias of drop'
        # Branches
        'branch:Manage feature branches (create, merge, unmerge)'
        'switch:Switch to any branch for testing (without weaving)'
        'worktree:Manage worktrees, each with its own integration branch'
        # Inspection
        'status:Show the branch-aware status'
        'tui:Interactive status TUI (tree + diff, with actions)'
        'show:Show the diff and metadata for a commit (like git show)'
        'diff:Show a diff using short IDs (like git diff)'
        'trace:Show the latest command trace'
        # Recovery
        'continue:Resume a paused operation after resolving conflicts'
        'abort:Cancel a paused operation and restore original state'
    )
    local -a help_flags=(
        '-h:Show help information'
        '--help:Show help information'
    )
    local cur=${words[CURRENT]}
    local i sub= subi=0 nested=

    if [[ ${words[CURRENT-1]} == --theme ]]; then
        local -a themes=(auto dark light)
        _describe -t themes 'color theme' themes
        return
    fi

    for (( i = 2; i < CURRENT; i++ )); do
        case ${words[i]} in
            --theme) (( i++ )) ;;
            -*) ;;
            *) sub=${words[i]}; subi=$i; break ;;
        esac
    done

    if [[ -z $sub ]]; then
        if [[ $cur == -* ]]; then
            local -a top_flags=(
                '--no-color:Disable colored output'
                '--theme:Color theme: auto, dark, or light'
                '--version:Show version information'
                $help_flags
            )
            _describe -t options 'option' top_flags
        else
            _describe -t commands 'git-loom command' commands
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
    # Clap reads any word after `branch -t` as the name of a branch to create,
    # never as a nested subcommand.
    for (( i = subi + 1; i < CURRENT; i++ )); do
        case ${words[i]} in
            -t | --target) [[ $sub == branch ]] && { nested=--target; break; } ;;
            -*) ;;
            *) nested=${words[i]}; break ;;
        esac
    done

    local -a flags subs
    case $sub in
        update)
            flags=(
                '-y:Remove local branches whose upstream was deleted'
                '--yes:Remove local branches whose upstream was deleted'
            ) ;;
        push)
            flags=(
                '--no-pr:Push without creating a PR or Gerrit review'
                '-f:Push with --force instead of --force-with-lease'
                '--force:Push with --force instead of --force-with-lease'
                '--title:Title of the PR created for this branch'
            ) ;;
        add)
            flags=(
                '-p:Interactively select hunks to stage'
                '--patch:Interactively select hunks to stage'
                '--hunks:Hunk id to pick, once per id (needs --hunks-from)'
                '--hunks-from:Fingerprint of the listing --hunks came from'
            ) ;;
        commit)
            flags=(
                '-b:Target feature branch'
                '--branch:Target feature branch'
                '-i:Commit to the integration branch (loose commit)'
                '--integration:Commit to the integration branch (loose commit)'
                '-m:Commit message'
                '--message:Commit message'
                '-p:Interactively select hunks to stage'
                '--patch:Interactively select hunks to stage'
                '--hunks:Hunk id to pick, once per id (needs --hunks-from)'
                '--hunks-from:Fingerprint of the listing --hunks came from'
            ) ;;
        fold)
            flags=(
                '-c:Create a new branch from the source commit(s)'
                '--create:Create a new branch from the source commit(s)'
                '-p:Interactively select hunks to fold'
                '--patch:Interactively select hunks to fold'
                '--above:Move the source commit(s) above this commit'
                '--below:Move the source commit(s) below this commit'
                '--hunks:Hunk id to pick, once per id (needs --hunks-from)'
                '--hunks-from:Fingerprint of the listing --hunks came from'
            ) ;;
        absorb)
            flags=(
                '-n:Show what would be absorbed without making changes'
                '--dry-run:Show what would be absorbed without making changes'
            ) ;;
        split)
            flags=(
                '-m:Message for the first commit'
                '--message:Message for the first commit'
                '-p:Interactively pick hunks for the first commit'
                '--patch:Interactively pick hunks for the first commit'
                '--hunks:Hunk id to pick, once per id (needs --hunks-from)'
                '--hunks-from:Fingerprint of the listing --hunks came from'
            ) ;;
        reword)
            flags=(
                '-m:New message or branch name'
                '--message:New message or branch name'
            ) ;;
        drop)
            flags=(
                '-y:Skip confirmation prompt'
                '--yes:Skip confirmation prompt'
            ) ;;
        status)
            flags=(
                '-f:Show files changed in each commit'
                '--files:Show files changed in each commit'
                '-a:Show all branches including hidden ones'
                '--all:Show all branches including hidden ones'
            ) ;;
        diff)
            # Git's own options go after a `--`, so only loom's are listed.
            flags=(
                '--staged:Show staged changes (index vs HEAD)'
                '--cached:Alias of --staged'
                '-a:Show all changes, staged and unstaged'
                '--all:Show all changes, staged and unstaged'
            ) ;;
        agent)
            if [[ -z $nested ]]; then
                subs=('install:Install the loom skill for an AI agent')
            elif [[ $nested == install ]]; then
                [[ $cur != -* ]] && subs=('claude:Claude Code')
                flags=('--project:Install into the repository instead of the home directory')
            fi ;;
        branch)
            if [[ -z $nested ]]; then
                subs=(
                    'new:Create a new feature branch'
                    'create:Alias of new'
                    'merge:Weave an existing branch into integration'
                    'unmerge:Remove a branch from integration'
                )
            fi
            case $nested in
                merge)
                    flags=(
                        '-a:Also show remote branches'
                        '--all:Also show remote branches'
                    ) ;;
                unmerge) ;;
                # `branch`, `branch new` and `branch create` all take a target.
                *)
                    flags=(
                        '-t:Target commit, branch, or shortID'
                        '--target:Target commit, branch, or shortID'
                    ) ;;
            esac ;;
        worktree)
            if [[ -z $nested ]]; then
                subs=(
                    'new:Create a worktree with its own integration branch'
                    'list:List the worktrees with their short IDs'
                    'ls:Alias of list'
                    'drop:Remove a worktree'
                    'rm:Alias of drop'
                    'path:Print a worktree path'
                )
            fi ;;
    esac

    if [[ $cur == -* ]]; then
        flags+=($help_flags)
        _describe -t options 'option' flags
    elif (( $#subs )); then
        _describe -t commands "$sub command" subs
    else
        # commit, add, fold, ... take paths.
        _files
    fi
}

# Git's own zsh completion calls this one for `git loom`, under ksh emulation
# with the whole command line in `words` and the 0-based `cword` and
# `__git_cmd_idx` of bash's completion.
_git_loom() {
    emulate -L zsh
    words=(${words[__git_cmd_idx+1,-1]})
    (( CURRENT = cword - __git_cmd_idx + 1 ))
    _git-loom
}

(( $+functions[compdef] )) && compdef _git-loom git-loom loom
