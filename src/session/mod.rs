//! An interactive session over the repository, apart from the front-end that
//! draws it: the snapshot of the repo it shows, the rows of its tree, their
//! diffs and where they can go, what each command takes of them, the hunks
//! picked before some, the actions it runs and the worker running them, what
//! stands in for the tree while an operation is paused, and what says the
//! snapshot is stale.

pub(crate) mod action;
pub(crate) mod commands;
pub(crate) mod diff_text;
pub(crate) mod paused;
pub(crate) mod pick;
pub(crate) mod snapshot;
pub(crate) mod targets;
pub(crate) mod tree;
pub(crate) mod watch;
pub(crate) mod worker;

/// Why a command takes nothing from the rows it was given, worded to follow
/// the command's name.
pub(crate) type Refusal = &'static str;
