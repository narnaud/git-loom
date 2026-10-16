# Shell Setup

*git-loom* completes commands and their flags in PowerShell, Clink, Bash, Zsh and Fish. Short aliases such as `ci` or `up` are not offered, and options passed to git after `--` are not completed.

## PowerShell

Add to your profile (`echo $PROFILE` shows where it is):

```powershell
Invoke-Expression (&git loom completions powershell | Out-String)
```

`loom` completes too, so `Set-Alias loom git-loom` works wherever you put it. Any other alias for `git-loom` completes only when it is defined before that line.

## Clink

[Clink](https://chrisant996.github.io/clink/) adds completion to `cmd.exe`. Create `%LocalAppData%\clink\git-loom.lua` with:

```lua
load(io.popen('git loom completions clink'):read("*a"))()
```

To complete your own doskey macros, list them in `loom_commands` before the `load` line (default `"loom"`):

```lua
loom_commands = "loom l"
```

## Bash

Add to `~/.bashrc`:

```bash
eval "$(git loom completions bash)"
```

`git loom <Tab>` completes too when git's own bash completion is loaded.

Bash does not complete through aliases, so `loom` is completed by name. To complete your own aliases, list them in `LOOM_COMMANDS` before the `eval` line (default `"loom"`):

```bash
LOOM_COMMANDS="loom gl"
```

## Zsh

Add to `~/.zshrc`, after `compinit`:

```zsh
eval "$(git loom completions zsh)"
```

`git loom <Tab>` completes too, through zsh's git completion. An alias for `git-loom` completes like it; a function needs `compdef _git-loom <name>`.

## Fish

Add to `~/.config/fish/config.fish`:

```fish
git loom completions fish | source
```

`git loom <Tab>` completes too, through fish's git completion. An `alias` for `git-loom` completes like it; a function needs `--wraps git-loom`.
