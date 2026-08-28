//! The app shell shared by every loom TUI: terminal lifecycle, event loop,
//! two-pane layout, focus, global keys, and the status bar.
//!
//! An app implements [`ShellApp`] and provides pane content, key/mouse
//! handling for what the shell doesn't consume, and status-bar hints; the
//! shell owns everything the TUIs would otherwise duplicate. The shell
//! consumes `q`/Ctrl-C (via [`ShellApp::quit_exit`]) and Tab/BackTab (focus),
//! and moves focus to a pane on mouse-down inside it.

use std::borrow::Cow;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Position, Rect},
    widgets::Paragraph,
};

use crate::tui::theme::TuiTheme;

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
    /// Horizontal split percentages (left, right).
    pub split: (u16, u16),
}

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
}

/// Whether a shell event loop is currently running, so the panic hook only
/// restores the terminal while a TUI is actually up.
static TUI_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Install the terminal-restoring panic hook, once per process.
fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if TUI_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
                ratatui::restore();
            }
            prev(info);
        }));
    });
}

/// Hosts a [`ShellApp`]: layout, focus, event loop, terminal lifecycle.
pub(crate) struct Shell<A: ShellApp> {
    pub app: A,
    focus: PaneId,
    /// Pane rects of the last render, for mouse routing.
    areas: [Rect; 2],
}

impl<A: ShellApp> Shell<A> {
    pub fn new(app: A) -> Self {
        Shell {
            app,
            focus: PaneId::Left,
            areas: [Rect::default(); 2],
        }
    }

    /// Set up the terminal, run the event loop, restore the terminal.
    /// Returns the app so callers can extract state from it.
    pub fn run(mut self) -> Result<(A, A::Exit)> {
        let mut terminal = ratatui::init();
        // Best-effort: a failure must not leave the terminal raw without
        // running the restore below, and the TUIs work without the mouse.
        let _ = crossterm::execute!(std::io::stdout(), EnableMouseCapture);

        // Panic-safe cleanup: restore the terminal before the previous
        // handler. The hook is installed once per process and is inert
        // between shell runs, so repeated runs don't nest wrappers.
        install_panic_hook();
        TUI_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);

        let result = self.event_loop(&mut terminal);

        TUI_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
        ratatui::restore();

        result.map(|exit| (self.app, exit))
    }

    fn event_loop(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<A::Exit> {
        loop {
            terminal.draw(|frame| self.render(frame))?;
            if let Some(exit) = self.handle_event(event::read()?) {
                return Ok(exit);
            }
        }
    }

    /// Draw both panes and the status bar. Public (crate) so tests can drive
    /// the real render path through a `TestBackend`.
    pub fn render(&mut self, frame: &mut Frame) {
        let split = self.app.config().split;

        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(frame.area());
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(split.0),
                Constraint::Percentage(split.1),
            ])
            .split(outer[0]);
        self.areas = [panes[0], panes[1]];

        self.app
            .render_pane(frame, PaneId::Left, panes[0], self.focus == PaneId::Left);
        self.app
            .render_pane(frame, PaneId::Right, panes[1], self.focus == PaneId::Right);
        self.render_status_bar(frame, outer[1]);
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
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Some(self.app.quit_exit());
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
