# Shell Setup

*git-loom* provides shell completions for tab-completion of commands and options.

Every subcommand and its aliases are completed, along with each command's own flags. `show` and `diff` forward unknown options to git, and those are not completed — git's option surface is not mirrored here.

## PowerShell

Add the following to your PowerShell profile (`$PROFILE`):

```powershell
Invoke-Expression (&git loom completions powershell | Out-String)
```

To find your profile path, run `echo $PROFILE` in PowerShell.

## Clink

[Clink](https://chrisant996.github.io/clink/) adds completion support to `cmd.exe`. Create a file at `%LocalAppData%\clink\git-loom.lua` with:

```lua
load(io.popen('git loom completions clink'):read("*a"))()
```
