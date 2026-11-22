//! The questions the window asks over everything else: the prompts of the
//! command running, what a command of its own needs before it runs, and what
//! went wrong.

use std::sync::mpsc::Sender;

use crate::core::ui::{Answer, Level, PromptKind};
use crate::gui::state::State;
use crate::session::action::Action;
use crate::session::snapshot::CommitDest;
use crate::session::targets;
use crate::session::tree::branch_key;
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

/// What a form has typed in it, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    None,
    Line,
    /// A message, of as many lines as it takes.
    Text,
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
    /// The action to run, its message, name or destination what is typed
    /// and picked; one that asks only a yes runs as it is.
    Complete(Action),
}

/// What the next commit dialog starts from.
pub(crate) struct Draft {
    /// Where the last commit went.
    pub(super) dest: CommitDest,
    /// The message of the last commit, until it goes through.
    pub(super) message: String,
    /// The command running is that commit.
    pub(super) running: bool,
}

impl Draft {
    pub(super) fn new() -> Draft {
        Draft {
            dest: CommitDest::Integration,
            message: String::new(),
            running: false,
        }
    }
}

pub(crate) struct Form {
    pub title: String,
    pub body: String,
    /// What the body says in more detail, a line each.
    pub detail: String,
    /// Why the last answer was refused.
    pub error: String,
    pub input: Input,
    /// What the input starts with.
    pub initial: String,
    pub placeholder: String,
    pub choices: Vec<Choice>,
    /// Whether any number of the choices are ticked, rather than one picked.
    pub multi: bool,
    /// The choice a list starts on.
    pub choice: Option<usize>,
    /// Where a commit can go, and the one picked at first.
    pub dests: Vec<CommitDest>,
    pub dest: usize,
    pub confirm: &'static str,
    /// Whether it can be dismissed without an answer.
    pub cancellable: bool,
    purpose: Purpose,
    /// What the input held before any answer: one left as it was runs
    /// nothing.
    was: String,
}

