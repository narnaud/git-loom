# Shell Setup

*git-loom* provides shell completions for tab-completion of commands and options.

Every subcommand and its aliases are completed, along with each command's own flags. Git options passed after a `--` are not completed — git's option surface is not mirrored here.

## PowerShell

Add the following to your PowerShell profile (`$PROFILE`):

```powershell
Invoke-Expression (&git loom completions powershell | Out-String)
```

To find your profile path, run `echo $PROFILE` in PowerShell.

`loom` completes too, so `Set-Alias loom git-loom` works wherever you put it. Any other alias for `git-loom` completes only when it is defined before that line.

## Clink

[Clink](https://chrisant996.github.io/clink/) adds completion support to `cmd.exe`. Create a file at `%LocalAppData%\clink\git-loom.lua` with:

```lua
load(io.popen('git loom completions clink'):read("*a"))()
```

To give your own doskey macros completion, list them in `loom_commands` before the `load` line; it defaults to `"loom"`:

```lua
loom_commands = "loom l"
```

## Bash

Add to `~/.bashrc`:

```bash
eval "$(git loom completions bash)"
```

`git loom <Tab>` completes as well when git's own bash completion is loaded.

Bash never completes through an alias, so `loom` gets completion by name. To give your own aliases completion, list them in `LOOM_COMMANDS` before the `eval` line; it defaults to `"loom"`:

```bash
LOOM_COMMANDS="loom gl"
```

## Zsh

Add to `~/.zshrc`, after `compinit`:

```zsh
eval "$(git loom completions zsh)"
```

`git loom <Tab>` completes as well, through zsh's git completion. An alias for `git-loom` completes like it; a function needs `compdef _git-loom <name>`.

## Fish

Add to `~/.config/fish/config.fish`:

```fish
git loom completions fish | source
```

`git loom <Tab>` completes as well: fish's git completion hands custom commands to `git-loom`'s. An `alias` for `git-loom` completes like it; a function needs `--wraps git-loom`.
