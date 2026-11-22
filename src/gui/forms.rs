//! The questions the window asks over everything else: the prompts of the
//! command running, and what went wrong.

use std::sync::mpsc::Sender;

use crate::core::ui::{Answer, PromptKind};
use crate::gui::state::State;
use crate::session::worker::Ask;

/// One item of a list to pick from or tick, or a heading over some.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub text: String,
    pub heading: bool,
    pub checked: bool,
}

impl Choice {
    fn item(text: &str) -> Choice {
        Choice {
            text: text.to_string(),
            heading: false,
            checked: false,
        }
    }
}

/// What an answer goes to.
enum Purpose {
    /// The command that asked, blocked until it gets it; `None` cancels it.
    Prompt {
        kind: PromptKind,
        reply: Sender<Option<Answer>>,
    },
    /// Nothing: the form is read, then dismissed.
    Notice,
}

pub(crate) struct Form {
    pub title: String,
    pub body: String,
    /// What the body says in more detail, a line each.
    pub detail: String,
    /// Why the last answer was refused.
    pub error: String,
    /// Whether a line is typed, and what it starts with.
    pub line: bool,
    pub initial: String,
    pub placeholder: String,
    pub choices: Vec<Choice>,
    /// Whether any number of the choices are ticked, rather than one picked.
    pub multi: bool,
    /// The choice a list starts on.
    pub choice: Option<usize>,
    pub confirm: &'static str,
    /// Whether it can be dismissed without an answer.
    pub cancellable: bool,
    purpose: Purpose,
}

impl Form {
    fn new(title: &str, purpose: Purpose) -> Form {
        Form {
            title: title.to_string(),
            body: String::new(),
            detail: String::new(),
            error: String::new(),
            line: false,
            initial: String::new(),
            placeholder: String::new(),
            choices: Vec::new(),
            multi: false,
            choice: None,
            confirm: "OK",
            cancellable: true,
            purpose,
        }
    }

    /// `text` under `title`, to read and dismiss: its first line is the body,
    /// the others the detail, as the CLI prints an error and its hints.
    pub(crate) fn notice(title: &str, text: &str) -> Form {
        let (body, detail) = text.split_once('\n').unwrap_or((text, ""));
        Form {
            body: body.to_string(),
            detail: detail.to_string(),
            confirm: "Close",
            cancellable: false,
            ..Form::new(title, Purpose::Notice)
        }
    }

    /// What the running command asks, as `msg` asks it (`core::ui`).
    pub(crate) fn prompt(ask: Ask) -> Form {
        let (question, detail) = ask.prompt.split_once('\n').unwrap_or((&ask.prompt, ""));
        let mut form = Form::new(&ask.command, Purpose::Notice);
        form.body = question.to_string();
        form.detail = detail.to_string();
        form.error = ask.error.unwrap_or_default();
        match &ask.kind {
            PromptKind::Confirm => form.confirm = "Yes",
            PromptKind::Input { placeholder } => {
                form.line = true;
                // `msg::input_with_placeholder` offers it as the answer.
                form.initial = placeholder.clone().unwrap_or_default();
            }
            PromptKind::Select { items, allow_other } => {
                form.choices = items.iter().map(|item| Choice::item(item)).collect();
                form.choice = (!items.is_empty()).then_some(0);
                if *allow_other {
                    form.line = true;
                    form.placeholder = "Or type another".to_string();
                }
            }
            PromptKind::MultiSelect { items } => {
                form.choices = items.iter().map(|item| Choice::item(item)).collect();
                form.multi = true;
            }
            PromptKind::GroupedSelect { groups } => {
                for (heading, items) in groups {
                    form.choices.push(Choice {
                        heading: true,
                        ..Choice::item(heading)
                    });
                    form.choices
                        .extend(items.iter().map(|item| Choice::item(item)));
                }
                form.choice = form.choices.iter().position(|choice| !choice.heading);
            }
        }
        form.purpose = Purpose::Prompt {
            kind: ask.kind,
            reply: ask.reply,
        };
        form
    }
}

impl State {
    pub(crate) fn form(&self) -> Option<&Form> {
        self.form.as_ref()
    }

    /// How many forms opened, so the window fills each in once.
    pub(crate) fn forms_opened(&self) -> u64 {
        self.forms_opened
    }

    pub(super) fn open_form(&mut self, form: Form) {
        self.form_cancelled();
        self.form = Some(form);
        self.forms_opened += 1;
    }

    /// The form was dismissed. A prompt's command is told so: it waits for
    /// an answer.
    pub(crate) fn form_cancelled(&mut self) {
        if let Some(Purpose::Prompt { reply, .. }) = self.form.take().map(|form| form.purpose) {
            let _ = reply.send(None);
        }
    }

    /// A tick of a list that takes any number.
    pub(crate) fn form_toggled(&mut self, index: usize) {
        let form = self.form.as_mut().filter(|form| form.multi);
        let choices = form.map(|form| &mut form.choices);
        if let Some(choice) = choices
            .and_then(|choices| choices.get_mut(index))
            .filter(|choice| !choice.heading)
        {
            choice.checked = !choice.checked;
        }
    }

    /// The form was confirmed with `text` typed and the choice at `choice`
    /// picked.
    pub(crate) fn form_accepted(&mut self, text: &str, choice: Option<usize>) {
        let Some(mut form) = self.form.take() else {
            return;
        };
        let ticked: Vec<String> = form
            .choices
            .iter()
            .filter(|choice| choice.checked)
            .map(|choice| choice.text.clone())
            .collect();
        if form.multi && ticked.is_empty() {
            // Asked again, as it was.
            form.error = "Tick at least one".to_string();
            form.initial = text.to_string();
            form.choice = choice;
            return self.open_form(form);
        }
        let picked = choice
            .and_then(|index| form.choices.get(index))
            .filter(|choice| !choice.heading)
            .map(|choice| choice.text.clone());
        let typed = text.trim();
        let Purpose::Prompt { kind, reply } = form.purpose else {
            return;
        };
        let answer = match kind {
            PromptKind::Confirm => Some(Answer::Bool(true)),
            PromptKind::Input { .. } => Some(Answer::Text(text.to_string())),
            PromptKind::Select {
                allow_other: true, ..
            } if !typed.is_empty() => Some(Answer::Text(typed.to_string())),
            PromptKind::Select { .. } | PromptKind::GroupedSelect { .. } => {
                picked.map(Answer::Text)
            }
            PromptKind::MultiSelect { .. } => Some(Answer::Many(ticked)),
        };
        // No item to answer with reads as a dismissal.
        let _ = reply.send(answer);
    }
}

#[cfg(test)]
#[path = "forms_test.rs"]
mod tests;
