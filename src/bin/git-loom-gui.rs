//! The `git-loom-gui` executable: the window of the library.

// A release build is a GUI program, with no console of its own; a debug build
// keeps one, for a panic to land somewhere.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    git_loom::run_gui()
}
