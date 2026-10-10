# git-loom completions for fish
# Add to ~/.config/fish/config.fish: git-loom completions fish | source
# `git loom <Tab>` needs nothing more: fish's git completion hands custom
# commands to `git-loom`'s completions.

# Succeeds when the words typed so far form the given subcommand path:
# `__git_loom_is ''` before any subcommand, `__git_loom_is fold` for fold and
# its aliases, `__git_loom_is branch ''` for branch with no nested subcommand
# yet, `__git_loom_is branch merge` once it is merge.
function __git_loom_is
    set -l tokens (commandline -opc)
    set -e tokens[1]
    set -l sub
    set -l nested
    set -l skip
    for t in $tokens
        if set -q skip[1]
            set -e skip
            continue
        end
        if not set -q sub[1]
            switch $t
                case --theme
                    set skip 1
                case '-*'
                case '*'
                    set sub $t
                    switch $sub
                        case up
                            set sub update
                        case pr
                            set sub push
                        case ci
                            set sub commit
                        case amend fixup mv rub
                            set sub fold
                        case rw
                            set sub reword
                        case rm
                            set sub drop
                        case br
                            set sub branch
                        case sw
                            set sub switch
                        case wt
                            set sub worktree
                        case sh
                            set sub show
                        case di
                            set sub diff
                        case c
                            set sub continue
                        case a
                            set sub abort
                    end
            end
        else
            # The nested subcommand is the first non-flag word after it.
            switch $t
                # Clap reads any word after `branch -t` as the name of a
                # branch to create, never as a nested subcommand.
                case -t --target
                    if test "$sub" = branch
                        set nested --target
                        break
                    end
                case '-*'
                case '*'
                    set nested $t
                    break
            end
        end
    end
    test "$sub" = "$argv[1]"; or return 1
    set -q argv[2]; or return 0
    test "$nested" = "$argv[2]"
end

set -l top "__git_loom_is ''"

complete -c git-loom -s h -l help -d 'Show help information'
complete -c git-loom -n $top -l no-color -d 'Disable colored output'
complete -c git-loom -n $top -l theme -x -a 'auto dark light' -d 'Color theme'
complete -c git-loom -n $top -l version -d 'Show version information'

# Every subcommand, in the order `git-loom --help` groups them. Aliases that
# are a distinct word (fixup, rm, ...) are listed next to the command they
# stand for; the ones that merely abbreviate it (ci, sh, ...) still work but
# are left out to keep the list readable.
# Workflow
complete -c git-loom -n $top -f -a init -d 'Initialize a new integration branch tracking a remote'
complete -c git-loom -n $top -f -a update -d 'Pull-rebase the integration branch and update submodules'
complete -c git-loom -n $top -f -a push -d 'Push a feature branch to remote'
complete -c git-loom -n $top -f -a pr -d 'Alias of push'
complete -c git-loom -n $top -f -a agent -d 'Install the loom skill for AI agents'
# Staging
complete -c git-loom -n $top -f -a add -d 'Stage files using short IDs, paths, or zz for all'
# Commits
complete -c git-loom -n $top -f -a commit -d 'Create a commit on a feature branch'
complete -c git-loom -n $top -f -a fold -d 'Amend, fixup, or move commits'
complete -c git-loom -n $top -f -a amend -d 'Alias of fold'
complete -c git-loom -n $top -f -a fixup -d 'Alias of fold'
complete -c git-loom -n $top -f -a mv -d 'Alias of fold'
complete -c git-loom -n $top -f -a rub -d 'Alias of fold'
complete -c git-loom -n $top -f -a absorb -d 'Absorb working tree changes into originating commits'
complete -c git-loom -n $top -f -a split -d 'Split a commit into two sequential commits'
complete -c git-loom -n $top -f -a swap -d 'Swap two commits within the same sequence'
complete -c git-loom -n $top -f -a reword -d 'Reword a commit message or rename a branch'
complete -c git-loom -n $top -f -a drop -d 'Drop a change, a commit, or a branch from history'
complete -c git-loom -n $top -f -a rm -d 'Alias of drop'
# Branches
complete -c git-loom -n $top -f -a branch -d 'Manage feature branches (create, merge, unmerge)'
complete -c git-loom -n $top -f -a switch -d 'Switch to any branch for testing (without weaving)'
complete -c git-loom -n $top -f -a worktree -d 'Manage worktrees, each with its own integration branch'
# Inspection
complete -c git-loom -n $top -f -a status -d 'Show the branch-aware status'
complete -c git-loom -n $top -f -a tui -d 'Interactive status TUI (tree + diff, with actions)'
complete -c git-loom -n $top -f -a show -d 'Show the diff and metadata for a commit (like git show)'
complete -c git-loom -n $top -f -a diff -d 'Show a diff using short IDs (like git diff)'
complete -c git-loom -n $top -f -a trace -d 'Show the latest command trace'
# Recovery
complete -c git-loom -n $top -f -a continue -d 'Resume a paused operation after resolving conflicts'
complete -c git-loom -n $top -f -a abort -d 'Cancel a paused operation and restore original state'

# Flags are offered beside files: commit, add, fold, ... take paths.
complete -c git-loom -n '__git_loom_is update' -s y -l yes -d 'Skip confirmation prompt'

