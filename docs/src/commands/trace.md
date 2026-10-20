# trace

Show the git commands the last loom command ran — the first thing to look at when a command fails, and to attach to a bug report.

## Usage

```
git loom trace
```

## What It Does

Loom records each git command it runs — arguments, duration, failures, git's error output, and the rebase steps it generated — for every command except `show` and `diff`, including actions run from the [TUI](tui.md). `trace` prints the latest record.

[`continue`](continue.md) and [`abort`](abort.md) add to the record of the command they resume, so the trace shows the whole operation.

The last 10 records are kept in `.git/loom/logs/`.

## Example

```bash
git loom trace
# [2026-03-04 14:30:00.123] git loom fold hug pop
# ================================================================================
#
#   [git] rebase --interactive --autostash ...  [230ms]
#   [git] commit --amend --no-edit  [12ms] FAILED
#     [stderr]
# error: could not apply abc1234
```
