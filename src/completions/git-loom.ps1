# git-loom completions for PowerShell
# Add to your $PROFILE: Invoke-Expression (&git-loom completions powershell | Out-String)

$_gitLoomCompleter = {
    param($wordToComplete, $commandAst, $cursorPosition)

    # Every subcommand, in the order `git-loom --help` groups them. Aliases that
    # are a distinct word (fixup, rm, ...) are listed next to the command they
    # stand for; the ones that merely abbreviate it (ci, sh, ...) still work but
    # are left out to keep the list readable.
    $commands = @(
        # Workflow
        @{ Name = 'init'; Description = 'Initialize a new integration branch tracking a remote' },
        @{ Name = 'update'; Description = 'Pull-rebase the integration branch and update submodules' },
        @{ Name = 'push'; Description = 'Push a feature branch to remote' },
        @{ Name = 'pr'; Description = 'Alias of push' },
        @{ Name = 'agent'; Description = 'Install the loom skill for AI agents' },
        # Staging
        @{ Name = 'add'; Description = 'Stage files using short IDs, paths, or zz for all' },
        # Commits
        @{ Name = 'commit'; Description = 'Create a commit on a feature branch' },
        @{ Name = 'fold'; Description = 'Amend, fixup, or move commits' },
        @{ Name = 'amend'; Description = 'Alias of fold' },
        @{ Name = 'fixup'; Description = 'Alias of fold' },
        @{ Name = 'mv'; Description = 'Alias of fold' },
        @{ Name = 'rub'; Description = 'Alias of fold' },
        @{ Name = 'absorb'; Description = 'Absorb working tree changes into originating commits' },
        @{ Name = 'split'; Description = 'Split a commit into two sequential commits' },
        @{ Name = 'swap'; Description = 'Swap two commits within the same sequence' },
        @{ Name = 'reword'; Description = 'Reword a commit message or rename a branch' },
        @{ Name = 'drop'; Description = 'Drop a change, a commit, or a branch from history' },
        @{ Name = 'rm'; Description = 'Alias of drop' },
        # Branches
        @{ Name = 'branch'; Description = 'Manage feature branches (create, merge, unmerge)' },
        @{ Name = 'switch'; Description = 'Switch to any branch for testing (without weaving)' },
        # Inspection
        @{ Name = 'status'; Description = 'Show the branch-aware status' },
        @{ Name = 'show'; Description = 'Show the diff and metadata for a commit (like git show)' },
        @{ Name = 'diff'; Description = 'Show a diff using short IDs (like git diff)' },
        @{ Name = 'trace'; Description = 'Show the latest command trace' },
        # Recovery
        @{ Name = 'continue'; Description = 'Resume a paused operation after resolving conflicts' },
        @{ Name = 'abort'; Description = 'Cancel a paused operation and restore original state' }
    )

    # Alias -> the command whose flags apply. Includes the abbreviations that
    # are not offered above, so typing one still completes its flags.
    $aliases = @{
        'up' = 'update'; 'pr' = 'push'; 'ci' = 'commit'
        'amend' = 'fold'; 'am' = 'fold'; 'fixup' = 'fold'; 'mv' = 'fold'; 'rub' = 'fold'
        'rw' = 'reword'; 'rm' = 'drop'; 'br' = 'branch'; 'sw' = 'switch'
        'sh' = 'show'; 'di' = 'diff'; 'c' = 'continue'; 'a' = 'abort'
    }

    $helpFlags = @(
        @{ Name = '-h'; Description = 'Show help information' },
        @{ Name = '--help'; Description = 'Show help information' }
    )

    # Valid anywhere (global flags).
    $globalFlags = @(
        @{ Name = '--agent'; Description = 'Machine-readable JSON status output for AI agents' }
    )

    # Only valid before the subcommand.
    $topFlags = @(
        @{ Name = '--no-color'; Description = 'Disable colored output' },
        @{ Name = '--theme'; Description = 'Color theme: auto, dark, or light' },
        @{ Name = '--version'; Description = 'Show version information' }
    ) + $globalFlags + $helpFlags

    $themeValues = @('auto', 'dark', 'light')

    # Tokens already typed, minus the one under the cursor (it is a prefix).
    $tokens = @($commandAst.ToString() -split '\s+' | Where-Object { $_ -ne '' })
    if ($wordToComplete -and $tokens.Count -gt 1 -and $tokens[-1] -eq $wordToComplete) {
        $tokens = $tokens[0..($tokens.Count - 2)]
    }

    # `--theme <Tab>` completes its values.
    if ($tokens.Count -gt 0 -and $tokens[-1] -eq '--theme') {
        return $themeValues | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', 'Color theme')
        }
    }

    # Find the subcommand, skipping any global flag (and the value of --theme).
    $subcommand = $null
    $subIndex = -1
    for ($i = 1; $i -lt $tokens.Count; $i++) {
        if ($tokens[$i] -match '^-') {
            if ($tokens[$i] -eq '--theme') { $i++ }
            continue
        }
        $subcommand = $tokens[$i]
        $subIndex = $i
        break
    }

    # Nothing decided yet: offer the subcommands, or the global flags.
    if ($null -eq $subcommand) {
        $candidates = if ($wordToComplete -match '^-') { $topFlags } else { $commands }
        return $candidates | Where-Object { $_.Name -like "$wordToComplete*" } | ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_.Name, $_.Name, 'ParameterValue', $_.Description)
        }
    }

    if ($aliases.ContainsKey($subcommand)) { $subcommand = $aliases[$subcommand] }

    $subFlags = @()
    switch ($subcommand) {
        'update' {
            $subFlags = @(
                @{ Name = '-y'; Description = 'Remove local branches whose upstream was deleted' },
                @{ Name = '--yes'; Description = 'Remove local branches whose upstream was deleted' }
            )
        }
        'push' {
            $subFlags = @(
                @{ Name = '--no-pr'; Description = 'Push without creating a PR or Gerrit review' },
                @{ Name = '-f'; Description = 'Push with --force instead of --force-with-lease' },
                @{ Name = '--force'; Description = 'Push with --force instead of --force-with-lease' }
            )
        }
        'add' {
            $subFlags = @(
                @{ Name = '-p'; Description = 'Interactively select hunks to stage' },
                @{ Name = '--patch'; Description = 'Interactively select hunks to stage' }
            )
        }
        'commit' {
            $subFlags = @(
                @{ Name = '-b'; Description = 'Target feature branch' },
                @{ Name = '--branch'; Description = 'Target feature branch' },
                @{ Name = '-m'; Description = 'Commit message' },
                @{ Name = '--message'; Description = 'Commit message' },
                @{ Name = '-p'; Description = 'Interactively select hunks to stage' },
                @{ Name = '--patch'; Description = 'Interactively select hunks to stage' }
            )
        }
        'fold' {
            $subFlags = @(
                @{ Name = '-c'; Description = 'Create a new branch from the source commit(s)' },
                @{ Name = '--create'; Description = 'Create a new branch from the source commit(s)' },
                @{ Name = '-p'; Description = 'Interactively select hunks to fold' },
                @{ Name = '--patch'; Description = 'Interactively select hunks to fold' }
            )
        }
        'absorb' {
            $subFlags = @(
                @{ Name = '-n'; Description = 'Show what would be absorbed without making changes' },
                @{ Name = '--dry-run'; Description = 'Show what would be absorbed without making changes' }
            )
        }
        'split' {
            $subFlags = @(
                @{ Name = '-m'; Description = 'Message for the first commit' },
                @{ Name = '--message'; Description = 'Message for the first commit' },
                @{ Name = '-p'; Description = 'Interactively pick hunks for the first commit' },
                @{ Name = '--patch'; Description = 'Interactively pick hunks for the first commit' }
            )
        }
        'reword' {
            $subFlags = @(
                @{ Name = '-m'; Description = 'New message or branch name' },
                @{ Name = '--message'; Description = 'New message or branch name' }
            )
        }
        'drop' {
            $subFlags = @(
                @{ Name = '-y'; Description = 'Skip confirmation prompt' },
                @{ Name = '--yes'; Description = 'Skip confirmation prompt' }
            )
        }
        'status' {
            $subFlags = @(
                @{ Name = '-f'; Description = 'Show files changed in each commit' },
                @{ Name = '--files'; Description = 'Show files changed in each commit' },
                @{ Name = '-a'; Description = 'Show all branches including hidden ones' },
                @{ Name = '--all'; Description = 'Show all branches including hidden ones' }
            )
        }
        'diff' {
            # Any other option is forwarded to `git diff`, so only loom's own are listed.
            $subFlags = @(
                @{ Name = '--staged'; Description = 'Show staged changes (index vs HEAD)' },
                @{ Name = '--cached'; Description = 'Alias of --staged' },
                @{ Name = '-a'; Description = 'Show all changes, staged and unstaged' },
                @{ Name = '--all'; Description = 'Show all changes, staged and unstaged' }
            )
        }
        'agent' {
            # The sub-subcommand is the first non-flag token after `agent`.
            $agentSubcommand = $null
            for ($i = $subIndex + 1; $i -lt $tokens.Count; $i++) {
                if ($tokens[$i] -match '^-') { continue }
                $agentSubcommand = $tokens[$i]
                break
            }

            if ($null -eq $agentSubcommand -and -not ($wordToComplete -match '^-')) {
                $agentSubs = @(
                    @{ Name = 'init'; Description = 'Install the loom skill for an AI agent' }
                )
                return $agentSubs | Where-Object { $_.Name -like "$wordToComplete*" } | ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_.Name, $_.Name, 'ParameterValue', $_.Description)
                }
            }

            if ($agentSubcommand -eq 'init') {
                $subFlags = @(
                    @{ Name = '--project'; Description = 'Install into the repository instead of the home directory' }
                )
            }
        }
        'branch' {
            # The sub-subcommand is the first non-flag token after `branch`.
            $branchSubcommand = $null
            for ($i = $subIndex + 1; $i -lt $tokens.Count; $i++) {
                if ($tokens[$i] -match '^-') { continue }
                $branchSubcommand = $tokens[$i]
                break
            }

            if ($null -eq $branchSubcommand -and -not ($wordToComplete -match '^-')) {
                $branchSubs = @(
                    @{ Name = 'new'; Description = 'Create a new feature branch' },
                    @{ Name = 'create'; Description = 'Alias of new' },
                    @{ Name = 'merge'; Description = 'Weave an existing branch into integration' },
                    @{ Name = 'unmerge'; Description = 'Remove a branch from integration' }
                )
                return $branchSubs | Where-Object { $_.Name -like "$wordToComplete*" } | ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_.Name, $_.Name, 'ParameterValue', $_.Description)
                }
            }

            switch ($branchSubcommand) {
                'merge' {
                    $subFlags = @(
                        @{ Name = '-a'; Description = 'Also show remote branches' },
                        @{ Name = '--all'; Description = 'Also show remote branches' }
                    )
                }
                'unmerge' { $subFlags = @() }
                default {
                    # `branch`, `branch new` and `branch create` all take a target.
                    $subFlags = @(
                        @{ Name = '-t'; Description = 'Target commit, branch, or shortID' },
                        @{ Name = '--target'; Description = 'Target commit, branch, or shortID' }
                    )
                }
            }
        }
    }

    # Only offer flags once the user commits to one, so that plain words fall
    # back to PowerShell's file completion (commit, add, fold, ... take paths).
    if (-not ($wordToComplete -match '^-')) { return }

    $allFlags = $subFlags + $globalFlags + $helpFlags
    $allFlags | Where-Object { $_.Name -like "$wordToComplete*" } | ForEach-Object {
        [System.Management.Automation.CompletionResult]::new($_.Name, $_.Name, 'ParameterValue', $_.Description)
    }
}

$_gitLoomNames = @('git-loom') + @(Get-Alias -ErrorAction SilentlyContinue | Where-Object { $_.Definition -eq 'git-loom' } | Select-Object -ExpandProperty Name)
Register-ArgumentCompleter -Native -CommandName $_gitLoomNames -ScriptBlock $_gitLoomCompleter
