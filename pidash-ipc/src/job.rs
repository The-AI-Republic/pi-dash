//! Windows Job Object helper (NEWPIDASH-241 T5): safe wrappers over
//! windows-sys so the runner's `forbid(unsafe_code)` crate can do
//! verified teardown without raw FFI at call sites.
//!
//! The Job is created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so a
//! dropped handle terminates every assigned process (the anchor's
//! session/group on Windows). Identity is the process creation time
//! from `GetProcessTimes`, stable per pid and changed on PID reuse —
//! the same contract as Linux starttime and macOS `ps` start time.

use std::io;
use std::sync::Arc;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

/// A Windows Job Object handle (owning; closes on drop).
pub struct JobHandle(HANDLE);
// HANDLES are Send; the guard moves one per scope across tasks.
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}
impl std::fmt::Debug for JobHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("JobHandle").field(&self.0).finish()
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE: closing terminates all members.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

impl JobHandle {
    /// Assign `pid` to this Job (the process must exist).
    pub fn assign(&self, pid: u32) -> io::Result<()> {
        unsafe {
            let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if proc.is_null() {
                return Err(io::Error::last_os_error());
            }
            let ok = AssignProcessToJobObject(self.0, proc);
            CloseHandle(proc);
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

/// Create a Job with kill-on-close (force-kill propagation).
pub fn create_job() -> io::Result<Arc<JobHandle>> {
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            CloseHandle(job);
            return Err(io::Error::last_os_error());
        }
        // Return Arc so scopes share one owner; drop closes.
        Ok(Arc::new(JobHandle(job)))
    }
}

/// Process creation time (for identity), or None when the pid is gone.
pub fn process_identity(pid: u32) -> Option<String> {
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if proc.is_null() {
            return None;
        }
        let mut creation = std::mem::zeroed();
        let mut exit = std::mem::zeroed();
        let mut kernel = std::mem::zeroed();
        let mut user = std::mem::zeroed();
        let ok = GetProcessTimes(proc, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(proc);
        if ok == 0 {
            return None;
        }
        // FILETIME (low, high) as a single comparable value.
        let hi = creation.dwHighDateTime as u64;
        let lo = creation.dwLowDateTime as u64;
        Some(format!("{:08x}-{:08x}", hi, lo))
    }
}

/// True when `pid` exists (group containment alone is checked by the
/// caller via identity + liveness).
pub fn pid_alive(pid: u32) -> bool {
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if proc.is_null() {
            return false;
        }
        CloseHandle(proc);
        true
    }
}
