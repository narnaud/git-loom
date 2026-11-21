//! git-loom as a library, which `src/bin/git-loom.rs` runs. Nothing here is an
//! API for other crates: `run` is all it exports.

mod absorb;
mod add;
mod agent;
mod branch;
mod cli;
mod commit;
mod completions;
mod core;
mod diff;
mod drop;
mod fold;
mod git;
mod init;
mod merge;
mod push;
mod reword;
mod session;
mod show;
mod split;
mod status;
mod swap;
mod switch;
mod trace;
mod tui;
mod unmerge;
mod update;
mod worktree;

pub use cli::run;
