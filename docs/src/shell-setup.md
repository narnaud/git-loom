# Shell Setup

*git-loom* provides shell completions for tab-completion of commands and options.

Every subcommand and its aliases are completed, along with each command's own flags. Git options passed after a `--` are not completed — git's option surface is not mirrored here.

## PowerShell

Add the following to your PowerShell profile (`$PROFILE`):

```powershell
Invoke-Expression (&git loom completions powershell | Out-String)
```

To find your profile path, run `echo $PROFILE` in PowerShell.

## The `loom` function

Both scripts also define `loom`: the same as `git-loom`, except that `loom wt cd <worktree>` changes the shell's current directory. A process can never change its parent shell's directory, so `git loom wt cd` prints the path; the function, which runs in the shell itself, moves there:

```powershell
loom wt cd hotfix   # into the hotfix worktree
loom wt cd          # back to the main worktree from a linked one
```

In PowerShell an alias `loom` that points at `git-loom` gives way to the function; in cmd `loom` is whatever you made of it, a `doskey loom=git-loom $*` macro for instance, and Clink rewrites the `wt cd` line into a `cd`. For bash or zsh, a function does the same:

```bash
loom() {
    if [ "$1" = wt ] || [ "$1" = worktree ]; then
        case " $* " in *" -h "*|*" --help "*) ;; *)
            if [ "$2" = cd ]; then local p; p=$(git-loom "$@") && cd "$p"; return; fi
        esac
    fi
    git-loom "$@"
}
```

## Clink

[Clink](https://chrisant996.github.io/clink/) adds completion support to `cmd.exe`. Create a file at `%LocalAppData%\clink\git-loom.lua` with:

```lua
load(io.popen('git loom completions clink'):read("*a"))()
```
