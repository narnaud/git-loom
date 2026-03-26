# trace

Show the latest command trace — a detailed audit trail of every git operation performed by the last loom command.

## Usage

```
git loom trace
```

No arguments.

## What It Does

Every time you run a loom command that modifies the repository (e.g. `fold`, `commit`, `drop`, `reword`, `split`, `absorb`, `branch`, `push`, `update`), *git-loom* records a trace file to `.git/loom/logs/`. The trace captures:

- **Every git command** executed, with the full argument list
- **Timing** for each command (in milliseconds)
- **Success/failure** status
- **stderr output** on failure
- **Rebase todo annotations** — both the original git todo and the generated todo

Only the 10 most recent trace files are kept; older ones are automatically pruned.

Running `git loom trace` prints the latest trace file to stdout with colored output.

## Output Format

```
[2026-03-04 14:30:00.123] git loom fold aa bb
================================================================================

  [git] rebase --interactive --autostash ...abc1234  [230ms]
    [original git todo]
pick abc1234 First commit
noop
    [generated todo]
label onto
reset onto
pick abc1234 First commit

  [git] reset --hard HEAD  [5ms]

  [git] commit --amend --no-edit  [12ms] FAILED
    [stderr]
error: could not apply abc1234
```

- **Header** — timestamp and the full command line
- **Command entries** — program, arguments, duration, and optional `FAILED` marker
- **Annotations** — rebase todo content (original and generated)
- **stderr** — shown only for failed commands

## Colors

When output is a terminal:

- Header: **bold**
- Command entries: **cyan** (with `FAILED` in **red bold**)
- Annotation labels: **yellow**
- Annotation content: **dimmed**
- stderr: **red**

## Storage

Trace files are stored at `.git/loom/logs/<timestamp>.log` with the naming pattern `YYYY-MM-DD_HH-MM-SS_mmm.log`. The file path is printed at the end of the output.

## Examples

### After a fold operation

```bash
git loom fold aa bb
git loom trace
# Shows the full sequence: rebase, reset, commit --amend, etc.
```

### After a failed rebase

```bash
git loom drop aa
# x Rebase failed with conflicts — aborted
git loom trace
# Shows the rebase command with FAILED status and stderr output
```

### No trace available

```bash
git loom trace
# x No log files found
#   › Run a command first to generate a log
```

## Notes

- Read-only commands (`status`, `trace`) do not generate trace files
- The `internal-write-todo` subprocess does not generate its own trace
- Trace files are plain text and can be inspected directly in `.git/loom/logs/`
