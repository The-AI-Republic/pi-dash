//! Single-line text editor.
//!
//! Owns text + cursor. Inserts characters; deletes backwards; moves
//! cursor with Left/Right/Home/End. Filters `KeyEventKind::Release`
//! at its own boundary (defense in depth — the event stream already
//! filters too).
//!
//! This is the leaf widget that fixes Bug 3: forms own one TextArea
//! per field; they never have their own `Char(c)` arm. When a
//! TextArea is the focused child, the dispatcher's input pipeline
//! returns `Consumed` for any printable key, so digit / `h` / `l`
//! keys cannot be reinterpreted by the global keymap.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::super::view::KeyHandled;

#[derive(Debug, Clone, Default)]
pub struct TextArea {
    text: String,
    cursor: usize,
    /// Optional placeholder shown in dim style when `text` is empty.
    placeholder: String,
    /// If true, render character bullets instead of plaintext (token field).
    masked: bool,
}

impl TextArea {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self {
            text,
            cursor,
            placeholder: String::new(),
            masked: false,
        }
    }

    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn masked(mut self, on: bool) -> Self {
        self.masked = on;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, t: impl Into<String>) {
        self.text = t.into();
        self.cursor = self.text.chars().count();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Hand a key event to the textarea. Returns `Consumed` for keys
    /// that produce edits or cursor moves; `NotConsumed` for keys
    /// the textarea doesn't claim (Tab, Esc, Enter, Up/Down — those
    /// belong to the surrounding form).
    pub fn handle_key(&mut self, key: KeyEvent) -> KeyHandled {
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return KeyHandled::Consumed;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char(c), m)
                if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
            {
                self.insert_char(c);
                KeyHandled::Consumed
            }
            (KeyCode::Backspace, _) => {
                self.delete_backward();
                KeyHandled::Consumed
            }
            (KeyCode::Delete, _) => {
                self.delete_forward();
                KeyHandled::Consumed
            }
            (KeyCode::Left, _) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                KeyHandled::Consumed
            }
            (KeyCode::Right, _) => {
                let max = self.text.chars().count();
                if self.cursor < max {
                    self.cursor += 1;
                }
                KeyHandled::Consumed
            }
            (KeyCode::Home, _) => {
                self.cursor = 0;
                KeyHandled::Consumed
            }
            (KeyCode::End, _) => {
                self.cursor = self.text.chars().count();
                KeyHandled::Consumed
            }
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.cursor = 0;
                KeyHandled::Consumed
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.cursor = self.text.chars().count();
                KeyHandled::Consumed
            }
            _ => KeyHandled::NotConsumed,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let byte = self.byte_index(self.cursor);
        self.text.insert(byte, c);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        let byte = self.byte_index(self.cursor);
        self.text.insert_str(byte, s);
        self.cursor += s.chars().count();
    }

    pub fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let end = self.byte_index(self.cursor);
        let start = self.byte_index(self.cursor - 1);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete_forward(&mut self) {
        let max = self.text.chars().count();
        if self.cursor >= max {
            return;
        }
        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    fn byte_index(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    /// Render the textarea at `area` using a single-line layout. When
    /// `focused` is true, draws a cursor-block at the cursor position
    /// and bolds the value; otherwise renders dim. Caller is
    /// responsible for the surrounding label / border.
    pub fn render(&self, area: Rect, buf: &mut Buffer, focused: bool) {
        let display = if self.masked {
            "*".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        };
        let value_style = if focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if self.text.is_empty() {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(Color::White)
        };
        let body = if self.text.is_empty() && !focused {
            Span::styled(self.placeholder.clone(), Style::default().add_modifier(Modifier::DIM))
        } else {
            Span::styled(display.clone(), value_style)
        };
        let cursor_glyph = if focused { "▊" } else { "" };
        let line = Line::from(vec![body, Span::styled(cursor_glyph.to_string(), value_style)]);
        Paragraph::new(line).render_ref(area, buf);
    }
}

// Helper trait so we can call render_ref consistently via Paragraph.
trait RenderRef {
    fn render_ref(&self, area: Rect, buf: &mut Buffer);
}

impl RenderRef for Paragraph<'_> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        ratatui::widgets::Widget::render(self.clone(), area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn insert_and_delete_chars() {
        let mut t = TextArea::new();
        t.insert_char('h');
        t.insert_char('i');
        assert_eq!(t.text(), "hi");
        t.delete_backward();
        assert_eq!(t.text(), "h");
        assert_eq!(t.cursor(), 1);
    }

    #[test]
    fn handle_key_typing_consumed() {
        let mut t = TextArea::new();
        let h = t.handle_key(ev('a'));
        assert_eq!(h, KeyHandled::Consumed);
        assert_eq!(t.text(), "a");
    }

    #[test]
    fn release_events_swallowed_at_boundary() {
        let mut t = TextArea::new();
        let release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        // Force release kind:
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..release
        };
        let h = t.handle_key(release);
        // Release is filtered (consumed silently — not double-typed).
        assert_eq!(h, KeyHandled::Consumed);
        assert_eq!(t.text(), "");
    }

    #[test]
    fn enter_not_consumed_so_form_can_act() {
        let mut t = TextArea::new();
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(t.handle_key(enter), KeyHandled::NotConsumed);
    }

    #[test]
    fn cursor_moves_left_right() {
        let mut t = TextArea::with_text("abc");
        assert_eq!(t.cursor(), 3);
        t.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(t.cursor(), 2);
        t.insert_char('X');
        assert_eq!(t.text(), "abXc");
        assert_eq!(t.cursor(), 3);
    }
}
