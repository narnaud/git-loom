use std::borrow::Cow;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::{Frame, widgets::Paragraph};

use super::*;
use crate::core::graph::Theme;
use crate::tui::theme::TuiTheme;
use crate::tui::widgets::common::pane_block;

/// Minimal app recording what the shell forwards to it.
struct StubApp {
    theme: TuiTheme,
    keys: Vec<(PaneId, KeyCode)>,
    mouse: Vec<(PaneId, MouseEventKind)>,
    notice: Option<String>,
    mode_hint: Option<String>,
    cleared: usize,
}

#[derive(Debug, PartialEq)]
enum StubExit {
    Quit,
    Done,
}

impl StubApp {
    fn new() -> Self {
        StubApp {
            theme: TuiTheme::from_graph_theme(&Theme::dark()),
            keys: Vec::new(),
            mouse: Vec::new(),
            notice: None,
            mode_hint: None,
            cleared: 0,
        }
    }
}

impl ShellApp for StubApp {
    type Exit = StubExit;

    fn config(&self) -> ShellConfig {
        ShellConfig { split: (50, 50) }
    }

    fn theme(&self) -> &TuiTheme {
        &self.theme
    }

    fn quit_exit(&mut self) -> StubExit {
        StubExit::Quit
    }

    fn handle_key(
        &mut self,
        focused: PaneId,
        code: KeyCode,
        _modifiers: KeyModifiers,
    ) -> KeyResult<StubExit> {
        if code == KeyCode::Enter {
            return KeyResult::Exit(StubExit::Done);
        }
        self.keys.push((focused, code));
        KeyResult::Handled
    }

    fn handle_mouse(&mut self, pane: PaneId, kind: MouseEventKind, _pos: Position, _area: Rect) {
        self.mouse.push((pane, kind));
    }

    fn render_pane(&mut self, frame: &mut Frame, _pane: PaneId, area: Rect, focused: bool) {
        let block = pane_block(" Stub ", &self.theme, focused);
        frame.render_widget(Paragraph::new("content").block(block), area);
    }

    fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    fn clear_notice(&mut self) {
        self.cleared += 1;
        self.notice = None;
    }

    fn mode_hint(&self) -> Option<String> {
        self.mode_hint.clone()
    }

    fn status_hints(&self, _focused: PaneId) -> Vec<Cow<'static, str>> {
        vec!["a hint".into(), "another".into()]
    }
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn render<A: ShellApp>(shell: &mut Shell<A>, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    terminal.backend().buffer().clone()
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn tab_toggles_focus() {
    let mut shell = Shell::new(StubApp::new());
    assert_eq!(shell.focus(), PaneId::Left);
    assert!(shell.handle_event(key(KeyCode::Tab)).is_none());
    assert_eq!(shell.focus(), PaneId::Right);
    assert!(shell.handle_event(key(KeyCode::BackTab)).is_none());
    assert_eq!(shell.focus(), PaneId::Left);
}

#[test]
fn q_and_ctrl_c_quit() {
    let mut shell = Shell::new(StubApp::new());
    assert_eq!(
        shell.handle_event(key(KeyCode::Char('q'))),
        Some(StubExit::Quit)
    );
    assert_eq!(
        shell.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        ))),
        Some(StubExit::Quit)
    );
}

#[test]
fn other_keys_reach_the_app_with_focus() {
    let mut shell = Shell::new(StubApp::new());
    shell.handle_event(key(KeyCode::Char('x')));
    shell.handle_event(key(KeyCode::Tab));
    shell.handle_event(key(KeyCode::Char('y')));
    assert_eq!(
        shell.app.keys,
        vec![
            (PaneId::Left, KeyCode::Char('x')),
            (PaneId::Right, KeyCode::Char('y')),
        ]
    );
}

#[test]
fn app_exit_value_passes_through() {
    let mut shell = Shell::new(StubApp::new());
    assert_eq!(
        shell.handle_event(key(KeyCode::Enter)),
        Some(StubExit::Done)
    );
}

#[test]
fn every_key_clears_the_notice() {
    let mut shell = Shell::new(StubApp::new());
    shell.app.notice = Some("oops".to_string());
    shell.handle_event(key(KeyCode::Char('x')));
    assert_eq!(shell.app.cleared, 1);
    assert!(shell.app.notice.is_none());
}

#[test]
fn mouse_down_moves_focus_and_reaches_the_app() {
    let mut shell = Shell::new(StubApp::new());
    render(&mut shell, 80, 12);
    let right = shell.areas()[1];

    shell.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: right.x + 1,
        row: right.y + 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(shell.focus(), PaneId::Right);
    assert_eq!(
        shell.app.mouse,
        vec![(PaneId::Right, MouseEventKind::Down(MouseButton::Left))]
    );
}

#[test]
fn wheel_does_not_move_focus() {
    let mut shell = Shell::new(StubApp::new());
    render(&mut shell, 80, 12);
    let right = shell.areas()[1];

    shell.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: right.x + 1,
        row: right.y + 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(shell.focus(), PaneId::Left);
    assert_eq!(
        shell.app.mouse,
        vec![(PaneId::Right, MouseEventKind::ScrollDown)]
    );
}

#[test]
fn status_bar_prefers_notice_then_mode_hint_then_hints() {
    let mut shell = Shell::new(StubApp::new());

    let rows = buffer_text(&render(&mut shell, 60, 8));
    assert!(rows.last().unwrap().starts_with(" a hint | another"));

    shell.app.mode_hint = Some(" special mode".to_string());
    let rows = buffer_text(&render(&mut shell, 60, 8));
    assert!(rows.last().unwrap().starts_with(" special mode"));

    shell.app.notice = Some("something happened".to_string());
    let rows = buffer_text(&render(&mut shell, 60, 8));
    assert!(rows.last().unwrap().starts_with(" something happened"));
}

/// The hunk selector's composed status bar must match the spec string.
#[test]
fn hunk_selector_status_bar_matches_spec() {
    use crate::tui::hunk_selector::test_support::sample_app;

    let mut shell = Shell::new(sample_app());
    let rows = buffer_text(&render(&mut shell, 110, 10));
    assert!(
        rows.last().unwrap().starts_with(
            " Navigate: \u{2191}/\u{2193} | Switch Pane: tab | Toggle: space \
             | Confirm: c or Enter | Quit: q or Esc"
        ),
        "got: {:?}",
        rows.last().unwrap()
    );
}
