//! Popup-style single-select picker.
//!
//! Owns its own scroll state, filter buffer, and key handling. The
//! caller passes a flat `Vec<PickerRow>` + initial selection; the picker
//! returns the confirmed *original* index (i.e., the index into the
//! caller's source list, not the index in the filtered view).
//!
//! Keys handled:
//! - `Up` / `Down` / `Ctrl+P` / `Ctrl+N` — move highlight.
//! - `Home` / `End`                       — jump to first / last.
//! - `Enter`                              — confirm selection.
//! - `Esc`                                — cancel.
//! - any other printable char             — append to filter.
//! - `Backspace`                          — pop one char from filter.
//!
//! Rendering centers the popup on the supplied frame area.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

#[derive(Debug, Clone)]
pub struct PickerRow {
    /// Primary label rendered in the list.
    pub label: String,
    /// Optional secondary text rendered dim after the label.
    pub hint: Option<String>,
}

impl PickerRow {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

#[derive(Debug)]
pub struct Picker {
    title: String,
    rows: Vec<PickerRow>,
    state: ListState,
    filter: String,
    /// Indices into `rows` that pass the current filter, in display order.
    filtered: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerOutcome {
    /// Key consumed by the picker but no terminal action.
    None,
    /// User pressed Enter on a row; payload is the *original* index.
    Confirmed(usize),
    /// User pressed Esc.
    Cancelled,
}

impl Picker {
    pub fn new(title: impl Into<String>, rows: Vec<PickerRow>, initial: usize) -> Self {
        let mut p = Self {
            title: title.into(),
            rows,
            state: ListState::default(),
            filter: String::new(),
            filtered: Vec::new(),
        };
        p.apply_filter();
        p.select_original(initial);
        p
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Set highlight to the row whose original index is `original_idx`.
    /// Falls back to the first filtered row if the target was filtered out.
    fn select_original(&mut self, original_idx: usize) {
        if let Some(pos) = self.filtered.iter().position(|&i| i == original_idx) {
            self.state.select(Some(pos));
        } else if !self.filtered.is_empty() {
            self.state.select(Some(0));
        } else {
            self.state.select(None);
        }
    }

    fn apply_filter(&mut self) {
        let q = self.filter.to_ascii_lowercase();
        self.filtered = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                if q.is_empty() {
                    return true;
                }
                let label = row.label.to_ascii_lowercase();
                let hint = row.hint.as_deref().unwrap_or("").to_ascii_lowercase();
                label.contains(&q) || hint.contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        if self.filtered.is_empty() {
            self.state.select(None);
        } else if let Some(s) = self.state.selected() {
            if s >= self.filtered.len() {
                self.state.select(Some(self.filtered.len() - 1));
            }
        } else {
            self.state.select(Some(0));
        }
    }

    fn move_by(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len() as isize;
        let cur = self.state.selected().unwrap_or(0) as isize;
        let next = ((cur + delta).rem_euclid(n)) as usize;
        self.state.select(Some(next));
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> PickerOutcome {
        // Defence in depth: callers (the TUI's main event loop) already
        // filter to Press, but on terminals with full key reporting (e.g.
        // kitty / Windows-style) Press + Release pairs can otherwise
        // double-count and look like a single tap moved twice — or, worse,
        // make a no-op event undo a move on the next frame.
        if key.kind != KeyEventKind::Press {
            return PickerOutcome::None;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => PickerOutcome::Cancelled,
            (KeyCode::Enter, _) => match self.state.selected() {
                Some(view_idx) => match self.filtered.get(view_idx) {
                    Some(&orig) => PickerOutcome::Confirmed(orig),
                    None => PickerOutcome::None,
                },
                None => PickerOutcome::None,
            },
            (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.move_by(-1);
                PickerOutcome::None
            }
            (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.move_by(1);
                PickerOutcome::None
            }
            (KeyCode::Home, _) => {
                if !self.filtered.is_empty() {
                    self.state.select(Some(0));
                }
                PickerOutcome::None
            }
            (KeyCode::End, _) => {
                if !self.filtered.is_empty() {
                    self.state.select(Some(self.filtered.len() - 1));
                }
                PickerOutcome::None
            }
            (KeyCode::Backspace, _) => {
                self.filter.pop();
                self.apply_filter();
                PickerOutcome::None
            }
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.apply_filter();
                PickerOutcome::None
            }
            _ => PickerOutcome::None,
        }
    }

    /// Render a centred popup over the supplied frame area. Caller draws
    /// the rest of the form behind it; the popup clears its own footprint
    /// before drawing so the underlying widgets bleed through cleanly.
    pub fn render(&mut self, f: &mut ratatui::Frame<'_>, area: Rect) {
        let popup = centered(area, 60, 60);
        f.render_widget(Clear, popup);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // filter / hint
                Constraint::Min(3),    // list
            ])
            .split(popup);

