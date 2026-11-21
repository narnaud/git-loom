//! The child processes loom starts: every one but an editor's goes through
//! [`command`], which clippy's `disallowed-methods` enforces.

use std::ffi::OsStr;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by a program with no console of its own, the window: Windows opens
/// one, on screen, for every console child such a program starts.
static NO_CHILD_CONSOLE: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "gui")]
pub fn hide_child_consoles() {
    NO_CHILD_CONSOLE.store(true, Ordering::Relaxed);
}

/// A command running `program`, as loom starts every child but an editor's
/// (see `git::run_interactive`): with no console window under
/// [`hide_child_consoles`].
#[expect(clippy::disallowed_methods, reason = "the one constructor of children")]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    if NO_CHILD_CONSOLE.load(Ordering::Relaxed) {
        without_console_window(&mut command);
    }
    command
}

#[cfg(windows)]
fn without_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn without_console_window(_command: &mut Command) {}
