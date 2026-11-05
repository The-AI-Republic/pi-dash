//! Read-only process inspection for process-tree cleanup and crash recovery.
//!
//! Everything here is best effort and same-uid only: a process owned by
//! another user is reported as unreadable rather than guessed at. Used by
//! [`crate::util::proctree`] to verify that a recorded process group still
//! belongs to this daemon before signalling it. Design:
//! `.ai_design/runner_cleanup/design.md` §4.1 and §4.3.
//!
//! * Linux: `/proc/<pid>/stat` (pgid, state, start time) and
//!   `/proc/<pid>/environ`.
//! * macOS: `proc_listpids` / `proc_pidinfo(PROC_PIDTBSDINFO)` and
//!   `sysctl(KERN_PROCARGS2)`.
//! * Elsewhere: nothing is readable, so callers fall back to "do not signal".

/// What we know about one live process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub pgid: i32,
    pub uid: u32,
    /// Process start time, milliseconds since the Unix epoch. Comparable
    /// across processes on the same boot.
    pub start_time_ms: u64,
    /// Exited but not yet reaped. Counts as a group member for `kill(2)`,
    /// but can never run again.
    pub zombie: bool,
}

/// The effective uid of this daemon.
pub fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        nix::unistd::geteuid().as_raw()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// Inspect one process. `None` when it does not exist or cannot be read.
pub fn info(pid: i32) -> Option<ProcInfo> {
    imp::info(pid)
}

/// Every process currently in process group `pgid`.
pub fn group_members(pgid: i32) -> Vec<ProcInfo> {
    imp::all_pids()
        .into_iter()
        .filter_map(imp::info)
        .filter(|p| p.pgid == pgid)
        .collect()
}

/// Every live process owned by this uid.
pub fn same_uid_processes() -> Vec<ProcInfo> {
    let me = current_uid();
    imp::all_pids()
        .into_iter()
        .filter_map(imp::info)
        .filter(|p| p.uid == me && !p.zombie)
        .collect()
}

/// The environment of a same-uid process as `KEY=VALUE` byte strings.
///
/// `None` when the process is gone, belongs to another user, or the platform
/// does not let us read it. Callers must treat `None` as "unknown", never as
/// "no tag".
pub fn environ(pid: i32) -> Option<Vec<Vec<u8>>> {
    let me = current_uid();
    match info(pid) {
        Some(p) if p.uid == me => imp::environ(pid),
        _ => None,
    }
}

/// Value of `key` in a process environment read by [`environ`].
pub fn env_value<'a>(env: &'a [Vec<u8>], key: &str) -> Option<&'a [u8]> {
    let key = key.as_bytes();
    env.iter().find_map(|kv| {
        let rest = kv.strip_prefix(key)?;
        rest.strip_prefix(b"=")
    })
}

