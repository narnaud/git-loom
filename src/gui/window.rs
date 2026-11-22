//! The window: its callbacks change the [`State`], and what the state then
//! shows is copied onto its properties.

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Result;
use slint::ComponentHandle;

use crate::gui::model::{self, update_model};
use crate::gui::state::State;
use crate::gui::ui::{AppWindow, Theme};

/// The monospace family of short IDs.
fn mono_family() -> &'static str {
    if cfg!(windows) {
        "Consolas"
    } else if cfg!(target_os = "macos") {
        "Menlo"
    } else {
        "DejaVu Sans Mono"
    }
}

/// Show the window on `state`, until it is closed.
pub(super) fn run(state: State) -> Result<()> {
    let window = AppWindow::new()?;
    window
        .global::<Theme>()
        .set_mono_family(mono_family().into());
    let gui = Rc::new(Gui {
        window: window.as_weak(),
        state: RefCell::new(state),
    });
    gui.connect(&window);
    gui.sync();
    window.run()?;
    Ok(())
}

struct Gui {
    window: slint::Weak<AppWindow>,
    state: RefCell<State>,
}

impl Gui {
    /// Route the window's callbacks to the state, each followed by a sync.
    fn connect(self: &Rc<Self>, window: &AppWindow) {
        let gui = self.clone();
        window.on_board_resized(move |room| gui.update(|state| state.set_room(room)));
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.state.borrow_mut());
        self.sync();
    }

    /// Copy what the state shows onto the window.
    fn sync(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = self.state.borrow();
        window.set_repository(state.repository().into());
        window.set_heading(state.heading().into());
        let board = state.board();
        update_model(&window.get_pieces(), model::pieces(&state), |pieces| {
            window.set_pieces(pieces)
        });
        update_model(&window.get_lines(), model::lines(board), |lines| {
            window.set_lines(lines)
        });
        window.set_board_width(board.width);
        window.set_board_height(board.height);
    }
}
