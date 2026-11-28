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
//!   A group scan is complete or an error: a listed process that cannot
//!   be read, or a `/proc` mounted with `hidepid=2`, could hide a member.
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
    /// its pid and group membership, but can never run again. On Linux a
    /// process whose leader thread exited is a zombie only once none of
    /// its threads can run.
    pub zombie: bool,
    /// Short command name, for logs only.
    pub name: String,
}

/// Whether this platform can inspect processes at all. Where it cannot,
/// every [`group_members`] call fails, so no group is ever proven.
pub const SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "macos"));

/// Inspect one process. `None` when it does not exist or cannot be read.
pub fn info(pid: i32) -> Option<ProcInfo> {
    if pid <= 0 {
        return None;
    }
    imp::info(pid)
}

/// Every process currently in group `pgid`, zombies included.
///
/// `Ok(empty)` means the kernel confirmed the group has no member; `Err`
/// means the group could not be inspected, which callers must treat as
/// "unknown" — neither empty nor proven.
pub fn group_members(pgid: i32) -> std::io::Result<Vec<ProcInfo>> {
    if pgid <= 0 {
        return Ok(Vec::new());
    }
    imp::group_members(pgid)
}

/// Parse `(state, pgid, starttime, comm)` out of a Linux `/proc/<pid>/stat`
/// record. The command name sits in parentheses and may itself contain
/// spaces, parentheses or bytes that are not UTF-8, so fields are counted
/// from the last `)` and only the name is decoded (lossily, for logs).
pub fn parse_linux_stat(stat: &[u8]) -> Option<(char, i32, u64, String)> {
    let open = stat.iter().position(|b| *b == b'(')?;
    let close = stat.iter().rposition(|b| *b == b')')?;
    let comm = String::from_utf8_lossy(stat.get(open + 1..close)?).into_owned();
    // Everything after the name is ASCII; a stray byte fails the parse.
    let rest = std::str::from_utf8(&stat[close + 1..]).ok()?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After the comm: state(3) ppid(4) pgrp(5) ... starttime(22); index 0
    // is field 3.
    let state = fields.first()?.chars().next()?;
    let pgid = fields.get(2)?.parse().ok()?;
    let start = fields.get(19)?.parse().ok()?;
    Some((state, pgid, start, comm))
}

/// How many times a list fetch is retried when the list outgrew the buffer
/// between sizing it and reading it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const FETCH_ATTEMPTS: usize = 5;

/// Size a kernel list with `needed` (entry count), then read it with
/// `fetch` (buffer capacity in entries). A list that grew past the buffer
/// in between (`ENOMEM`) is re-sized and read again with more headroom;
/// any other failure is returned as an error, never as an empty list.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn fetch_retrying<T>(
    mut needed: impl FnMut() -> std::io::Result<usize>,
    mut fetch: impl FnMut(usize) -> std::io::Result<Vec<T>>,
) -> std::io::Result<Vec<T>> {
    let mut headroom = 8;
    let mut last = None;
    for _ in 0..FETCH_ATTEMPTS {
        match fetch(needed()? + headroom) {
            Ok(list) => return Ok(list),
            Err(e) if e.kind() == std::io::ErrorKind::OutOfMemory => {
                headroom *= 4;
                last = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::OutOfMemory)))
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

/// Whether a failed read of `/proc/<pid>/stat` means the process is gone
/// (it exited during the scan). Any other failure (`EACCES`, `EPERM` under
/// `hidepid=1`, an LSM denial) hides a process that may still be a member.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_entry_gone(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::NotFound || err.raw_os_error() == Some(3 /* ESRCH */)
}

/// Collect the members of `pgid` from the processes `pids`, read one at a
/// time with `read` (`Ok(None)`: the process is gone). The scan is complete
/// or it fails: an entry that exists but cannot be read makes the whole
/// scan an error, never a smaller group, since the hidden process may be a
/// live member (review of #3499).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn collect_group(
    pgid: i32,
    pids: impl IntoIterator<Item = i32>,
    read: impl Fn(i32) -> std::io::Result<Option<ProcInfo>>,
) -> std::io::Result<Vec<ProcInfo>> {
    let mut members = Vec::new();
    for pid in pids {
        if let Some(p) = read(pid)?
            && p.pgid == pgid
        {
            members.push(p);
        }
    }
    Ok(members)
}

