//! Choice menu, the default shape of a question in `loom tui`: a titled list
//! of actions where the highlighted one is a full-width bar, `Enter` picks
//! it and `Esc` always leaves. A help text for the highlighted item, when
//! there is one, sits in a box below.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::tui::theme::TuiTheme;
use crate::tui::widgets::popup::highlight_backticks;

/// Default width, like lazygit's menus; narrower terminals shrink it.
const WIDTH: u16 = 80;

pub(crate) struct MenuItem {
    pub label: String,
    /// Shortcut shown in the left gutter; pressing it picks the item.
    pub key: Option<char>,
    /// Shown below the menu while the item is highlighted; may span lines.
    pub help: Option<String>,
}

impl MenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        MenuItem {
            label: label.into(),
            key: None,
            help: None,
        }
    }

    #[allow(dead_code)] // for the menus that replace the other prompts
    pub fn key(mut self, key: char) -> Self {
        self.key = Some(key);
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

pub(crate) enum MenuOutcome {
    Pending,
    /// Index of the picked item.
    Choose(usize),
    Cancel,
}

pub(crate) struct Menu {
    title: String,
    items: Vec<MenuItem>,
    cursor: usize,
}

impl Menu {
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Menu {
            title: title.into(),
            items,
            cursor: 0,
        }
    }

    #[cfg(test)]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> MenuOutcome {
        match code {
            KeyCode::Esc => return MenuOutcome::Cancel,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                return MenuOutcome::Cancel;
            }
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(self.items.len().saturating_sub(1));
            }
            KeyCode::Enter if !self.items.is_empty() => return MenuOutcome::Choose(self.cursor),
            KeyCode::Char(c) => {
                if let Some(i) = self.items.iter().position(|item| item.key == Some(c)) {
                    return MenuOutcome::Choose(i);
                }
            }
            _ => {}
        }
        MenuOutcome::Pending
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &TuiTheme) {
        let width = WIDTH.min(area.width.saturating_sub(2)).max(20);
        let inner = width.saturating_sub(2) as usize;
        let has_key = self.items.iter().any(|i| i.key.is_some());
        let help: Vec<Line<'static>> = self
            .items
            .get(self.cursor)
            .and_then(|i| i.help.as_deref())
            .map(|help| {
                help.lines()
                    .map(|l| Line::from(highlight_backticks(l, Style::default(), theme)))
                    .collect()
            })
            .unwrap_or_default();

        // The menu alone is centered, so it stays put whether or not the
        // cursor item has help; the help box hangs below it.
        let rows = (self.items.len() as u16 + 2).min(area.height);
        let top = area.y + area.height.saturating_sub(rows) / 2;
        let help_rows = if help.is_empty() {
            0
        } else {
            (help.len() as u16 + 2).min(area.bottom().saturating_sub(top + rows))
        };
        let x = area.x + (area.width - width) / 2;
        let menu_rect = Rect {
            x,
            y: top,
            width,
            height: rows,
        };

        let lines: Vec<Line<'static>> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let base = if i == self.cursor {
                    theme.file_selected.add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let gutter = match (has_key, item.key) {
                    (true, Some(k)) => format!("{} ", k),
                    (true, None) => "  ".to_string(),
                    (false, _) => " ".to_string(),
                };
                let mut spans = vec![Span::styled(gutter, base.patch(theme.hint))];
                spans.extend(highlight_backticks(&item.label, base, theme));
                let used: usize = spans.iter().map(|s| s.width()).sum();
                spans.push(Span::styled(" ".repeat(inner.saturating_sub(used)), base));
                Line::from(spans)
            })
            .collect();

        frame.render_widget(Clear, menu_rect);
        let counter = format!(" {} of {} ", self.cursor + 1, self.items.len());
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .title_bottom(Line::from(counter).right_aligned())
            .borders(Borders::ALL)
            .border_style(theme.border_active);
        frame.render_widget(Paragraph::new(lines).block(block), menu_rect);

        if help_rows > 0 {
            let help_rect = Rect {
                x,
                y: top + rows,
                width,
                height: help_rows,
            };
            frame.render_widget(Clear, help_rect);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border);
            frame.render_widget(Paragraph::new(help).block(block), help_rect);
        }
    }
}

#[cfg(test)]
#[path = "menu_test.rs"]
mod tests;
