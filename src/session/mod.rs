//! An interactive session over the repository, apart from the front-end that
//! draws it: the snapshot of the repo it shows, the rows of its tree, what
//! stands in for them while an operation is paused, and what says they are
//! stale.

pub(crate) mod paused;
pub(crate) mod snapshot;
pub(crate) mod tree;
pub(crate) mod watch;
