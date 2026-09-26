//! Modal popups for `loom tui`: the prompts a running command asks through
//! `core::ui`, error/pause notices, the action log, and the key help.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::core::ui::{Answer, Level, PromptKind};
use crate::tui::theme::TuiTheme;
use crate::tui::widgets::diff_pane::DiffPane;
use crate::tui::widgets::menu::{Menu, MenuItem, MenuOutcome};

/// Centered rect of at most `width`×`height` inside `area`.
pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Style `text` with backtick-quoted words highlighted, like the CLI's yellow.
pub(crate) fn highlight_backticks(text: &str, base: Style, theme: &TuiTheme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('`') else { break };
        if start > 0 {
            spans.push(Span::styled(rest[..start].to_string(), base));
        }
        spans.push(Span::styled(after[..end].to_string(), theme.highlight));
        rest = &after[end + 1..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), base));
    }
    spans
}

/// The `✓`/`!`/`✗` line for a message and its `›` hint continuations, as the
/// CLI prints them.
pub(crate) fn message_lines(level: Level, text: &str, theme: &TuiTheme) -> Vec<Line<'static>> {
    let (symbol, style) = match level {
        Level::Success => ("✓", theme.ok),
        Level::Warn => ("!", theme.warn),
        Level::Error => ("✗", theme.err),
    };
    text.lines()
        .enumerate()
        .map(|(i, line)| {
            let mut spans = if i == 0 {
                vec![Span::styled(format!("{} ", symbol), style)]
            } else {
                vec![Span::styled("  › ", theme.hint)]
            };
            spans.extend(highlight_backticks(line, Style::default(), theme));
            Line::from(spans)
        })
        .collect()
}

fn hint_line(text: &str, theme: &TuiTheme) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), theme.dim))
}

/// Draw `lines` in a bordered, cleared box centered in `area`, sized to fit.
fn draw_box(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    theme: &TuiTheme,
) {
    let widest = lines
        .iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(0)
        .max(title.chars().count() + 2);
    let width = (widest as u16 + 4).clamp(30, area.width.saturating_sub(4).max(30));
    let height = lines.len() as u16 + 2;
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(theme.border_active);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

// ── Text field ───────────────────────────────────────────────────────────

/// A single-line text field with a cursor.
pub(crate) struct TextField {
    chars: Vec<char>,
    cursor: usize,
}

impl TextField {
    pub fn new(initial: &str) -> Self {
        let chars: Vec<char> = initial.chars().collect();
        TextField {
            cursor: chars.len(),
            chars,
        }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Apply an editing key; `false` when the key is not an editing key. A
    /// chord (Ctrl-U, Alt-B, ...) is not one: typing its letter would silently
    /// turn an editing shortcut into text.
    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        match code {
            KeyCode::Char(_) if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                return false;
            }
            KeyCode::Char(c) => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }

    /// The text with the cursor cell reversed; a trailing space carries the
    /// cursor at the end.
    pub fn spans(&self, theme: &TuiTheme) -> Vec<Span<'static>> {
        let before: String = self.chars[..self.cursor].iter().collect();
        let at = self.chars.get(self.cursor).map_or(' ', |c| *c);
        let after: String = self
            .chars
            .get(self.cursor + 1..)
            .unwrap_or(&[])
            .iter()
            .collect();
        vec![
            Span::raw(before),
            Span::styled(at.to_string(), theme.cursor),
            Span::raw(after),
        ]
    }
}

// ── Prompt ───────────────────────────────────────────────────────────────

pub(crate) enum PromptOutcome {
    Pending,
    Answer(Answer),
    Cancel,
}

enum PromptState {
    /// The question as the first item, `Cancel` as the second.
    Confirm(Menu),
    Input(TextField),
    Select {
        items: Vec<String>,
        cursor: usize,
    },
    /// `highlight` is `None` while the typed text itself is the answer.
    SelectOrInput {
        items: Vec<String>,
        field: TextField,
        highlight: Option<usize>,
    },
    MultiSelect {
        items: Vec<String>,
        cursor: usize,
        checked: Vec<bool>,
    },
}

/// A prompt asked by a running command, answered with the keyboard.
pub(crate) struct Prompt {
    title: String,
    error: Option<String>,
    state: PromptState,
}

/// Rows of a list shown at once; longer lists scroll around the cursor.
const LIST_ROWS: usize = 12;

