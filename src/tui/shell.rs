//! The app shell shared by every loom TUI: terminal lifecycle, event loop,
//! two-pane layout, focus, global keys, and the status bar.
//!
//! An app implements [`ShellApp`] and provides pane content, key/mouse
//! handling for what the shell doesn't consume, and status-bar hints; the
//! shell owns everything the TUIs would otherwise duplicate. The shell
//! consumes `q`/Ctrl-C (via [`ShellApp::quit_exit`]), Tab/BackTab (focus) and
//! Ctrl-Left/Ctrl-Right (split width), and moves focus to a pane on
//! mouse-down inside it — unless the app reports a modal, which then gets
//! every key.
//!
//! The shell sets up the terminal itself rather than through `ratatui::init`,
//! whose panic hook restores the terminal from any thread: an app's worker
//! thread panicking must not tear down the TUI that is about to report it.

use std::borrow::Cow;
use std::io::stdout;
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::thread::ThreadId;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Position, Rect},
    widgets::Paragraph,
};

use crate::tui::theme::TuiTheme;

/// How long the event loop waits for a terminal event before giving the app a
/// [`ShellApp::poll_background`] turn.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// What [`ShellApp::poll_background`] asks of the shell.
pub(crate) enum Tick<E> {
    Idle,
    Redraw,
    /// Restore the terminal for a subprocess, ack on the sender, then block
    /// in [`ShellApp::wait_for_resume`] until the app says it is back.
    Suspend(Sender<()>),
    /// Hand the live terminal to [`ShellApp::take_over`] — a nested shell of
    /// the app's own — then redraw. Nothing is torn down, so the screen does
    /// not flicker between the two.
    TakeOver,
    Exit(E),
}

/// The two panes of the shell layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneId {
    Left,
    Right,
}

impl PaneId {
    fn other(self) -> PaneId {
        match self {
            PaneId::Left => PaneId::Right,
            PaneId::Right => PaneId::Left,
        }
    }
}

/// Static layout configuration an app hands to the shell.
pub(crate) struct ShellConfig {
    /// Initial horizontal split percentages (left, right). The user can
    /// resize the split at runtime with Ctrl-Left/Ctrl-Right.
    pub split: (u16, u16),
}

/// Bounds and step for the user-resizable split, in percent of the width.
const SPLIT_MIN: u16 = 10;
const SPLIT_MAX: u16 = 90;
const SPLIT_STEP: u16 = 2;

/// What an app's key handler tells the shell.
pub(crate) enum KeyResult<E> {
    /// Key consumed (or ignored); keep looping.
    Handled,
    /// Leave the event loop with this exit value.
    Exit(E),
}

/// One TUI application hosted by the [`Shell`].
pub(crate) trait ShellApp {
    /// Why the event loop ended.
    type Exit;

    fn config(&self) -> ShellConfig;
    fn theme(&self) -> &TuiTheme;

    /// Exit value for the shell-global quit keys (`q`, Ctrl-C).
    fn quit_exit(&mut self) -> Self::Exit;

