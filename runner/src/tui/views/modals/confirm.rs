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

/// Which option the cursor is on. Two layouts share the enum so the
/// render and key paths don't have to fork on `dirty` for selection
/// state — they only fork on which choices are *visible*. The
/// non-dirty layout uses just `SaveExit` (= "Yes") and `Cancel`
/// (= "No").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitChoice {
    /// Save unsaved edits then quit. In the clean layout this slot is
    /// repurposed as plain "Yes, exit" since there's nothing to save.
    SaveExit,
    /// Discard unsaved edits and quit. Hidden in the clean layout.
    DiscardExit,
    /// Stay in the TUI.
    Cancel,
}

pub struct ConfirmExitView {
    /// True when `config_working` differs from `config_loaded`. Drives
    /// the 3-option vs 2-option branching in render / key handling.
    dirty: bool,
    selected: ExitChoice,
    complete: bool,
    completion: Option<ViewCompletion>,
}

impl ConfirmExitView {
    pub fn new(dirty: bool) -> Self {
        Self {
            dirty,
            // Default to the safest choice in each layout: Save & Exit
            // when dirty (preserves edits), plain Yes-Exit when clean.
            selected: ExitChoice::SaveExit,
            complete: false,
            completion: None,
        }
    }

    fn confirm(&mut self, ctx: &mut ViewCtx<'_>) {
        match self.selected {
            ExitChoice::SaveExit if self.dirty => {
                ctx.tx.send(AppEvent::SaveAndQuit);
                self.complete = true;
                self.completion = Some(ViewCompletion::Accepted);
            }
            ExitChoice::SaveExit => {
                // Clean layout: this slot is plain "exit."
                ctx.tx.send(AppEvent::Quit);
                self.complete = true;
                self.completion = Some(ViewCompletion::Accepted);
            }
            ExitChoice::DiscardExit => {
                ctx.tx.send(AppEvent::Quit);
                self.complete = true;
                self.completion = Some(ViewCompletion::Accepted);
            }
            ExitChoice::Cancel => {
                self.complete = true;
                self.completion = Some(ViewCompletion::Cancelled);
            }
        }
    }

    fn cycle(&mut self, forward: bool) {
        let order: &[ExitChoice] = if self.dirty {
            &[
                ExitChoice::SaveExit,
                ExitChoice::DiscardExit,
                ExitChoice::Cancel,
            ]
        } else {
            &[ExitChoice::SaveExit, ExitChoice::Cancel]
        };
        let cur = order
            .iter()
            .position(|c| *c == self.selected)
            .unwrap_or(0);
        let next = if forward {
            (cur + 1) % order.len()
        } else {
            (cur + order.len() - 1) % order.len()
        };
        self.selected = order[next];
    }
}

impl Renderable for ConfirmExitView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let (w, h) = if self.dirty { (60, 25) } else { (40, 20) };
        let modal = centered_rect(w, h, area);
        Clear.render(modal, buf);
        let sel = Style::default().add_modifier(Modifier::REVERSED);
        let unsel = Style::default().add_modifier(Modifier::DIM);
        let style_for = |choice: ExitChoice| -> Style {
            if self.selected == choice { sel } else { unsel }
        };

        let lines: Vec<Line<'_>> = if self.dirty {
            vec![
                Line::from(Span::styled(
                    "Unsaved configuration changes",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::raw(""),
                Line::from("You have edits that haven't been written to disk."),
                Line::raw(""),
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(" Save & Exit ", style_for(ExitChoice::SaveExit)),
                    Span::raw("  "),
                    Span::styled(" Discard & Exit ", style_for(ExitChoice::DiscardExit)),
                    Span::raw("  "),
                    Span::styled(" Cancel ", style_for(ExitChoice::Cancel)),
                ]),
                Line::raw(""),
                Line::from("↵ confirm   ←/→ switch   s save   d discard   Esc cancel"),
            ]
        } else {
            vec![
                Line::from("Are you sure to exit?"),
                Line::raw(""),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(" Yes ", style_for(ExitChoice::SaveExit)),
                    Span::raw("    "),
                    Span::styled(" No ", style_for(ExitChoice::Cancel)),
                ]),
                Line::raw(""),
                Line::from("↵ confirm   ←/→ switch   y / n / Esc"),
            ]
        };

        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Exit "))
            .render(modal, buf);
    }
}

impl View for ConfirmExitView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled {
        match key.code {
            // Direct hotkeys — work in both layouts. `s` and `d` are
            // inert in the clean layout (no save / discard concept).
            KeyCode::Char('s') | KeyCode::Char('S') if self.dirty => {
                self.selected = ExitChoice::SaveExit;
                self.confirm(ctx);
            }
            KeyCode::Char('d') | KeyCode::Char('D') if self.dirty => {
                self.selected = ExitChoice::DiscardExit;
                self.confirm(ctx);
            }
            // `y` is a quick-confirm in the clean layout (legacy
            // muscle memory). When dirty, `y` is ambiguous between
            // Save and Discard — fall through to whatever is selected
            // so the user has to be explicit.
            KeyCode::Char('y') | KeyCode::Char('Y') if !self.dirty => {
                self.selected = ExitChoice::SaveExit;
                self.confirm(ctx);
            }
            KeyCode::Enter => self.confirm(ctx),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.selected = ExitChoice::Cancel;
                self.complete = true;
                self.completion = Some(ViewCompletion::Cancelled);
            }
            KeyCode::Right | KeyCode::Char('l') => self.cycle(true),
            KeyCode::Left | KeyCode::Char('h') => self.cycle(false),
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
        // Ctrl-C is hard-quit regardless of dirty — matches the rest
        // of the TUI's "Ctrl-C escapes everything" contract. Unsaved
        // edits are dropped silently.
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
