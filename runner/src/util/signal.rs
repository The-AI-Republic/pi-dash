use anyhow::Result;

#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

/// Resolves when SIGTERM or SIGINT fires.
#[cfg(unix)]
pub async fn shutdown() -> Result<()> {
    let mut term = signal(SignalKind::terminate())?;
    let mut intr = signal(SignalKind::interrupt())?;
    tokio::select! {
        _ = term.recv() => tracing::info!("SIGTERM received"),
        _ = intr.recv() => tracing::info!("SIGINT received"),
    }
    Ok(())
}

/// Resolves when Ctrl+C fires.
#[cfg(not(unix))]
pub async fn shutdown() -> Result<()> {
    tokio::signal::ctrl_c().await?;
    tracing::info!("Ctrl+C received");
    Ok(())
}
