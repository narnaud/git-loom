//! Auto-reload: polls a cheap fingerprint of the repo and says when the view is stale.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const INTERVAL: Duration = Duration::from_secs(2);

/// Polls a fingerprint at a fixed interval while on, so a reload is due at most once per interval.
pub(crate) struct Watch<T> {
    loaded: T,
    on: bool,
    next_poll: Instant,
}

impl<T: PartialEq> Watch<T> {
    pub(crate) fn new(loaded: T, on: bool, now: Instant) -> Self {
        Watch {
            loaded,
            on,
            next_poll: now + INTERVAL,
        }
    }

    pub(crate) fn is_on(&self) -> bool {
        self.on
    }

    /// Flip on/off and return the new state; the fingerprint kept meanwhile is still compared.
    pub(crate) fn toggle(&mut self, now: Instant) -> bool {
        self.on = !self.on;
        self.next_poll = now + INTERVAL;
        self.on
    }

    pub(crate) fn due(&mut self, now: Instant, read: impl FnOnce() -> T) -> bool {
        if !self.on || now < self.next_poll {
            return false;
        }
        self.next_poll = now + INTERVAL;
        read() != self.loaded
    }

    /// Record the fingerprint taken just before a load.
    pub(crate) fn loaded(&mut self, stamp: T, now: Instant) {
        self.loaded = stamp;
        self.next_poll = now + INTERVAL;
    }
}

/// How one file is fingerprinted; `None` in [`FsStamp`] when it is missing.
#[derive(Debug, PartialEq)]
pub(crate) enum Stamp {
    /// A ref keeps its length when it moves, so only its value tells.
    Content(Vec<u8>),
    /// Large files: reading them every poll would cost more than a rare miss.
    Meta(SystemTime, u64),
}

pub(crate) type FsStamp = Vec<(PathBuf, Option<Stamp>)>;

/// Content of `HEAD` and every file under `refs/`; modification time and size
/// of the index and `packed-refs`. Linked worktrees keep refs in the common dir.
pub(crate) fn fs_stamp(git_dir: &Path) -> FsStamp {
    let common = std::fs::read_to_string(git_dir.join("commondir"))
        .map(|rel| git_dir.join(rel.trim()))
        .unwrap_or_else(|_| git_dir.to_path_buf());
    let mut out = vec![
        content(git_dir.join("HEAD")),
        meta(git_dir.join("index")),
        meta(common.join("packed-refs")),
    ];
    walk(&common.join("refs"), &mut out);
    out
}

fn content(path: PathBuf) -> (PathBuf, Option<Stamp>) {
    let stamp = std::fs::read(&path).ok().map(Stamp::Content);
    (path, stamp)
}

fn meta(path: PathBuf) -> (PathBuf, Option<Stamp>) {
    let stamp = std::fs::metadata(&path)
        .ok()
        .and_then(|m| Some(Stamp::Meta(m.modified().ok()?, m.len())));
    (path, stamp)
}

fn walk(dir: &Path, out: &mut FsStamp) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = read.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(content(path));
        }
    }
}

#[cfg(test)]
#[path = "watch_test.rs"]
mod tests;
