# git-loom completions for PowerShell
# Add to your $PROFILE: Invoke-Expression (&git-loom completions powershell | Out-String)

$_gitLoomCompleter = {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commands = @(
        @{ Name = 'status'; Description = 'Show the branch-aware status' },
        @{ Name = 'init'; Description = 'Initialize a new integration branch tracking a remote' },
        @{ Name = 'branch'; Description = 'Manage feature branches (create, merge, unmerge)' },
        @{ Name = 'reword'; Description = 'Reword a commit message or rename a branch' },
        @{ Name = 'commit'; Description = 'Create a commit on a feature branch' },
        @{ Name = 'drop'; Description = 'Drop a commit or a branch from history' },
        @{ Name = 'fold'; Description = 'Fold source(s) into a target' },
        @{ Name = 'absorb'; Description = 'Absorb working tree changes into originating commits' },
        @{ Name = 'update'; Description = 'Pull-rebase the integration branch' },
        @{ Name = 'push'; Description = 'Push the integration branch to the remote' },
        @{ Name = 'show'; Description = 'Show the diff and metadata for a commit' },
        @{ Name = 'trace'; Description = 'Trace loom operations for debugging' },
        @{ Name = 'split'; Description = 'Split a commit into two sequential commits' },
        @{ Name = 'continue'; Description = 'Continue a paused loom operation after resolving conflicts' },
        @{ Name = 'abort'; Description = 'Abort a paused loom operation and restore original state' }
    )

    $globalFlags = @(
        @{ Name = '--no-color'; Description = 'Disable colored output' },
        @{ Name = '--help'; Description = 'Show help information' },
        @{ Name = '-h'; Description = 'Show help information' }
    )

    $tokens = $commandAst.ToString() -split '\s+'
    $subcommand = $null
    if ($tokens.Count -gt 1) {
        $subcommand = $tokens[1]
    }

    # Complete subcommands (skip if already on 'branch', which has its own sub-subcommands)
    if ($tokens.Count -le 2 -and $subcommand -ne 'branch' -and -not ($wordToComplete -match '^-')) {
        $commands | Where-Object { $_.Name -like "$wordToComplete*" } | ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_.Name, $_.Name, 'ParameterValue', $_.Description)
        }
        return
    }

    # Complete flags based on subcommand
    $subFlags = @()
    switch ($subcommand) {
        'status' {
            $subFlags = @(
                @{ Name = '-f'; Description = 'Show files changed in each commit' },
                @{ Name = '--files'; Description = 'Show files changed in each commit' },
                @{ Name = '-a'; Description = 'Show all branches including hidden ones' },
                @{ Name = '--all'; Description = 'Show all branches including hidden ones' }
            )
        }
        'branch' {
            $branchSubcommand = if ($tokens.Count -gt 2) { $tokens[2] } else { $null }

            # Complete the branch sub-subcommand itself
            if ($tokens.Count -le 3 -and -not ($wordToComplete -match '^-') -and $branchSubcommand -notin @('new', 'create', 'merge', 'unmerge')) {
                $branchSubs = @(
                    @{ Name = 'new'; Description = 'Create a new feature branch' },
                    @{ Name = 'create'; Description = 'Create a new feature branch (alias)' },
                    @{ Name = 'merge'; Description = 'Weave an existing branch into integration' },
                    @{ Name = 'unmerge'; Description = 'Remove a branch from integration' }
                )
                $branchSubs | Where-Object { $_.Name -like "$wordToComplete*" } | ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_.Name, $_.Name, 'ParameterValue', $_.Description)
                }
                return
            }

            switch ($branchSubcommand) {
                { $_ -in 'new', 'create' } {
                    $subFlags = @(
                        @{ Name = '-t'; Description = 'Target commit, branch, or shortID' },
                        @{ Name = '--target'; Description = 'Target commit, branch, or shortID' }
                    )
                }
                'merge' {
                    $subFlags = @(
                        @{ Name = '-a'; Description = 'Also show remote branches' },
                        @{ Name = '--all'; Description = 'Also show remote branches' }
                    )
                }
                'unmerge' {
                    $subFlags = @()
                }
            }
        }
        'reword' {
            $subFlags = @(
                @{ Name = '-m'; Description = 'New message or branch name' },
                @{ Name = '--message'; Description = 'New message or branch name' }
            )
        }
        'commit' {
            $subFlags = @(
                @{ Name = '-b'; Description = 'Target feature branch' },
                @{ Name = '--branch'; Description = 'Target feature branch' },
                @{ Name = '-m'; Description = 'Commit message' },
                @{ Name = '--message'; Description = 'Commit message' }
            )
        }
        'drop' {
            $subFlags = @(
                @{ Name = '-y'; Description = 'Skip confirmation prompt' },
                @{ Name = '--yes'; Description = 'Skip confirmation prompt' }
            )
        }
        'split' {
            $subFlags = @(
                @{ Name = '-m'; Description = 'Message for the first commit' },
                @{ Name = '--message'; Description = 'Message for the first commit' }
            )
        }
        'absorb' {
            $subFlags = @(
                @{ Name = '-n'; Description = 'Show what would be absorbed without making changes' },
                @{ Name = '--dry-run'; Description = 'Show what would be absorbed without making changes' }
            )
        }
    }

    $allFlags = $globalFlags + $subFlags
    $allFlags | Where-Object { $_.Name -like "$wordToComplete*" } | ForEach-Object {
        [System.Management.Automation.CompletionResult]::new($_.Name, $_.Name, 'ParameterValue', $_.Description)
    }
}

$_gitLoomNames = @('git-loom') + @(Get-Alias -ErrorAction SilentlyContinue | Where-Object { $_.Definition -eq 'git-loom' } | Select-Object -ExpandProperty Name)
Register-ArgumentCompleter -Native -CommandName $_gitLoomNames -ScriptBlock $_gitLoomCompleter
