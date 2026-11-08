//! Owner-only named pipes for the Windows IPC transport (PIDESKAPP-152).
//!
//! The Windows pipe namespace (`\\.\pipe\`) is machine-wide and every
//! account can list it, so the pipe name alone proves nothing: another local
//! user can create `\\.\pipe\pidash-<id>` while our daemon is down and then
//! receive whatever the desktop app sends. Unix gets the equivalent
//! protection from a 0700 runtime dir and a 0600 socket; here both ends
//! enforce it explicitly:
//!
//! * the daemon creates every pipe instance with a security descriptor that
//!   names the current user as owner and grants access to that user only
//!   ([`create_owner_only_pipe`]), and
//! * the client refuses a pipe whose owner is not its own user before it
//!   sends a byte ([`verify_pipe_owner`]). Another account cannot make a
//!   pipe that passes, because it cannot assign someone else's SID as owner.

use std::ffi::c_void;
use std::io;
use std::os::windows::io::AsRawHandle;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The current process's user SID (`TokenUser`), owned in a buffer that
/// keeps the `PSID` it points into alive.
struct CurrentUserSid {
    buf: Vec<u64>,
}

impl CurrentUserSid {
    fn get() -> io::Result<Self> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is a
        // valid out-pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut len = 0u32;
            // SAFETY: size query with a null buffer; fails with
            // ERROR_INSUFFICIENT_BUFFER and reports the needed length.
            unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
            if len == 0 {
                return Err(io::Error::last_os_error());
            }
            // u64 storage keeps TOKEN_USER suitably aligned.
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            // SAFETY: `buf` holds at least `len` bytes.
            let ok = unsafe {
                GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { buf })
        })();
        // SAFETY: `token` was opened above and is closed exactly once.
        unsafe { CloseHandle(token) };
        result
    }

    fn psid(&self) -> PSID {
        // SAFETY: `buf` was filled by GetTokenInformation(TokenUser).
        unsafe { (*(self.buf.as_ptr() as *const TOKEN_USER)).User.Sid }
    }

    fn to_sddl(&self) -> io::Result<String> {
        sid_to_string(self.psid())
    }
}

fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut s: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sid` is a valid SID; `s` receives a LocalAlloc'd string.
    if unsafe { ConvertSidToStringSidW(sid, &mut s) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `s` is NUL-terminated and freed once below.
    let out = unsafe {
        let len = (0..).take_while(|&i| *s.add(i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(s, len))
    };
    unsafe { LocalFree(s.cast()) };
    Ok(out)
}

/// SDDL for a pipe only the current user may own or open: owner = user,
/// protected DACL with a single full-access ACE for that user.
fn owner_only_sddl(user_sid: &str) -> String {
    format!("O:{user_sid}D:P(A;;GA;;;{user_sid})")
}

/// Create one instance of the daemon's pipe, restricted to the current
/// user. `first` sets `FILE_FLAG_FIRST_PIPE_INSTANCE` so creation fails if
/// anyone else already holds the name.
///
/// Synchronous on purpose: the raw security descriptor is not `Send`, and
/// keeping it inside this call keeps the IPC accept loop's future `Send`.
pub fn create_owner_only_pipe(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let sid = CurrentUserSid::get()?.to_sddl()?;
    let sddl: Vec<u16> = owner_only_sddl(&sid).encode_utf16().chain([0]).collect();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated; `sd` receives a LocalAlloc'd SD.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    // SAFETY: `sa` and the SD it points to outlive the call; CreateNamedPipe
    // copies the descriptor into the pipe object.
    let created = unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, &mut sa as *mut _ as *mut c_void)
    };
    // SAFETY: `sd` came from ConvertStringSecurityDescriptor... and is freed once.
    unsafe { LocalFree(sd) };
    created
}

/// Fail unless the pipe behind `handle` is owned by the current user.
pub fn verify_pipe_owner(handle: &impl AsRawHandle, name: &str) -> io::Result<()> {
    let me = CurrentUserSid::get()?;
    let mut owner: PSID = std::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: the handle is a live pipe handle opened with GENERIC_READ
    // (which includes READ_CONTROL); `owner` points into `sd`, freed below.
    let rc = unsafe {
        GetSecurityInfo(
            handle.as_raw_handle() as HANDLE,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut sd,
        )
    };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    // SAFETY: both SIDs are valid for the duration of the comparison.
    let same = !owner.is_null() && unsafe { EqualSid(owner, me.psid()) } != 0;
    let owner_str = if same || owner.is_null() {
        None
    } else {
        sid_to_string(owner).ok()
    };
    // SAFETY: `sd` was allocated by GetSecurityInfo and is freed once.
    unsafe { LocalFree(sd) };
    if same {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "named pipe {name} is not owned by the current user (owner {}); \
             refusing to use it: another account may be holding the daemon's pipe name",
            owner_str.as_deref().unwrap_or("unknown"),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW;
    use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;

    fn pipe_sddl(handle: &impl AsRawHandle) -> String {
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let rc = unsafe {
            GetSecurityInfo(
                handle.as_raw_handle() as HANDLE,
                SE_KERNEL_OBJECT,
                info,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut sd,
            )
        };
        assert_eq!(rc, 0, "GetSecurityInfo");
        let mut s: *mut u16 = std::ptr::null_mut();
        let ok = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                info,
                &mut s,
                std::ptr::null_mut(),
            )
        };
        assert!(ok != 0, "SD to SDDL");
        let out = unsafe {
            let len = (0..).take_while(|&i| *s.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(s, len))
        };
        unsafe {
            LocalFree(s.cast());
            LocalFree(sd);
        }
        out
    }

    /// Render `sddl` the way Windows prints a stored descriptor, so SIDs
    /// that have SDDL aliases (e.g. the built-in Administrator, `LA`)
    /// compare equal to the aliased form `pipe_sddl` returns.
    fn canonical_sddl(sddl: &str) -> String {
        let wide: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        assert!(ok != 0, "parse {sddl}");
        let mut s: *mut u16 = std::ptr::null_mut();
        let ok = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut s,
                std::ptr::null_mut(),
            )
        };
        assert!(ok != 0, "render {sddl}");
        let out = unsafe {
            let len = (0..).take_while(|&i| *s.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(s, len))
        };
        unsafe {
            LocalFree(s.cast());
            LocalFree(sd);
        }
        out
    }

    fn unique_name() -> String {
        format!(r"\\.\pipe\pidash-test-{}", uuid::Uuid::new_v4())
    }

    #[tokio::test]
    async fn daemon_pipe_is_owned_by_and_open_only_to_the_current_user() {
        let me = CurrentUserSid::get().unwrap().to_sddl().unwrap();
        let pipe = create_owner_only_pipe(&unique_name(), true).unwrap();
        // Owner = me; protected DACL with a single full-access ACE for me
        // (GA is stored as the pipe's mapped FILE_ALL_ACCESS, `FA`).
        let expected = canonical_sddl(&format!("O:{me}D:P(A;;FA;;;{me})"));
        assert_eq!(pipe_sddl(&pipe), expected);
        verify_pipe_owner(&pipe, "test").unwrap();
    }

    #[tokio::test]
    async fn later_instances_keep_the_owner_only_descriptor() {
        let name = unique_name();
        let _first = create_owner_only_pipe(&name, true).unwrap();
        let second = create_owner_only_pipe(&name, false).unwrap();
        verify_pipe_owner(&second, &name).unwrap();
        // A second "first" instance must still be refused.
        assert!(create_owner_only_pipe(&name, true).is_err());
    }
}
