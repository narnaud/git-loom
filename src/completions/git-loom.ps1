# git-loom completions for PowerShell
# Add to your $PROFILE: Invoke-Expression (&git-loom completions powershell | Out-String)

Register-ArgumentCompleter -Native -CommandName 'git-loom' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commands = @(
        @{ Name = 'status';  Description = 'Show the branch-aware status' },
        @{ Name = 'init';    Description = 'Initialize a new integration branch tracking a remote' },
        @{ Name = 'branch';  Description = 'Create a new feature branch' },
        @{ Name = 'reword';  Description = 'Reword a commit message or rename a branch' },
        @{ Name = 'commit';  Description = 'Create a commit on a feature branch' },
        @{ Name = 'drop';    Description = 'Drop a commit or a branch from history' },
        @{ Name = 'fold';    Description = 'Fold source(s) into a target' },
        @{ Name = 'update';  Description = 'Pull-rebase the integration branch' },
        @{ Name = 'completions'; Description = 'Generate shell completions' }
    )

    $globalFlags = @(
        @{ Name = '--no-color'; Description = 'Disable colored output' },
        @{ Name = '--help';     Description = 'Show help information' },
        @{ Name = '-h';         Description = 'Show help information' }
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
        'branch' {
            $subFlags = @(
                @{ Name = '-t';       Description = 'Target commit, branch, or shortID' },
                @{ Name = '--target'; Description = 'Target commit, branch, or shortID' }
            )
        }
        'reword' {
            $subFlags = @(
                @{ Name = '-m';        Description = 'New message or branch name' },
                @{ Name = '--message'; Description = 'New message or branch name' }
            )
        }
        'commit' {
            $subFlags = @(
                @{ Name = '-b';       Description = 'Target feature branch' },
                @{ Name = '--branch'; Description = 'Target feature branch' },
                @{ Name = '-m';        Description = 'Commit message' },
                @{ Name = '--message'; Description = 'Commit message' }
            )
        }
        'completions' {
            @('powershell', 'cmd') | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', "$_ completions")
            }
            return
        }
    }

    $allFlags = $globalFlags + $subFlags
    $allFlags | Where-Object { $_.Name -like "$wordToComplete*" } | ForEach-Object {
        [System.Management.Automation.CompletionResult]::new($_.Name, $_.Name, 'ParameterValue', $_.Description)
    }
}
