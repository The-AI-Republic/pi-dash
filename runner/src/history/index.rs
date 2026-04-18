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
        // Bound at 500 entries; evict the oldest by `started_at`. The
        // BTreeMap is keyed by random Uuid, so taking from its iterator
        // would evict by Uuid value, not by age.
        const CAP: usize = 500;
        if self.runs.len() > CAP {
            let to_drop = self.runs.len() - CAP;
            let mut by_age: Vec<(Uuid, DateTime<Utc>)> =
                self.runs.iter().map(|(k, v)| (*k, v.started_at)).collect();
            by_age.sort_by_key(|(_, ts)| *ts);
            for (k, _) in by_age.into_iter().take(to_drop) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn s(uuid_seed: u64, mins: i64) -> RunSummary {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&uuid_seed.to_le_bytes());
        RunSummary {
            run_id: Uuid::from_bytes(bytes),
            work_item_id: None,
            status: "completed".into(),
            started_at: Utc.timestamp_opt(1_700_000_000 + mins * 60, 0).unwrap(),
            ended_at: None,
            title: None,
        }
    }

    #[test]
    fn upsert_evicts_oldest_by_started_at_not_by_uuid() {
        let mut idx = RunsIndex::default();
        // Construct: i=0 is the OLDEST entry by time, but its Uuid sorts
        // *highest*. A buggy "evict by BTreeMap iteration order" would keep
        // the oldest entries instead of the newest.
        for i in 0..600u64 {
            let uuid_seed = u64::MAX - i;
            idx.upsert(s(uuid_seed, i as i64));
        }
        assert_eq!(idx.runs.len(), 500);
        // The first 100 inserts (i=0..100) are the oldest, should be evicted.
        let oldest_kept = idx.runs.values().map(|r| r.started_at).min().unwrap();
        assert!(
            oldest_kept.timestamp() >= 1_700_000_000 + 100 * 60,
            "oldest 100 entries should have been evicted; got {oldest_kept}",
        );
    }
}
