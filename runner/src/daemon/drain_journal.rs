//! Crash-safe journal for terminal run signals the daemon failed to deliver.
//!
//! On shutdown the supervisor drains every in-flight run by sending
//! `RunFailed{DaemonRestart}` to the cloud, but each send is bounded by a
//! 2s timeout (and the whole drain by 5s) so a slow or wedged cloud can't
//! hang the shutdown. Before this journal existed, a send that missed the
//! window was simply dropped: the cloud kept the run in a busy state
//! forever, pinned to a runner that no longer knew about it — observed in
//! production as `drain send timed out at 2s` followed by a permanently
//! "busy" runner (see PDASHOSS01 wedged-runner incident, 2026-09-25).
//!
//! The journal closes that hole: every in-flight run is recorded here
//! *before* the drain sends are attempted, successful sends are removed
//! after, and whatever survives once journaled (a send timeout, a crash
//! or SIGKILL mid-drain) is replayed by the next daemon start via
//! [`supervisor`]'s replay pass. A death that never reaches the drain at
//! all (panic, OOM-kill, power loss mid-run) journals nothing — the
//! cloud's heartbeat reaper remains the only fallback for that class.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::cloud::protocol::FailureReason;
use crate::util::paths::Paths;

/// One undelivered `RunFailed` signal, serialized to
/// `data_dir/pending_run_failures.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingRunFailure {
    pub runner_id: Uuid,
    pub run_id: Uuid,
    pub reason: FailureReason,
    pub detail: String,
    /// When the run was failed (stamped at drain time, replayed verbatim).
    pub ended_at: DateTime<Utc>,
    /// When the entry was journaled; replay drops entries older than
    /// [`REPLAY_TTL`] so a permanently unreachable cloud can't grow the
    /// journal without bound.
    pub recorded_at: DateTime<Utc>,
}

/// How long replay keeps retrying an entry before giving up. After this
/// long the cloud's own heartbeat reaper has long since failed the run,
/// so redelivering the signal no longer adds information.
pub const REPLAY_TTL: chrono::Duration = chrono::Duration::hours(24);

/// Serializes every read-modify-write of the journal file. `record_all`
/// and `remove` each do a load → mutate → save sequence; without this
/// lock the shutdown drain's `record_all` can interleave with the
/// background replay task's `remove` and one side's write silently
/// clobbers the other's (atomic rename protects each write, not the
/// sequence).
static JOURNAL_MUTATION: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn journal_path(paths: &Paths) -> PathBuf {
    paths.drain_journal_path()
}

/// Load the journal. Missing file means an empty journal; a corrupt file
/// is logged and treated as empty rather than wedging daemon startup.
pub fn load(paths: &Paths) -> Vec<PendingRunFailure> {
    let path = journal_path(paths);
    let raw = match std::fs::read(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!("failed to read drain journal {}: {e:#}", path.display());
            return Vec::new();
        }
    };
    match serde_json::from_slice(&raw) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                "drain journal {} is corrupt, ignoring it: {e:#}",
                path.display()
            );
            Vec::new()
        }
    }
}

/// Merge `entries` into the journal, replacing any existing entry for the
/// same run (a retried drain must not duplicate). Written atomically
/// (tmp + rename) so a crash mid-write can't corrupt the journal.
pub fn record_all(paths: &Paths, entries: &[PendingRunFailure]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let _guard = JOURNAL_MUTATION.lock().unwrap_or_else(|e| e.into_inner());
    let mut merged = load(paths);
    for entry in entries {
        merged.retain(|e| e.run_id != entry.run_id);
        merged.push(entry.clone());
    }
    save(paths, &merged)
}

/// Remove the entries for `run_ids`. Deletes the file once empty so a
/// healthy daemon leaves nothing behind.
pub fn remove(paths: &Paths, run_ids: &[Uuid]) -> Result<()> {
    if run_ids.is_empty() {
        return Ok(());
    }
    let _guard = JOURNAL_MUTATION.lock().unwrap_or_else(|e| e.into_inner());
    let mut entries = load(paths);
    let before = entries.len();
    entries.retain(|e| !run_ids.contains(&e.run_id));
    if entries.len() == before {
        return Ok(());
    }
    save(paths, &entries)
}

fn save(paths: &Paths, entries: &[PendingRunFailure]) -> Result<()> {
    let path = journal_path(paths);
    if entries.is_empty() {
        match std::fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(e).with_context(|| format!("remove drain journal {}", path.display()));
            }
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create journal dir {}", parent.display()))?;
    }
    let raw = serde_json::to_vec_pretty(entries).context("serialize drain journal")?;
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f =
            std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        f.write_all(&raw)
            .with_context(|| format!("write {}", tmp.display()))?;
        // fsync before rename (matching config's `write_private`): without
        // it a power loss right after the rename can leave a truncated
        // journal, which `load` would then discard as corrupt — silently
        // dropping the signals this file exists to preserve.
        f.sync_all()
            .with_context(|| format!("sync {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
        }
    }

    fn entry(run_id: Uuid, detail: &str) -> PendingRunFailure {
        PendingRunFailure {
            runner_id: Uuid::new_v4(),
            run_id,
            reason: FailureReason::DaemonRestart,
            detail: detail.to_string(),
            ended_at: Utc::now(),
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn missing_file_loads_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load(&paths_for(tmp.path())).is_empty());
    }

    #[test]
    fn record_then_load_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let e = entry(Uuid::new_v4(), "daemon shutdown requested");
        record_all(&paths, std::slice::from_ref(&e)).unwrap();
        assert_eq!(load(&paths), vec![e]);
    }

    #[test]
    fn record_replaces_same_run_id() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let run_id = Uuid::new_v4();
        record_all(&paths, &[entry(run_id, "first")]).unwrap();
        record_all(&paths, &[entry(run_id, "second")]).unwrap();
        let loaded = load(&paths);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].detail, "second");
    }

    #[test]
    fn remove_to_empty_deletes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let e = entry(Uuid::new_v4(), "x");
        record_all(&paths, std::slice::from_ref(&e)).unwrap();
        assert!(journal_path(&paths).exists());
        remove(&paths, &[e.run_id]).unwrap();
        assert!(!journal_path(&paths).exists());
        assert!(load(&paths).is_empty());
    }

    #[test]
    fn corrupt_file_loads_empty_without_error() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());
        std::fs::create_dir_all(paths.data_dir.clone()).unwrap();
        std::fs::write(journal_path(&paths), b"not json{{{").unwrap();
        assert!(load(&paths).is_empty());
    }
}
