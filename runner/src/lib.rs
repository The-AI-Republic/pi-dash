#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod agent;
pub mod api_client;
pub mod approval;
pub mod claude_code;
pub mod cli;
pub mod cloud;
pub mod codex;
pub mod config;
pub mod cursor_agent;
pub mod daemon;
pub mod grok;
pub mod history;
pub mod ipc;
pub mod muse_code;
pub mod openclaw;
pub mod service;
pub mod tui;
pub mod util;
pub mod workspace;

/// Cloud this CLI talks to when the user hasn't said otherwise — i.e. the
/// AI Republic-hosted Pi Dash. The released `install.sh` runs straight into
/// `pidash auth login`, and making that first run stop to ask for a URL meant
/// every hosted user typed our own production hostname in by hand. Self-hosted
/// installs override it with `--url` (or a pre-seeded `[daemon].cloud_url`).
pub const DEFAULT_CLOUD_URL: &str = "https://pidash.airepublic.com";

pub const PROTOCOL_VERSION: u32 = 3;
pub const RUNNER_VERSION: &str = env!("CARGO_PKG_VERSION");
