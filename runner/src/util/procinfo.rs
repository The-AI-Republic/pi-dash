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
//! * macOS: `proc_listpids` / `proc_pidinfo(PROC_PIDTBSDINFO)`,
//!   `sysctl(KERN_PROC_PGRP)` for group members and `sysctl(KERN_PROCARGS2)`.
//!   `proc_pidinfo` returns nothing for a zombie, so group members are listed
//!   with `KERN_PROC_PGRP` (whose `kinfo_proc` entries include zombies) and
//!   `proc_pidinfo` is used only for the details of live members.
//! * Elsewhere: nothing is readable, so callers fall back to "do not signal".
//!
//! macOS strips other processes' environments: `KERN_PROCARGS2` for another
//! process (same uid, even an unsigned binary) succeeds but returns only argc,
//! the exec path and argv, with no environment (`ps -E` shows none either).
//! So on macOS an environment read is never evidence of ownership:
//! [`environ`] reports a successful read with no entries as unreadable, and
//! [`ENV_OWNERSHIP_PROOF`] is false, so ownership checks there use pid and
//! start times only. Environment-based escapee detection (design §4.4) is
//! Linux only for the same reason.

/// Whether a process environment read can prove (or disprove) that a process
/// carries our tag. True only where other same-uid processes' environments are
/// actually readable (Linux). See the module docs for macOS.
pub const ENV_OWNERSHIP_PROOF: bool = cfg!(target_os = "linux");

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

