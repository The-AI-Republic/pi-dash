//! Engine runners the user removed on this computer only.
//!
//! `pidash runner remove --local-only`, and the fallback `runner remove`
//! takes when the cloud can't be reached, strip a runner's block but leave
//! its server row live. Without a record of that, the reconcile loop
//! (`daemon/engine_reconcile.rs`) would see a live engine row with no
//! local block and re-add it within ten minutes. A tombstone is that
//! record: reconcile skips tombstoned rows and forgets a tombstone once the
//! server no longer lists the row as live. Adding a runner with the same id
//! again (any `apply_enroll_response`) also forgets it.
//!
//! Stored as a JSON list of runner ids in
//! `<data_dir>/engine/removed_runners.json`, read-modify-written under an
//! exclusive lock on a sibling `.lock` file, so the CLI and the daemon can
//! both write it.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use fs2::FileExt;
use uuid::Uuid;

use crate::util::paths::Paths;

fn file_path(paths: &Paths) -> PathBuf {
    paths.data_dir.join("engine").join("removed_runners.json")
}

fn read(path: &std::path::Path) -> BTreeSet<Uuid> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!(path = %path.display(), "ignoring an unreadable tombstone file: {e}");
            BTreeSet::new()
        }),
        Err(_) => BTreeSet::new(),
    }
}

/// Read-modify-write the tombstone set under its lock. The closure
/// returns whether it changed the set; the file is written only then.
fn mutate(paths: &Paths, f: impl FnOnce(&mut BTreeSet<Uuid>) -> bool) -> Result<()> {
    let path = file_path(paths);
    let dir = path.parent().expect("tombstone path has a parent");
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let lock_path = dir.join(".removed_runners.lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("locking {}", lock_path.display()))?;
    let mut set = read(&path);
    let result = if f(&mut set) {
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(&set)?)
            .with_context(|| format!("writing {}", tmp.display()))
            .and_then(|()| {
                fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))
            })
    } else {
        Ok(())
    };
    let _ = FileExt::unlock(&lock);
    result
}

/// The runner ids removed on this computer only.
pub fn load(paths: &Paths) -> BTreeSet<Uuid> {
    read(&file_path(paths))
}

/// Record that `runner_id` was removed locally while its server row may
/// still be live.
pub fn add(paths: &Paths, runner_id: Uuid) -> Result<()> {
    mutate(paths, |set| set.insert(runner_id))
}

/// Forget `runner_id` (it was added again). Best effort.
pub fn forget(paths: &Paths, runner_id: Uuid) {
    if !file_path(paths).exists() {
        return;
    }
    if let Err(e) = mutate(paths, |set| set.remove(&runner_id)) {
        tracing::warn!(%runner_id, "forgetting an engine runner tombstone failed: {e:#}");
    }
}

/// Keep only the tombstones whose server row is still live (`live`).
pub fn retain_live(paths: &Paths, live: &BTreeSet<Uuid>) -> Result<()> {
    if !file_path(paths).exists() {
        return Ok(());
    }
    mutate(paths, |set| {
        let before = set.len();
        set.retain(|id| live.contains(id));
        set.len() != before
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    #[test]
    fn add_forget_and_retain() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        assert!(load(&p).is_empty());
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        add(&p, a).unwrap();
        add(&p, b).unwrap();
        add(&p, a).unwrap();
        assert_eq!(load(&p), BTreeSet::from([a, b]));
        forget(&p, a);
        assert_eq!(load(&p), BTreeSet::from([b]));
        retain_live(&p, &BTreeSet::from([a])).unwrap();
        assert!(load(&p).is_empty());
    }
}