impl Prompt {
    /// `command` is the running command line; a confirmation is titled with
    /// it, since its question becomes the menu's first item and the detail
    /// lines after it the item's help.
    pub fn new(kind: PromptKind, prompt: String, error: Option<String>, command: &str) -> Self {
        let mut title = prompt;
        let state = match kind {
            PromptKind::Confirm => {
                let (question, detail) = title.split_once('\n').unwrap_or((&title, ""));
                let mut item = MenuItem::new(question.trim_end_matches('?'));
                if !detail.is_empty() {
                    item = item.help(detail);
                }
                title = command.to_string();
                PromptState::Confirm(Menu::new(
                    title.clone(),
                    vec![item, MenuItem::new("Cancel")],
                ))
            }
            PromptKind::Input { placeholder } => {
                PromptState::Input(TextField::new(placeholder.as_deref().unwrap_or("")))
            }
            PromptKind::Select {
                items,
                allow_other: false,
            } => PromptState::Select { items, cursor: 0 },
            PromptKind::Select {
                items,
                allow_other: true,
            } => PromptState::SelectOrInput {
                items,
                field: TextField::new(""),
                highlight: None,
            },
            PromptKind::MultiSelect { items } => PromptState::MultiSelect {
                checked: vec![false; items.len()],
                items,
                cursor: 0,
            },
        };
        Prompt {
            title,
            error,
            state,
        }
    }

    #[cfg(test)]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Indices of the suggestions matching the typed text.
    fn filtered(items: &[String], field: &TextField) -> Vec<usize> {
        let typed = field.value();
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.contains(&typed))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> PromptOutcome {
        if code == KeyCode::Esc
            || (code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL))
        {
            return PromptOutcome::Cancel;
        }
        match &mut self.state {
            PromptState::Confirm(menu) => match menu.handle_key(code, modifiers) {
                MenuOutcome::Choose(0) => PromptOutcome::Answer(Answer::Bool(true)),
                MenuOutcome::Choose(_) => PromptOutcome::Answer(Answer::Bool(false)),
                MenuOutcome::Cancel => PromptOutcome::Cancel,
                MenuOutcome::Pending => PromptOutcome::Pending,
            },
            PromptState::Input(field) => {
                if code == KeyCode::Enter {
                    return PromptOutcome::Answer(Answer::Text(field.value()));
                }
                field.handle_key(code, modifiers);
                PromptOutcome::Pending
            }
            PromptState::Select { items, cursor } => {
                match code {
                    KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => {
                        *cursor = (*cursor + 1).min(items.len().saturating_sub(1))
                    }
                    KeyCode::Enter => {
                        if let Some(item) = items.get(*cursor) {
                            return PromptOutcome::Answer(Answer::Text(item.clone()));
                        }
                    }
                    _ => {}
                }
                PromptOutcome::Pending
            }
            PromptState::SelectOrInput {
                items,
                field,
                highlight,
            } => {
                let filtered = Self::filtered(items, field);
                match code {
                    KeyCode::Up => {
                        *highlight = highlight.and_then(|h| h.checked_sub(1));
                    }
                    KeyCode::Down if !filtered.is_empty() => {
                        *highlight = Some(highlight.map_or(0, |h| (h + 1).min(filtered.len() - 1)));
                    }
                    KeyCode::Tab => {
                        if let Some(&i) = highlight.and_then(|h| filtered.get(h)) {
                            *field = TextField::new(&items[i]);
                            *highlight = None;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(&i) = highlight.and_then(|h| filtered.get(h)) {
                            return PromptOutcome::Answer(Answer::Text(items[i].clone()));
                        }
                        if !field.is_empty() {
                            return PromptOutcome::Answer(Answer::Text(field.value()));
                        }
                    }
                    _ => {
                        if field.handle_key(code, modifiers) {
                            *highlight = None;
                        }
                    }
                }
                PromptOutcome::Pending
            }
            PromptState::MultiSelect {
                items,
                cursor,
                checked,
            } => {
                match code {
                    KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => {
                        *cursor = (*cursor + 1).min(items.len().saturating_sub(1))
                    }
                    KeyCode::Char(' ') => {
                        if let Some(c) = checked.get_mut(*cursor) {
                            *c = !*c;
                        }
                    }
                    KeyCode::Enter => {
                        let selected: Vec<String> = items
                            .iter()
                            .zip(checked.iter())
                            .filter(|(_, c)| **c)
                            .map(|(item, _)| item.clone())
                            .collect();
                        if selected.is_empty() {
                            self.error = Some("Select at least one item".to_string());
                        } else {
                            return PromptOutcome::Answer(Answer::Many(selected));
                        }
                    }
                    _ => {}
                }
                PromptOutcome::Pending
            }
        }
    }

