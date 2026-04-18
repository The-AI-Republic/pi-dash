use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init(level: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let fmt_layer = fmt::layer().with_target(false).with_thread_ids(false);
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init()
        .ok();
    Ok(())
}