/// Every process currently in process group `pgid`, zombies included.
///
/// Zombies matter: on macOS `kill(-pgid, _)` answers EPERM for a group whose
/// only members are zombies, and the caller can only tell that apart from a
/// real permission failure by seeing them here.
pub fn group_members(pgid: i32) -> Vec<ProcInfo> {
    imp::group_members(pgid)
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

/// Combine a kernel group listing that includes zombies (macOS
/// `KERN_PROC_PGRP`) with per-process details that only exist for live
/// processes (macOS `proc_pidinfo`).
///
/// * A zombie keeps its listed values: it can never run again, and the live
///   lookup cannot see it.
/// * A live entry takes the live details when the lookup still places it in
///   `pgid`, and is dropped when it has moved to another group since.
/// * A live entry the lookup cannot read (it exited or became a zombie in
///   between) keeps its listed values and stays live, so a caller never
///   mistakes it for gone; the next poll sees its new state.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn merge_group_members(
    pgid: i32,
    listed: Vec<ProcInfo>,
    live: impl Fn(i32) -> Option<ProcInfo>,
) -> Vec<ProcInfo> {
    listed
        .into_iter()
        .filter(|p| p.pgid == pgid)
        .filter_map(|p| {
            if p.zombie {
                return Some(p);
            }
            match live(p.pid) {
                Some(l) if l.pgid == pgid => Some(l),
                Some(_) => None,
                None => Some(p),
            }
        })
        .collect()
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

    pub fn group_members(pgid: i32) -> Vec<ProcInfo> {
        all_pids()
            .into_iter()
            .filter_map(info)
            .filter(|p| p.pgid == pgid)
            .collect()
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

    /// `struct extern_proc` from `<sys/proc.h>` (64-bit layout). `libc` does
    /// not export `kinfo_proc` for Apple targets; only `p_starttime`,
    /// `p_stat` and `p_pid` are read, the rest is layout.
    #[repr(C)]
    struct ExternProc {
        p_starttime: libc::timeval,
        p_vmspace: *mut libc::c_void,
        p_sigacts: *mut libc::c_void,
        p_flag: libc::c_int,
        p_stat: libc::c_char,
        p_pid: libc::pid_t,
        p_oppid: libc::pid_t,
        p_dupfd: libc::c_int,
        user_stack: *mut libc::c_char,
        exit_thread: *mut libc::c_void,
        p_debugger: libc::c_int,
        sigwait: libc::c_int,
        p_estcpu: libc::c_uint,
        p_cpticks: libc::c_int,
        p_pctcpu: u32,
        p_wchan: *mut libc::c_void,
        p_wmesg: *mut libc::c_char,
        p_swtime: libc::c_uint,
        p_slptime: libc::c_uint,
        p_realtimer: [libc::timeval; 2],
        p_rtime: libc::timeval,
        p_uticks: u64,
        p_sticks: u64,
        p_iticks: u64,
        p_traceflag: libc::c_int,
        p_tracep: *mut libc::c_void,
        p_siglist: libc::c_int,
        p_textvp: *mut libc::c_void,
        p_holdcnt: libc::c_int,
        p_sigmask: u32,
        p_sigignore: u32,
        p_sigcatch: u32,
        p_priority: u8,
        p_usrpri: u8,
        p_nice: libc::c_char,
        p_comm: [libc::c_char; 17],
        p_pgrp: *mut libc::c_void,
        p_addr: *mut libc::c_void,
        p_xstat: u16,
        p_acflag: u16,
        p_ru: *mut libc::c_void,
    }

    /// `struct _pcred` from `<sys/sysctl.h>`.
    #[repr(C)]
    struct Pcred {
        pc_lock: [libc::c_char; 72],
        pc_ucred: *mut libc::c_void,
        p_ruid: u32,
        p_svuid: u32,
        p_rgid: u32,
        p_svgid: u32,
        p_refcnt: libc::c_int,
    }

    /// `struct _ucred` from `<sys/sysctl.h>`.
    #[repr(C)]
    struct Ucred {
        cr_ref: i32,
        cr_uid: u32,
        cr_ngroups: libc::c_short,
        cr_groups: [u32; 16],
    }

    /// `struct vmspace` from `<sys/vm.h>`.
    #[repr(C)]
    struct Vmspace {
        vm_refcnt: i32,
        vm_shm: *mut libc::c_char,
        vm_sizes: [i32; 5],
        vm_addrs: [*mut libc::c_char; 3],
    }

    /// `struct eproc` from `<sys/sysctl.h>` (64-bit layout). Only `e_pgid`
    /// and `e_ucred.cr_uid` are read.
    #[repr(C)]
    struct Eproc {
        e_paddr: *mut libc::c_void,
        e_sess: *mut libc::c_void,
        e_pcred: Pcred,
        e_ucred: Ucred,
        e_vm: Vmspace,
        e_ppid: libc::pid_t,
        e_pgid: libc::pid_t,
        e_jobc: libc::c_short,
        e_tdev: i32,
        e_tpgid: libc::pid_t,
        e_tsess: *mut libc::c_void,
        e_wmesg: [libc::c_char; 8],
        e_xsize: i32,
        e_xrssize: libc::c_short,
        e_xccount: libc::c_short,
        e_xswrss: libc::c_short,
        e_flag: i32,
        e_login: [libc::c_char; 12],
        e_spare: [i32; 4],
    }

    #[repr(C)]
    struct KinfoProc {
        kp_proc: ExternProc,
        kp_eproc: Eproc,
    }

    // `sizeof(struct kinfo_proc)` is 648 on 64-bit macOS (x86_64 and arm64).
    const _: () = assert!(std::mem::size_of::<KinfoProc>() == 648);
    const _: () = assert!(std::mem::offset_of!(KinfoProc, kp_eproc) == 296);
    const _: () = assert!(std::mem::offset_of!(ExternProc, p_stat) == 36);
    const _: () = assert!(std::mem::offset_of!(ExternProc, p_pid) == 40);
    const _: () = assert!(std::mem::offset_of!(Eproc, e_ucred) == 120);
    const _: () = assert!(std::mem::offset_of!(Eproc, e_pgid) == 268);

    /// Every process in group `pgid`, zombies included, via
    /// `sysctl({CTL_KERN, KERN_PROC, KERN_PROC_PGRP, pgid})`. Live members
    /// take their details from `proc_pidinfo`; zombies (which `proc_pidinfo`
    /// cannot see) keep the `kinfo_proc` values.
    pub fn group_members(pgid: i32) -> Vec<ProcInfo> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PGRP, pgid];
        let mut size: libc::size_t = 0;
        // SAFETY: a null buffer asks for the required size only.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                4,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return Vec::new();
        }
        let entry = std::mem::size_of::<KinfoProc>();
        // Headroom for processes that join between the two calls.
        let cap = size / entry + 8;
        let mut buf: Vec<KinfoProc> = Vec::with_capacity(cap);
        let mut size = cap * entry;
        // SAFETY: the buffer holds `cap` entries and we pass its byte size;
        // the kernel writes at most `size` bytes and reports how many.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                4,
                buf.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return Vec::new();
        }
        // SAFETY: the kernel initialised `size / entry` whole entries.
        unsafe { buf.set_len((size / entry).min(cap)) };
        let listed = buf
            .iter()
            .map(|k| {
                let tv = k.kp_proc.p_starttime;
                ProcInfo {
                    pid: k.kp_proc.p_pid,
                    pgid: k.kp_eproc.e_pgid,
                    uid: k.kp_eproc.e_ucred.cr_uid,
                    start_time_ms: (tv.tv_sec as u64) * 1000 + (tv.tv_usec as u64) / 1000,
                    zombie: k.kp_proc.p_stat as u32 == libc::SZOMB,
                }
            })
            .collect();
        super::merge_group_members(pgid, listed, info)
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
        // macOS returns argv but no environment for other processes: an
        // empty environment means "not readable", never "no tag".
        parse_procargs2(&buf).filter(|env| !env.is_empty())
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

    pub fn group_members(_pgid: i32) -> Vec<ProcInfo> {
        Vec::new()
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

    fn proc(pid: i32, pgid: i32, start: u64, zombie: bool) -> ProcInfo {
        ProcInfo {
            pid,
            pgid,
            uid: 501,
            start_time_ms: start,
            zombie,
        }
    }

    #[test]
    fn group_listing_keeps_zombies_the_live_lookup_cannot_see() {
        // macOS: KERN_PROC_PGRP lists zombies, proc_pidinfo returns nothing
        // for them. A zombie must stay in the member list.
        let listed = vec![
            proc(10, 10, 1_000, true),
            proc(11, 10, 2_000, false),
            proc(12, 10, 3_000, false),
            proc(13, 10, 4_000, false),
            proc(14, 99, 5_000, false),
        ];
        let live = |pid: i32| match pid {
            // Live details win over the listing.
            11 => Some(proc(11, 10, 2_500, false)),
            // Moved to another group since the listing: not a member.
            12 => Some(proc(12, 77, 3_000, false)),
            // 13 exited between the two reads: stays a live member.
            _ => None,
        };
        let got = merge_group_members(10, listed, live);
        assert_eq!(
            got,
            vec![
                proc(10, 10, 1_000, true),
                proc(11, 10, 2_500, false),
                proc(13, 10, 4_000, false),
            ]
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn an_unreaped_zombie_is_listed_as_a_group_member() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();
        // Not reaped yet: the group's only member is a zombie.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let members = loop {
            let m = group_members(pid);
            if m.iter().any(|p| p.zombie) || std::time::Instant::now() > deadline {
                break m;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let _ = child.wait();
        assert_eq!(members.len(), 1, "{members:?}");
        assert_eq!(members[0].pid, pid);
        assert!(members[0].zombie, "{members:?}");
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
