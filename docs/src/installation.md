# Installation

## Cargo (all platforms)

If you have Rust installed, the easiest way to install git-loom is via [crates.io](https://crates.io/crates/git-loom):

```bash
cargo install git-loom
```

## Scoop (Windows)

Install git-loom with [Scoop](https://scoop.sh/):

```
scoop bucket add narnaud https://github.com/narnaud/scoop-bucket
scoop install git-loom
```

## Pre-built binaries

Download the latest archive for your platform from the [Releases](https://github.com/narnaud/git-loom/releases) page:

| Platform | Archive |
|----------|---------|
| Linux x86_64 | `git-loom-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 | `git-loom-aarch64-unknown-linux-gnu.tar.gz` |
| macOS x86_64 | `git-loom-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `git-loom-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `git-loom-x86_64-pc-windows-msvc.zip` |

Extract the binary and place it somewhere on your `PATH`.

## From Source

Requires Rust 1.90 or later.

```bash
git clone https://github.com/narnaud/git-loom.git
cd git-loom
cargo install --path .
```

## Requirements

- **Git 2.38 or later** — git-loom checks the Git version at startup and will report an error if the version is too old.
- **`gh` CLI** (optional) — needed for automatic GitHub PR creation with `git loom push`. Install from [cli.github.com](https://cli.github.com).
