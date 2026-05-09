//! Remove-runner confirmation modal.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::tui::event::AppEvent;
use crate::tui::render::Renderable;
use crate::tui::view::{KeyHandled, View, ViewCompletion, ViewCtx};

use super::confirm::centered_rect;

pub struct RemoveRunnerView {
    name: String,
    complete: bool,
    completion: Option<ViewCompletion>,
}

impl RemoveRunnerView {
    pub fn new(name: String) -> Self {
        Self {
            name,
            complete: false,
            completion: None,
        }
    }
}

impl Renderable for RemoveRunnerView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let modal = centered_rect(50, 30, area);
        Clear.render(modal, buf);
        let body = Paragraph::new(vec![
            Line::from(vec![
                Span::raw("Remove runner "),
                Span::styled(
                    format!("{:?}", self.name),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("?"),
            ]),
            Line::raw(""),
            Line::from("Deregisters cloud-side, strips it from config.toml,"),
            Line::from("and deletes the local data directory. The other"),
            Line::from("runners on this machine keep running."),
            Line::raw(""),
            Line::from("[y] yes     [any other key] cancel"),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm remove "),
        );
        body.render(modal, buf);
    }
}

impl View for RemoveRunnerView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                ctx.tx.send(AppEvent::SubmitRemoveRunner(self.name.clone()));
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
