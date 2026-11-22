//! The window: its callbacks change the [`State`], and what the state then
//! shows is copied onto its properties.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use anyhow::Result;
use slint::platform::Key;
use slint::{CloseRequestResponse, ComponentHandle, SharedString, Timer, TimerMode};

use crate::gui::commands::Cmd;
use crate::gui::hunks::HunkPick;
use crate::gui::model::{self, model_of, update_model};
use crate::gui::state::State;
use crate::gui::ui::{AppWindow, Theme};

/// How often what the running command sent is taken in.
const TICK: Duration = Duration::from_millis(100);

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
        shown_form: Cell::new(0),
        shown_hunks: Cell::new((0, 0)),
        dialog_shown: Cell::new(false),
        log_shown: Cell::new(false),
        picker_shown: Cell::new(false),
    });
    gui.connect(&window);
    gui.sync();
    let timer = Timer::default();
    let ticking = gui.clone();
    timer.start(TimerMode::Repeated, TICK, move || {
        let changed = ticking.state.borrow_mut().tick();
        if changed {
            ticking.sync();
        }
    });
    window.run()?;
    Ok(())
}

struct Gui {
    window: slint::Weak<AppWindow>,
    state: RefCell<State>,
    /// Key of the row whose diff the window holds, and how many times the
    /// repository had been read then: its lines are built when the
    /// inspector turns to another row or the repository is read again, not
    /// at each sync.
    shown_diff: RefCell<Option<(u64, String)>>,
    /// How many forms the window was given: each is filled in once.
    shown_form: Cell<u64>,
    /// Which picker, and which of its files, the window shows the hunks of:
    /// another starts at its top.
    shown_hunks: Cell<(u64, usize)>,
    /// What was over the panes at the last sync, which the keyboard goes
    /// back from as it closes.
    dialog_shown: Cell<bool>,
    log_shown: Cell<bool>,
    picker_shown: Cell<bool>,
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
        let shown = window.as_weak();
        window.on_menu_step(move |from, down| {
            let menu = shown.upgrade().map(|window| window.get_menu());
            let entries: Vec<_> = menu.iter().flat_map(slint::Model::iter).collect();
            model::menu_step(&entries, from, down)
        });
        let gui = self.clone();
        window.on_row_menu(move |row, x, y| {
            if let Ok(row) = usize::try_from(row) {
                gui.menu(row, x, y);
            }
        });
        let gui = self.clone();
        window.on_key_pressed(move |text| gui.key(&text));
        let gui = self.clone();
        window.on_command(move |id| {
            if let Some(cmd) = Cmd::from_id(id) {
                gui.update(|state| state.command(cmd));
            }
        });
        let gui = self.clone();
        window.on_dialog_accepted(move |text, choice, dest| {
            let choice = usize::try_from(choice).ok();
            // None picked is refused, as one out of the list.
            let dest = usize::try_from(dest).unwrap_or(usize::MAX);
            gui.update(|state| state.form_accepted(&text, choice, dest));
        });
        let gui = self.clone();
        window.on_dialog_cancelled(move || gui.update(State::form_cancelled));
        let gui = self.clone();
        window.on_dialog_toggled(move |index| {
            if let Ok(index) = usize::try_from(index) {
                gui.update(|state| state.form_toggled(index));
            }
        });
        let gui = self.clone();
        window.on_log_opened(move || gui.update(State::open_log));
        let gui = self.clone();
        window.on_log_closed(move || gui.update(State::close_log));
        let gui = self.clone();
        window.on_hunk_file_picked(move |index| {
            if let Ok(index) = usize::try_from(index) {
                gui.update(|state| state.hunk_file_picked(index));
            }
        });
        let gui = self.clone();
        window.on_hunk_file_toggled(move |index| {
            if let Ok(index) = usize::try_from(index) {
                gui.update(|state| state.hunk_file_toggled(index));
            }
        });
        let gui = self.clone();
        window.on_hunk_toggled(move |index| {
            if let Ok(index) = usize::try_from(index) {
                gui.update(|state| state.hunk_toggled(index));
            }
        });
        let gui = self.clone();
        window.on_hunks_kept(move |keep| gui.update(|state| state.hunks_kept(keep)));
        let gui = self.clone();
        window.on_hunks_confirmed(move || gui.update(State::hunks_confirmed));
        let gui = self.clone();
        window.on_hunks_cancelled(move || gui.update(State::hunks_cancelled));
        let gui = self.clone();
        window.window().on_close_requested(move || {
            let closes = gui.state.borrow_mut().may_close();
            gui.sync();
            match closes {
                true => CloseRequestResponse::HideWindow,
                false => CloseRequestResponse::KeepWindowShown,
            }
        });
    }

    /// The right button on what stands for the row at `index`, at `x`, `y`
    /// of the window: its menu opens there.
    fn menu(&self, index: usize, x: f32, y: f32) {
        let opens = self.state.borrow_mut().row_menu(index);
        self.sync();
        if let (true, Some(window)) = (opens, self.window.upgrade()) {
            let menu = model::menu(&self.state.borrow().menu());
            window.set_menu(model_of(menu));
            window.invoke_show_menu(x, y);
        }
    }

    /// A key pressed over the panes; false when it is not one of theirs, or
    /// something is over them.
    fn key(&self, text: &str) -> bool {
        let over = {
            let state = self.state.borrow();
            state.form().is_some() || state.log_open() || state.hunk_pick().is_some()
        };
        let handled = !over && key_pressed(&mut self.state.borrow_mut(), text);
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
        update_model(&window.get_bar(), model::buttons(&state.bar()), |bar| {
            window.set_bar(bar)
        });
        window.set_local_changes(model::local_changes(&state));
        update_model(&window.get_changes(), model::changes(&state), |changes| {
            window.set_changes(changes)
        });
        let buttons = model::buttons(&state.commit_buttons());
        update_model(&window.get_commit_buttons(), buttons, |buttons| {
            window.set_commit_buttons(buttons)
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

        window.set_busy(state.busy());
        let (level, status) = state.status();
        window.set_status(model::plain(&status).into());
        window.set_status_tone(model::tone(level));
        update_model(
            &window.get_actions(),
            model::buttons(&state.actions()),
            |actions| window.set_actions(actions),
        );
        self.sync_over(&window, &state);

        let tab_width = state.tab_width();
        let reads = state.reads();
        let inspected = state.inspected();
        let title = inspected.as_ref().map_or("", |shown| shown.title.as_str());
        window.set_inspector_title(title.into());
        let key = inspected
            .as_ref()
            .map(|shown| (reads, shown.key.to_string()));
        if *self.shown_diff.borrow() != key {
            let diff = inspected.as_ref().map_or("", |shown| shown.diff);
            let rows = model::diff_rows(diff, tab_width);
            window.set_diff_columns(model::columns(&rows));
            window.set_diff(model_of(rows));
            // A list keeps where it was scrolled to across its models.
            window.invoke_diff_to_top();
            self.shown_diff.replace(key);
        }
    }

    /// What is over the panes: the log, the hunk picker, then the dialog
    /// over them. Whichever opens takes the keyboard, and gives it back as it
    /// closes.
    fn sync_over(&self, window: &AppWindow, state: &State) {
        let log_open = state.log_open();
        window.set_log_open(log_open);
        if log_open {
            let rows = model::log_rows(state.log());
            window.set_log_columns(model::columns(&rows));
            update_model(&window.get_log(), rows, |log| window.set_log(log));
        }
        let pick = state.hunk_pick();
        window.set_picker_open(pick.is_some());
        if let Some(pick) = pick {
            self.sync_picker(window, state, pick);
        }
        let form = state.form();
        window.set_dialog_open(form.is_some());
        if let Some(form) = form {
            let opened = state.forms_opened();
            if self.shown_form.replace(opened) != opened {
                window.set_dialog(model::dialog(form, state.integration_branch()));
                let choice = form.choice.map_or(-1, |choice| choice as i32);
                let initial: SharedString = form.initial.as_str().into();
                let dest = form.dest as i32;
                // Once the event that opened it is done with: a menu closing
                // under it would take the keyboard back to the panes.
                let shown = window.as_weak();
                Timer::single_shot(Duration::ZERO, move || {
                    if let Some(window) = shown.upgrade() {
                        window.invoke_open_dialog(initial, choice, dest);
                    }
                });
            } else {
                // Only its ticks change while it is open.
                let choices = window.get_dialog().choices;
                update_model(&choices, model::choices(form), |_| {});
            }
        }
        let was_open = self.dialog_shown.replace(form.is_some());
        let was_shown = self.log_shown.replace(log_open);
        let was_picking = self.picker_shown.replace(pick.is_some());
        if form.is_some() {
            return;
        }
        if pick.is_some() {
            if was_open || !was_picking {
                window.invoke_open_picker();
            }
        } else if log_open && (was_open || was_picking || !was_shown) {
            window.invoke_open_log();
        } else if !log_open && (was_open || was_picking || was_shown) {
            window.invoke_focus_panes();
        }
    }

    /// The hunk picker: another one, or another of its files, from its top.
    fn sync_picker(&self, window: &AppWindow, state: &State, pick: &HunkPick) {
        window.set_picking(model::picking(pick));
        let files = model::hunk_files(pick, state.cwd_prefix());
        update_model(&window.get_hunk_files(), files, |files| {
            window.set_hunk_files(files)
        });
        window.set_hunk_file(pick.current as i32);
        let lines = model::hunk_lines(pick, state.tab_width());
        let shown = (state.picks_opened(), pick.current);
        if self.shown_hunks.replace(shown) != shown {
            window.set_hunk_columns(model::hunk_columns(&lines));
            window.set_hunk_lines(model_of(lines));
            window.invoke_hunks_to_top();
        } else {
            update_model(&window.get_hunk_lines(), lines, |lines| {
                window.set_hunk_lines(lines)
            });
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
