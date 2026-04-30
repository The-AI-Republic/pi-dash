//! `Tui` — the only thing in the runner TUI that touches stdout, raw
//! mode, bracketed paste, focus events, and the alternate screen.
//!
//! Lifecycle (matches codex `tui.rs:272-368`):
//! 1. `Tui::init()` enables raw mode + alt-screen + bracketed paste +
//!    focus events, installs a panic hook that restores the terminal
//!    on panic.
//! 2. The `App` owns the `Tui` and asks it to draw / pull events.
//! 3. `Drop` restores the terminal — RAII guarantees cleanup even on
//!    `?` early returns.

use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::broadcast;

use super::event_stream::TuiEventStream;
use super::frame_requester::{FrameRequester, FrameScheduler};

pub struct Tui {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    frame_requester: FrameRequester,
    draw_rx: Option<broadcast::Receiver<()>>,
}

impl Tui {
    pub fn init() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        // Bracketed paste + focus events. Some terminals don't support
        // these, but `execute!` returns Err only on stdout failure;
        // unsupported sequences are silently ignored.
        crossterm::execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableFocusChange,
        )?;

        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        // Panic hook: restore the terminal so the user isn't left with
        // a wedged TTY after a stray `unwrap()`.
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore_terminal();
            original_hook(info);
        }));

        let (frame_requester, draw_rx) = FrameScheduler::spawn();

        Ok(Self {
            terminal,
            frame_requester,
            draw_rx: Some(draw_rx),
        })
    }

    pub fn frame_requester(&self) -> &FrameRequester {
        &self.frame_requester
    }

    /// Take the draw subscription. Only the App's event-stream wrapper
    /// should call this — there's exactly one consumer.
    pub fn take_draw_rx(&mut self) -> broadcast::Receiver<()> {
        self.draw_rx.take().expect("draw_rx already taken")
    }

    pub fn event_stream(&mut self) -> TuiEventStream {
        TuiEventStream::new(self.take_draw_rx())
    }

    pub fn draw<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.terminal.draw(|frame| f(frame))?;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = restore_terminal();
        let _ = self.terminal.show_cursor();
    }
}

fn restore_terminal() -> Result<()> {
    let mut stdout = io::stdout();
    let _ = crossterm::execute!(
        stdout,
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen,
    );
    disable_raw_mode()?;
    Ok(())
}
