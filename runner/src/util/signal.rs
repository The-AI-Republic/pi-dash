use anyhow::Result;
use tokio::signal::unix::{SignalKind, signal};

/// Resolves when SIGTERM or SIGINT fires.
pub async fn shutdown() -> Result<()> {
    let mut term = signal(SignalKind::terminate())?;
    let mut intr = signal(SignalKind::interrupt())?;
    tokio::select! {
        _ = term.recv() => tracing::info!("SIGTERM received"),
        _ = intr.recv() => tracing::info!("SIGINT received"),
    }
    Ok(())
}
