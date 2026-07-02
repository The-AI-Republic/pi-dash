//! Deprecated token-compatibility add-runner modal.
//!
//! New runners should be added with `pidash runner add`. This modal is
//! kept only for existing one-time enrollment/revive tokens.
//!
//! Three fields: enrollment token (masked), host label (defaults to
//! hostname), working_dir, Submit. The cloud assigns workspace +
//! project from the one-time token, so no client-side project picker.
//! Text fields are real `TextArea`s so digits / `h` / `l` cannot escape
//! into tab switches while editing (Bug-3 invariant carries to the
//! modal too).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::tui::app::AppData;
use crate::tui::event::AppEvent;
use crate::tui::render::Renderable;
use crate::tui::view::{Cancellation, KeyHandled, View, ViewCompletion, ViewCtx};
use crate::tui::widgets::TextArea;

use super::confirm::centered_rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Token = 0,
    HostLabel = 1,
    WorkingDir = 2,
    Submit = 3,
}

impl Focus {
    fn from_idx(i: u8) -> Self {
        match i {
            0 => Self::Token,
            1 => Self::HostLabel,
            2 => Self::WorkingDir,
            _ => Self::Submit,
        }
    }
    fn idx(self) -> u8 {
        self as u8
    }
}

pub struct AddRunnerView {
    token: TextArea,
    host_label: TextArea,
    working_dir: TextArea,
    focus: Focus,
    busy: bool,
    error: Option<String>,
    complete: bool,
    completion: Option<ViewCompletion>,
}

impl AddRunnerView {
    pub fn open(data: &AppData) -> Self {
        Self {
            token: TextArea::new()
                .masked(true)
                .placeholder("paste enrollment token"),
            host_label: TextArea::with_text(default_host_label()),
            working_dir: TextArea::with_text(default_working_dir(data)),
            focus: Focus::Token,
            busy: false,
            error: None,
            complete: false,
            completion: None,
        }
    }

    fn focused_textarea_mut(&mut self) -> Option<&mut TextArea> {
        match self.focus {
            Focus::Token => Some(&mut self.token),
            Focus::HostLabel => Some(&mut self.host_label),
            Focus::WorkingDir => Some(&mut self.working_dir),
            Focus::Submit => None,
        }
    }

    fn advance_focus(&mut self, forward: bool) {
        let n = 4u8;
        let i = self.focus.idx();
        self.focus = Focus::from_idx(if forward {
            (i + 1) % n
        } else if i == 0 {
            n - 1
        } else {
            i - 1
        });
    }

    fn submit(&mut self, ctx: &mut ViewCtx<'_>) {
        let token = self.token.text().trim().to_string();
        let host_label = self.host_label.text().trim().to_string();
        let working_dir = self.working_dir.text().trim().to_string();
        if token.is_empty() {
            self.error = Some("enrollment token is required".into());
            return;
        }
        if host_label.is_empty() {
            self.error = Some("host label is required".into());
            return;
        }
        if working_dir.is_empty() {
            self.error = Some("working_dir is required".into());
            return;
        }
        let working_dir = std::path::PathBuf::from(working_dir);

        self.busy = true;
        self.error = None;

        let paths = ctx.paths.clone();
        let tx = ctx.tx.clone();
        tokio::spawn(async move {
            let outcome = match crate::cli::connect::enroll_additional_runner(
                &paths,
                &token,
                &host_label,
                Some(working_dir),
            )
            .await
            {
                Ok(_runner) => crate::service::reload::restart_and_verify(&paths).await,
                Err(e) => crate::service::reload::ReloadOutcome {
                    ok: false,
                    summary: "add runner failed".into(),
                    detail: Some(e.to_string()),
                    service_state: "unknown".into(),
                },
            };
            tx.send(AppEvent::ReloadOutcomeUpdated(outcome));
            tx.send(AppEvent::PopView);
        });
    }
}

impl Renderable for AddRunnerView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let modal = centered_rect(72, 55, area);
        Clear.render(modal, buf);

        let mut lines: Vec<Line<'_>> = vec![
            Line::from(Span::styled(
                "Legacy token enrollment",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Prefer `pidash runner add --project <PROJECT>` outside the TUI.",
                Style::default().add_modifier(Modifier::DIM),
            )),
            Line::from(Span::styled(
                "Paste a token here only for compatibility or revive flows.",
                Style::default().add_modifier(Modifier::DIM),
            )),
            Line::raw(""),
            field_line(
                "Token       ",
                &mask_token(self.token.text()),
                self.focus == Focus::Token,
            ),
            field_line(
                "Host label  ",
                self.host_label.text(),
                self.focus == Focus::HostLabel,
            ),
            field_line(
                "Working dir ",
                self.working_dir.text(),
                self.focus == Focus::WorkingDir,
            ),
            Line::raw(""),
        ];
        let submit_label = if self.busy { " Adding… " } else { " Submit " };
        let submit_style = if self.focus == Focus::Submit {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        };
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(submit_label.to_string(), submit_style),
            Span::raw("   "),
            Span::styled("Esc cancel", Style::default().add_modifier(Modifier::DIM)),
        ]));
        if let Some(e) = &self.error {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                e.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        let p = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Add runner "),
            )
            .wrap(Wrap { trim: false });
        p.render(modal, buf);
    }
}

