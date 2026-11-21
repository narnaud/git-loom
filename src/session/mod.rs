//! An interactive session over the repository, apart from the front-end that
//! draws it: the snapshot of the repo it shows, the rows of its tree, the
//! actions it runs, what stands in for the tree while an operation is paused,
//! and what says the snapshot is stale.

pub(crate) mod action;
pub(crate) mod paused;
pub(crate) mod snapshot;
pub(crate) mod tree;
pub(crate) mod watch;