    /// Every key press the shell didn't consume.
    fn handle_key(
        &mut self,
        focused: PaneId,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> KeyResult<Self::Exit>;

    /// Mouse event inside `pane`'s rect. The shell has already hit-tested and
    /// moved focus on mouse-down; `area` is the pane rect for offset math.
    fn handle_mouse(&mut self, pane: PaneId, kind: MouseEventKind, pos: Position, area: Rect);

    /// Full pane content including its Block — titles stay app-owned.
    fn render_pane(&mut self, frame: &mut Frame, pane: PaneId, area: Rect, focused: bool);

    /// Transient status-bar message; shown instead of hints until cleared.
    fn notice(&self) -> Option<&str> {
        None
    }
    /// Called by the shell on every key press, before dispatch.
    fn clear_notice(&mut self) {}
    /// Full status-bar override for a modal state (shown after `notice`).
    fn mode_hint(&self) -> Option<String> {
        None
    }
    /// Status-bar hint segments, joined with " | ".
    fn status_hints(&self, focused: PaneId) -> Vec<Cow<'static, str>>;

    /// Whether a modal (popup, running action) owns the keyboard: the shell
    /// then forwards every key, its own included, to `handle_key`, and drops
    /// mouse events.
    fn modal_active(&self) -> bool {
        false
    }
    /// Background work between terminal events, every [`POLL_INTERVAL`].
    fn poll_background(&mut self) -> Tick<Self::Exit> {
        Tick::Idle
    }
    /// After a [`Tick::Suspend`]: block until the subprocess is done with the
    /// terminal. Must not read terminal events.
    fn wait_for_resume(&mut self) {}
    /// After a [`Tick::TakeOver`]: the app draws on `terminal` until this
    /// returns, by running a nested shell on it ([`Shell::run_nested`]). The
    /// terminal is the host's, already set up — leave it that way.
    fn take_over(&mut self, _terminal: &mut ratatui::DefaultTerminal) {}
    /// Drawn last, over the panes and the status bar (popups).
    fn render_overlay(&mut self, _frame: &mut Frame, _area: Rect) {}
}

/// The thread running a shell event loop, so the panic hook only restores
/// the terminal for a panic of the TUI itself — an app's worker thread
/// panicking is reported by the app, not by tearing the TUI down.
static TUI_THREAD: Mutex<Option<ThreadId>> = Mutex::new(None);

/// Points the panic hook at the thread running the loop; returns the thread
/// it pointed at, so a shell run from inside another one puts it back.
fn set_tui_thread(id: Option<ThreadId>) -> Option<ThreadId> {
    std::mem::replace(
        &mut *TUI_THREAD.lock().unwrap_or_else(|e| e.into_inner()),
        id,
    )
}

/// Install the terminal-restoring panic hook, once per process.
fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let tui_thread = *TUI_THREAD.lock().unwrap_or_else(|e| e.into_inner());
            match tui_thread {
                Some(id) if id == std::thread::current().id() => {
                    leave_terminal();
                    prev(info);
                }
                // A worker's panic would print into the alternate screen, which
                // is discarded on exit: unreadable, and it corrupts the frame
                // ratatui still believes it drew. The join reports it instead.
                Some(_) => {}
                None => prev(info),
            }
        }));
    });
}

/// Raw mode, alternate screen, mouse capture; a cleared terminal to draw on.
fn enter_terminal() -> Result<ratatui::DefaultTerminal> {
    enable_raw_mode()?;
    // Without the alternate screen the clear below wipes the user's own
    // terminal, so only mouse capture is best-effort: the TUIs work without it.
    crossterm::execute!(stdout(), EnterAlternateScreen)?;
    let _ = crossterm::execute!(stdout(), EnableMouseCapture);
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    Ok(terminal)
}