        let title = format!(
            " {} ({} of {}) ",
            self.title,
            self.filtered.len(),
            self.rows.len()
        );
        let filter_line = if self.filter.is_empty() {
            Line::from(Span::styled(
                "type to filter — ↑/↓ move — Enter select — Esc cancel",
                Style::default().add_modifier(Modifier::DIM),
            ))
        } else {
            Line::from(vec![
                Span::raw("filter: "),
                Span::styled(self.filter.clone(), Style::default().fg(Color::Yellow)),
            ])
        };
        let header = Paragraph::new(filter_line).block(
            Block::default()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .title(title.clone()),
        );
        f.render_widget(header, chunks[0]);

        let items: Vec<ListItem<'_>> = self
            .filtered
            .iter()
            .map(|&orig| {
                let row = &self.rows[orig];
                let mut spans = vec![Span::raw(row.label.clone())];
                if let Some(hint) = &row.hint {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        hint.clone(),
                        Style::default().add_modifier(Modifier::DIM),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT))
            // Leading symbol + REVERSED + BOLD so the cursor is
            // unambiguous on every terminal theme — REVERSED-only is
            // hard to spot against some palettes.
            .highlight_symbol("▶ ")
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD),
            );
        f.render_stateful_widget(list, chunks[1], &mut self.state);
    }
}

/// Compute a centered sub-rect taking up `pct_w` × `pct_h` of `area`.
fn centered(area: Rect, pct_w: u16, pct_h: u16) -> Rect {
    let h = (area.height * pct_h / 100).clamp(8, 24);
    let w = (area.width * pct_w / 100).clamp(40, 100);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<PickerRow> {
        (0..n)
            .map(|i| PickerRow::new(format!("item-{i}")).with_hint(format!("hint-{i}")))
            .collect()
    }

    #[test]
    fn empty_picker_returns_none_on_enter() {
        let mut p = Picker::new("t", Vec::new(), 0);
        let out = p.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(out, PickerOutcome::None);
    }

    #[test]
    fn enter_confirms_selected_original_index() {
        let mut p = Picker::new("t", rows(3), 1);
        let out = p.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(out, PickerOutcome::Confirmed(1));
    }

    #[test]
    fn down_moves_selection_with_wrap() {
        let mut p = Picker::new("t", rows(3), 2);
        p.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        // wrapped from 2 → 0
        let out = p.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(out, PickerOutcome::Confirmed(0));
    }

    #[test]
    fn typing_filters_and_enter_returns_original_index() {
        let mut p = Picker::new("t", rows(5), 0);
        p.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        // filter narrows to "item-3" (original index 3)
        let out = p.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(out, PickerOutcome::Confirmed(3));
    }

    #[test]
    fn esc_cancels() {
        let mut p = Picker::new("t", rows(2), 0);
        let out = p.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(out, PickerOutcome::Cancelled);
    }

    #[test]
    fn filter_with_no_matches_yields_no_confirm() {
        let mut p = Picker::new("t", rows(3), 0);
        for c in "zzz".chars() {
            p.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let out = p.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(out, PickerOutcome::None);
    }

    #[test]
    fn backspace_restores_filter() {
        let mut p = Picker::new("t", rows(3), 0);
        for c in "z3".chars() {
            p.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        // "z3" matches nothing
        // backspace clears '3', leaving "z" — still no match
        p.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        // backspace clears 'z', leaving "" — all rows match
        p.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        let out = p.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        // selection re-anchored to filtered[0] = original 0
        assert_eq!(out, PickerOutcome::Confirmed(0));
    }
}
