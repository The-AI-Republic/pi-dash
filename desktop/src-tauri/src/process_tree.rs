// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A child process whose descendants die with it.
//!
//! The daemon tears its agent engine down from destructors (`kill_on_drop`),
//! and destructors do not run in a process that is killed outright — which is
//! how the daemon goes whenever it outlives the grace period, and always on
//! Windows. Killing only the daemon's pid therefore leaves the engine, and
//! whatever the engine started, running after the app is gone.
//!
//! So the daemon is started inside a kernel-level grouping that one call can
//! take down regardless of what state the daemon itself is in: its own session
//! (and so process group) on Unix, a Job Object on Windows.
//!
//! Deliberately free of Tauri types so the teardown can be tested against a
//! real process tree.

use std::process::{Child, Command};
use std::time::Duration;

/// A spawned process that owns its descendants. Dropping it kills all of them.
pub struct ProcessTree {
    child: Child,
    #[cfg(windows)]
    job: Option<job::Job>,
}

impl ProcessTree {
    /// Spawn `cmd` as the root of a new tree.
    pub fn spawn(cmd: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            // The root leads a new session, and with it a new process group
            // that every descendant inherits unless it deliberately leaves, so
            // the group id (the root's pid) names the whole tree.
            //
            // A new session rather than just a new group: launched from a
            // terminal, a group of its own would be a background job of that
            // terminal, and the `bash -i` the daemon starts its engine through
            // stops its whole group — the daemon included — when it finds
            // itself in one. A new session has no terminal to be a job of.
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let child = cmd.spawn()?;
        #[cfg(windows)]
        let job = {
            // Assigned right after the spawn rather than atomically with it:
            // std exposes no way to start a process suspended. Only processes
            // created after the assignment join the job, which is every one
            // that matters — the daemon starts no engine before it has read
            // its config and been handed work.
            //
            // A tree we cannot group is still better than no daemon, so a
            // failure here degrades to killing the root alone.
            match job::Job::containing(&child) {
                Ok(job) => Some(job),
                Err(e) => {
                    eprintln!("process tree: no job object, children may outlive the daemon: {e}");
                    None
                }
            }
        };
        Ok(Self {
            child,
            #[cfg(windows)]
            job,
        })
    }

    /// Whether the root process is still running.
    pub fn is_running(&mut self) -> bool {
        #[cfg(unix)]
        {
            // Observe the exit without reaping it. Once the root is reaped its
            // pid — and with it the group id — can be handed to an unrelated
            // process, and `Drop` still has to signal the group.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            // With WNOHANG a still-running child leaves `si_pid` at zero.
            rc == 0 && unsafe { info.si_pid() } == 0
        }
        #[cfg(not(unix))]
        {
            matches!(self.child.try_wait(), Ok(None))
        }
    }

