use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use uuid::Uuid;

use crate::util::paths::Paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: Uuid,
    pub work_item_id: Option<Uuid>,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunsIndex {
    #[serde(default)]
    pub runs: BTreeMap<Uuid, RunSummary>,
}

impl RunsIndex {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.runs_index_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).with_context(|| format!("reading {path:?}"))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        paths.ensure()?;
        let path = paths.runs_index_path();
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn upsert(&mut self, s: RunSummary) {
        self.runs.insert(s.run_id, s);
        // Bound at 500 entries; keep the newest.
        if self.runs.len() > 500 {
            let oldest: Vec<_> = self
                .runs
                .iter()
                .take(self.runs.len() - 500)
                .map(|(k, _)| *k)
                .collect();
            for k in oldest {
                self.runs.remove(&k);
            }
        }
    }

    pub fn recent(&self, n: usize) -> Vec<RunSummary> {
        let mut v: Vec<_> = self.runs.values().cloned().collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v.truncate(n);
        v
    }
}
