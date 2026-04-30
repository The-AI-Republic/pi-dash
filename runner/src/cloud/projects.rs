//! ``GET /api/runners/projects/`` — fetch project + pod metadata using
//! connection-bearer auth so the daemon's TUI / CLI can render a
//! cascaded picker.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;
use uuid::Uuid;

use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectInfo {
    pub id: Uuid,
    pub identifier: String,
    pub name: String,
    pub default_pod_id: Option<Uuid>,
    #[serde(default)]
    pub pod_count: u32,
    /// Per-project pod list. The cloud sorts the default pod first.
    #[serde(default)]
    pub pods: Vec<PodInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PodInfo {
    pub id: Uuid,
    pub name: String,
    pub is_default: bool,
}

/// Read the local credentials and call the project-list endpoint over
/// the connection-bearer auth path.
pub async fn list_projects(paths: &Paths) -> Result<Vec<ProjectInfo>> {
    let creds = file::load_credentials(paths)
        .context("no credentials.toml — run `pidash connect` first")?;
    let cfg = file::load_config(paths)?;
    let url = format!(
        "{}/api/runners/projects/",
        cfg.daemon.cloud_url.trim_end_matches('/')
    );
    let resp = http_client()?
        .get(&url)
        .bearer_auth(&creds.connection_secret)
        .header("X-Connection-Id", creds.connection_id.to_string())
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("list-projects failed: HTTP {status}: {body}");
    }
    resp.json::<Vec<ProjectInfo>>()
        .await
        .context("parsing project list response")
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("pidash/{}", crate::RUNNER_VERSION))
        .build()?)
}