/// Undo [`enter_terminal`]; best-effort, there is nothing to do on failure.
fn leave_terminal() {
    let _ = crossterm::execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

/// Hosts a [`ShellApp`]: layout, focus, event loop, terminal lifecycle.
pub(crate) struct Shell<A: ShellApp> {
    pub app: A,
    focus: PaneId,
    /// Left pane width in percent; `None` until the first render seeds it
    /// from [`ShellConfig::split`].
    split: Option<u16>,
    /// Pane rects of the last render, for mouse routing.
    areas: [Rect; 2],
}

impl<A: ShellApp> Shell<A> {
    pub fn new(app: A) -> Self {
        Shell {
            app,
            focus: PaneId::Left,
            split: None,
            areas: [Rect::default(); 2],
        }
    }

    /// Set up the terminal, run the event loop, restore the terminal.
    /// Returns the app so callers can extract state from it.
    pub fn run(mut self) -> Result<(A, A::Exit)> {
        // Panic-safe cleanup: restore the terminal before the previous
        // handler. The hook is installed once per process and is inert
        // between shell runs, so repeated runs don't nest wrappers.
        install_panic_hook();
        let outer = set_tui_thread(Some(std::thread::current().id()));

        let result = enter_terminal().and_then(|mut terminal| self.event_loop(&mut terminal));

        set_tui_thread(outer);
        leave_terminal();

        result.map(|exit| (self.app, exit))
    }

    /// Run the event loop on a terminal the caller set up, for a shell hosted
    /// inside another one: the host's alternate screen, raw mode and mouse
    /// capture stay as they are, so the two draw over each other seamlessly.
    pub fn run_nested(mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<(A, A::Exit)> {
        self.event_loop(terminal).map(|exit| (self.app, exit))
    }

    fn event_loop(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<A::Exit> {
        let mut dirty = true;
        loop {
            if dirty {
                terminal.draw(|frame| self.render(frame))?;
                dirty = false;
            }
            match self.app.poll_background() {
                Tick::Idle => {}
                Tick::Redraw => dirty = true,
                Tick::Suspend(ack) => {
                    leave_terminal();
                    let _ = ack.send(());
                    self.app.wait_for_resume();
                    *terminal = enter_terminal()?;
                    dirty = true;
                }
                Tick::TakeOver => {
                    self.app.take_over(terminal);
                    dirty = true;
                }
                Tick::Exit(exit) => return Ok(exit),
            }
            if event::poll(POLL_INTERVAL)? {
                if let Some(exit) = self.handle_event(event::read()?) {
                    return Ok(exit);
                }
                dirty = true;
            }
        }
    }

    /// Draw both panes and the status bar. Public (crate) so tests can drive
    /// the real render path through a `TestBackend`.
    pub fn render(&mut self, frame: &mut Frame) {
        let left = match self.split {
            Some(left) => left,
            None => {
                let left = self.app.config().split.0.clamp(SPLIT_MIN, SPLIT_MAX);
                self.split = Some(left);
                left
            }
        };

        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(frame.area());
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(left),
                Constraint::Percentage(100 - left),
            ])
            .split(outer[0]);
        self.areas = [panes[0], panes[1]];

        self.app
            .render_pane(frame, PaneId::Left, panes[0], self.focus == PaneId::Left);
        self.app
            .render_pane(frame, PaneId::Right, panes[1], self.focus == PaneId::Right);
        self.render_status_bar(frame, outer[1]);
        self.app.render_overlay(frame, frame.area());
    }

    /// Status bar priority: notice, then mode hint, then the hint segments.
    fn render_status_bar(&mut self, frame: &mut Frame, area: Rect) {
        let text = if let Some(notice) = self.app.notice() {
            format!(" {}", notice)
        } else if let Some(hint) = self.app.mode_hint() {
            hint
        } else {
            format!(" {}", self.app.status_hints(self.focus).join(" | "))
        };
        frame.render_widget(
            Paragraph::new(text).style(self.app.theme().status_bar),
            area,
        );
    }

    /// Handle one terminal event; `Some` means the loop is done. Public
    /// (crate) so tests can drive the real dispatch path.
    pub fn handle_event(&mut self, event: Event) -> Option<A::Exit> {
        match event {
            Event::Key(key) => {
                // On Windows, crossterm fires Press and Release; only handle
                // Press.
                if key.kind != KeyEventKind::Press {
                    return None;
                }
                self.app.clear_notice();
                if self.app.modal_active() {
                    return match self.app.handle_key(self.focus, key.code, key.modifiers) {
                        KeyResult::Handled => None,
                        KeyResult::Exit(exit) => Some(exit),
                    };
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Some(self.app.quit_exit());
                }
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Left | KeyCode::Right)
                {
                    self.resize_split(if key.code == KeyCode::Left {
                        -(SPLIT_STEP as i32)
                    } else {
                        SPLIT_STEP as i32
                    });
                    return None;
                }
                match key.code {
                    KeyCode::Char('q') => Some(self.app.quit_exit()),
                    KeyCode::Tab | KeyCode::BackTab => {
                        self.focus = self.focus.other();
                        None
                    }
                    code => match self.app.handle_key(self.focus, code, key.modifiers) {
                        KeyResult::Handled => None,
                        KeyResult::Exit(exit) => Some(exit),
                    },
                }
            }
            Event::Mouse(_) if self.app.modal_active() => None,
            Event::Mouse(mouse) => {
                let pos = Position {
                    x: mouse.column,
                    y: mouse.row,
                };
                let (pane, area) = if self.areas[0].contains(pos) {
                    (PaneId::Left, self.areas[0])
                } else if self.areas[1].contains(pos) {
                    (PaneId::Right, self.areas[1])
                } else {
                    return None;
                };
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    self.focus = pane;
                }
                self.app.handle_mouse(pane, mouse.kind, pos, area);
                None
            }
            _ => None,
        }
    }

    /// Widen (positive) or narrow (negative) the left pane, clamped so both
    /// panes stay visible.
    fn resize_split(&mut self, delta: i32) {
        let current = self
            .split
            .unwrap_or_else(|| self.app.config().split.0.clamp(SPLIT_MIN, SPLIT_MAX));
        let next = (current as i32 + delta).clamp(SPLIT_MIN as i32, SPLIT_MAX as i32);
        self.split = Some(next as u16);
    }

    #[cfg(test)]
    pub fn split(&self) -> Option<u16> {
        self.split
    }

    #[cfg(test)]
    pub fn focus(&self) -> PaneId {
        self.focus
    }

    #[cfg(test)]
    pub fn areas(&self) -> [Rect; 2] {
        self.areas
    }
}

#[cfg(test)]
#[path = "shell_test.rs"]
mod tests;
