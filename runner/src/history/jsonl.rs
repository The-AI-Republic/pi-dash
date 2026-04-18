use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

use crate::util::paths::Paths;

/// One record per line; lines are append-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryEntry {
    Header {
        run_id: Uuid,
        work_item_id: Option<Uuid>,
        prompt_preview: String,
        started_at: DateTime<Utc>,
        repo_url: Option<String>,
    },
    Lifecycle {
        ts: DateTime<Utc>,
        state: String,
        detail: Option<String>,
    },
    CodexEvent {
        ts: DateTime<Utc>,
        method: String,
        params: serde_json::Value,
    },
    Approval {
        ts: DateTime<Utc>,
        approval_id: String,
        status: String,
        payload: serde_json::Value,
    },
    Footer {
        ts: DateTime<Utc>,
        final_status: String,
        done_payload: Option<serde_json::Value>,
        error: Option<String>,
    },
}

pub struct HistoryWriter {
    file: File,
    path: PathBuf,
}

impl HistoryWriter {
    pub async fn open(paths: &Paths, run_id: Uuid) -> Result<Self> {
        paths.ensure()?;
        let path = paths.runs_dir().join(format!("{run_id}.jsonl"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("opening {path:?}"))?;
        Ok(Self { file, path })
    }

    pub async fn append(&mut self, entry: &HistoryEntry) -> Result<()> {
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');
        self.file.write_all(&line).await?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub async fn read_all(path: &Path) -> Result<Vec<HistoryEntry>> {
    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut buf = String::new();
    let mut out = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).await?;
        if n == 0 {
            break;
        }
        let line = buf.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<HistoryEntry>(line) {
            Ok(e) => out.push(e),
            Err(err) => tracing::warn!("skipping bad history line in {path:?}: {err}"),
        }
    }
    Ok(out)
}