complete -c git-loom -n '__git_loom_is push' -l no-pr -d 'Push without creating a PR or Gerrit review'
complete -c git-loom -n '__git_loom_is push' -s f -l force -d 'Push with --force instead of --force-with-lease'
complete -c git-loom -n '__git_loom_is push' -l title -x -d 'Title of the PR created for this branch'

complete -c git-loom -n '__git_loom_is add' -s p -l patch -d 'Interactively select hunks to stage'
complete -c git-loom -n '__git_loom_is add' -l hunks -x -d 'Hunk id to pick, once per id (needs --hunks-from)'
complete -c git-loom -n '__git_loom_is add' -l hunks-from -x -d 'Fingerprint of the listing --hunks came from'

complete -c git-loom -n '__git_loom_is commit' -s b -l branch -x -d 'Target feature branch'
complete -c git-loom -n '__git_loom_is commit' -s i -l integration -d 'Commit to the integration branch (loose commit)'
complete -c git-loom -n '__git_loom_is commit' -s m -l message -x -d 'Commit message'
complete -c git-loom -n '__git_loom_is commit' -s p -l patch -d 'Interactively select hunks to stage'
complete -c git-loom -n '__git_loom_is commit' -l hunks -x -d 'Hunk id to pick, once per id (needs --hunks-from)'
complete -c git-loom -n '__git_loom_is commit' -l hunks-from -x -d 'Fingerprint of the listing --hunks came from'

complete -c git-loom -n '__git_loom_is fold' -s c -l create -d 'Create a new branch from the source commit(s)'
complete -c git-loom -n '__git_loom_is fold' -s p -l patch -d 'Interactively select hunks to fold'
complete -c git-loom -n '__git_loom_is fold' -l above -x -d 'Move the source commit(s) above this commit'
complete -c git-loom -n '__git_loom_is fold' -l below -x -d 'Move the source commit(s) below this commit'
complete -c git-loom -n '__git_loom_is fold' -l hunks -x -d 'Hunk id to pick, once per id (needs --hunks-from)'
complete -c git-loom -n '__git_loom_is fold' -l hunks-from -x -d 'Fingerprint of the listing --hunks came from'

complete -c git-loom -n '__git_loom_is absorb' -s n -l dry-run -d 'Show what would be absorbed without making changes'

complete -c git-loom -n '__git_loom_is split' -s m -l message -x -d 'Message for the first commit'
complete -c git-loom -n '__git_loom_is split' -s p -l patch -d 'Interactively pick hunks for the first commit'
complete -c git-loom -n '__git_loom_is split' -l hunks -x -d 'Hunk id to pick, once per id (needs --hunks-from)'
complete -c git-loom -n '__git_loom_is split' -l hunks-from -x -d 'Fingerprint of the listing --hunks came from'

complete -c git-loom -n '__git_loom_is reword' -s m -l message -x -d 'New message or branch name'

complete -c git-loom -n '__git_loom_is drop' -s y -l yes -d 'Skip confirmation prompt'

complete -c git-loom -n '__git_loom_is status' -s f -l files -d 'Show files changed in each commit'
complete -c git-loom -n '__git_loom_is status' -s n -l context -x -d 'Number of commits to show at and before the base'
complete -c git-loom -n '__git_loom_is status' -s a -l all -d 'Show all branches including hidden ones'

# Git's own options go after a `--`, so only loom's are listed.
complete -c git-loom -n '__git_loom_is diff' -l staged -d 'Show staged changes (index vs HEAD)'
complete -c git-loom -n '__git_loom_is diff' -l cached -d 'Alias of --staged'
complete -c git-loom -n '__git_loom_is diff' -s a -l all -d 'Show all changes, staged and unstaged'

complete -c git-loom -n "__git_loom_is agent ''" -f -a install -d 'Install the loom skill for an AI agent'
complete -c git-loom -n '__git_loom_is agent install' -f -a claude -d 'Claude Code'
complete -c git-loom -n '__git_loom_is agent install' -l project -d 'Install into the repository instead of the home directory'

complete -c git-loom -n "__git_loom_is branch ''" -f -a new -d 'Create a new feature branch'
complete -c git-loom -n "__git_loom_is branch ''" -f -a create -d 'Alias of new'
complete -c git-loom -n "__git_loom_is branch ''" -f -a merge -d 'Weave an existing branch into integration'
complete -c git-loom -n "__git_loom_is branch ''" -f -a unmerge -d 'Remove a branch from integration'
# `branch`, `branch new` and `branch create` all take a target.
complete -c git-loom -n '__git_loom_is branch; and not __git_loom_is branch merge; and not __git_loom_is branch unmerge' -s t -l target -x -d 'Target commit, branch, or shortID'
complete -c git-loom -n '__git_loom_is branch merge' -s a -l all -d 'Also show remote branches'

complete -c git-loom -n "__git_loom_is worktree ''" -f -a new -d 'Create a worktree with its own integration branch'
complete -c git-loom -n "__git_loom_is worktree ''" -f -a list -d 'List the worktrees with their short IDs'
complete -c git-loom -n "__git_loom_is worktree ''" -f -a ls -d 'Alias of list'
complete -c git-loom -n "__git_loom_is worktree ''" -f -a drop -d 'Remove a worktree'
complete -c git-loom -n "__git_loom_is worktree ''" -f -a rm -d 'Alias of drop'
complete -c git-loom -n "__git_loom_is worktree ''" -f -a path -d 'Print a worktree path'
