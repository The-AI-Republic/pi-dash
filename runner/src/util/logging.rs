use anyhow::Result;
use std::io::IsTerminal;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Whether ANSI colour escapes should be emitted. Colours are only useful on
/// an interactive terminal; redirected output (desktop-daemon.log, systemd /
/// launchd journals, `> out`) must stay plain text. `NO_COLOR` is honoured
/// as in `cli::runner_ops::warn_model_fallback`.
fn ansi_enabled(stdout_is_terminal: bool, no_color_present: bool) -> bool {
    stdout_is_terminal && !no_color_present
}

pub fn init(level: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let use_ansi = ansi_enabled(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
    );
    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_thread_ids(false)
        .with_ansi(use_ansi);
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init()
        .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ansi_enabled;

    #[test]
    fn tty_without_no_color_enables_ansi() {
        assert!(ansi_enabled(true, false));
    }

    #[test]
    fn redirected_output_disables_ansi() {
        // desktop-daemon.log / systemd capture: stdout is not a terminal.
        assert!(!ansi_enabled(false, false));
    }

    #[test]
    fn no_color_disables_ansi_even_on_tty() {
        assert!(!ansi_enabled(true, true));
    }

    #[test]
    fn no_color_and_redirected_stays_plain() {
        assert!(!ansi_enabled(false, true));
    }
}
