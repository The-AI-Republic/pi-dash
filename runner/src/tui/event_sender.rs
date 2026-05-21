//! `AppEventSender` — clonable handle to the bus. Widgets and
//! background tasks hold one of these; they never see the `App`
//! struct itself. Modeled on codex's `app_event_sender.rs`.

use tokio::sync::mpsc::UnboundedSender;

use super::event::AppEvent;
use super::view::View;

#[derive(Clone)]
pub struct AppEventSender {
    tx: UnboundedSender<AppEvent>,
}

impl AppEventSender {
    pub fn new(tx: UnboundedSender<AppEvent>) -> Self {
        Self { tx }
    }

    /// Best-effort send. Failure means the receiver was dropped (App
    /// is shutting down) — there's nobody to tell, so we swallow.
    pub fn send(&self, ev: AppEvent) {
        let _ = self.tx.send(ev);
    }

    pub fn quit(&self) {
        self.send(AppEvent::Quit);
    }

    pub fn refresh(&self) {
        self.send(AppEvent::Refresh);
    }

    pub fn push_view(&self, v: Box<dyn View + Send>) {
        self.send(AppEvent::PushView(v));
    }

    pub fn pop_view(&self) {
        self.send(AppEvent::PopView);
    }
}
