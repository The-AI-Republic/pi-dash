#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod approval;
pub mod cli;
pub mod cloud;
pub mod codex;
pub mod config;
pub mod daemon;
pub mod history;
pub mod ipc;
pub mod service;
pub mod tui;
pub mod util;
pub mod workspace;

pub const PROTOCOL_VERSION: u32 = 1;
pub const RUNNER_VERSION: &str = env!("CARGO_PKG_VERSION");
