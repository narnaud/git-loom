//! `git-loom gui`: start the window, the `git-loom-gui` installed beside
//! `git-loom` (Spec 024).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Result, bail};

use crate::core::{agent_mode, process};

/// Start the window, `git-loom-gui`, on the current directory, without
/// waiting for it to close.
pub fn run() -> Result<()> {
    if agent_mode::enabled() {
        bail!(
            "the GUI is interactive and unavailable in agent mode\n\
             Use `loom status` instead"
        );
    }
    keep_streams_from_children();
    start(&std::env::current_exe()?)
}

/// Make this process's own streams uninheritable: a child on Windows inherits
/// every inheritable handle, not only its stdio, so a pipe `git-loom gui`
/// writes to would otherwise stay open until the window closes.
#[cfg(windows)]
fn keep_streams_from_children() {
    use std::os::windows::io::{AsRawHandle, RawHandle};
    const HANDLE_FLAG_INHERIT: u32 = 1;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetHandleInformation(handle: RawHandle, mask: u32, flags: u32) -> i32;
    }
    for handle in [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ] {
        // SAFETY: only flags of this process's own handles change; a missing
        // stream fails the call, which leaves nothing to keep.
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
    }
}

/// On Unix the child's own stdio replaces the streams it would inherit.
#[cfg(not(windows))]
fn keep_streams_from_children() {}

/// Start the window's executable installed beside `cli`, this one.
fn start(cli: &Path) -> Result<()> {
    let Some(gui) = gui_beside(cli) else {
        // Named, because the advice installs both in cargo's own directory,
        // which is not where a `git-loom` from elsewhere looks.
        let searched = cli.parent().unwrap_or(cli).display();
        bail!(
            "`git-loom-gui` is not installed beside `git-loom`, in `{searched}`\n\
             `cargo install git-loom --features gui` installs both, in cargo's directory"
        );
    };
    // Off this command's streams: the window writes nothing to a terminal
    // that has its prompt back, and holds no pipe open.
    process::command(&gui)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        // Whole: the caller prints only the outermost context, with no trace.
        .map_err(|e| anyhow::anyhow!("Failed to start `{}`: {e}", gui.display()))?;
    Ok(())
}

/// The window's executable, when it is installed in the directory of `cli`,
/// or of the file `cli` links to: `current_exe` resolves a link on some
/// platforms only.
fn gui_beside(cli: &Path) -> Option<PathBuf> {
    let beside = |cli: &Path| {
        let gui = cli.with_file_name(format!("git-loom-gui{}", std::env::consts::EXE_SUFFIX));
        gui.is_file().then_some(gui)
    };
    beside(cli).or_else(|| beside(&cli.canonicalize().ok()?))
}

#[cfg(test)]
#[path = "gui_launch_test.rs"]
mod tests;
