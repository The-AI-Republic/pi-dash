use anyhow::Result;

#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

/// Which shutdown signal fired (NEWPIDASH-253): the daemon records it as
/// the `DaemonRestart` trigger so the failure comment can state the real
/// reason instead of the mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalTrigger {
    Term,
    Int,
}

/// Resolves when SIGTERM or SIGINT fires.
#[cfg(unix)]
pub async fn shutdown() -> Result<SignalTrigger> {
    let mut term = signal(SignalKind::terminate())?;
    let mut intr = signal(SignalKind::interrupt())?;
    tokio::select! {
        _ = term.recv() => {
            tracing::info!("SIGTERM received");
            Ok(SignalTrigger::Term)
        }
        _ = intr.recv() => {
            tracing::info!("SIGINT received");
            Ok(SignalTrigger::Int)
        }
    }
}

/// Resolves when Ctrl+C fires.
#[cfg(not(unix))]
pub async fn shutdown() -> Result<SignalTrigger> {
    tokio::signal::ctrl_c().await?;
    tracing::info!("Ctrl+C received");
    Ok(SignalTrigger::Int)
}
