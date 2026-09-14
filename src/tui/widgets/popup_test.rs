use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;

use super::*;
use crate::core::graph;

fn theme() -> TuiTheme {
    TuiTheme::from_graph_theme(&graph::Theme::dark())
}

fn key(prompt: &mut Prompt, code: KeyCode) -> PromptOutcome {
    prompt.handle_key(code, KeyModifiers::NONE)
}

fn type_text(prompt: &mut Prompt, text: &str) {
    for c in text.chars() {
        key(prompt, KeyCode::Char(c));
    }
}

fn items(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn render_text(mut f: impl FnMut(&mut Frame, Rect)) -> String {
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| f(frame, frame.area())).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn text_field_edits_around_the_cursor() {
    let mut field = TextField::new("ac");
    field.handle_key(KeyCode::Left);
    field.handle_key(KeyCode::Char('b'));
    assert_eq!(field.value(), "abc");
    field.handle_key(KeyCode::Home);
    field.handle_key(KeyCode::Delete);
    assert_eq!(field.value(), "bc");
    field.handle_key(KeyCode::End);
    field.handle_key(KeyCode::Backspace);
    assert_eq!(field.value(), "b");
    assert!(!field.handle_key(KeyCode::Enter));
}

#[test]
fn input_prompt_answers_with_the_typed_text_and_prefills_the_placeholder() {
    let mut prompt = Prompt::new(
        PromptKind::Input {
            placeholder: Some("old".into()),
        },
        "Rename".into(),
        None,
    );
    key(&mut prompt, KeyCode::Backspace);
    type_text(&mut prompt, "d-name");
    match key(&mut prompt, KeyCode::Enter) {
        PromptOutcome::Answer(Answer::Text(t)) => assert_eq!(t, "old-name"),
        _ => panic!("expected a text answer"),
    }
}

#[test]
fn escape_and_ctrl_c_cancel() {
    let mut prompt = Prompt::new(PromptKind::Confirm, "Sure?".into(), None);
    assert!(matches!(
        key(&mut prompt, KeyCode::Esc),
        PromptOutcome::Cancel
    ));
    assert!(matches!(
        prompt.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL),
        PromptOutcome::Cancel
    ));
}

#[test]
fn confirm_prompt_maps_keys() {
    let mut prompt = Prompt::new(PromptKind::Confirm, "Sure?".into(), None);
    assert!(matches!(
        key(&mut prompt, KeyCode::Char('x')),
        PromptOutcome::Pending
    ));
    assert!(matches!(
        key(&mut prompt, KeyCode::Char('y')),
        PromptOutcome::Answer(Answer::Bool(true))
    ));
    assert!(matches!(
        key(&mut prompt, KeyCode::Enter),
        PromptOutcome::Answer(Answer::Bool(false))
    ));
}

#[test]
fn select_prompt_moves_and_chooses() {
    let mut prompt = Prompt::new(
        PromptKind::Select {
            items: items(&["a", "b", "c"]),
            allow_other: false,
        },
        "Pick".into(),
        None,
    );
    key(&mut prompt, KeyCode::Down);
    key(&mut prompt, KeyCode::Char('j'));
    key(&mut prompt, KeyCode::Down); // clamped at the end
    match key(&mut prompt, KeyCode::Enter) {
        PromptOutcome::Answer(Answer::Text(t)) => assert_eq!(t, "c"),
        _ => panic!("expected a text answer"),
    }
}

#[test]
fn select_or_input_takes_typed_text_or_a_highlighted_suggestion() {
    let kind = PromptKind::Select {
        items: items(&["feature-a", "feature-b", "other"]),
        allow_other: true,
    };
    let mut prompt = Prompt::new(kind.clone(), "Branch".into(), None);
    type_text(&mut prompt, "brand-new");
    match key(&mut prompt, KeyCode::Enter) {
        PromptOutcome::Answer(Answer::Text(t)) => assert_eq!(t, "brand-new"),
        _ => panic!("expected the typed text"),
    }

    let mut prompt = Prompt::new(kind.clone(), "Branch".into(), None);
    type_text(&mut prompt, "feat");
    key(&mut prompt, KeyCode::Down);
    key(&mut prompt, KeyCode::Down);
    match key(&mut prompt, KeyCode::Enter) {
        PromptOutcome::Answer(Answer::Text(t)) => assert_eq!(t, "feature-b"),
        _ => panic!("expected the highlighted suggestion"),
    }

    // Tab completes into the field; typing afterwards drops the highlight.
    let mut prompt = Prompt::new(kind, "Branch".into(), None);
    key(&mut prompt, KeyCode::Down);
    key(&mut prompt, KeyCode::Tab);
    type_text(&mut prompt, "-v2");
    match key(&mut prompt, KeyCode::Enter) {
        PromptOutcome::Answer(Answer::Text(t)) => assert_eq!(t, "feature-a-v2"),
        _ => panic!("expected the completed text"),
    }
}

