//! Best-effort "open this URL in the operator's default browser".
//!
//! Shared by `pidash auth login` (device-code approval page) and
//! `pidash runner add` (an agent's install page when the agent CLI is
//! missing). Opening a browser is never load-bearing — every caller also
//! prints the URL so a headless or locked-down box still gets a usable
//! link. An `Err` only means we couldn't even launch the platform handler.

use anyhow::{Context, Result};
use std::process::{Command, Stdio};

/// Spawn the platform's default URL handler for `url`. Returns as soon as
/// the handler process is launched — it does not wait for the browser to
/// finish opening. `Err` means the handler itself couldn't be started
/// (e.g. no `xdg-open` on a minimal Linux host); callers treat that as
/// "tell the user to open the link themselves".
pub fn open_url(url: &str) -> Result<()> {
    let (program, arg_prefix): (&str, Option<&str>) = if cfg!(target_os = "macos") {
        ("open", None)
    } else if cfg!(target_os = "windows") {
        ("cmd", Some("/C start"))
    } else {
        ("xdg-open", None)
    };
    let mut cmd = Command::new(program);
    if let Some(prefix) = arg_prefix {
        for arg in prefix.split_whitespace() {
            cmd.arg(arg);
        }
    }
    cmd.arg(url);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().context("opening browser")?;
    Ok(())
}
