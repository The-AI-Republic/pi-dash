//! Best-effort "open this URL in the operator's default browser" helper.
//!
//! Used as a convenience by interactive CLI flows — `pidash auth login`
//! (open the device-code verification page) and `pidash runner add`
//! (open an agent's install page when its CLI is missing). Opening a
//! browser is never load-bearing: callers treat a failure as a no-op and
//! fall back to printing the URL for the operator to open by hand.

use anyhow::{Context, Result};

/// Spawn the platform's "open this URL" helper for `url`.
///
/// Fire-and-forget: we spawn the helper with its stdio nulled and return
/// as soon as it launches; we don't wait for the browser to actually
/// render anything. Returns `Err` only when the helper process itself
/// can't be spawned (e.g. `xdg-open` absent on a headless Linux box), so
/// callers can print a "visit this URL manually" fallback.
pub fn open(url: &str) -> Result<()> {
    let (program, arg_prefix): (&str, Option<&str>) = if cfg!(target_os = "macos") {
        ("open", None)
    } else if cfg!(target_os = "windows") {
        ("cmd", Some("/C start"))
    } else {
        ("xdg-open", None)
    };
    let mut cmd = std::process::Command::new(program);
    if let Some(prefix) = arg_prefix {
        for arg in prefix.split_whitespace() {
            cmd.arg(arg);
        }
    }
    cmd.arg(url);
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn().context("opening browser")?;
    Ok(())
}