    /// Ask the root to exit, wait up to `grace` for it, then kill the tree.
    ///
    /// `grace` applies on Unix only. Windows has no way to ask this daemon to
    /// exit: it is spawned with CREATE_NO_WINDOW so a GUI app never flashes a
    /// console, and a process with no console cannot receive console control
    /// events — GenerateConsoleCtrlEvent only reaches a process group sharing
    /// the caller's console. A real graceful stop there needs an out-of-band
    /// channel the daemon listens on; none exists today. Until one does,
    /// waiting would only delay a kill that is going to happen regardless, so
    /// the stop is immediate rather than hanging every app exit for `grace`.
    #[cfg_attr(not(unix), allow(unused_mut))]
    pub fn stop(mut self, grace: Duration) {
        #[cfg(unix)]
        {
            // To the root alone, not the group: SIGTERM asks the daemon to
            // finish its current run and shut down cleanly, and that shutdown
            // is what reports the run's outcome. Its children are its to stop.
            unsafe {
                libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
            }
            // Poll rather than sleeping the full grace period: a daemon with
            // nothing in flight exits immediately and the user should not wait
            // for a timer.
            let deadline = std::time::Instant::now() + grace;
            while self.is_running() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        #[cfg(not(unix))]
        let _ = grace;
        // Dropping `self` kills whatever is left, root included.
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            // Everything still in the group, whether or not the root got as
            // far as stopping its own children. The root has not been reaped
            // yet (see `is_running`), so the id cannot have been reused.
            libc::killpg(self.child.id() as libc::pid_t, libc::SIGKILL);
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        // On Unix the root is already dead and this is a no-op; on Windows it
        // is the fallback for a tree that never made it into a job.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };

    /// An anonymous Job Object. Closing the last handle to it kills every
    /// process inside, so the tree dies with this app even when the app is
    /// itself killed and never gets to run its exit handler.
    pub struct Job(HANDLE);

    // A job handle is a process-wide kernel reference with no thread affinity.
    unsafe impl Send for Job {}

    impl Job {
        pub fn containing(child: &Child) -> std::io::Result<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            // Owned from here on, so every early return below closes it.
            let job = Self(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if configured == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(job)
        }

        pub fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::process::{ChildStdout, Stdio};
    use std::sync::mpsc;
    use std::time::Instant;

    /// A stand-in daemon: a shell that starts one long-lived child (the
    /// "engine"), prints that child's pid, then runs `then`.
    ///
    /// The engine inherits the shell's stdout, so the pipe reaches EOF only
    /// once *every* process in the tree is gone. That is what the tests wait
    /// on — unlike probing a pid, it cannot be fooled by a zombie or by the
    /// pid being reused.
    fn daemon_with_engine(prelude: &str, then: &str) -> (ProcessTree, BufReader<ChildStdout>, i32) {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(format!("{prelude} sleep 300 & echo $!; {then}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        let mut tree = ProcessTree::spawn(&mut cmd).expect("spawn stand-in daemon");
        let mut stdout = BufReader::new(tree.child.stdout.take().expect("piped stdout"));
        let mut line = String::new();
        stdout.read_line(&mut line).expect("read engine pid");
        let engine = line.trim().parse().expect("engine pid");
        (tree, stdout, engine)
    }

    /// Whether the whole tree is gone within a few seconds. Kills the engine
    /// on a miss so a failing run does not leave a `sleep 300` behind.
    fn tree_is_gone(mut stdout: BufReader<ChildStdout>, engine: i32) -> bool {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut rest = Vec::new();
            let _ = stdout.read_to_end(&mut rest);
            let _ = tx.send(());
        });
        let gone = rx.recv_timeout(Duration::from_secs(5)).is_ok();
        if !gone {
            unsafe {
                libc::kill(engine, libc::SIGKILL);
            }
        }
        gone
    }

    /// The root must lead its own session, not merely its own group: as a
    /// background job of a launching terminal, the daemon would be stopped by
    /// the interactive shell it starts its engine through.
    #[test]
    fn the_root_leads_its_own_session() {
        let (tree, stdout, engine) = daemon_with_engine("", "wait");
        let root = tree.child.id() as libc::pid_t;
        assert_eq!(unsafe { libc::getsid(root) }, root);
        assert_eq!(unsafe { libc::getpgid(engine) }, root);
        drop(tree);
        assert!(
            tree_is_gone(stdout, engine),
            "the engine outlived its daemon"
        );
    }

    /// The reported bug: a daemon still busy when the grace period runs out is
    /// killed outright, never runs its destructors, and so never stops its
    /// engine. The engine must not outlive it (PIDESKAPP-31).
    #[test]
    fn stop_kills_the_engine_of_a_daemon_that_outlives_the_grace() {
        // Ignoring SIGTERM is the daemon that is still mid-run at the deadline.
        let (tree, stdout, engine) = daemon_with_engine("trap '' TERM;", "wait");
        tree.stop(Duration::from_millis(300));
        assert!(
            tree_is_gone(stdout, engine),
            "the engine outlived its daemon"
        );
    }

    /// A daemon that honours SIGTERM is not made to sit out the grace period,
    /// and whatever it left running is still cleaned up.
    #[test]
    fn stop_returns_as_soon_as_the_daemon_exits() {
        let (tree, stdout, engine) = daemon_with_engine("", "wait");
        let started = Instant::now();
        tree.stop(Duration::from_secs(30));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "stop waited out the grace period for a daemon that had already exited"
        );
        assert!(
            tree_is_gone(stdout, engine),
            "the engine outlived its daemon"
        );
    }

    /// A daemon that died on its own leaves the same orphans; letting go of
    /// its handle cleans them up too.
    #[test]
    fn dropping_a_dead_daemon_kills_the_engine_it_left_behind() {
        let (mut tree, stdout, engine) = daemon_with_engine("", "exit 0");
        let deadline = Instant::now() + Duration::from_secs(5);
        while tree.is_running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!tree.is_running(), "stand-in daemon did not exit");
        drop(tree);
        assert!(
            tree_is_gone(stdout, engine),
            "the engine outlived its daemon"
        );
    }
}