impl Form {
    fn new(title: &str, purpose: Purpose) -> Form {
        Form {
            title: title.to_string(),
            body: String::new(),
            detail: String::new(),
            error: String::new(),
            input: Input::None,
            initial: String::new(),
            placeholder: String::new(),
            choices: Vec::new(),
            multi: false,
            choice: None,
            dests: Vec::new(),
            dest: 0,
            confirm: "OK",
            cancellable: true,
            purpose,
            was: String::new(),
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
                form.input = Input::Line;
                // `msg::input_with_placeholder` offers it as the answer.
                form.initial = placeholder.clone().unwrap_or_default();
            }
            PromptKind::Select { items, allow_other } => {
                form.choices = items.iter().map(|item| Choice::item(item)).collect();
                form.choice = (!items.is_empty()).then_some(0);
                if *allow_other {
                    form.input = Input::Line;
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

    /// A form of the window's own, headed `body`, asking `input` and
    /// confirmed by `confirm`; [`Form::complete_with`] gives it the action
    /// it asks for.
    pub(crate) fn complete(body: String, input: Input, confirm: &'static str) -> Form {
        Form {
            body,
            input,
            confirm,
            ..Form::new("", Purpose::Notice)
        }
    }

    /// Ask what `action` needs, or a yes: it runs once answered, unless with
    /// the input left as it started. A commit's message is new, whatever it
    /// starts from.
    pub(crate) fn complete_with(&mut self, action: Action) {
        if !matches!(action, Action::Commit { .. }) {
            self.was = self.initial.clone();
        }
        self.purpose = Purpose::Complete(action);
    }
}

impl State {
    pub(crate) fn form(&self) -> Option<&Form> {
        self.form.as_ref()
    }

    /// Where a commit can go, in the order of the board.
    pub(crate) fn commit_dests(&self) -> Vec<CommitDest> {
        targets::commit_dests(&self.rows)
    }

    /// Which of [`State::commit_dests`] the last commit went to; none when
    /// its branch is gone.
    pub(crate) fn commit_dest(&self) -> Option<usize> {
        let dests = self.commit_dests();
        dests.iter().position(|dest| *dest == self.draft.dest)
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
    /// an answer. A split of hunks picked ends as its picker cancelled does:
    /// the log holds their read.
    pub(crate) fn form_cancelled(&mut self) {
        match self.form.take().map(|form| form.purpose) {
            Some(Purpose::Prompt { reply, .. }) => {
                let _ = reply.send(None);
            }
            Some(Purpose::Complete(Action::Split { hunks: Some(_), .. })) => {
                self.worker.log_line(Level::Warn, "Cancelled");
                self.say(Level::Plain, "split cancelled");
            }
            _ => {}
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

    /// The form was confirmed with `text` typed, the choice at `choice`
    /// picked and the destination at `dest`.
    pub(crate) fn form_accepted(&mut self, text: &str, choice: Option<usize>, dest: usize) {
        if let Some(action) = self.accept(text, choice, dest) {
            self.start(action);
        }
    }

    /// Answer the form as [`State::form_accepted`] is told: the action it
    /// completes, to run.
    pub(super) fn accept(
        &mut self,
        text: &str,
        choice: Option<usize>,
        dest: usize,
    ) -> Option<Action> {
        let mut form = self.form.take()?;
        if let Some(error) = refusal(&form, text, dest) {
            // Asked again, as it was answered.
            form.error = error.to_string();
            form.initial = text.to_string();
            form.choice = choice;
            form.dest = dest;
            self.open_form(form);
            return None;
        }
        let ticked: Vec<String> = form
            .choices
            .iter()
            .filter(|choice| choice.checked)
            .map(|choice| choice.text.clone())
            .collect();
        let picked = choice
            .and_then(|index| form.choices.get(index))
            .filter(|choice| !choice.heading)
            .map(|choice| choice.text.clone());
        let typed = text.trim();
        match form.purpose {
            Purpose::Notice => {}
            Purpose::Prompt { kind, reply } => {
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
            Purpose::Complete(_) if form.input != Input::None && typed == form.was.trim() => {
                self.say(Level::Plain, "Nothing changed");
            }
            Purpose::Complete(mut action) => {
                let typed = typed.to_string();
                match &mut action {
                    Action::Commit {
                        dest: to, message, ..
                    } => {
                        *to = self.drafted(&form.dests, dest, &typed);
                        *message = Some(typed);
                    }
                    Action::Split { message, .. } => *message = Some(typed),
                    Action::Reword { message, .. } => {
                        // A branch renamed is followed to its row.
                        if form.input == Input::Line {
                            self.next_cursor = Some(branch_key(&typed));
                        }
                        *message = Some(typed);
                    }
                    Action::Branch { name, .. } => {
                        self.next_cursor = Some(branch_key(&typed));
                        *name = typed;
                    }
                    _ => {}
                }
                return Some(action);
            }
        }
        None
    }

    /// A commit is typed, under `message` to the destination at `dest` of
    /// `dests`: where the next starts from, until it goes through.
    fn drafted(&mut self, dests: &[CommitDest], dest: usize, message: &str) -> CommitDest {
        if let Some(picked) = dests.get(dest) {
            self.draft.dest = picked.clone();
        }
        self.draft.message = message.to_string();
        self.draft.running = true;
        self.draft.dest.clone()
    }
}

/// Why `text` and the destination at `dest` do not answer `form`.
fn refusal(form: &Form, text: &str, dest: usize) -> Option<&'static str> {
    let empty = text.trim().is_empty();
    match &form.purpose {
        Purpose::Prompt { .. } if form.multi && !form.choices.iter().any(|c| c.checked) => {
            Some("Tick at least one")
        }
        Purpose::Complete(Action::Commit { .. }) if form.dests.get(dest).is_none() => {
            Some("Pick where the commit goes")
        }
        Purpose::Complete(_) if empty && form.input == Input::Text => Some("The message is empty"),
        Purpose::Complete(_) if empty && form.input == Input::Line => Some("The name is empty"),
        _ => None,
    }
}

/// How a destination of a commit reads where it is picked.
pub(crate) fn dest_label(dest: &CommitDest, integration: &str) -> String {
    match dest {
        CommitDest::Integration => format!("{integration} (loose commit)"),
        CommitDest::Branch(name) => name.clone(),
    }
}

#[cfg(test)]
#[path = "forms_test.rs"]
mod tests;
