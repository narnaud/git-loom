//! The window of git-loom, which `git-loom-gui` runs (Spec 024).

mod board;
mod commands;
mod drag;
mod forms;
mod hunks;
mod model;
mod state;
mod window;

use std::io::Write;

use anyhow::Result;
use slint::ComponentHandle;

use crate::core::weave::SEQUENCE_EDITOR;
use crate::gui::state::State;

/// The windows and their types, generated from `ui/app.slint`.
#[allow(clippy::all)]
mod ui {
    slint::include_modules!();
}

/// Run `git-loom-gui` on the arguments of the process, and exit with its
/// status.
pub fn run() -> ! {
    crate::core::process::hide_child_consoles();
    // A rebase names the running executable as git's sequence editor
    // (`git::loom_exe_path`): that call is the CLI's to answer, here too.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == SEQUENCE_EDITOR)
    {
        crate::cli::run()
    }
    let code = match open() {
        Ok(()) => 0,
        Err(error) => {
            report(&error);
            1
        }
    };
    std::process::exit(code)
}

/// Show the window on the repository of the process directory, until it is
/// closed.
fn open() -> Result<()> {
    crate::git::check_git_version()?;
    window::run(State::load()?)
}

/// Say why no window opened: in a dialog, since a GUI program has no terminal
/// to write to, and on stderr for whoever started it from one.
fn report(error: &anyhow::Error) {
    // Not `eprintln!`, which panics on a stderr whose reader is gone.
    let _ = writeln!(std::io::stderr(), "git-loom-gui: {error}");
    let Ok(dialog) = ui::ErrorWindow::new() else {
        return;
    };
    dialog.set_message(error.to_string().into());
    let shown = dialog.as_weak();
    dialog.on_dismissed(move || {
        if let Some(dialog) = shown.upgrade() {
            let _ = dialog.hide();
        }
    });
    let _ = dialog.run();
}
