# Installation

## Cargo (all platforms)

```bash
cargo install git-loom
```

## Scoop (Windows)

```
scoop bucket add narnaud https://github.com/narnaud/scoop-bucket
scoop install git-loom
```

## Pre-built binaries

Download the archive for your platform from the [Releases](https://github.com/narnaud/git-loom/releases) page and put the binary on your `PATH`:

| Platform | Archive |
|----------|---------|
| Linux x86_64 | `git-loom-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 | `git-loom-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `git-loom-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `git-loom-x86_64-pc-windows-msvc.zip` |

## From Source

Requires Rust 1.90 or later.

```bash
git clone https://github.com/narnaud/git-loom.git
cd git-loom
cargo install --path .
```

## Requirements

- **Git 2.40 or later.**
- **Optional:** the [`gh`](https://cli.github.com) or [`az`](https://learn.microsoft.com/en-us/cli/azure/) CLI, for `git loom push` to create pull requests on GitHub or Azure DevOps.

Once installed, the binary answers to `git loom` as well as `git-loom`.