/// Whether the `/proc` mount in `mountinfo` (the text of
/// `/proc/self/mountinfo`) hides other users' processes from listing
/// (`hidepid=2`/`invisible` or `hidepid=4`/`ptraceable`). Then a process
/// of the run that changed its user is not even listed, so a scan can
/// never show the group complete. `hidepid=1`/`noaccess` keeps entries
/// listed (their reads fail, which [`collect_group`] catches).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn proc_hides_pids(mountinfo: &str) -> bool {
    let mut hides = false;
    for line in mountinfo.lines() {
        let Some((pre, post)) = line.split_once(" - ") else {
            continue;
        };
        if pre.split_whitespace().nth(4) != Some("/proc") {
            continue;
        }
        let mut post = post.split_whitespace();
        if post.next() != Some("proc") {
            continue;
        }
        let opts = post.nth(1).unwrap_or("");
        // The last matching line is the mount on top.
        hides = opts.split(',').any(|o| {
            matches!(
                o.strip_prefix("hidepid="),
                Some("2" | "invisible" | "4" | "ptraceable")
            )
        });
    }
    hides
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{ProcInfo, collect_group, linux_entry_gone, parse_linux_stat, proc_hides_pids};

    pub fn info(pid: i32) -> Option<ProcInfo> {
        read_info(pid).ok().flatten()
    }

    /// `Ok(None)` when the process is gone; `Err` when it exists but its
    /// record cannot be read or parsed.
    fn read_info(pid: i32) -> std::io::Result<Option<ProcInfo>> {
        let stat = match std::fs::read(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat,
            Err(e) if linux_entry_gone(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
        let (state, pgid, start, name) = parse_linux_stat(&stat).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unparseable /proc/{pid}/stat"),
            )
        })?;
        Ok(Some(ProcInfo {
            pid,
            pgid,
            start,
            zombie: exited(state) && !has_live_thread(pid),
            name,
        }))
    }

    fn exited(state: char) -> bool {
        matches!(state, 'Z' | 'X')
    }

    /// Whether any thread of `pid` can still run. The stat of a process
    /// whose leader thread exited reads `Z` while its other threads keep
    /// running, so a `Z` leader is only dead once every thread is. A task
    /// list or thread that cannot be read counts as live: wrongly calling a process
    /// dead could report its group empty.
    fn has_live_thread(pid: i32) -> bool {
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
            return true;
        };
        tasks.flatten().any(|t| {
            std::fs::read(t.path().join("stat"))
                .ok()
                .and_then(|s| parse_linux_stat(&s))
                .is_none_or(|(state, ..)| !exited(state))
        })
    }

    /// Fails when `/proc` cannot be listed, when it hides other users'
    /// processes, or when a listed process cannot be read: each could hide
    /// a live member, so the group is unknown rather than smaller. A
    /// process that exited during the scan is skipped.
    pub fn group_members(pgid: i32) -> std::io::Result<Vec<ProcInfo>> {
        let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")?;
        if proc_hides_pids(&mountinfo) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "/proc is mounted with hidepid: a group scan cannot be complete",
            ));
        }
        let pids: Vec<i32> = std::fs::read_dir("/proc")?
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
            .collect();
        collect_group(pgid, pids, read_info)
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
    fn query(which: libc::c_int, arg: libc::c_int) -> std::io::Result<Vec<ProcInfo>> {
        let mib = [libc::CTL_KERN, libc::KERN_PROC, which, arg];
        let entry = std::mem::size_of::<KinfoProc>();
        let needed = || {
            let mut mib = mib;
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
                return Err(std::io::Error::last_os_error());
            }
            Ok(size / entry)
        };
        let fetch = |cap: usize| {
            let mut mib = mib;
            let mut buf: Vec<KinfoProc> = Vec::with_capacity(cap);
            let mut size = cap * entry;
            // SAFETY: the buffer holds `cap` entries and we pass its byte
            // size; the kernel writes at most `size` bytes and reports how
            // many.
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
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: the kernel initialised `size / entry` whole entries.
            unsafe { buf.set_len((size / entry).min(cap)) };
            Ok(buf.iter().map(convert).collect())
        };
        super::fetch_retrying(needed, fetch)
    }

    pub fn info(pid: i32) -> Option<ProcInfo> {
        query(libc::KERN_PROC_PID, pid)
            .ok()?
            .into_iter()
            .find(|p| p.pid == pid)
    }

    pub fn group_members(pgid: i32) -> std::io::Result<Vec<ProcInfo>> {
        Ok(query(libc::KERN_PROC_PGRP, pgid)?
            .into_iter()
            .filter(|p| p.pgid == pgid)
            .collect())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    use super::ProcInfo;

    pub fn info(_pid: i32) -> Option<ProcInfo> {
        None
    }

    pub fn group_members(_pgid: i32) -> std::io::Result<Vec<ProcInfo>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "process inspection is not available on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_stat_parses_fields_after_a_tricky_comm() {
        let line = b"1234 (a b) (c)) S 1 999 999 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 5555 0 0";
        assert_eq!(
            parse_linux_stat(line),
            Some(('S', 999, 5555, "a b) (c)".to_string()))
        );
        assert_eq!(parse_linux_stat(b"garbage"), None);
    }

    #[test]
    fn linux_stat_with_a_non_utf8_name_keeps_its_identity_fields() {
        // Review item (#3497): a command name that is not UTF-8 must not
        // discard the pid, pgid and start time around it.
        let mut line =
            b"1234 (bad\xff\xfename) S 1 999 999 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 5555 0 0"
                .to_vec();
        line.push(b'\n');
        let (state, pgid, start, name) = parse_linux_stat(&line).expect("identity parsed");
        assert_eq!((state, pgid, start), ('S', 999, 5555));
        assert!(name.starts_with("bad") && name.ends_with("name"), "{name}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_with_a_non_utf8_name_is_still_inspected() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join(std::ffi::OsStr::from_bytes(b"sl\xffep"));
        let sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .expect("sleep binary");
        std::os::unix::fs::symlink(sleep, &link).unwrap();
        let mut child = std::process::Command::new(&link)
            .arg("300")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let seen = info(pid);
        let listed = seen
            .as_ref()
            .and_then(|p| group_members(p.pgid).ok())
            .is_some_and(|m| m.iter().any(|p| p.pid == pid));
        let _ = child.kill();
        let _ = child.wait();
        let seen = seen.expect("a non-UTF-8 name must not hide the process");
        assert!(listed, "group listing lost the process");
        assert!(seen.start > 0);
    }

    /// Review item 3 on #3499: a Linux thread-group leader that has exited
    /// shows `Z` in `/proc/<pid>/stat` while its other threads still run.
    /// Such a process is alive and must not be reported as a zombie.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_exited_leader_with_running_threads_is_not_a_zombie() {
        use std::io::BufRead;
        use std::os::unix::process::CommandExt;
        let script = "import threading, time, ctypes\n\
                      threading.Thread(target=lambda: time.sleep(300)).start()\n\
                      print('ready', flush=True)\n\
                      ctypes.CDLL(None).pthread_exit(None)\n";
        let Ok(mut child) = std::process::Command::new("python3")
            .args(["-c", script])
            .stdout(std::process::Stdio::piped())
            .process_group(0)
            .spawn()
        else {
            eprintln!("python3 not available; skipping");
            return;
        };
        let pid = child.id() as i32;
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        // Wait until the leader thread itself has exited.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::fs::read(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|s| parse_linux_stat(&s))
            .is_none_or(|(state, ..)| state != 'Z')
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let seen = info(pid);
        let listed = group_members(pid).unwrap();
        let _ = child.kill();
        let _ = child.wait();
        let seen = seen.expect("process readable");
        assert!(!seen.zombie, "a process with a running thread is alive");
        assert!(
            listed.iter().any(|p| p.pid == pid && !p.zombie),
            "{listed:?}"
        );
    }

    /// The hand-written `kinfo_proc` layout must put `p_starttime` where
    /// the kernel does: a just-spawned child's start time (µs since the
    /// epoch) must fall between the wall-clock times around its spawn.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_start_time_matches_the_wall_clock_at_spawn() {
        let micros = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64
        };
        let before = micros();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let after = micros();
        let seen = info(child.id() as i32);
        let _ = child.kill();
        let _ = child.wait();
        let start = seen.expect("child readable").start;
        // One second of slack for clock granularity.
        assert!(
            start + 1_000_000 >= before && start <= after + 1_000_000,
            "start {start} outside [{before}, {after}]"
        );
    }

    fn enomem() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::OutOfMemory)
    }

    #[test]
    fn a_list_that_outgrew_its_buffer_is_fetched_again() {
        // Review item (#3497): a group that grows between sizing and
        // reading must be re-read, not reported as empty.
        let mut fetches = 0;
        let got = fetch_retrying(
            || Ok(2),
            |_cap| {
                fetches += 1;
                if fetches == 1 {
                    Err(enomem())
                } else {
                    Ok(vec![1, 2, 3])
                }
            },
        );
        assert_eq!(got.unwrap(), vec![1, 2, 3]);
        assert_eq!(fetches, 2);
    }

    #[test]
    fn a_failed_inspection_is_an_error_not_an_empty_group() {
        let got: std::io::Result<Vec<u8>> = fetch_retrying(
            || Ok(1),
            |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        );
        assert!(got.is_err());
        let got: std::io::Result<Vec<u8>> = fetch_retrying(|| Ok(1), |_| Err(enomem()));
        assert!(got.is_err(), "endless growth must end in an error");
        let got: std::io::Result<Vec<u8>> = fetch_retrying(
            || Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            |_| Ok(vec![1]),
        );
        assert!(got.is_err());
    }

    fn member(pid: i32, pgid: i32) -> ProcInfo {
        ProcInfo {
            pid,
            pgid,
            start: 100,
            zombie: false,
            name: "m".into(),
        }
    }

    /// Review item 3 on #3499: a listed process that cannot be read (for
    /// example `EPERM` under `hidepid=1` after it changed user) may be a
    /// live member of the run's group, so the scan is an error, never a
    /// group that looks empty except for the anchor.
    #[test]
    fn an_unreadable_process_makes_the_group_scan_an_error() {
        let read = |pid: i32| match pid {
            1 => Ok(Some(member(1, 1))),
            2 => Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            _ => Ok(None),
        };
        assert!(collect_group(1, [1, 2], read).is_err());
        let eperm = |_pid: i32| Err(std::io::Error::from_raw_os_error(1));
        assert!(collect_group(1, [7], eperm).is_err());
    }

    #[test]
    fn a_process_that_exited_during_the_scan_is_skipped() {
        let read = |pid: i32| match pid {
            1 => Ok(Some(member(1, 1))),
            2 => Ok(Some(member(2, 9))),
            _ => Ok(None),
        };
        let got = collect_group(1, [1, 2, 3], read).unwrap();
        assert_eq!(got, vec![member(1, 1)]);
        assert!(linux_entry_gone(&std::io::Error::from(
            std::io::ErrorKind::NotFound
        )));
        assert!(linux_entry_gone(&std::io::Error::from_raw_os_error(3)));
        assert!(!linux_entry_gone(&std::io::Error::from_raw_os_error(1)));
        assert!(!linux_entry_gone(&std::io::Error::from_raw_os_error(13)));
    }

    #[test]
    fn a_proc_mount_that_hides_pids_is_detected() {
        let line = |opts: &str| {
            format!(
                "22 1 0:5 / /sys rw - sysfs sysfs rw\n\
                 25 1 0:23 / /proc rw,nosuid,nodev,noexec,relatime shared:13 - proc proc {opts}\n"
            )
        };
        assert!(!proc_hides_pids(&line("rw")));
        assert!(!proc_hides_pids(&line("rw,hidepid=0")));
        assert!(!proc_hides_pids(&line("rw,hidepid=1")));
        assert!(!proc_hides_pids(&line("rw,hidepid=noaccess")));
        assert!(proc_hides_pids(&line("rw,hidepid=2")));
        assert!(proc_hides_pids(&line("rw,hidepid=invisible,gid=10")));
        assert!(proc_hides_pids(&line("rw,hidepid=ptraceable")));
        // A proc mounted elsewhere does not count; the top /proc mount does.
        assert!(!proc_hides_pids(
            "30 1 0:40 / /mnt/proc rw - proc proc rw,hidepid=2\n"
        ));
        let over = format!(
            "{}31 25 0:41 / /proc rw - proc proc rw\n",
            line("rw,hidepid=2")
        );
        assert!(!proc_hides_pids(&over));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn this_hosts_own_group_scan_is_complete() {
        let me = std::process::id() as i32;
        let pgid = info(me).expect("own process").pgid;
        let got = group_members(pgid).expect("scan of a normal /proc");
        assert!(got.iter().any(|p| p.pid == me));
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
        assert!(group_members(0).unwrap().is_empty());
        assert!(group_members(-1).unwrap().is_empty());
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
        let members = group_members(a.pgid).unwrap();
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
            let m = group_members(pid).unwrap();
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
