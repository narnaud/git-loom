# git-loom completions for PowerShell
# Add to your $PROFILE: Invoke-Expression (&git-loom completions powershell | Out-String)

Register-ArgumentCompleter -Native -CommandName 'git-loom' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commands = @(
        @{ Name = 'status'; Description = 'Show the branch-aware status' },
        @{ Name = 'init'; Description = 'Initialize a new integration branch tracking a remote' },
        @{ Name = 'branch'; Description = 'Create a new feature branch' },
        @{ Name = 'reword'; Description = 'Reword a commit message or rename a branch' },
        @{ Name = 'commit'; Description = 'Create a commit on a feature branch' },
        @{ Name = 'drop'; Description = 'Drop a commit or a branch from history' },
        @{ Name = 'fold'; Description = 'Fold source(s) into a target' },
        @{ Name = 'absorb'; Description = 'Absorb working tree changes into originating commits' },
        @{ Name = 'update'; Description = 'Pull-rebase the integration branch' },
        @{ Name = 'push'; Description = 'Push the integration branch to the remote' },
        @{ Name = 'split'; Description = 'Split a commit into two sequential commits' }
    )

    $globalFlags = @(
        @{ Name = '--no-color'; Description = 'Disable colored output' },
        @{ Name = '-f'; Description = 'Show files changed in each commit' },
        @{ Name = '--files'; Description = 'Show files changed in each commit' },
        @{ Name = '--help'; Description = 'Show help information' },
        @{ Name = '-h'; Description = 'Show help information' }
    )

    $tokens = $commandAst.ToString() -split '\s+'
    $subcommand = $null
    if ($tokens.Count -gt 1) {
        $subcommand = $tokens[1]
    }

    # Complete subcommands
    if ($tokens.Count -le 2 -and -not ($wordToComplete -match '^-')) {
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
                @{ Name = '--files'; Description = 'Show files changed in each commit' }
            )
        }
        'branch' {
            $subFlags = @(
                @{ Name = '-t'; Description = 'Target commit, branch, or shortID' },
                @{ Name = '--target'; Description = 'Target commit, branch, or shortID' }
            )
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
