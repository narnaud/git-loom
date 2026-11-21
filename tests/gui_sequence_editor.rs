//! A rebase the window starts names `git-loom-gui` as git's sequence editor.
//! The unit tests cannot see that call: under `cargo test` the editor is
//! always `git-loom`. Answered with a window instead, git would wait for it
//! to close, take the exit for an edited todo, and replay its own.
#![cfg(feature = "gui")]
#![allow(clippy::disallowed_methods, reason = "it starts the executable itself")]

use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn the_window_executable_answers_as_gits_sequence_editor() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let todo = dir.path().join("git-rebase-todo");
    std::fs::write(&source, "pick 1234567 one\n").unwrap();
    std::fs::write(&todo, "noop\n").unwrap();

    let mut editor = Command::new(env!("CARGO_BIN_EXE_git-loom-gui"))
        .arg("internal-write-todo")
        .arg("--source")
        .arg(&source)
        .arg(&todo)
        .spawn()
        .unwrap();
    // A window would never exit by itself.
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = editor.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            editor.kill().unwrap();
            panic!("no answer: a window opened instead");
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    assert!(status.success(), "{status}");
    assert_eq!(
        std::fs::read_to_string(&todo).unwrap(),
        "pick 1234567 one\n"
    );
}
