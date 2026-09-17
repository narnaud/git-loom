use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{Frame, layout::Rect};

use super::*;
use crate::core::graph;

fn theme() -> TuiTheme {
    TuiTheme::from_graph_theme(&graph::Theme::dark())
}

fn menu() -> Menu {
    Menu::new(
        "Discard changes",
        vec![
            MenuItem::new("Discard all changes")
                .help("Discard both staged and unstaged changes.\nrestore `foo`"),
            MenuItem::new("Discard unstaged changes").key('u'),
            MenuItem::new("Cancel"),
        ],
    )
}

fn key(menu: &mut Menu, code: KeyCode) -> MenuOutcome {
    menu.handle_key(code, KeyModifiers::NONE)
}

fn render_lines(mut f: impl FnMut(&mut Frame, Rect)) -> Vec<String> {
    let backend = ratatui::backend::TestBackend::new(100, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| f(frame, frame.area())).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn moves_within_bounds_and_enter_picks_the_cursor_item() {
    let mut menu = menu();
    assert!(matches!(key(&mut menu, KeyCode::Up), MenuOutcome::Pending));
    assert_eq!(menu.cursor(), 0);
    key(&mut menu, KeyCode::Down);
    key(&mut menu, KeyCode::Char('j'));
    key(&mut menu, KeyCode::Down);
    assert_eq!(menu.cursor(), 2);
    key(&mut menu, KeyCode::Char('k'));
    assert!(matches!(
        key(&mut menu, KeyCode::Enter),
        MenuOutcome::Choose(1)
    ));
}

#[test]
fn shortcut_letter_picks_its_item_and_other_letters_do_nothing() {
    let mut menu = menu();
    assert!(matches!(
        key(&mut menu, KeyCode::Char('x')),
        MenuOutcome::Pending
    ));
    assert!(matches!(
        key(&mut menu, KeyCode::Char('u')),
        MenuOutcome::Choose(1)
    ));
}

#[test]
fn escape_and_ctrl_c_cancel() {
    let mut menu = menu();
    assert!(matches!(key(&mut menu, KeyCode::Esc), MenuOutcome::Cancel));
    assert!(matches!(
        menu.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL),
        MenuOutcome::Cancel
    ));
}

#[test]
fn enter_on_an_empty_menu_picks_nothing() {
    let mut menu = Menu::new("Empty", vec![]);
    assert!(matches!(
        key(&mut menu, KeyCode::Enter),
        MenuOutcome::Pending
    ));
}

#[test]
fn renders_title_counter_gutter_and_help_for_the_cursor_item() {
    let theme = theme();
    let mut menu = menu();
    let lines = render_lines(|f, area| menu.render(f, area, &theme));
    let text = lines.join("\n");
    assert!(text.contains(" Discard changes "));
    assert!(text.contains(" 1 of 3 "));
    assert!(text.contains("u Discard unstaged changes"));
    assert!(text.contains("Discard both staged and unstaged changes."));
    let help_row = lines
        .iter()
        .position(|l| l.contains("Discard both staged"))
        .unwrap();
    assert!(lines[help_row + 1].contains("restore foo"));

    // 80 columns wide, centered in 100.
    let top = lines
        .iter()
        .find(|l| l.contains("Discard changes"))
        .unwrap();
    let start = top.chars().position(|c| c != ' ').unwrap();
    assert_eq!(start, 10);
    assert_eq!(top.trim().chars().count(), 80);

    let title_row = lines
        .iter()
        .position(|l| l.contains("Discard changes"))
        .unwrap();
    key(&mut menu, KeyCode::Down);
    let lines = render_lines(|f, area| menu.render(f, area, &theme));
    let text = lines.join("\n");
    assert!(text.contains(" 2 of 3 "));
    assert!(!text.contains("Discard both staged"));
    // The menu does not move when the help box goes away.
    assert_eq!(
        lines.iter().position(|l| l.contains("Discard changes")),
        Some(title_row)
    );
}
