//! PIDESKAPP-236: the daemon pipe name must be unguessable, stored in
//! an owner-only `pipe-id` file under the runtime dir, and reused
//! across restarts. A foreign account holding the old deterministic
//! `UUIDv5(path)` name must not block the daemon.

use std::path::PathBuf;

fn temp_runtime() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("pidash-pipe-id-{}", uuid::Uuid::new_v4()));
    let runtime = base.join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    (base, runtime)
}

#[test]
fn pipe_id_is_created_owner_only_and_reused() {
    let (base, runtime) = temp_runtime();
    // Exercise the helper that must exist after the fix. Pre-fix this
    // does not compile / fails, proving the file contract is missing.
    let id1 = pidash_ipc::pipe_id::read_or_create_pipe_id(&runtime).expect("create pipe-id");
    let id2 = pidash_ipc::pipe_id::read_or_create_pipe_id(&runtime).expect("reuse pipe-id");
    assert_eq!(id1, id2, "stored ID must be reused, not regenerated each call");
    assert_ne!(id1, uuid::Uuid::nil());
    let file = pidash_ipc::pipe_id::pipe_id_path(&runtime);
    assert!(file.exists(), "pipe-id file must exist at {:?}", file);
    let stored = std::fs::read_to_string(&file)
        .unwrap()
        .trim()
        .parse::<uuid::Uuid>()
        .expect("file contains a UUID");
    assert_eq!(stored, id1);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "pipe-id must be owner-only (0600) on Unix");
    }
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn different_runtime_dirs_get_different_ids() {
    let (base1, rt1) = temp_runtime();
    let id1 = pidash_ipc::pipe_id::read_or_create_pipe_id(&rt1).unwrap();
    let (base2, rt2) = temp_runtime();
    let id2 = pidash_ipc::pipe_id::read_or_create_pipe_id(&rt2).unwrap();
    assert_ne!(id1, id2, "ids must be random, not a hash of the path");
    let _ = std::fs::remove_dir_all(base1);
    let _ = std::fs::remove_dir_all(base2);
}

#[cfg(windows)]
#[test]
fn windows_pipe_name_uses_stored_random_id_not_v5_hash() {
    use pidash_ipc::windows_pipe_name;
    let (base, runtime) = temp_runtime();
    let sock = runtime.join("pidash.sock");
    let name1 = windows_pipe_name(&sock);
    let id = pidash_ipc::pipe_id::read_or_create_pipe_id(&runtime).unwrap();
    let expected = format!(r"\\.\\pipe\pidash-{id}");
    assert_eq!(name1, expected, "pipe name must embed the stored random ID");
    let deterministic = {
        let h = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            sock.to_string_lossy().as_bytes(),
        );
        format!(r"\\.\\pipe\pidash-{h}")
    };
    assert_ne!(
        name1, deterministic,
        "pipe name must not be the deterministic v5 hash any local user can compute"
    );
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn pipe_id_file_is_atomic_under_concurrent_creation() {
    // Two callers racing the first creation must converge on one ID,
    // not leave two files or an error.
    let (base, runtime) = temp_runtime();
    let rt = runtime.clone();
    let h1 = std::thread::spawn(move || {
        pidash_ipc::pipe_id::read_or_create_pipe_id(&rt).unwrap()
    });
    let rt2 = runtime.clone();
    let h2 = std::thread::spawn(move || {
        pidash_ipc::pipe_id::read_or_create_pipe_id(&rt2).unwrap()
    });
    let a = h1.join().unwrap();
    let b = h2.join().unwrap();
    assert_eq!(a, b, "concurrent first-start must converge on one ID");
    let _ = std::fs::remove_dir_all(base);
}
