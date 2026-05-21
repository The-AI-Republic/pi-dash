//! Runner TUI.
//!
//! Layered architecture (see `.ai_design/tui_refactor/design.md`):
//! - L7 `render::` (Renderable trait)
//! - L6 `widgets::` (TextArea, SelectableList, ScrollState, picker)
//! - L5 `view::`   (View trait + Tab trait + completion model)
//! - L4 `views::`  (per-tab and per-modal concrete impls)
//! - L3 `app::`    (App orchestrator, dispatcher, three-source select!)
//! - L2 `tui_runtime::` (Tui struct, event_stream, frame requester)
//! - L1 entry: `pub fn run` below

pub mod app;
pub mod event;
pub mod event_sender;
pub mod input;
pub mod ipc_client;
pub mod render;
pub mod tui_runtime;
pub mod view;
pub mod views;
pub mod widgets;

use anyhow::Result;

use crate::util::paths::Paths;

pub async fn run(paths: Paths, no_onboarding: bool, initial_tab: app::Tab) -> Result<()> {
    // `no_onboarding` is kept as a CLI arg for backward compatibility but
    // is now a no-op: the General tab renders the inline registration
    // form when `config.toml` is missing, so there's no separate wizard
    // screen to skip. Route straight to the main app either way.
    let _ = no_onboarding;
    // On a fresh machine without config, send the user straight to the
    // General tab so the inline register form is the first thing they
    // see, regardless of which tab they asked for.
    let initial_tab = if paths.config_path().exists() {
        initial_tab
    } else {
        app::Tab::General
    };
    app::run(paths, initial_tab).await
}
