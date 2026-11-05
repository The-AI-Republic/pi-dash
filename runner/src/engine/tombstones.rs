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
//! Each tombstone records the workspace the runner belonged to. The
//! machine payload a workspace's daemon credential reads lists only that
//! workspace's rows, so a tombstone is forgotten only after its own
//! workspace was read: signing out of a workspace must not make its
//! removed runners come back at the next sign-in.
//!
//! Stored as a JSON list of `{runner_id, workspace}` in
//! `<data_dir>/engine/removed_runners.json` (a bare list of runner ids, the
//! first format, is still read, with no workspace), read-modify-written
//! under an exclusive lock on a sibling `.lock` file, so the CLI and the
//! daemon can both write it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use fs2::FileExt;
use uuid::Uuid;

use crate::util::paths::Paths;

fn file_path(paths: &Paths) -> PathBuf {
    paths.data_dir.join("engine").join("removed_runners.json")
}

/// Runner id → the workspace it was removed from (`None`: unknown).
type Set = BTreeMap<Uuid, Option<String>>;

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    runner_id: Uuid,
    #[serde(default)]
    workspace: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Stored {
    Entry(Entry),
    Bare(Uuid),
}

fn read(path: &std::path::Path) -> Set {
    let Ok(bytes) = fs::read(path) else {
        return Set::new();
    };
    match serde_json::from_slice::<Vec<Stored>>(&bytes) {
        Ok(list) => list
            .into_iter()
            .map(|s| match s {
                Stored::Entry(e) => (e.runner_id, e.workspace),
                Stored::Bare(id) => (id, None),
            })
            .collect(),
        Err(e) => {
            tracing::warn!(path = %path.display(), "ignoring an unreadable tombstone file: {e}");
            Set::new()
        }
    }
}

fn encode(set: &Set) -> serde_json::Result<Vec<u8>> {
    let list: Vec<Entry> = set
        .iter()
        .map(|(id, ws)| Entry {
            runner_id: *id,
            workspace: ws.clone(),
        })
        .collect();
    serde_json::to_vec(&list)
}

/// Read-modify-write the tombstone set under its lock. The closure
/// returns whether it changed the set; the file is written only then.
fn mutate(paths: &Paths, f: impl FnOnce(&mut Set) -> bool) -> Result<()> {
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
        fs::write(&tmp, encode(&set)?)
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
    read(&file_path(paths)).into_keys().collect()
}

/// Record that `runner_id`, of `workspace`, was removed locally while its
/// server row may still be live.
pub fn add(paths: &Paths, runner_id: Uuid, workspace: Option<&str>) -> Result<()> {
    let workspace = workspace.filter(|w| !w.is_empty()).map(str::to_string);
    mutate(paths, |set| {
        set.insert(runner_id, workspace.clone()).as_ref() != Some(&workspace)
    })
}

/// Forget `runner_id` (it was added again). Best effort.
pub fn forget(paths: &Paths, runner_id: Uuid) {
    if !file_path(paths).exists() {
        return;
    }
    if let Err(e) = mutate(paths, |set| set.remove(&runner_id).is_some()) {
        tracing::warn!(%runner_id, "forgetting an engine runner tombstone failed: {e:#}");
    }
}

/// Forget the tombstones whose server row is no longer live, judged only
/// from what was read: a tombstone goes when its workspace is in `read`
/// and its id is not in `live`. One with no recorded workspace goes only
/// when every signed-in workspace was read (`complete`).
pub fn prune(
    paths: &Paths,
    read_workspaces: &BTreeSet<String>,
    complete: bool,
    live: &BTreeSet<Uuid>,
) -> Result<()> {
    if !file_path(paths).exists() {
        return Ok(());
    }
    mutate(paths, |set| {
        let before = set.len();
        set.retain(|id, ws| {
            let seen = match ws {
                Some(w) => read_workspaces.contains(w),
                None => complete,
            };
            !seen || live.contains(id)
        });
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

    fn ws(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn add_forget_and_prune() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        assert!(load(&p).is_empty());
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        add(&p, a, Some("acme")).unwrap();
        add(&p, b, Some("beta")).unwrap();
        add(&p, a, Some("acme")).unwrap();
        assert_eq!(load(&p), BTreeSet::from([a, b]));
        forget(&p, a);
        assert_eq!(load(&p), BTreeSet::from([b]));
        prune(&p, &ws(&["beta"]), true, &BTreeSet::from([a])).unwrap();
        assert!(load(&p).is_empty());
    }

    #[test]
    fn prune_keeps_tombstones_of_workspaces_not_read() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        add(&p, a, Some("acme")).unwrap();
        add(&p, b, Some("beta")).unwrap();
        // Only acme was read (beta is signed out), and the read was
        // "complete" for the signed-in set: beta's tombstone stays.
        prune(&p, &ws(&["acme"]), true, &BTreeSet::new()).unwrap();
        assert_eq!(load(&p), BTreeSet::from([b]));
    }

    #[test]
    fn a_bare_id_list_is_read_and_pruned_only_from_a_complete_view() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let a = Uuid::new_v4();
        let path = file_path(&p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec(&vec![a]).unwrap()).unwrap();
        assert_eq!(load(&p), BTreeSet::from([a]));
        prune(&p, &ws(&["acme"]), false, &BTreeSet::new()).unwrap();
        assert_eq!(load(&p), BTreeSet::from([a]));
        prune(&p, &ws(&["acme"]), true, &BTreeSet::new()).unwrap();
        assert!(load(&p).is_empty());
    }
}
