//! Unguessable pipe identity for Windows named pipes (PIDESKAPP-236).
//!
//! The old scheme derived the pipe name as `UUIDv5(socket path)` —
//! computable by any local user who knows the socket path, so a
//! second account could pre-create `\\.\pipe\pidash-<id>` with
//! `FILE_FLAG_FIRST_PIPE_INSTANCE` and keep the daemon down until the
//! next logon (the scheduled task only runs at logon).
//!
//! The fix stores a fresh random UUID (v4, 122 bits of entropy) in
//! `<runtime_dir>/pipe-id`, a file only the current user can read.
//! Both ends (`IpcServer` and `Client`) resolve the pipe name through
//! this file, so another account cannot compute it. The file is
//! created atomically (`create_new`) and reused across restarts; a
//! concurrent first-start converges on one ID.

use std::path::{Path, PathBuf};

/// The file holding the random pipe ID under the runtime dir.
pub fn pipe_id_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("pipe-id")
}

/// Read the stored UUID, tolerating a concurrent writer that has
/// created the file but not yet written its contents (brief empty
/// window). Retries briefly before failing.
fn read_stored_id(path: &Path) -> std::io::Result<uuid::Uuid> {
    use std::io;
    for _ in 0..40 {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                let t = s.trim();
                if let Ok(id) = t.parse::<uuid::Uuid>()
                    && !id.is_nil()
                {
                    return Ok(id);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                break;
            }
            Err(_) => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, "corrupt pipe-id file"))
}

/// Read the stored pipe ID, or create a fresh random one and persist
/// it. The file is owner-only (0600 on Unix, owner-only DACL on
/// Windows) and created atomically so two racing first-starts converge
/// on the same ID.
///
/// Callers that need a fresh ID after `ERROR_ALREADY_EXISTS` should
/// use [`regenerate_pipe_id`] instead — this function always returns
/// the existing ID if one is already stored.
#[cfg(not(windows))]
pub fn read_or_create_pipe_id(runtime_dir: &Path) -> std::io::Result<uuid::Uuid> {
    use std::io;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    std::fs::create_dir_all(runtime_dir)?;
    let path = pipe_id_path(runtime_dir);
    // Fast path: read existing.
    if let Ok(id) = read_stored_id(&path) {
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4();
    let contents = id.to_string();
    // Atomic create-new. If another thread/process won the race,
    // read their ID instead.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(contents.as_bytes())?;
            f.sync_all().ok();
            Ok(id)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => read_stored_id(&path),
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
pub fn read_or_create_pipe_id(runtime_dir: &Path) -> std::io::Result<uuid::Uuid> {
    use std::io;
    std::fs::create_dir_all(runtime_dir)?;
    let path = pipe_id_path(runtime_dir);
    if let Ok(id) = read_stored_id(&path) {
        return Ok(id);
    }
    // Create owner-only file atomically. On Windows we create via
    // `CreateFileW` with a security descriptor so the file never
    // appears world-readable, then fall back to retry-on-race.
    let id = uuid::Uuid::new_v4();
    let contents = id.to_string();
    match create_owner_only_file(&path, contents.as_bytes()) {
        Ok(()) => Ok(id),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => read_stored_id(&path),
        Err(e) => Err(e),
    }
}

/// Overwrite the stored ID with a fresh random one (used when the
/// saved name itself is already taken — belt-and-braces after the
/// unguessable-name fix). Persists before returning.
pub fn regenerate_pipe_id(runtime_dir: &Path) -> std::io::Result<uuid::Uuid> {
    std::fs::create_dir_all(runtime_dir)?;
    let path = pipe_id_path(runtime_dir);
    let id = uuid::Uuid::new_v4();
    let contents = id.to_string();
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all().ok();
    }
    #[cfg(windows)]
    {
        // Overwrite with owner-only creation. Delete-then-create would
        // briefly expose a missing file; truncate after ensuring the
        // file carries an owner-only DACL is preferable, but the
        // simple owner-only create after remove is acceptable for a
        // rare recovery path. We recreate owner-only.
        let _ = std::fs::remove_file(&path);
        create_owner_only_file(&path, contents.as_bytes())?;
    }
    Ok(id)
}

/// `\\.\pipe\pidash-<id>` for the stored ID. The daemon and the
/// client share the same `runtime_dir`, so they derive the same name
/// without any computable hash.
#[cfg(windows)]
pub fn windows_pipe_name_from_id(id: uuid::Uuid) -> String {
    format!(r"\\.\\pipe\pidash-{id}")
}

// -- owner-only file creation on Windows ---------------------------------

#[cfg(windows)]
fn create_owner_only_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{INVALID_HANDLE_VALUE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    const CREATE_NEW: u32 = 1;
    // Build owner-only SDDL for the current user.
    let sddl_str = current_user_sddl()?;
    let sddl_wide: Vec<u16> = sddl_str.encode_utf16().chain([0]).collect();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl_wide.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    let path_wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    const GENERIC_WRITE: u32 = 0x40000000;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    let handle = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &mut sa as *mut SECURITY_ATTRIBUTES,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    unsafe { LocalFree(sd) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // Write contents via the raw handle.
    let mut written: u32 = 0;
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    let ok = unsafe {
        WriteFile(
            handle,
            contents.as_ptr() as *const _,
            contents.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    use windows_sys::Win32::Foundation::CloseHandle;
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if written as usize != contents.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "short write to pipe-id file",
        ));
    }
    Ok(())
}

/// The current user's SID in string form (`S-1-5-...`). Used by
/// the scheduled-task XML to scope the logon trigger to this user.
#[cfg(windows)]
pub fn current_user_sid() -> std::io::Result<String> {
    use std::io;
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut len = 0u32;
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let ok = unsafe {
            GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let sid = unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid };
        let mut s: *mut u16 = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut s) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let out = unsafe {
            let len = (0..).take_while(|&i| *s.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(s, len))
        };
        unsafe { LocalFree(s.cast()) };
        Ok(out)
    })();
    unsafe { CloseHandle(token) };
    result
}

#[cfg(windows)]
fn current_user_sddl() -> std::io::Result<String> {
    let sid = current_user_sid()?;
    Ok(format!("O:{sid}D:P(A;;GA;;;{sid})"))
}
