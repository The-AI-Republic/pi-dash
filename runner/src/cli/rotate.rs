use anyhow::Result;
use clap::Args as ClapArgs;

use crate::config::schema::Credentials;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {}

pub async fn run(_args: Args, paths: &Paths) -> Result<()> {
    let (config, creds) = crate::config::file::load_all(paths)?;
    let resp = crate::cloud::register::rotate(
        &config.daemon.cloud_url,
        &creds.runner_id,
        &creds.runner_secret,
    )
    .await?;
    let new_creds = Credentials {
        runner_id: resp.runner_id,
        runner_secret: resp.runner_secret,
        // The rotate endpoint only mints a new runner_secret; the existing
        // api_token is unaffected, so preserve it across the rotation.
        api_token: creds.api_token.clone(),
        issued_at: chrono::Utc::now(),
    };
    crate::config::file::write_credentials(paths, &new_creds)?;
    println!(
        "rotated secret for runner {}; restart the daemon to use it.",
        new_creds.runner_id
    );
    Ok(())
}
