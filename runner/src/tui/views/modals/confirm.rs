//! Confirm dialogs — exit and stop-daemon.
//!
//! Both are tiny `View` impls that listen for y/n/Esc/Enter and post
//! a single `AppEvent` on accept. Esc and `n` cancel.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::tui::event::AppEvent;
use crate::tui::render::Renderable;
use crate::tui::view::{Cancellation, KeyHandled, View, ViewCompletion, ViewCtx};

pub struct ConfirmExitView {
    yes_selected: bool,
    complete: bool,
    completion: Option<ViewCompletion>,
}

impl ConfirmExitView {
    pub fn new() -> Self {
        Self {
            yes_selected: true,
            complete: false,
            completion: None,
        }
    }
}

impl Default for ConfirmExitView {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for ConfirmExitView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let modal = centered_rect(40, 20, area);
        Clear.render(modal, buf);
        let sel = Style::default().add_modifier(Modifier::REVERSED);
        let unsel = Style::default().add_modifier(Modifier::DIM);
        let (yes_style, no_style) = if self.yes_selected {
            (sel, unsel)
        } else {
            (unsel, sel)
        };
        let body = Paragraph::new(vec![
            Line::from("Are you sure to exit?"),
            Line::raw(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(" Yes ", yes_style),
                Span::raw("    "),
                Span::styled(" No ", no_style),
            ]),
            Line::raw(""),
            Line::from("↵ confirm   ←/→ switch   y / n / Esc"),
        ])
        .block(Block::default().borders(Borders::ALL).title(" Exit "));
        body.render(modal, buf);
    }
}

impl View for ConfirmExitView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                ctx.tx.send(AppEvent::Quit);
                self.complete = true;
                self.completion = Some(ViewCompletion::Accepted);
            }
            KeyCode::Enter => {
                if self.yes_selected {
                    ctx.tx.send(AppEvent::Quit);
                    self.complete = true;
                    self.completion = Some(ViewCompletion::Accepted);
                } else {
                    self.complete = true;
                    self.completion = Some(ViewCompletion::Cancelled);
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.complete = true;
                self.completion = Some(ViewCompletion::Cancelled);
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('h')
            | KeyCode::Char('l') => {
                self.yes_selected = !self.yes_selected;
            }
            _ => {}
        }
        KeyHandled::Consumed
    }

    fn is_complete(&self) -> bool {
        self.complete
    }
    fn completion(&self) -> Option<ViewCompletion> {
        self.completion
    }
    fn on_ctrl_c(&mut self, ctx: &mut ViewCtx<'_>) -> Cancellation {
        ctx.tx.send(AppEvent::Quit);
        Cancellation::Handled
    }
}

pub struct ConfirmStopView {
    complete: bool,
    completion: Option<ViewCompletion>,
}

impl ConfirmStopView {
    pub fn new() -> Self {
        Self {
            complete: false,
            completion: None,
        }
    }
}

impl Default for ConfirmStopView {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for ConfirmStopView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let modal = centered_rect(40, 20, area);
        Clear.render(modal, buf);
        let body = Paragraph::new(vec![
            Line::from("Stop the runner daemon?"),
            Line::raw(""),
            Line::from("Any active run will be cancelled."),
            Line::raw(""),
            Line::from("[y] yes     [any other key] cancel"),
        ])
        .block(Block::default().borders(Borders::ALL).title(" Confirm "));
        body.render(modal, buf);
    }
}

impl View for ConfirmStopView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Ask the daemon to disconnect, then quit.
                let ipc = crate::tui::ipc_client::TuiIpc {
                    socket: ctx.paths.ipc_socket_path(),
                    selected_runner: None,
                };
                let tx = ctx.tx.clone();
                tokio::spawn(async move {
                    let _ = ipc
                        .decide("__stop__", crate::cloud::protocol::ApprovalDecision::Accept)
                        .await
                        .ok();
                    tx.send(AppEvent::Quit);
                });
                self.complete = true;
                self.completion = Some(ViewCompletion::Accepted);
            }
            _ => {
                self.complete = true;
                self.completion = Some(ViewCompletion::Cancelled);
            }
        }
        KeyHandled::Consumed
    }

    fn is_complete(&self) -> bool {
        self.complete
    }
    fn completion(&self) -> Option<ViewCompletion> {
        self.completion
    }
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup)[1]
}
