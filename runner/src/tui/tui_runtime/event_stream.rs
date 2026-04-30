//! `TuiEvent` — physical terminal event after filtering / mapping.
//!
//! Crossterm's raw `Event` enum carries Mouse, FocusGained/Lost,
//! Resize, Paste, Key. We strip Mouse, drop FocusLost (not useful),
//! map FocusGained to `Draw` (force a redraw on tab focus return),
//! and filter `KeyEventKind::Release` so widgets only ever see
//! Press/Repeat. The 4-variant `TuiEvent` is what the App's
//! `select!` arm consumes.

use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures_util::StreamExt;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub enum TuiEvent {
    Key(KeyEvent),
    Paste(String),
    Resize(u16, u16),
    /// A redraw request — produced by the FrameRequester (`schedule_frame`)
    /// or by FocusGained.
    Draw,
}

pub struct TuiEventStream {
    inner: EventStream,
    draw_rx: broadcast::Receiver<()>,
}

impl TuiEventStream {
    pub fn new(draw_rx: broadcast::Receiver<()>) -> Self {
        Self {
            inner: EventStream::new(),
            draw_rx,
        }
    }

    /// Pull the next event. We round-robin between crossterm's
    /// stdin events and draw broadcasts so neither starves the other.
    pub async fn next(&mut self) -> Option<TuiEvent> {
        loop {
            tokio::select! {
                biased;
                ev = self.draw_rx.recv() => {
                    match ev {
                        Ok(()) => return Some(TuiEvent::Draw),
                        Err(broadcast::error::RecvError::Closed) => {
                            // No more draw signals; fall through to crossterm
                            // until that closes too.
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            // We dropped some draw signals — coalesce by
                            // emitting one Draw.
                            return Some(TuiEvent::Draw);
                        }
                    }
                }
                ev = self.inner.next() => {
                    match ev? {
                        Ok(Event::Key(k)) => {
                            // Filter Release at the stream boundary.
                            if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                                return Some(TuiEvent::Key(k));
                            }
                        }
                        Ok(Event::Paste(s)) => return Some(TuiEvent::Paste(s)),
                        Ok(Event::Resize(w, h)) => return Some(TuiEvent::Resize(w, h)),
                        Ok(Event::FocusGained) => return Some(TuiEvent::Draw),
                        Ok(Event::FocusLost) => { /* drop */ }
                        Ok(Event::Mouse(_)) => { /* drop */ }
                        Err(_) => return None,
                    }
                }
            }
        }
    }
}

/// Convenience helper used by tests / fallback paths: pull the next
/// crossterm event off-thread with a 200ms timeout. Not used by the
/// main loop — the real path uses `EventStream` directly.
#[allow(dead_code)]
pub async fn poll_event_blocking() -> Option<Event> {
    tokio::task::spawn_blocking(|| {
        if crossterm::event::poll(Duration::from_millis(200)).ok()? {
            crossterm::event::read().ok()
        } else {
            None
        }
    })
    .await
    .ok()
    .flatten()
}
