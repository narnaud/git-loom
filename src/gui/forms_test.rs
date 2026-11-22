use std::sync::mpsc::{Receiver, channel};

use super::*;
use crate::gui::state::tests::state;

/// What the running `loom drop a` asks, and where its answer arrives.
fn ask(kind: PromptKind, prompt: &str) -> (Ask, Receiver<Option<Answer>>) {
    let (reply, answer) = channel();
    let ask = Ask {
        command: "loom drop a".to_string(),
        kind,
        prompt: prompt.to_string(),
        error: None,
        reply,
    };
    (ask, answer)
}

fn select(items: &[&str], allow_other: bool) -> PromptKind {
    PromptKind::Select {
        items: items.iter().map(|item| item.to_string()).collect(),
        allow_other,
    }
}

#[test]
fn a_prompt_is_headed_by_the_command_asking_its_first_line_the_question() {
    let (ask, _answer) = ask(
        PromptKind::Confirm,
        "Discard all local changes?\nrestore `a.rs`\ndelete `b.rs`",
    );

    let form = Form::prompt(ask);

    assert_eq!(form.title, "loom drop a");
    assert_eq!(form.body, "Discard all local changes?");
    assert_eq!(form.detail, "restore `a.rs`\ndelete `b.rs`");
    assert_eq!((form.confirm, form.cancellable), ("Yes", true));
    assert!(form.input == Input::None && form.choices.is_empty());
}

#[test]
fn a_confirmed_prompt_answers_yes_and_a_dismissed_one_cancels() {
    let mut state = state();
    let (yes, answer) = ask(PromptKind::Confirm, "Drop it?");
    state.open_form(Form::prompt(yes));

    state.form_accepted("", None, 0);

    assert_eq!(answer.recv().unwrap(), Some(Answer::Bool(true)));
    assert!(state.form().is_none());
    let (no, answer) = ask(PromptKind::Confirm, "Drop it?");
    state.open_form(Form::prompt(no));
    state.form_cancelled();
    assert_eq!(answer.recv().unwrap(), None);
}

/// The command is blocked on its answer: a form opened over it must not
/// leave it waiting.
#[test]
fn a_prompt_another_form_replaces_is_cancelled() {
    let mut state = state();
    let (first, answer) = ask(PromptKind::Confirm, "Drop it?");
    state.open_form(Form::prompt(first));

    state.open_form(Form::notice("loom drop a", "It failed"));

    assert_eq!(answer.recv().unwrap(), None);
}

#[test]
fn an_input_starts_from_what_it_offers_and_answers_what_was_typed() {
    let mut state = state();
    let (input, answer) = ask(
        PromptKind::Input {
            placeholder: Some("feature".to_string()),
        },
        "Name of the branch",
    );
    state.open_form(Form::prompt(input));
    let form = state.form().unwrap();
    assert_eq!(form.input, Input::Line);
    assert_eq!(form.initial, "feature");

    state.form_accepted("feature-x", None, 0);

    assert_eq!(
        answer.recv().unwrap(),
        Some(Answer::Text("feature-x".to_string()))
    );
}

#[test]
fn a_pick_answers_the_item_picked_or_what_was_typed_instead() {
    let mut state = state();
    let (pick, answer) = ask(select(&["origin", "upstream"], false), "Which remote?");
    state.open_form(Form::prompt(pick));
    assert_eq!(state.form().unwrap().choice, Some(0));
    assert_eq!(state.form().unwrap().input, Input::None);
    state.form_accepted("", Some(1), 0);
    assert_eq!(
        answer.recv().unwrap(),
        Some(Answer::Text("upstream".to_string()))
    );

    let (other, answer) = ask(select(&["github"], true), "Which forge?");
    state.open_form(Form::prompt(other));
    assert_eq!(state.form().unwrap().input, Input::Line);
    state.form_accepted(" gitea ", Some(0), 0);
    assert_eq!(
        answer.recv().unwrap(),
        Some(Answer::Text("gitea".to_string()))
    );
}

#[test]
fn a_grouped_pick_starts_on_its_first_item_and_never_answers_a_heading() {
    let mut state = state();
    let groups = vec![
        ("Local".to_string(), vec!["feature-a".to_string()]),
        ("Remote".to_string(), vec!["origin/feature-b".to_string()]),
    ];
    let (pick, answer) = ask(PromptKind::GroupedSelect { groups }, "Merge which branch?");
    state.open_form(Form::prompt(pick));
    let form = state.form().unwrap();
    let headings: Vec<bool> = form.choices.iter().map(|choice| choice.heading).collect();
    assert_eq!(headings, [true, false, true, false]);
    assert_eq!(form.choice, Some(1));

    state.form_accepted("", Some(2), 0);

    assert_eq!(answer.recv().unwrap(), None, "a heading reads as no answer");
}

#[test]
fn a_tick_list_answers_every_item_ticked_and_at_least_one() {
    let mut state = state();
    let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let (ticks, answer) = ask(PromptKind::MultiSelect { items }, "Which ones?");
    state.open_form(Form::prompt(ticks));

    state.form_accepted("", None, 0);
    assert_eq!(state.form().unwrap().error, "Tick at least one");
    assert!(answer.try_recv().is_err(), "still asking");
    state.form_toggled(0);
    state.form_toggled(2);
    state.form_toggled(0);
    state.form_accepted("", None, 0);

    assert_eq!(
        answer.recv().unwrap(),
        Some(Answer::Many(vec!["c".to_string()]))
    );
}

#[test]
fn a_notice_reads_its_first_line_over_the_hints_under_it() {
    let form = Form::notice("loom push a", "Push refused\nIf `a` has diverged, force it");

    assert_eq!(form.title, "loom push a");
    assert_eq!(form.body, "Push refused");
    assert_eq!(form.detail, "If `a` has diverged, force it");
    assert_eq!((form.confirm, form.cancellable), ("Close", false));
}
