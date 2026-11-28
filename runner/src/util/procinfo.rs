//! Read-only process inspection for ownership-proven group signalling
//! (PDASHOSS01-417, part 1; adapted from PR #2952's `procinfo`).
//!
//! [`crate::util::containment`]'s process-group backend uses this to prove
//! that a recorded process group is still the one the run created before
//! it signals it. A process is identified by its pid together with its
//! **exact** kernel start time ([`ProcInfo::start`]): a reused pid has a
//! different start time, so it can never be mistaken for the original.
//!
//! * Linux: `/proc/<pid>/stat` (state, pgid, `starttime` in clock ticks
//!   since boot, compared raw — never converted, so never rounded).
//! * macOS: `sysctl(KERN_PROC_PID)` / `sysctl(KERN_PROC_PGRP)`. Both
//!   return `kinfo_proc`, so one source gives every start time
//!   (`p_starttime`, microseconds) and a comparison is always exact. The
//!   group listing includes zombies.
//! * Elsewhere: nothing is readable, so every proof fails and callers do
//!   not signal.

/// What we know about one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub pgid: i32,
    /// Kernel start time in platform units (Linux: clock ticks since boot;
    /// macOS: microseconds since the epoch). Opaque: only ever compared for
    /// exact equality with another value read on the same host.
    pub start: u64,
    /// Exited but not reaped yet, or (macOS) already exiting. Still holds
    /// its pid and group membership, but can never run again.
    pub zombie: bool,
    /// Short command name, for logs only.
    pub name: String,
}

/// Inspect one process. `None` when it does not exist or cannot be read.
pub fn info(pid: i32) -> Option<ProcInfo> {
    if pid <= 0 {
        return None;
    }
    imp::info(pid)
}

/// Every process currently in group `pgid`, zombies included.
pub fn group_members(pgid: i32) -> Vec<ProcInfo> {
    if pgid <= 0 {
        return Vec::new();
    }
    imp::group_members(pgid)
}

/// Parse `(state, pgid, starttime, comm)` out of a Linux `/proc/<pid>/stat`
/// line. The command name sits in parentheses and may itself contain spaces
/// or parentheses, so fields are counted from the last `)`.
pub fn parse_linux_stat(stat: &str) -> Option<(char, i32, u64, String)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_string();
    let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    // After the comm: state(3) ppid(4) pgrp(5) ... starttime(22); index 0
    // is field 3.
    let state = fields.first()?.chars().next()?;
    let pgid = fields.get(2)?.parse().ok()?;
    let start = fields.get(19)?.parse().ok()?;
    Some((state, pgid, start, comm))
}

/// `SZOMB` from macOS `<sys/proc.h>`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MACOS_SZOMB: u32 = 5;
/// `P_WEXIT` from macOS `<sys/proc.h>`: the process is working on exiting.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MACOS_P_WEXIT: i32 = 0x0000_2000;