/// Split a NUL-separated buffer into its non-empty entries.
pub fn split_nul(buf: &[u8]) -> Vec<Vec<u8>> {
    buf.split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// Parse a macOS `KERN_PROCARGS2` buffer into the environment entries.
///
/// Layout: `argc: i32`, the exec path, NUL padding, `argc` argv strings, then
/// the environment strings, each NUL-terminated, ending at an empty string.
/// Pure so it is unit-tested on every platform.
pub fn parse_procargs2(buf: &[u8]) -> Option<Vec<Vec<u8>>> {
    if buf.len() < 4 {
        return None;
    }
    let argc = i32::from_ne_bytes(buf[..4].try_into().ok()?);
    if argc < 0 {
        return None;
    }
    let mut pos = 4;
    // Exec path.
    let end = buf[pos..].iter().position(|b| *b == 0)? + pos;
    pos = end;
    // NUL padding after the exec path.
    while pos < buf.len() && buf[pos] == 0 {
        pos += 1;
    }
    // argv.
    for _ in 0..argc {
        let end = buf[pos..].iter().position(|b| *b == 0)? + pos;
        pos = end + 1;
    }
    let mut env = Vec::new();
    while pos < buf.len() {
        let len = buf[pos..]
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(buf.len() - pos);
        if len == 0 {
            break;
        }
        env.push(buf[pos..pos + len].to_vec());
        pos += len + 1;
    }
    Some(env)
}

/// Parse the fields we need out of a Linux `/proc/<pid>/stat` line:
/// `(state, pgid, starttime_ticks)`. The command name sits in parentheses and
/// may itself contain spaces or parentheses, so fields are counted from the
/// last `)`.
pub fn parse_linux_stat(stat: &str) -> Option<(char, i32, u64)> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After the comm: state(3) ppid(4) pgrp(5) ... starttime(22); index 0 is
    // field 3.
    let state = fields.first()?.chars().next()?;
    let pgid = fields.get(2)?.parse().ok()?;
    let start = fields.get(19)?.parse().ok()?;
    Some((state, pgid, start))
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{ProcInfo, parse_linux_stat};
    use std::os::unix::fs::MetadataExt;
    use std::sync::OnceLock;

    fn boot_time_ms() -> u64 {
        static BOOT: OnceLock<u64> = OnceLock::new();
        *BOOT.get_or_init(|| {
            std::fs::read_to_string("/proc/stat")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find_map(|l| l.strip_prefix("btime "))
                        .and_then(|v| v.trim().parse::<u64>().ok())
                })
                .map(|secs| secs * 1000)
                .unwrap_or(0)
        })
    }

    fn ticks_per_sec() -> u64 {
        static TICKS: OnceLock<u64> = OnceLock::new();
        *TICKS.get_or_init(|| {
            nix::unistd::sysconf(nix::unistd::SysconfVar::CLK_TCK)
                .ok()
                .flatten()
                .and_then(|v| u64::try_from(v).ok())
                .filter(|v| *v > 0)
                .unwrap_or(100)
        })
    }

    pub fn all_pids() -> Vec<i32> {
        let Ok(rd) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        rd.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
            .collect()
    }

    pub fn info(pid: i32) -> Option<ProcInfo> {
        let dir = format!("/proc/{pid}");
        let uid = std::fs::metadata(&dir).ok()?.uid();
        let stat = std::fs::read_to_string(format!("{dir}/stat")).ok()?;
        let (state, pgid, ticks) = parse_linux_stat(&stat)?;
        Some(ProcInfo {
            pid,
            pgid,
            uid,
            start_time_ms: boot_time_ms() + ticks * 1000 / ticks_per_sec(),
            zombie: matches!(state, 'Z' | 'X'),
        })
    }

    pub fn environ(pid: i32) -> Option<Vec<Vec<u8>>> {
        let buf = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
        Some(super::split_nul(&buf))
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod imp {
    use super::{ProcInfo, parse_procargs2};

    /// `PROC_ALL_PIDS` from `<sys/proc_info.h>`; not exported by `libc`.
    const PROC_ALL_PIDS: u32 = 1;

    pub fn all_pids() -> Vec<i32> {
        // SAFETY: a null buffer asks for the required size only.
        let needed = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
        if needed <= 0 {
            return Vec::new();
        }
        // Headroom for processes started between the two calls.
        let cap = needed as usize / std::mem::size_of::<i32>() + 64;
        let mut pids = vec![0i32; cap];
        // SAFETY: the buffer is `cap` i32s long and we pass its byte size.
        let got = unsafe {
            libc::proc_listpids(
                PROC_ALL_PIDS,
                0,
                pids.as_mut_ptr().cast(),
                (cap * std::mem::size_of::<i32>()) as libc::c_int,
            )
        };
        if got <= 0 {
            return Vec::new();
        }
        pids.truncate(got as usize / std::mem::size_of::<i32>());
        pids.retain(|p| *p > 0);
        pids
    }

    pub fn info(pid: i32) -> Option<ProcInfo> {
        // SAFETY: proc_bsdinfo is plain old data; zeroed is a valid value.
        let mut bsd: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: the buffer is exactly one proc_bsdinfo.
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut bsd as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if got != size {
            return None;
        }
        Some(ProcInfo {
            pid,
            pgid: bsd.pbi_pgid as i32,
            uid: bsd.pbi_uid,
            start_time_ms: bsd.pbi_start_tvsec * 1000 + bsd.pbi_start_tvusec / 1000,
            // SZOMB from <sys/proc.h>.
            zombie: bsd.pbi_status == 5,
        })
    }

    pub fn environ(pid: i32) -> Option<Vec<Vec<u8>>> {
        let mut argmax: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: mib and the out pointer are valid for the call.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut argmax as *mut libc::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || argmax <= 0 {
            return None;
        }
        let mut buf = vec![0u8; argmax as usize];
        let mut size = buf.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        // SAFETY: the buffer is `size` bytes long.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        buf.truncate(size);
        parse_procargs2(&buf)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    use super::ProcInfo;

    pub fn all_pids() -> Vec<i32> {
        Vec::new()
    }

    pub fn info(_pid: i32) -> Option<ProcInfo> {
        None
    }

    pub fn environ(_pid: i32) -> Option<Vec<Vec<u8>>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_stat_parses_fields_after_a_tricky_comm() {
        let line = "1234 (a b) (c)) S 1 999 999 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 5555 0 0";
        assert_eq!(parse_linux_stat(line), Some(('S', 999, 5555)));
    }

    #[test]
    fn procargs2_yields_only_the_environment() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/bin/agent\0\0\0\0");
        buf.extend_from_slice(b"agent\0--flag\0");
        buf.extend_from_slice(b"A=1\0PIDASH_PROC_TAG=i:run:x\0\0junk");
        let env = parse_procargs2(&buf).unwrap();
        assert_eq!(
            env,
            vec![b"A=1".to_vec(), b"PIDASH_PROC_TAG=i:run:x".to_vec()]
        );
        assert_eq!(env_value(&env, "PIDASH_PROC_TAG"), Some(&b"i:run:x"[..]));
        assert_eq!(env_value(&env, "PIDASH_PROC"), None);
    }

    #[test]
    fn procargs2_rejects_short_buffers() {
        assert_eq!(parse_procargs2(&[1, 0]), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn reads_own_process_info_and_environment() {
        let me = std::process::id() as i32;
        let info = info(me).expect("own process is readable");
        assert_eq!(info.uid, current_uid());
        assert!(!info.zombie);
        assert!(info.start_time_ms > 0);
        // The test harness always has PATH.
        let env = environ(me).expect("own environment is readable");
        assert!(env_value(&env, "PATH").is_some());
        assert!(group_members(info.pgid).iter().any(|p| p.pid == me));
    }
}
