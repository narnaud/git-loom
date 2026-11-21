//! git-loom as a library, which the executables of `src/bin/` run. Nothing here
//! is an API for other crates: their entry points are all it exports.

#![cfg_attr(
    test,
    allow(clippy::disallowed_methods, reason = "tests start what they need")
)]

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
#[cfg(feature = "gui")]
mod gui;
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
#[cfg(feature = "gui")]
pub use gui::run as run_gui;
