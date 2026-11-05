//! Daemon instance identity and the exclusive instance lock.
//!
//! `data_dir/instance_id` holds a random id, created once with
//! `O_CREAT|O_EXCL` so two daemons racing on a fresh data dir agree on it.
//! `data_dir/instance.lock` carries an exclusive advisory lock for the
//! daemon's lifetime: only its holder runs crash recovery and writes process
//! records, so a second daemon on the same data dir never acts on the first
//! one's live agents. Design: `.ai_design/runner_cleanup/design.md` §4.1.

use anyhow::{Context, Result};
use std::io::{ErrorKind, Write};
use std::path::Path;

pub const INSTANCE_ID_FILE: &str = "instance_id";
pub const INSTANCE_LOCK_FILE: &str = "instance.lock";

/// Read the instance id, creating it atomically on first use.
pub fn load_or_create_id(data_dir: &Path) -> Result<String> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating {}", data_dir.display()))?;
    let path = data_dir.join(INSTANCE_ID_FILE);
    for _ in 0..20 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                let id = uuid::Uuid::new_v4().simple().to_string();
                f.write_all(id.as_bytes())?;
                f.sync_all()?;
                return Ok(id);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let id = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let id = id.trim();
                if !id.is_empty() {
                    return Ok(id.to_string());
                }
                // A racing creator has not written yet.
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(e) => {
                return Err(e).with_context(|| format!("creating {}", path.display()));
            }
        }
    }
    anyhow::bail!("{} exists but stays empty", path.display())
}

/// The exclusive lifetime lock. Released when dropped (or when the process
/// dies, which is what makes it safe after a crash).
pub struct InstanceLock {
    #[cfg(unix)]
    _lock: nix::fcntl::Flock<std::fs::File>,
    #[cfg(not(unix))]
    _file: std::fs::File,
}

/// Try to take the instance lock. `Ok(None)` when another live daemon holds
/// it.
pub fn try_lock(data_dir: &Path) -> Result<Option<InstanceLock>> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating {}", data_dir.display()))?;
    let path = data_dir.join(INSTANCE_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    #[cfg(unix)]
    {
        use nix::errno::Errno;
        use nix::fcntl::{Flock, FlockArg};
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(lock) => Ok(Some(InstanceLock { _lock: lock })),
            Err((_, Errno::EWOULDBLOCK)) => Ok(None),
            Err((_, e)) => Err(anyhow::anyhow!("locking {}: {e}", path.display())),
        }
    }
    #[cfg(not(unix))]
    {
        Ok(Some(InstanceLock { _file: file }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_created_once_and_then_reused() {
        let dir = tempfile::tempdir().unwrap();
        let a = load_or_create_id(dir.path()).unwrap();
        let b = load_or_create_id(dir.path()).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        let other = tempfile::tempdir().unwrap();
        assert_ne!(a, load_or_create_id(other.path()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn second_holder_on_the_same_data_dir_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let first = try_lock(dir.path()).unwrap();
        assert!(first.is_some());
        assert!(try_lock(dir.path()).unwrap().is_none());
        drop(first);
        assert!(try_lock(dir.path()).unwrap().is_some());
    }
}
