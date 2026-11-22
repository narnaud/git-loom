//! The window: its callbacks change the [`State`], and what the state then
//! shows is copied onto its properties.

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Result;
use slint::platform::Key;
use slint::{ComponentHandle, SharedString};

use crate::gui::model::{self, model_of, update_model};
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
        shown_diff: RefCell::new(None),
    });
    gui.connect(&window);
    gui.sync();
    window.run()?;
    Ok(())
}

struct Gui {
    window: slint::Weak<AppWindow>,
    state: RefCell<State>,
    /// Key of the row whose diff the window holds: its lines are built when
    /// the inspector turns to another row, not at each sync.
    shown_diff: RefCell<Option<String>>,
}

impl Gui {
    /// Route the window's callbacks to the state, each followed by a sync.
    fn connect(self: &Rc<Self>, window: &AppWindow) {
        let gui = self.clone();
        window.on_board_resized(move |room| gui.update(|state| state.set_room(room)));
        let gui = self.clone();
        window.on_board_toggled(move |row| {
            if let Ok(row) = usize::try_from(row) {
                gui.update(|state| state.row_toggled(row));
            }
        });
        let gui = self.clone();
        window.on_row_pressed(move |row, control, shift| {
            if let Ok(row) = usize::try_from(row) {
                gui.update(|state| state.row_pressed(row, control, shift));
            }
        });
        let gui = self.clone();
        window.on_key_pressed(move |text| gui.key(&text));
    }

    /// A key pressed over the panes; false when it is not one of theirs.
    fn key(&self, text: &str) -> bool {
        let handled = key_pressed(&mut self.state.borrow_mut(), text);
        if handled && let Some(window) = self.window.upgrade() {
            self.sync();
            // Where a key put the selection may be out of view.
            let state = self.state.borrow();
            if let Some(index) = state.cursor_change() {
                window.invoke_reveal_in_changes(index as i32);
            }
            if let Some(rect) = state.cursor_rect() {
                window.invoke_reveal_on_board(rect.y, rect.h);
            }
        }
        handled
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
        let mut state = self.state.borrow_mut();
        window.set_repository(state.repository().into());
        window.set_heading(state.heading().into());
        window.set_local_changes(model::local_changes(&state));
        update_model(&window.get_changes(), model::changes(&state), |changes| {
            window.set_changes(changes)
        });
        let board = state.board();
        update_model(&window.get_pieces(), model::pieces(&state), |pieces| {
            window.set_pieces(pieces)
        });
        update_model(&window.get_lines(), model::lines(board), |lines| {
            window.set_lines(lines)
        });
        window.set_board_width(board.width);
        window.set_board_height(board.height);

        let tab_width = state.tab_width();
        let inspected = state.inspected();
        let title = inspected.as_ref().map_or("", |shown| shown.title.as_str());
        window.set_inspector_title(title.into());
        let key = inspected.as_ref().map(|shown| shown.key);
        if self.shown_diff.borrow().as_deref() != key {
            let diff = inspected.as_ref().map_or("", |shown| shown.diff);
            let rows = model::diff_rows(diff, tab_width);
            window.set_diff_columns(model::columns(&rows));
            window.set_diff(model_of(rows));
            // A list keeps where it was scrolled to across its models.
            window.invoke_diff_to_top();
            self.shown_diff.replace(key.map(str::to_string));
        }
    }
}

/// Do what the key `text` asks of the selection; false when it is not one of
/// its keys.
fn key_pressed(state: &mut State, text: &str) -> bool {
    let is = |key: Key| text == SharedString::from(key).as_str();
    if is(Key::DownArrow) {
        state.move_cursor(true);
    } else if is(Key::UpArrow) {
        state.move_cursor(false);
    } else if is(Key::RightArrow) {
        state.expand_cursor();
    } else if is(Key::LeftArrow) {
        state.collapse_cursor();
    } else if is(Key::Escape) {
        state.clear_selection();
    } else {
        return false;
    }
    true
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