    /// List rows around `cursor`, each `(index, is_cursor)`.
    fn list_lines(
        items: &[String],
        indices: &[usize],
        cursor: Option<usize>,
        checked: Option<&[bool]>,
        theme: &TuiTheme,
    ) -> Vec<Line<'static>> {
        let start = cursor
            .unwrap_or(0)
            .saturating_sub(LIST_ROWS / 2)
            .min(indices.len().saturating_sub(LIST_ROWS));
        indices
            .iter()
            .enumerate()
            .skip(start)
            .take(LIST_ROWS)
            .map(|(pos, &i)| {
                let mark = match checked {
                    Some(checked) if checked[i] => "[x] ",
                    Some(_) => "[ ] ",
                    None => "  ",
                };
                let style = if cursor == Some(pos) {
                    theme.file_selected
                } else {
                    Style::default()
                };
                Line::from(Span::styled(format!("{}{}", mark, items[i]), style))
            })
            .collect()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &TuiTheme) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let hint = match &self.state {
            PromptState::Confirm(menu) => return menu.render(frame, area, theme),
            PromptState::Input(field) => {
                let mut spans = vec![Span::styled("> ", theme.hint)];
                spans.extend(field.spans(theme));
                lines.push(Line::from(spans));
                "Enter: confirm · Esc: cancel"
            }
            PromptState::Select { items, cursor } => {
                let all: Vec<usize> = (0..items.len()).collect();
                lines.extend(Self::list_lines(items, &all, Some(*cursor), None, theme));
                "↑/↓: move · Enter: choose · Esc: cancel"
            }
            PromptState::SelectOrInput {
                items,
                field,
                highlight,
            } => {
                let mut spans = vec![Span::styled("> ", theme.hint)];
                spans.extend(field.spans(theme));
                lines.push(Line::from(spans));
                let filtered = Self::filtered(items, field);
                lines.extend(Self::list_lines(items, &filtered, *highlight, None, theme));
                "Type a value or ↑/↓ to pick · Tab: complete · Enter: confirm · Esc: cancel"
            }
            PromptState::MultiSelect {
                items,
                cursor,
                checked,
            } => {
                let all: Vec<usize> = (0..items.len()).collect();
                lines.extend(Self::list_lines(
                    items,
                    &all,
                    Some(*cursor),
                    Some(checked),
                    theme,
                ));
                "Space: toggle · Enter: confirm · Esc: cancel"
            }
        };
        match &self.error {
            Some(error) => lines.push(Line::from(Span::styled(error.clone(), theme.err))),
            None => lines.push(hint_line(hint, theme)),
        }
        draw_box(frame, area, &self.title, lines, theme);
    }
}

// ── Notice ───────────────────────────────────────────────────────────────

/// A message the user must dismiss: an error, or the conflict-pause notice.
pub(crate) struct Notice {
    title: String,
    level: Level,
    text: String,
}

impl Notice {
    pub fn new(title: &str, level: Level, text: &str) -> Self {
        Notice {
            title: title.to_string(),
            level,
            text: text.to_string(),
        }
    }

    /// Enter, Esc, or `q` dismiss it.
    pub fn dismisses(code: KeyCode) -> bool {
        matches!(code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q'))
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &TuiTheme) {
        let mut lines = message_lines(self.level, &self.text, theme);
        lines.push(Line::from(""));
        lines.push(hint_line("Enter: dismiss", theme));
        draw_box(frame, area, &self.title, lines, theme);
    }
}

// ── Log ──────────────────────────────────────────────────────────────────

/// One action in the TUI log: the equivalent command line and the message
/// lines it produced.
pub(crate) struct LogEntry {
    pub command: String,
    pub lines: Vec<(Level, String)>,
}

/// Draw the log over `area`; `scroll` persists across frames.
pub(crate) fn render_log(
    frame: &mut Frame,
    area: Rect,
    entries: &[LogEntry],
    scroll: &mut DiffPane,
    theme: &TuiTheme,
) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(vec![
            Span::styled("$ ", theme.hint),
            Span::styled(
                entry.command.clone(),
                theme.message.add_modifier(Modifier::BOLD),
            ),
        ]));
        for (level, text) in &entry.lines {
            lines.extend(message_lines(*level, text, theme));
        }
    }
    if lines.is_empty() {
        lines.push(hint_line("no actions yet", theme));
    }
    let rect = centered(area, area.width * 4 / 5, area.height * 4 / 5);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .title(" Log — L or Esc to close ")
        .borders(Borders::ALL)
        .border_style(theme.border_active);
    scroll.render(frame, rect, lines, block);
}

/// One group of the help popup: a header and its `(keys, effect)` rows.
pub(crate) type HelpSection = (&'static str, &'static [(&'static str, &'static str)]);

/// Draw the key help over `area`; `scroll` persists across frames.
pub(crate) fn render_help(
    frame: &mut Frame,
    area: Rect,
    sections: &[HelpSection],
    scroll: &mut DiffPane,
    theme: &TuiTheme,
) {
    let width = sections
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|(keys, _)| keys.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, (header, rows)) in sections.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            header.to_string(),
            theme.message.add_modifier(Modifier::BOLD),
        )));
        for (keys, effect) in rows.iter() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<width$}  ", keys), theme.highlight),
                Span::styled(effect.to_string(), theme.message),
            ]));
        }
    }
    let rect = centered(area, area.width * 4 / 5, area.height * 4 / 5);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .title(" Help — ? or Esc to close ")
        .borders(Borders::ALL)
        .border_style(theme.border_active);
    scroll.render(frame, rect, lines, block);
}

#[cfg(test)]
#[path = "popup_test.rs"]
mod tests;