/// Whether a macOS `kinfo_proc` entry can never run again: a zombie, or a
/// process still exiting (right after SIGKILL macOS reports `SRUN` with
/// `P_WEXIT` before `SZOMB`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_exited_or_exiting(p_stat: u32, p_flag: i32) -> bool {
    p_stat == MACOS_SZOMB || p_flag & MACOS_P_WEXIT != 0
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{ProcInfo, parse_linux_stat};

    pub fn info(pid: i32) -> Option<ProcInfo> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (state, pgid, start, name) = parse_linux_stat(&stat)?;
        Some(ProcInfo {
            pid,
            pgid,
            start,
            zombie: matches!(state, 'Z' | 'X'),
            name,
        })
    }

    pub fn group_members(pgid: i32) -> Vec<ProcInfo> {
        let Ok(rd) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        rd.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
            .filter_map(info)
            .filter(|p| p.pgid == pgid)
            .collect()
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod imp {
    use super::ProcInfo;

    /// `struct extern_proc` from `<sys/proc.h>` (64-bit layout). `libc` does
    /// not export `kinfo_proc` for Apple targets; only `p_starttime`,
    /// `p_flag`, `p_stat`, `p_pid` and `p_comm` are read, the rest is layout.
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

    /// `struct eproc` from `<sys/sysctl.h>` (64-bit layout); only `e_pgid`
    /// is read. `e_pcred`, `e_ucred` and `e_vm` are opaque blobs of their
    /// C sizes (8-byte aligned through the leading pointers).
    #[repr(C)]
    struct Eproc {
        e_paddr: *mut libc::c_void,
        e_sess: *mut libc::c_void,
        e_pcred: [u64; 13],
        e_ucred: [u32; 19],
        e_vm: [u64; 8],
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

    // `sizeof(struct kinfo_proc)` is 648 on 64-bit macOS (x86_64 and arm64);
    // offsets from <sys/sysctl.h> and <sys/proc.h>.
    const _: () = assert!(std::mem::size_of::<KinfoProc>() == 648);
    const _: () = assert!(std::mem::offset_of!(KinfoProc, kp_eproc) == 296);
    const _: () = assert!(std::mem::offset_of!(ExternProc, p_flag) == 32);
    const _: () = assert!(std::mem::offset_of!(ExternProc, p_stat) == 36);
    const _: () = assert!(std::mem::offset_of!(ExternProc, p_pid) == 40);
    const _: () = assert!(std::mem::offset_of!(Eproc, e_ucred) == 120);
    const _: () = assert!(std::mem::offset_of!(Eproc, e_pgid) == 268);
    const _: () = assert!(super::MACOS_SZOMB == libc::SZOMB);

    fn convert(k: &KinfoProc) -> ProcInfo {
        let tv = k.kp_proc.p_starttime;
        let comm = &k.kp_proc.p_comm;
        let len = comm.iter().position(|c| *c == 0).unwrap_or(comm.len());
        let name: Vec<u8> = comm[..len].iter().map(|c| *c as u8).collect();
        ProcInfo {
            pid: k.kp_proc.p_pid,
            pgid: k.kp_eproc.e_pgid,
            start: (tv.tv_sec as u64) * 1_000_000 + tv.tv_usec as u64,
            zombie: super::macos_exited_or_exiting(k.kp_proc.p_stat as u32, k.kp_proc.p_flag),
            name: String::from_utf8_lossy(&name).into_owned(),
        }
    }

    /// `sysctl({CTL_KERN, KERN_PROC, which, arg})` as a list of entries.
    fn query(which: libc::c_int, arg: libc::c_int) -> Vec<ProcInfo> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, which, arg];
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
        buf.iter().map(convert).collect()
    }

    pub fn info(pid: i32) -> Option<ProcInfo> {
        query(libc::KERN_PROC_PID, pid)
            .into_iter()
            .find(|p| p.pid == pid)
    }

    pub fn group_members(pgid: i32) -> Vec<ProcInfo> {
        query(libc::KERN_PROC_PGRP, pgid)
            .into_iter()
            .filter(|p| p.pgid == pgid)
            .collect()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    use super::ProcInfo;

    pub fn info(_pid: i32) -> Option<ProcInfo> {
        None
    }

    pub fn group_members(_pgid: i32) -> Vec<ProcInfo> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_stat_parses_fields_after_a_tricky_comm() {
        let line = "1234 (a b) (c)) S 1 999 999 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 5555 0 0";
        assert_eq!(
            parse_linux_stat(line),
            Some(('S', 999, 5555, "a b) (c)".to_string()))
        );
        assert_eq!(parse_linux_stat("garbage"), None);
    }

    #[test]
    fn an_exiting_macos_entry_counts_as_unable_to_run() {
        const SRUN: u32 = 2;
        assert!(macos_exited_or_exiting(SRUN, MACOS_P_WEXIT));
        assert!(macos_exited_or_exiting(MACOS_SZOMB, 0));
        assert!(!macos_exited_or_exiting(SRUN, 0));
        assert!(!macos_exited_or_exiting(SRUN, 0x0000_0004));
    }

    #[test]
    fn non_positive_ids_are_never_inspected() {
        // kill(0)/kill(-1) semantics must never leak in through a bad id.
        assert_eq!(info(0), None);
        assert!(group_members(0).is_empty());
        assert!(group_members(-1).is_empty());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn reads_own_process_with_a_stable_exact_start_time() {
        let me = std::process::id() as i32;
        let a = info(me).expect("own process is readable");
        let b = info(me).expect("own process is readable");
        assert!(!a.zombie);
        assert!(a.start > 0);
        assert_eq!(a.start, b.start, "start time must be exact and stable");
        let members = group_members(a.pgid);
        let listed = members.iter().find(|p| p.pid == me).expect("listed");
        // Both reads come from one source, so the comparison is exact.
        assert_eq!(listed.start, a.start);
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
        assert!(members[0].zombie, "{members:?}");
    }
}