#[test]
fn select_or_input_ignores_enter_on_empty_input() {
    let mut prompt = Prompt::new(
        PromptKind::Select {
            items: items(&["a"]),
            allow_other: true,
        },
        "Branch".into(),
        None,
    );
    assert!(matches!(
        key(&mut prompt, KeyCode::Enter),
        PromptOutcome::Pending
    ));
}

#[test]
fn multi_select_requires_one_checked_item() {
    let mut prompt = Prompt::new(
        PromptKind::MultiSelect {
            items: items(&["a", "b"]),
        },
        "Files".into(),
        None,
    );
    assert!(matches!(
        key(&mut prompt, KeyCode::Enter),
        PromptOutcome::Pending
    ));
    assert_eq!(prompt.error.as_deref(), Some("Select at least one item"));
    key(&mut prompt, KeyCode::Down);
    key(&mut prompt, KeyCode::Char(' '));
    match key(&mut prompt, KeyCode::Enter) {
        PromptOutcome::Answer(Answer::Many(v)) => assert_eq!(v, vec!["b".to_string()]),
        _ => panic!("expected a multi answer"),
    }
}

#[test]
fn prompts_render_title_error_and_hint() {
    let theme = theme();
    let prompt = Prompt::new(
        PromptKind::Input { placeholder: None },
        "Branch name".into(),
        Some("cannot be empty".into()),
    );
    let text = render_text(|f, area| prompt.render(f, area, &theme));
    assert!(text.contains(" Branch name "));
    assert!(text.contains("cannot be empty"));

    let prompt = Prompt::new(
        PromptKind::MultiSelect {
            items: items(&["src/a.rs", "src/b.rs"]),
        },
        "Files".into(),
        None,
    );
    let text = render_text(|f, area| prompt.render(f, area, &theme));
    assert!(text.contains("[ ] src/a.rs"));
    assert!(text.contains("Space: toggle"));

    let prompt = Prompt::new(PromptKind::Confirm, "Drop it?".into(), None);
    let text = render_text(|f, area| prompt.render(f, area, &theme));
    assert!(text.contains("y: yes"));
}

#[test]
fn message_lines_follow_the_cli_shape() {
    let theme = theme();
    let lines = message_lines(Level::Success, "Created `x`\nnext step", &theme);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].to_string(), "✓ Created x");
    assert_eq!(lines[1].to_string(), "  › next step");
    assert_eq!(lines[0].spans[2].style, theme.highlight);
}

#[test]
fn notice_and_log_render() {
    let theme = theme();
    let notice = Notice::new("Error", Level::Error, "Nothing to commit\nstage something");
    let text = render_text(|f, area| notice.render(f, area, &theme));
    assert!(text.contains("✗ Nothing to commit"));
    assert!(text.contains("› stage something"));
    assert!(Notice::dismisses(KeyCode::Enter));
    assert!(!Notice::dismisses(KeyCode::Char('x')));

    let entries = vec![LogEntry {
        command: "loom reword a1".into(),
        lines: vec![(Level::Success, "Reworded `a1`".into())],
    }];
    let mut scroll = DiffPane::new();
    let text = render_text(|f, area| render_log(f, area, &entries, &mut scroll, &theme));
    assert!(text.contains("$ loom reword a1"));
    assert!(text.contains("✓ Reworded a1"));

    let mut scroll = DiffPane::new();
    let text = render_text(|f, area| render_log(f, area, &[], &mut scroll, &theme));
    assert!(text.contains("no actions yet"));
}