impl View for AddRunnerView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled {
        // Layer 2: focused textarea sees the key.
        if let Some(area) = self.focused_textarea_mut() {
            let h = area.handle_key(key);
            if matches!(h, KeyHandled::Consumed) {
                return KeyHandled::Consumed;
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                KeyHandled::NotConsumed
            }
            (KeyCode::Esc, _) => {
                self.complete = true;
                self.completion = Some(ViewCompletion::Cancelled);
                KeyHandled::Consumed
            }
            (KeyCode::Up, _) | (KeyCode::BackTab, _) => {
                self.advance_focus(false);
                KeyHandled::Consumed
            }
            (KeyCode::Down, _) | (KeyCode::Tab, _) => {
                self.advance_focus(true);
                KeyHandled::Consumed
            }
            (KeyCode::Enter, _) => {
                match self.focus {
                    Focus::Submit => self.submit(ctx),
                    _ => self.advance_focus(true),
                }
                KeyHandled::Consumed
            }
            _ => KeyHandled::Consumed,
        }
    }

    fn handle_paste(&mut self, text: String, _ctx: &mut ViewCtx<'_>) -> KeyHandled {
        // Route the paste to the focused field. The token is the whole
        // reason this modal exists ("paste enrollment token"), so pasting
        // must land in whichever TextArea currently has focus.
        if let Some(area) = self.focused_textarea_mut() {
            area.insert_str(&text);
            return KeyHandled::Consumed;
        }
        KeyHandled::NotConsumed
    }

    fn is_complete(&self) -> bool {
        self.complete
    }
    fn completion(&self) -> Option<ViewCompletion> {
        self.completion
    }
    fn on_ctrl_c(&mut self, _ctx: &mut ViewCtx<'_>) -> Cancellation {
        Cancellation::NotHandled
    }
}

fn field_line(label: &str, value: &str, focused: bool) -> Line<'static> {
    let marker = if focused { "▶" } else { " " };
    let cursor = if focused { "▊" } else { "" };
    let value_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    Line::from(vec![
        Span::styled(
            format!(" {marker} "),
            Style::default()
                .fg(if focused {
                    Color::Cyan
                } else {
                    Color::DarkGray
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{} ", label)),
        Span::styled(format!("{value}{cursor}"), value_style),
    ])
}

fn default_working_dir(data: &AppData) -> String {
    if let Some(cfg) = data.config_working.as_ref()
        && let Some(primary) = cfg.runners.first()
        && let Some(parent) = primary.workspace.working_dir.parent()
    {
        return parent.join("runner-new").display().to_string();
    }
    data.paths
        .default_working_dir()
        .join("runner-new")
        .display()
        .to_string()
}

fn default_host_label() -> String {
    crate::util::hostname::default_hostname()
}

fn mask_token(raw: &str) -> String {
    if raw.is_empty() {
        String::new()
    } else if raw.len() <= 4 {
        "*".repeat(raw.len())
    } else {
        format!("{}…{}", &raw[..2], &raw[raw.len() - 2..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::event::AppEvent;
    use crate::tui::event_sender::AppEventSender;
    use crate::tui::input::keymap::KeymapRegistry;
    use crate::util::paths::Paths;
    use tokio::sync::mpsc;

    fn paths() -> Paths {
        let root = tempfile::tempdir().expect("tempdir").keep();
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
        }
    }

    fn view() -> AddRunnerView {
        let data = AppData::new(paths());
        AddRunnerView::open(&data)
    }

    fn with_ctx(f: impl FnOnce(&mut ViewCtx<'_>)) {
        let (tx, _rx) = mpsc::unbounded_channel::<AppEvent>();
        let sender = AppEventSender::new(tx);
        let keymap = KeymapRegistry::new();
        let p = paths();
        let mut ctx = ViewCtx {
            tx: &sender,
            keymap: &keymap,
            paths: &p,
        };
        f(&mut ctx);
    }

    #[test]
    fn paste_lands_in_focused_token_field() {
        let mut v = view();
        // Default focus is the token field — the whole point of this modal.
        with_ctx(|ctx| {
            let h = v.handle_paste("enroll-tok-123".into(), ctx);
            assert_eq!(h, KeyHandled::Consumed);
        });
        assert_eq!(v.token.text(), "enroll-tok-123");
    }

    #[test]
    fn paste_follows_focus_to_other_fields() {
        let mut v = view();
        v.focus = Focus::HostLabel;
        v.host_label.clear();
        with_ctx(|ctx| {
            assert_eq!(v.handle_paste("mac-mini".into(), ctx), KeyHandled::Consumed);
        });
        assert_eq!(v.host_label.text(), "mac-mini");
        assert_eq!(v.token.text(), "");
    }

    #[test]
    fn paste_ignored_when_submit_focused() {
        let mut v = view();
        v.focus = Focus::Submit;
        with_ctx(|ctx| {
            assert_eq!(v.handle_paste("nope".into(), ctx), KeyHandled::NotConsumed);
        });
        assert!(v.token.text().is_empty());
    }
}
