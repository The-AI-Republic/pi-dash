//! Agent-spawn helper.
//!
//! On Unix-like hosts the runner lives inside a daemon started by
//! `systemd --user` on Linux and `launchd` on macOS. Both managers expose a
//! stripped `PATH` that does not include anything the user's shell rc adds
//! (nvm, pyenv, asdf, brew on Linux, …). A plain `Command::new("claude")`
//! therefore fails with `ENOENT` on machines where `claude` works
//! interactively.
//!
//! We sidestep this by wrapping Unix agent spawns in a login+interactive
//! `bash`. The `-i` flag is load-bearing: Debian/Ubuntu's stock `.bashrc`
//! (and nvm's default installer, which appends to `.bashrc`) short-circuits
//! for non-interactive shells via `case $- in *i*) ;; *) return;; esac`. A
//! pure `bash -lc` invocation therefore never reaches the nvm loader on a
//! default dev-machine setup. `bash -ilc` passes the guard, sources the
//! user's full interactive environment, and gives us the same PATH the
//! operator sees in their terminal.
//!
//! The script we pass to `-c` is:
//!   `[ -n "${PIDASH_AGENT_CWD-}" ] && cd -- "$PIDASH_AGENT_CWD"; exec "$@"`
//!
//! Two things earn their keep there:
//!   * `cd -- "$PIDASH_AGENT_CWD"` re-asserts the caller's requested cwd
//!     *after* rc files have run. Without it, any `cd` statement in the
//!     operator's `.bashrc` (surprisingly common — `cd ~/projects` at the
//!     end of an rc file, or a tmux "open in default dir" hook) silently
//!     overrides the `current_dir` that tokio set on the outer bash
//!     process, and the agent starts in the wrong directory. The `--`
//!     guards against cwd strings that start with a dash.
//!   * `exec "$@"` replaces bash with the target (so the spawned PID and
//!     any signals we deliver hit the agent, not a lingering shell) and
//!     preserves our structured argv without shell re-parsing of agent
//!     flags.
//!
//! On Windows, where the daemon is started as a per-user scheduled task, we
//! run the agent binary directly. That keeps native Windows installs working
//! without requiring a separate `bash.exe` installation.
//!
//! Running an interactive bash without a controlling TTY makes bash emit
//! two diagnostic lines to stderr during startup:
//!   `bash: cannot set terminal process group (-1): Inappropriate ioctl for device`
//!   `bash: no job control in this shell`
//! Those lines are harmless (bash just can't install a foreground job
//! group), but they're emitted before our `-c` script runs so they can't
//! be silenced from inside the script. Callers should filter them out of
//! any captured stderr via [`is_benign_login_shell_warning`]. We also
//! pin `LC_MESSAGES=C` on the spawned bash so those strings stay English
//! regardless of the host locale — the filter is exact-match and would
//! otherwise go stale under a non-C/non-English locale.

use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::util::containment::BackendKind;

tokio::task_local! {
    /// Per-run identity for the agent process, scoped to the task that
    /// drives one `AgentRun` (see `Supervisor::handle_assign`). Every bridge
    /// builds its agent `Command` through [`login_shell_command`] from
    /// inside that task, so reading it here is the one place that reaches
    /// all of them — the value lands on the child as `PIDASH_RUN_ID`, which
    /// the agent's `pidash` invocations forward on every write. Chat
    /// sessions never set it. Design: `.ai_design/ticking_relevance` §5.6.
    pub static RUN_ENV: RunEnv;
}

tokio::task_local! {
    /// Containment for a chat runtime's agents (PDASHOSS01-417), scoped to
    /// the task that spawns them. A chat session is not a run: it carries
    /// none of [`RUN_ENV`]'s run identity, only the process group of the
    /// session's containment unit, so the agents it starts can be stopped
    /// when the session ends. Ignored inside a [`RUN_ENV`] scope.
    pub static CHAT_ENV: ChatEnv;
}

/// What [`CHAT_ENV`] carries.
#[derive(Debug, Clone)]
pub struct ChatEnv {
    pub backend: BackendKind,
    /// The session's anchored process group, as [`RunEnv::process_group`].
    pub process_group: Option<i32>,
    /// Cgroup backend: the `cgroup.procs` of the unit's cgroup. The login
    /// shell writes its own pid there before it starts the agent, so every
    /// agent a chat spawns joins the unit, including one-shot engines that
    /// spawn a new process per turn (a move after spawn reaches only the
    /// first bridge's pid).
    pub cgroup_procs: Option<PathBuf>,
}

/// What [`RUN_ENV`] carries. Kept as a struct so more per-run values can
/// ride along without touching the bridges.
#[derive(Debug, Clone)]
pub struct RunEnv {
    pub run_id: uuid::Uuid,
    /// Which process-containment backend this run uses (NEWPIDASH-255).
    /// `login_shell_command` reads it to put the agent in its own process
    /// group when the backend is [`BackendKind::ProcessGroup`]; the
    /// cgroup backend needs no command-line help (the supervisor moves
    /// the spawned pid after spawn).
    pub backend: BackendKind,
    /// The run's anchored process group (process-group backend): agents
    /// join it rather than leading groups of their own, so the run's claim
    /// never depends on reading a pid after spawn (PDASHOSS01-417). `None`
    /// on other backends, or when no anchored group could be created.
    pub process_group: Option<i32>,
    /// Directory holding only a `pidash` symlink to the daemon's own binary
    /// (`<data_dir>/runtime/bin`, NEWPIDASH-363). Prepended to the agent's
    /// `PATH` after rc files run so the agent always invokes the daemon's
    /// build, never a shadowing `pidash` from the operator's `PATH`.
    pub runner_bin_dir: Option<PathBuf>,
    /// The daemon's own `PIDASH_CONFIG_DIR` / `PIDASH_DATA_DIR`, forwarded
    /// for every run (not only managed ones) so the agent's `pidash` calls
    /// land on the same tree the daemon uses.
    pub config_dir: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    /// Canonical path of the daemon binary itself; the agent sees it as
    /// [`RUNNER_EXE_ENV`] so the CLI can warn when it is not that binary.
    pub runner_exe: Option<PathBuf>,
    /// Issue identifier for issue runs (NEWPIDASH-320 §6.3). `None` for
    /// scheduler runs, chat sessions, and assigns from old servers.
    pub issue_identifier: Option<String>,
    /// Project identifier for issue and scheduler runs (NEWPIDASH-320
    /// §6.3). `None` for chat sessions and old servers.
    pub project_identifier: Option<String>,
    /// Agent display name plus model, e.g. `Claude Code (claude-opus-4-8)`
    /// (NEWPIDASH-320 §6.3). `None` for chat sessions.
    pub agent_name: Option<String>,
}

/// Environment variable name the agent reads for its run id.
pub const RUN_ID_ENV: &str = "PIDASH_RUN_ID";

/// Outer (daemon-set) name for the directory holding the daemon's own
/// `pidash` symlink. Re-exported onto `PATH` by [`SHELL_SCRIPT`] after the
/// login shell's rc files run, so it wins over anything they prepend.
pub const RUNNER_BIN_ENV: &str = "PIDASH_RUNNER_BIN";

/// Outer (daemon-set) name for the daemon binary's canonical path.
/// Re-exported as [`RUNNER_EXE_ENV`] by [`SHELL_SCRIPT`] after rc files run.
pub const RUNNER_EXE_OUTER_ENV: &str = "PIDASH_AGENT_RUNNER_EXE";

/// Agent-visible name of the daemon binary's canonical path. The CLI warns
/// on stderr when it runs under [`RUN_ID_ENV`] and its own canonical path
/// differs from this (NEWPIDASH-363 safety net).
pub const RUNNER_EXE_ENV: &str = "PIDASH_RUNNER_EXE";

/// Environment variable naming the run's issue (NEWPIDASH-320 §6.3).
pub const ISSUE_IDENTIFIER_ENV: &str = "PIDASH_ISSUE_IDENTIFIER";

/// Environment variable naming the run's project (NEWPIDASH-320 §6.3).
pub const PROJECT_ENV: &str = "PIDASH_PROJECT";

/// Environment variable naming the agent (NEWPIDASH-320 §6.3).
pub const AGENT_NAME_ENV: &str = "PIDASH_AGENT_NAME";

/// Apply the task-scoped per-run environment, when there is one.
///
/// The `PIDASH_*` namespacing is the rc-file hardening (see the module
/// docs): rc files set names like `CODEX_HOME` and `PATH`, never these,
/// so plain inheritance through the login shell is exactly as safe as
/// `PIDASH_RUN_ID` has always been.
fn apply_run_env(cmd: &mut Command) {
    if let Ok(env) = RUN_ENV.try_with(|e| e.clone()) {
        cmd.env(RUN_ID_ENV, env.run_id.to_string());
        if let Some(issue) = env.issue_identifier {
            cmd.env(ISSUE_IDENTIFIER_ENV, issue);
        }
        if let Some(project) = env.project_identifier {
            cmd.env(PROJECT_ENV, project);
        }
        if let Some(agent) = env.agent_name {
            cmd.env(AGENT_NAME_ENV, agent);
        }
        #[cfg(unix)]
        if env.backend == BackendKind::ProcessGroup {
            // The run's anchored group (or a group of its own when there
            // is none) so cleanup can signal the whole group. A child that
            // `setsid`s escapes it (documented limit of the process-group
            // backend).
            cmd.process_group(env.process_group.unwrap_or(0));
        }
        if let Some(dir) = &env.runner_bin_dir {
            cmd.env(RUNNER_BIN_ENV, dir.as_os_str());
        }
        if let Some(dir) = &env.config_dir {
            cmd.env("PIDASH_AGENT_CONFIG_DIR", dir.as_os_str());
        }
        if let Some(dir) = &env.data_dir {
            cmd.env("PIDASH_AGENT_DATA_DIR", dir.as_os_str());
        }
        if let Some(exe) = &env.runner_exe {
            cmd.env(RUNNER_EXE_OUTER_ENV, exe.as_os_str());
        }
        #[cfg(target_os = "windows")]
        apply_run_env_windows(cmd, &env);
    } else {
        apply_chat_env(cmd);
    }
}

/// Outside a run, put a chat runtime's agent in its session's group (see
/// [`CHAT_ENV`]).
#[cfg(unix)]
fn apply_chat_env(cmd: &mut Command) {
    let Ok(env) = CHAT_ENV.try_with(|e| e.clone()) else {
        return;
    };
    match env.backend {
        BackendKind::ProcessGroup => {
            cmd.process_group(env.process_group.unwrap_or(0));
        }
        // The login shell joins the cgroup itself before it starts the
        // agent ([`SHELL_SCRIPT`]), and refuses to start it otherwise.
        BackendKind::Cgroup => {
            if let Some(procs) = &env.cgroup_procs {
                cmd.env(CHAT_CGROUP_PROCS_ENV, procs.as_os_str());
            }
        }
        BackendKind::None => {}
    }
}

/// Where [`SHELL_SCRIPT`] writes its own pid to join a chat unit's cgroup
/// (see [`ChatEnv::cgroup_procs`]). Unset before the agent starts.
#[cfg(unix)]
const CHAT_CGROUP_PROCS_ENV: &str = "PIDASH_CHAT_CGROUP_PROCS";

#[cfg(not(unix))]
fn apply_chat_env(_cmd: &mut Command) {}

/// Windows half of [`apply_run_env`]: there is no shell, so nothing can
/// clobber what we set — export the real names directly instead of relying
/// on a re-export script.
#[cfg(target_os = "windows")]
fn apply_run_env_windows(cmd: &mut Command, env: &RunEnv) {
    if let Some(dir) = &env.runner_bin_dir {
        let existing = cmd
            .as_std()
            .get_envs()
            .find_map(|(k, v)| (k == "PATH").then_some(v).flatten())
            .map(|v| v.to_os_string())
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default();
        let mut parts = vec![dir.clone()];
        parts.extend(std::env::split_paths(&existing));
        if let Ok(joined) = std::env::join_paths(parts) {
            cmd.env("PATH", joined);
        }
    }
    if let Some(dir) = &env.config_dir {
        cmd.env("PIDASH_CONFIG_DIR", dir.as_os_str());
    }
    if let Some(dir) = &env.data_dir {
        cmd.env("PIDASH_DATA_DIR", dir.as_os_str());
    }
    if let Some(exe) = &env.runner_exe {
        cmd.env(RUNNER_EXE_ENV, exe.as_os_str());
    }
}

/// Re-assert Pi Dash's environment *after* the login shell's rc files have run.
///
/// `bash -ilc` sources `~/.bash_profile` / `~/.bashrc`, which happens after the
/// values we set with `Command::env` are applied — so a user with
/// `export CODEX_HOME=...` or a `PATH` rewrite in their profile would silently
/// win over the daemon. Passing our values under `PIDASH_*` names and
/// re-exporting them here is the same trick the working directory has always
/// used, extended to the Pi Dash Agent's environment and to the per-run
/// daemon pin (NEWPIDASH-363).
///
/// Every clause is a no-op when its `PIDASH_*` variable is unset, so a
/// run without that piece of per-run environment produces exactly the
/// command line it always did.
#[cfg(not(target_os = "windows"))]
const SHELL_SCRIPT: &str = r#"if [ -n "${PIDASH_CHAT_CGROUP_PROCS-}" ]; then printf '%s\n' "$$" >"$PIDASH_CHAT_CGROUP_PROCS" || { echo "pidash: could not join the chat session's cgroup" >&2; exit 125; }; unset PIDASH_CHAT_CGROUP_PROCS; fi
[ -n "${PIDASH_AGENT_CWD-}" ] && cd -- "$PIDASH_AGENT_CWD"
[ -n "${PIDASH_CODEX_HOME-}" ] && export CODEX_HOME="$PIDASH_CODEX_HOME"
[ -n "${PIDASH_AGENT_PATH_PREPEND-}" ] && export PATH="$PIDASH_AGENT_PATH_PREPEND:$PATH"
[ -n "${PIDASH_RUNNER_BIN-}" ] && export PATH="$PIDASH_RUNNER_BIN:$PATH"
[ -n "${PIDASH_AGENT_CONFIG_DIR-}" ] && export PIDASH_CONFIG_DIR="$PIDASH_AGENT_CONFIG_DIR"
[ -n "${PIDASH_AGENT_DATA_DIR-}" ] && export PIDASH_DATA_DIR="$PIDASH_AGENT_DATA_DIR"
[ -n "${PIDASH_AGENT_RUNNER_EXE-}" ] && export PIDASH_RUNNER_EXE="$PIDASH_AGENT_RUNNER_EXE"
[ -n "${PIDASH_MODEL_TOKEN_FILE-}" ] && [ -r "$PIDASH_MODEL_TOKEN_FILE" ] &&   export PIDASH_GATEWAY_TOKEN="$(cat "$PIDASH_MODEL_TOKEN_FILE")"
exec "$@""#;

/// Pi Dash-controlled environment for a spawned agent process.
///
/// All fields are `None` for a user-enrolled runner, which is what keeps that
/// path byte-identical. The Pi Dash Agent fills them from its `CodexSection`
/// so the bundled engine reads Pi Dash's own config, finds the bundled CLI,
/// and receives a credential that never touches `config.toml` or a command
/// line.
#[derive(Debug, Clone, Default)]
pub struct AgentEnv {
    /// Private `CODEX_HOME` for the agent.
    pub codex_home: Option<PathBuf>,
    /// Directory prepended to the agent's `PATH` (holds the bundled CLI).
    pub path_prepend: Option<PathBuf>,
    /// `PIDASH_CONFIG_DIR` for the agent's own `pidash` invocations.
    pub config_dir: Option<PathBuf>,
    /// `PIDASH_DATA_DIR` for the agent's own `pidash` invocations.
    pub data_dir: Option<PathBuf>,
    /// File the model credential is read from, once per spawn.
    pub model_token_file: Option<PathBuf>,
}

impl AgentEnv {
    pub fn is_empty(&self) -> bool {
        self.codex_home.is_none()
            && self.path_prepend.is_none()
            && self.config_dir.is_none()
            && self.data_dir.is_none()
            && self.model_token_file.is_none()
    }

    /// Apply to a [`Command`] under `PIDASH_*` names.
    ///
    /// On Unix the wrapper script re-exports them after rc files run; on
    /// Windows there is no shell, so the direct names are set as well.
    fn apply(&self, cmd: &mut Command) {
        if let Some(v) = &self.codex_home {
            cmd.env("PIDASH_CODEX_HOME", v.as_os_str());
        }
        if let Some(v) = &self.path_prepend {
            cmd.env("PIDASH_AGENT_PATH_PREPEND", v.as_os_str());
        }
        if let Some(v) = &self.config_dir {
            cmd.env("PIDASH_AGENT_CONFIG_DIR", v.as_os_str());
        }
        if let Some(v) = &self.data_dir {
            cmd.env("PIDASH_AGENT_DATA_DIR", v.as_os_str());
        }
        if let Some(v) = &self.model_token_file {
            cmd.env("PIDASH_MODEL_TOKEN_FILE", v.as_os_str());
        }
    }
}

// ---------------------------------------------------------------------------
// Daemon-binary pin (NEWPIDASH-363)
// ---------------------------------------------------------------------------

/// Directory, under the daemon data dir, holding only a `pidash` symlink to
/// the daemon's own binary. Prepended to every agent run's `PATH` after rc
/// files run, so the agent's `pidash` invocations always resolve to the
/// daemon's build. A directory (rather than a bare binary path) is what is
/// prepended so nothing else on the operator's `PATH` changes.
pub fn daemon_bin_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("bin")
}

/// File name of the pinned binary inside [`daemon_bin_dir`].
pub fn daemon_bin_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "pidash.exe"
    } else {
        "pidash"
    }
}

/// Canonical path of this process's own binary, falling back to the raw
/// `current_exe` when the path cannot be canonicalized (e.g. deleted).
pub fn current_exe_canonical() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .map(|p| p.canonicalize().unwrap_or(p))
}

/// Create [`daemon_bin_dir`] holding only a `pidash` symlink to this
/// binary. Recreated on every call so it follows self-updates; a `read_link`
/// fast-path keeps steady-state calls (one per run) to a single syscall.
/// Never fails the caller: on any error a warning is logged and the
/// directory is still returned, leaving resolution exactly as it was before
/// the pin existed.
pub fn ensure_daemon_pidash_link(data_dir: &Path) -> PathBuf {
    let dir = daemon_bin_dir(data_dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(dir = ?dir, error = %e, "could not create daemon bin dir");
        return dir;
    }
    let Some(target) = current_exe_canonical() else {
        tracing::warn!("could not resolve current exe for daemon pidash pin");
        return dir;
    };
    let link = dir.join(daemon_bin_name());
    if std::fs::read_link(&link).ok().as_deref() == Some(target.as_path()) {
        return dir;
    }
    // Best effort: drop a stale link (e.g. from before a self-update) before
    // recreating it. Removing a non-symlink that somehow landed here is
    // deliberate — this directory holds only the pin.
    let _ = std::fs::remove_file(&link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link);
    #[cfg(target_os = "windows")]
    let made = std::os::windows::fs::symlink_file(&target, &link);
    #[cfg(not(any(unix, target_os = "windows")))]
    let made: std::io::Result<()> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no symlink support",
    ));
    if let Err(e) = made {
        tracing::warn!(link = ?link, target = ?target, error = %e, "could not pin daemon pidash");
    }
    dir
}

/// Compare two binary paths by canonical form, falling back to the raw path
/// when canonicalization fails on either side.
fn same_binary(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// Build the one-line stderr warning for when the running `pidash` is not
/// the daemon's binary. Returns `None` unless the caller runs inside an
/// agent run (`run_id_present`, i.e. `PIDASH_RUN_ID` is set) and an expected
/// daemon path is known yet differs from the running binary. Never fails:
/// the caller prints the message and carries on with the command.
pub fn runner_exe_mismatch_message(
    current_exe: &Path,
    runner_exe: Option<&Path>,
    run_id_present: bool,
) -> Option<String> {
    if !run_id_present {
        return None;
    }
    let expected = runner_exe?;
    if same_binary(current_exe, expected) {
        return None;
    }
    Some(format!(
        "pidash warning: this pidash ({}) is not the runner daemon's binary ({}) (pidash version {}); run commands may fail — use the daemon's binary",
        current_exe.display(),
        expected.display(),
        crate::RUNNER_VERSION,
    ))
}

/// Emit the daemon-binary mismatch warning to stderr when it applies. Call
/// at CLI startup, before dispatch, so even an unknown-subcommand error
/// (the wrong build lacking the verb) still carries the explanation.
pub fn warn_if_runner_exe_mismatch() {
    if std::env::var_os(RUN_ID_ENV).is_none() {
        return;
    }
    let runner_exe = std::env::var_os(RUNNER_EXE_ENV).map(PathBuf::from);
    if runner_exe.is_none() {
        return;
    }
    let current = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("pidash"));
    if let Some(msg) = runner_exe_mismatch_message(&current, runner_exe.as_deref(), true) {
        eprintln!("{msg}");
    }
}

/// Build a [`Command`] that runs `program args…` directly on Windows.
///
/// The Windows daemon is a per-user scheduled task, so native CLI installs
/// should be runnable from the task environment without requiring `bash.exe`.
/// `cwd`, when `Some`, is applied directly to the child process.
///
/// The returned command still needs the caller's usual stdio and
/// `kill_on_drop` wiring before being spawned.
#[cfg(target_os = "windows")]
pub fn login_shell_command(program: &str, args: &[&str], cwd: Option<&Path>) -> Command {
    login_shell_command_with_env(program, args, cwd, &AgentEnv::default())
}

/// Windows variant of [`login_shell_command_with_env`].
///
/// There is no shell here, so nothing can clobber what we set — the values are
/// exported under both the `PIDASH_*` names (for parity with Unix) and their
/// real names, and the credential is read from disk by the daemon rather than
/// by a shell.
#[cfg(target_os = "windows")]
pub fn login_shell_command_with_env(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    env: &AgentEnv,
) -> Command {
    let mut cmd = Command::new(program);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    env.apply(&mut cmd);
    apply_run_env(&mut cmd);
    if let Some(v) = &env.codex_home {
        cmd.env("CODEX_HOME", v.as_os_str());
    }
    if let Some(v) = &env.config_dir {
        cmd.env("PIDASH_CONFIG_DIR", v.as_os_str());
    }
    if let Some(v) = &env.data_dir {
        cmd.env("PIDASH_DATA_DIR", v.as_os_str());
    }
    if let Some(dir) = &env.path_prepend {
        let existing = std::env::var_os("PATH").unwrap_or_default();
        let mut parts = vec![dir.clone()];
        parts.extend(std::env::split_paths(&existing));
        if let Ok(joined) = std::env::join_paths(parts) {
            cmd.env("PATH", joined);
        }
    }
    if let Some(path) = &env.model_token_file {
        // No shell to `cat` it, so read here. A missing/unreadable file leaves
        // the variable unset and the provider refuses — which is the correct,
        // loud failure for "the desktop has not written a token yet".
        if let Ok(token) = std::fs::read_to_string(path) {
            let token = token.trim();
            if !token.is_empty() {
                cmd.env("PIDASH_GATEWAY_TOKEN", token);
            }
        }
    }
    cmd.args(args);
    cmd
}

/// Build a [`Command`] that runs `program args…` the same way the daemon
/// launches agents on Unix-like hosts.
///
/// See the module docs for why `-i` is required alongside `-l`.
///
/// `cwd`, when `Some`, is both applied to the outer bash (via `current_dir`)
/// and re-asserted inside the script after rc files run, so `.bashrc`'s
/// `cd` side effects can't clobber the agent's starting directory.
///
/// The returned command still needs the caller's usual stdio and
/// `kill_on_drop` wiring before being spawned.
#[cfg(not(target_os = "windows"))]
pub fn login_shell_command(program: &str, args: &[&str], cwd: Option<&Path>) -> Command {
    login_shell_command_with_env(program, args, cwd, &AgentEnv::default())
}

/// [`login_shell_command`] plus a Pi Dash-controlled environment for the agent.
///
/// The values are passed under `PIDASH_*` names and re-exported by
/// [`SHELL_SCRIPT`] *after* the login shell's rc files run, so a user profile
/// cannot override them. With an empty [`AgentEnv`] this is exactly
/// [`login_shell_command`].
#[cfg(not(target_os = "windows"))]
pub fn login_shell_command_with_env(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    env: &AgentEnv,
) -> Command {
    let mut cmd = Command::new("bash");
    // `LC_ALL` overrides every more-specific locale category, so drop it
    // first; otherwise `LC_MESSAGES=C` would be ignored when the parent
    // environment exports `LC_ALL=...`.
    cmd.env_remove("LC_ALL");
    // Pin the message locale so `is_benign_login_shell_warning` matches
    // regardless of the operator's LANG / LC_ALL.
    cmd.env("LC_MESSAGES", "C");
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
        cmd.env("PIDASH_AGENT_CWD", cwd.as_os_str());
    }
    env.apply(&mut cmd);
    // Plain `env`, not re-exported by the script: the login shell's rc
    // files have no reason to touch `PIDASH_RUN_ID`, and `exec "$@"`
    // inherits it.
    apply_run_env(&mut cmd);
    cmd.arg("-ilc").arg(SHELL_SCRIPT).arg("bash").arg(program);
    cmd.args(args);
    cmd
}

/// Read `binary --version` through the same wrapper a real spawn uses.
///
/// Returns the first line of stdout, trimmed and capped, or `None` when the
/// binary is missing or fails. Used to report the *agent engine's* version to
/// the cloud, which for a desktop-bundled runner is the one number support
/// needs — the binary ships inside the app, so the user cannot report it.
///
/// Probing through the same wrapper as a run is what makes the answer
/// trustworthy: a binary that only resolves under the login shell (or only
/// with the managed `PATH` prepended) reports here exactly as it will behave.
pub async fn binary_version(binary: &str, env: &AgentEnv) -> Option<String> {
    let mut cmd = login_shell_command_with_env(binary, &["--version"], None, env);
    cmd.stdin(std::process::Stdio::null());
    let out = cmd.output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| !l.trim().is_empty())?.trim();
    if line.is_empty() {
        return None;
    }
    Some(line.chars().take(64).collect())
}

/// Probe whether `binary --version` runs successfully through the same
/// platform-specific wrapper the daemon uses to spawn agents. Returns `true`
/// only when the handler launches and the binary exits `0`.
///
/// `pidash runner add` uses this to decide whether to remind the operator
/// to install the chosen agent CLI. Probing through [`login_shell_command`] is
/// what makes the answer match reality: the probe uses the same Unix login
/// shell or native Windows direct spawn that a real run will use.
pub async fn binary_runs_version(binary: &str) -> bool {
    let mut cmd = login_shell_command(binary, &["--version"], None);
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    matches!(cmd.output().await, Ok(out) if out.status.success())
}

/// True for the two stderr lines bash always emits when started with `-i`
/// under a daemon with no controlling TTY. Drain loops consuming a child's
/// stderr should drop these so logs aren't polluted with a warning on every
/// agent spawn.
pub fn is_benign_login_shell_warning(line: &str) -> bool {
    matches!(
        line.trim_end(),
        "bash: cannot set terminal process group (-1): Inappropriate ioctl for device"
            | "bash: no job control in this shell"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn argv(cmd: &Command) -> Vec<OsString> {
        cmd.as_std().get_args().map(|a| a.to_os_string()).collect()
    }

    fn env_value(cmd: &Command, key: &str) -> Option<OsString> {
        cmd.as_std()
            .get_envs()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.map(|v| v.to_os_string()))
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn wraps_program_and_args_in_login_bash() {
        let cmd = login_shell_command("claude", &["--print", "--model", "sonnet-4"], None);
        assert_eq!(cmd.as_std().get_program(), "bash");
        assert_eq!(
            argv(&cmd),
            vec![
                OsString::from("-ilc"),
                OsString::from(SHELL_SCRIPT),
                OsString::from("bash"),
                OsString::from("claude"),
                OsString::from("--print"),
                OsString::from("--model"),
                OsString::from("sonnet-4"),
            ]
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn no_args_produces_bare_invocation() {
        let cmd = login_shell_command("codex", &[], None);
        assert_eq!(
            argv(&cmd),
            vec![
                OsString::from("-ilc"),
                OsString::from(SHELL_SCRIPT),
                OsString::from("bash"),
                OsString::from("codex"),
            ]
        );
    }

    #[tokio::test]
    async fn run_env_scope_puts_run_id_on_the_child() {
        let run_id = uuid::Uuid::new_v4();
        let cmd = RUN_ENV
            .scope(
                RunEnv {
                    run_id,
                    backend: BackendKind::None,
                    process_group: None,
                    runner_bin_dir: None,
                    config_dir: None,
                    data_dir: None,
                    runner_exe: None,
                    issue_identifier: None,
                    project_identifier: None,
                    agent_name: None,
                },
                async { login_shell_command("claude", &[], None) },
            )
            .await;
        assert_eq!(
            env_value(&cmd, RUN_ID_ENV),
            Some(OsString::from(run_id.to_string()))
        );
    }

    /// PDASHOSS01-417: on the process-group backend the agent joins the
    /// run's anchored group instead of leading one of its own, so the
    /// run's claim never depends on reading the agent's pid after spawn.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn process_group_runs_spawn_agents_into_the_runs_group() {
        let owner = crate::util::group_ownership::GroupOwner::new_anchored().expect("anchored");
        let pgid = owner.pgid();
        let mut child = RUN_ENV
            .scope(
                RunEnv {
                    run_id: uuid::Uuid::new_v4(),
                    backend: BackendKind::ProcessGroup,
                    process_group: Some(pgid),
                    runner_bin_dir: None,
                    config_dir: None,
                    data_dir: None,
                    runner_exe: None,
                    issue_identifier: None,
                    project_identifier: None,
                    agent_name: None,
                },
                async { login_shell_command("sleep", &["30"], None) },
            )
            .await
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let joined = child
            .id()
            .and_then(|pid| crate::util::procinfo::info(pid as i32))
            .map(|p| p.pgid);
        let _ = child.kill().await;
        assert_eq!(joined, Some(pgid), "agent did not join the run's group");
    }

    /// PDASHOSS01-417: a chat runtime's agents join the session's group
    /// (so they can be stopped when it ends) without any run identity.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn chat_scope_spawns_agents_into_the_sessions_group() {
        let owner = crate::util::group_ownership::GroupOwner::new_anchored().expect("anchored");
        let pgid = owner.pgid();
        let cmd = CHAT_ENV
            .scope(
                ChatEnv {
                    backend: BackendKind::ProcessGroup,
                    process_group: Some(pgid),
                    cgroup_procs: None,
                },
                async { login_shell_command("sleep", &["30"], None) },
            )
            .await;
        assert_eq!(env_value(&cmd, RUN_ID_ENV), None, "chat is not a run");
        let mut cmd = cmd;
        let mut child = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let joined = child
            .id()
            .and_then(|pid| crate::util::procinfo::info(pid as i32))
            .map(|p| p.pgid);
        let _ = child.kill().await;
        assert_eq!(
            joined,
            Some(pgid),
            "chat agent did not join the session's group"
        );
    }

    /// PDASHOSS01-417: on the cgroup backend a chat agent joins the unit's
    /// cgroup from inside its own login shell, before the agent starts, so
    /// an agent spawned per turn is contained as well as the first one.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn chat_scope_on_cgroup_joins_the_units_cgroup_before_the_agent_starts() {
        let dir = tempfile::tempdir().unwrap();
        let procs = dir.path().join("cgroup.procs");
        std::fs::write(&procs, "").unwrap();
        let mut cmd = CHAT_ENV
            .scope(
                ChatEnv {
                    backend: BackendKind::Cgroup,
                    process_group: None,
                    cgroup_procs: Some(procs.clone()),
                },
                async { login_shell_command("true", &[], None) },
            )
            .await;
        let mut child = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let status = child.wait().await.unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&procs).unwrap().trim(),
            pid.to_string(),
            "the chat agent did not join the unit's cgroup"
        );
    }

    /// A chat agent that cannot join its unit's cgroup is not started:
    /// left outside, it would outlive a cleanup that reports clean.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn chat_agent_that_cannot_join_its_cgroup_is_not_started() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let procs = dir.path().join("missing").join("cgroup.procs");
        let marker_arg = marker.to_string_lossy().into_owned();
        let mut cmd = CHAT_ENV
            .scope(
                ChatEnv {
                    backend: BackendKind::Cgroup,
                    process_group: None,
                    cgroup_procs: Some(procs),
                },
                async { login_shell_command("touch", &[marker_arg.as_str()], None) },
            )
            .await;
        let status = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(!status.success(), "started outside its cgroup");
        assert!(!marker.exists(), "the agent ran outside its cgroup");
    }

    /// NEWPIDASH-320 §6.3: issue runs export the issue, project and agent
    /// name; scheduler runs omit the issue; chat (no scope) exports none.
    #[tokio::test]
    async fn run_env_scope_exports_run_context() {
        let run_id = uuid::Uuid::new_v4();
        let issue_cmd = RUN_ENV
            .scope(
                RunEnv {
                    run_id,
                    backend: BackendKind::None,
                    process_group: None,
                    runner_bin_dir: None,
                    config_dir: None,
                    data_dir: None,
                    runner_exe: None,
                    issue_identifier: Some("FX-7".into()),
                    project_identifier: Some("FX".into()),
                    agent_name: Some("Muse Code (muse-spark)".into()),
                },
                async { login_shell_command("muse", &[], None) },
            )
            .await;
        assert_eq!(
            env_value(&issue_cmd, ISSUE_IDENTIFIER_ENV),
            Some(OsString::from("FX-7"))
        );
        assert_eq!(
            env_value(&issue_cmd, PROJECT_ENV),
            Some(OsString::from("FX"))
        );
        assert_eq!(
            env_value(&issue_cmd, AGENT_NAME_ENV),
            Some(OsString::from("Muse Code (muse-spark)"))
        );

        let sched_cmd = RUN_ENV
            .scope(
                RunEnv {
                    run_id,
                    backend: BackendKind::None,
                    process_group: None,
                    runner_bin_dir: None,
                    config_dir: None,
                    data_dir: None,
                    runner_exe: None,
                    issue_identifier: None,
                    project_identifier: Some("FX".into()),
                    agent_name: Some("Codex".into()),
                },
                async { login_shell_command("codex", &[], None) },
            )
            .await;
        assert_eq!(env_value(&sched_cmd, ISSUE_IDENTIFIER_ENV), None);
        assert_eq!(
            env_value(&sched_cmd, PROJECT_ENV),
            Some(OsString::from("FX"))
        );
    }

    #[tokio::test]
    async fn no_run_env_scope_means_no_run_id_on_the_child() {
        // Chat sessions never enter a run scope; the agent must not inherit
        // a stale id from anywhere.
        let cmd = login_shell_command("claude", &[], None);
        assert_eq!(env_value(&cmd, RUN_ID_ENV), None);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn pins_lc_messages_to_c_for_stable_warning_strings() {
        // The stderr filter is exact-match English; pinning LC_MESSAGES=C
        // keeps it working under localized hosts.
        let cmd = login_shell_command("claude", &[], None);
        assert_eq!(env_value(&cmd, "LC_MESSAGES"), Some(OsString::from("C")));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn removes_lc_all_so_lc_messages_takes_effect() {
        let cmd = login_shell_command("claude", &[], None);
        let lc_all = cmd
            .as_std()
            .get_envs()
            .find(|(k, _)| *k == "LC_ALL")
            .map(|(_, v)| v);
        assert_eq!(lc_all, Some(None));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn cwd_none_does_not_set_pidash_agent_cwd() {
        let cmd = login_shell_command("claude", &["--version"], None);
        assert_eq!(env_value(&cmd, "PIDASH_AGENT_CWD"), None);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn cwd_some_sets_both_current_dir_and_pidash_agent_cwd() {
        let cwd = PathBuf::from("/tmp/pidash-workspace");
        let cmd = login_shell_command("claude", &["--version"], Some(&cwd));
        assert_eq!(
            env_value(&cmd, "PIDASH_AGENT_CWD"),
            Some(OsString::from("/tmp/pidash-workspace"))
        );
        assert_eq!(cmd.as_std().get_current_dir(), Some(Path::new(&cwd)));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_runs_program_directly() {
        let cmd = login_shell_command("claude", &["--print", "--model", "sonnet-4"], None);
        assert_eq!(cmd.as_std().get_program(), "claude");
        assert_eq!(
            argv(&cmd),
            vec![
                OsString::from("--print"),
                OsString::from("--model"),
                OsString::from("sonnet-4"),
            ]
        );
        assert_eq!(env_value(&cmd, "PIDASH_AGENT_CWD"), None);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_applies_current_dir_without_shell_env() {
        let cwd = PathBuf::from(r"C:\pidash-workspace");
        let cmd = login_shell_command("codex", &["app-server"], Some(&cwd));
        assert_eq!(cmd.as_std().get_program(), "codex");
        assert_eq!(argv(&cmd), vec![OsString::from("app-server")]);
        assert_eq!(cmd.as_std().get_current_dir(), Some(Path::new(&cwd)));
        assert_eq!(env_value(&cmd, "PIDASH_AGENT_CWD"), None);
    }

    #[test]
    fn recognises_bash_no_tty_warnings() {
        assert!(is_benign_login_shell_warning(
            "bash: cannot set terminal process group (-1): Inappropriate ioctl for device"
        ));
        assert!(is_benign_login_shell_warning(
            "bash: no job control in this shell"
        ));
        assert!(is_benign_login_shell_warning(
            "bash: no job control in this shell\n"
        ));
        assert!(!is_benign_login_shell_warning(
            "claude: unexpected internal error"
        ));
        assert!(!is_benign_login_shell_warning(""));
    }

    #[tokio::test]
    async fn binary_runs_version_true_for_present_binary() {
        let binary = if cfg!(target_os = "windows") {
            // `cargo` is guaranteed to be on PATH during `cargo test` and
            // supports `--version` on every platform.
            "cargo"
        } else {
            // `true` is on PATH everywhere we run Unix tests and exits 0
            // regardless of args, so it stands in for an installed agent CLI.
            "true"
        };
        assert!(binary_runs_version(binary).await);
    }

    #[tokio::test]
    async fn binary_runs_version_false_for_missing_binary() {
        // The case `pidash runner add` cares about: an agent CLI the user
        // hasn't installed. bash's `exec` fails (127) → non-success.
        assert!(!binary_runs_version("pidash-no-such-agent-binary-xyz").await);
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn round_trips_argv_through_bash() {
        // Prove bash's `exec "$@"` preserves our argv exactly and the child
        // actually runs. `printf '%s\n' "$@"` echoes each arg on its own
        // line — verifies ordering and that args with spaces / `=` are not
        // split or re-parsed by the shell.
        let mut cmd = login_shell_command(
            "printf",
            &["%s\n", "first arg", "second-arg", "has=equal"],
            None,
        );
        cmd.kill_on_drop(true);
        let out = cmd
            .output()
            .await
            .expect("spawn printf through login shell");
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "first arg\nsecond-arg\nhas=equal\n"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn agent_cwd_survives_bashrc_cd() {
        // Regression: a `cd` in the operator's `.bashrc` used to silently
        // override the cwd we set on the outer bash, because rc files run
        // before `exec "$@"`. Build a throwaway HOME whose rc cds to `/`,
        // ask the wrapper for a specific cwd, and confirm the target
        // observes it via `pwd`.
        let home = tempfile::tempdir().expect("tempdir for fake HOME");
        let agent_cwd = tempfile::tempdir().expect("tempdir for agent cwd");

        // `bash -l` reads the first of .bash_profile / .bash_login / .profile
        // it finds; add one that sources .bashrc the way stock Ubuntu does.
        std::fs::write(
            home.path().join(".bash_profile"),
            "[ -f \"$HOME/.bashrc\" ] && . \"$HOME/.bashrc\"\n",
        )
        .expect("write .bash_profile");
        // And the .bashrc that would clobber cwd without our guard. No
        // non-interactive guard: we *want* `-i` to run this file fully.
        std::fs::write(home.path().join(".bashrc"), "cd /\n").expect("write .bashrc");

        let mut cmd = login_shell_command("pwd", &[], Some(agent_cwd.path()));
        cmd.env("HOME", home.path());
        // Isolate the login bash from the developer's real environment so
        // this test is deterministic on any box.
        cmd.env_remove("BASH_ENV");
        cmd.kill_on_drop(true);

        let out = cmd.output().await.expect("spawn pwd through login shell");
        assert!(
            out.status.success(),
            "pwd failed: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Take the *last* non-empty stdout line. On Ubuntu hosts the
        // system-wide `/etc/bash.bashrc` prints a sudo hint banner to
        // stdout before our `-c` script runs, so `pwd`'s output lands on
        // the final line rather than the only line.
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let observed = stdout
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .unwrap_or("")
            .to_string();
        // macOS `TMPDIR` resolves through a symlinked /var → /private/var,
        // so compare canonical paths rather than raw strings.
        let expected = std::fs::canonicalize(agent_cwd.path()).expect("canonicalize expected");
        let observed_canonical = std::fs::canonicalize(&observed).unwrap_or_else(|e| {
            panic!(
                "canonicalize observed={observed:?} failed: {e}; full stdout={stdout:?}; stderr={stderr:?}"
            )
        });
        assert_eq!(
            observed_canonical, expected,
            ".bashrc's `cd /` leaked past our guard; pwd reported {observed}"
        );
    }

    // -----------------------------------------------------------------
    // Managed-runner environment (.ai_design/managed_runner/design.md §12.3)
    // -----------------------------------------------------------------

    fn managed_env() -> AgentEnv {
        AgentEnv {
            codex_home: Some(PathBuf::from("/managed/codex-home")),
            path_prepend: Some(PathBuf::from("/managed/bin")),
            config_dir: Some(PathBuf::from("/managed/pidash")),
            data_dir: Some(PathBuf::from("/managed/pidash/data")),
            model_token_file: Some(PathBuf::from("/managed/runtime/model.token")),
        }
    }

    #[test]
    fn empty_agent_env_changes_nothing() {
        // The user-enrolled path must stay byte-identical: no PIDASH_* env
        // beyond the cwd marker, and the same argv as before.
        let plain = login_shell_command("codex", &["app-server"], None);
        let with_empty =
            login_shell_command_with_env("codex", &["app-server"], None, &AgentEnv::default());
        assert_eq!(argv(&plain), argv(&with_empty));
        for key in [
            "PIDASH_CODEX_HOME",
            "PIDASH_AGENT_PATH_PREPEND",
            "PIDASH_AGENT_CONFIG_DIR",
            "PIDASH_AGENT_DATA_DIR",
            "PIDASH_MODEL_TOKEN_FILE",
        ] {
            assert_eq!(env_value(&with_empty, key), None, "{key} should be unset");
        }
        assert!(AgentEnv::default().is_empty());
        assert!(!managed_env().is_empty());
    }

    #[test]
    fn managed_env_is_passed_under_pidash_names() {
        // The values must travel under PIDASH_* names, never as the real
        // names on the outer bash — otherwise rc files would clobber them
        // and the re-export in SHELL_SCRIPT would have nothing to restore.
        let cmd = login_shell_command_with_env("codex", &["app-server"], None, &managed_env());
        assert_eq!(
            env_value(&cmd, "PIDASH_CODEX_HOME"),
            Some(OsString::from("/managed/codex-home"))
        );
        assert_eq!(
            env_value(&cmd, "PIDASH_AGENT_PATH_PREPEND"),
            Some(OsString::from("/managed/bin"))
        );
        assert_eq!(
            env_value(&cmd, "PIDASH_AGENT_CONFIG_DIR"),
            Some(OsString::from("/managed/pidash"))
        );
        assert_eq!(
            env_value(&cmd, "PIDASH_MODEL_TOKEN_FILE"),
            Some(OsString::from("/managed/runtime/model.token"))
        );
        // The credential must never appear on the command line.
        let joined: Vec<String> = argv(&cmd)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            !joined
                .iter()
                .any(|a| a.contains("model.token") && !a.contains("PIDASH_MODEL_TOKEN_FILE")),
            "token path leaked into argv: {joined:?}"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn codex_home_survives_bashrc_export() {
        // The managed-runner analogue of `agent_cwd_survives_bashrc_cd`: a
        // user whose profile exports CODEX_HOME must not redirect the bundled
        // engine at their personal Codex config.
        let home = tempfile::tempdir().expect("tempdir for fake HOME");
        std::fs::write(
            home.path().join(".bash_profile"),
            "[ -f \"$HOME/.bashrc\" ] && . \"$HOME/.bashrc\"\n",
        )
        .expect("write .bash_profile");
        std::fs::write(
            home.path().join(".bashrc"),
            "export CODEX_HOME=/user/dot-codex\nexport PATH=/user/bin:$PATH\n",
        )
        .expect("write .bashrc");

        let env = AgentEnv {
            codex_home: Some(PathBuf::from("/managed/codex-home")),
            path_prepend: Some(PathBuf::from("/managed/bin")),
            ..AgentEnv::default()
        };
        let mut cmd = login_shell_command_with_env(
            "sh",
            &["-c", "printf '%s\\n%s\\n' \"$CODEX_HOME\" \"$PATH\""],
            None,
            &env,
        );
        cmd.env("HOME", home.path());
        cmd.env_remove("BASH_ENV");
        cmd.kill_on_drop(true);

        let out = cmd.output().await.expect("spawn through login shell");
        assert!(
            out.status.success(),
            "probe failed: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        let codex_home = lines[lines.len() - 2];
        let path = lines[lines.len() - 1];
        assert_eq!(
            codex_home, "/managed/codex-home",
            "rc file's CODEX_HOME won; full stdout: {stdout}"
        );
        assert!(
            path.starts_with("/managed/bin:"),
            "managed PATH prefix lost; PATH={path}"
        );
    }

    // -----------------------------------------------------------------
    // Daemon-binary pin (NEWPIDASH-363)
    // -----------------------------------------------------------------

    /// Fake HOME whose login shell prepends `decoy_bin` to PATH and exports
    /// hostile `PIDASH_CONFIG_DIR` / `PIDASH_DATA_DIR` — the operator setup
    /// from the issue (a released CLI shadowing the daemon's binary).
    #[cfg(not(target_os = "windows"))]
    fn hostile_home(decoy_bin: &Path) -> tempfile::TempDir {
        let home = tempfile::tempdir().expect("tempdir for fake HOME");
        std::fs::write(
            home.path().join(".bash_profile"),
            "[ -f \"$HOME/.bashrc\" ] && . \"$HOME/.bashrc\"\n",
        )
        .expect("write .bash_profile");
        std::fs::write(
            home.path().join(".bashrc"),
            format!(
                "export PATH=\"{}:$PATH\"\nexport PIDASH_CONFIG_DIR=/user/pidash\nexport PIDASH_DATA_DIR=/user/pidash/data\n",
                decoy_bin.display(),
            ),
        )
        .expect("write .bashrc");
        home
    }

    /// Write an executable file named `pidash` into `dir` that prints
    /// `marker`, standing in for one build of the CLI.
    #[cfg(not(target_os = "windows"))]
    fn marker_pidash(dir: &Path, marker: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("pidash");
        std::fs::write(&path, format!("#!/bin/sh\necho {marker}\n")).expect("write marker pidash");
        let mut perm = std::fs::metadata(&path).expect("metadata").permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&path, perm).expect("chmod marker pidash");
        path
    }

    /// Per-run environment carrying the daemon pin, as the supervisor builds
    /// it for every agent run regardless of enrollment or agent kind.
    #[cfg(not(target_os = "windows"))]
    fn pinned_run_env(bin_dir: &Path, exe: &Path) -> RunEnv {
        RunEnv {
            run_id: uuid::Uuid::new_v4(),
            backend: BackendKind::None,
            process_group: None,
            runner_bin_dir: Some(bin_dir.to_path_buf()),
            config_dir: Some(PathBuf::from("/daemon/pidash")),
            data_dir: Some(PathBuf::from("/daemon/pidash/data")),
            runner_exe: Some(exe.to_path_buf()),
            issue_identifier: None,
            project_identifier: None,
            agent_name: None,
        }
    }

    /// Run the wrapper to completion and return its full stdout. Callers
    /// match on the last non-empty lines: system bashrc banners land on
    /// earlier lines (see `agent_cwd_survives_bashrc_cd`).
    #[cfg(not(target_os = "windows"))]
    async fn probe_stdout(cmd: &mut Command) -> String {
        let out = cmd.output().await.expect("spawn through login shell");
        assert!(
            out.status.success(),
            "probe failed: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn runner_bin_pin_beats_decoy_with_empty_agent_env() {
        // User-enrolled runner: AgentEnv is empty (managed-only gate in
        // `agent_env_for_config`), yet the decoy pidash the operator's rc
        // files prepend must lose to the daemon pin in RUN_ENV.
        let decoy_dir = tempfile::tempdir().expect("tempdir for decoy bin");
        marker_pidash(decoy_dir.path(), "decoy-pidash");
        let daemon_dir = tempfile::tempdir().expect("tempdir for daemon bin");
        marker_pidash(daemon_dir.path(), "daemon-pidash");
        let home = hostile_home(decoy_dir.path());
        let daemon_exe = daemon_dir.path().join("pidash");

        let stdout = RUN_ENV
            .scope(pinned_run_env(daemon_dir.path(), &daemon_exe), async {
                let mut cmd =
                    login_shell_command_with_env("pidash", &[], None, &AgentEnv::default());
                cmd.env("HOME", home.path());
                cmd.env_remove("BASH_ENV");
                cmd.kill_on_drop(true);
                probe_stdout(&mut cmd).await
            })
            .await;
        assert!(
            stdout.lines().any(|l| l.trim() == "daemon-pidash"),
            "decoy pidash won; full stdout: {stdout}"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn runner_bin_pin_applies_to_plain_bridge_command() {
        // Non-Codex bridges (Muse Code, Claude Code, Cursor, OpenClaw, Grok,
        // Antigravity) call plain `login_shell_command` with no AgentEnv at
        // all; the RUN_ENV pin must still reach them.
        let decoy_dir = tempfile::tempdir().expect("tempdir for decoy bin");
        marker_pidash(decoy_dir.path(), "decoy-pidash");
        let daemon_dir = tempfile::tempdir().expect("tempdir for daemon bin");
        marker_pidash(daemon_dir.path(), "daemon-pidash");
        let home = hostile_home(decoy_dir.path());
        let daemon_exe = daemon_dir.path().join("pidash");

        let stdout = RUN_ENV
            .scope(pinned_run_env(daemon_dir.path(), &daemon_exe), async {
                let mut cmd = login_shell_command("pidash", &[], None);
                cmd.env("HOME", home.path());
                cmd.env_remove("BASH_ENV");
                cmd.kill_on_drop(true);
                probe_stdout(&mut cmd).await
            })
            .await;
        assert!(
            stdout.lines().any(|l| l.trim() == "daemon-pidash"),
            "decoy pidash won on the plain bridge path; full stdout: {stdout}"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn daemon_config_data_and_exe_survive_hostile_rc() {
        // The rc files overwrite PIDASH_CONFIG_DIR / PIDASH_DATA_DIR; the
        // re-export after rc files must restore the daemon's own dirs (for
        // a user-enrolled runner whose daemon has them set), plus the
        // daemon exe path for the CLI warning.
        let decoy_dir = tempfile::tempdir().expect("tempdir for decoy bin");
        marker_pidash(decoy_dir.path(), "decoy-pidash");
        let daemon_dir = tempfile::tempdir().expect("tempdir for daemon bin");
        let daemon_exe = marker_pidash(daemon_dir.path(), "daemon-pidash");
        let home = hostile_home(decoy_dir.path());

        let stdout = RUN_ENV
            .scope(
                pinned_run_env(daemon_dir.path(), &daemon_exe),
                async {
                    let mut cmd = login_shell_command(
                        "sh",
                        &[
                            "-c",
                            "printf '%s\\n%s\\n%s\\n' \"$PIDASH_CONFIG_DIR\" \"$PIDASH_DATA_DIR\" \"$PIDASH_RUNNER_EXE\"",
                        ],
                        None,
                    );
                    cmd.env("HOME", home.path());
                    cmd.env_remove("BASH_ENV");
                    cmd.kill_on_drop(true);
                    probe_stdout(&mut cmd).await
                },
            )
            .await;
        let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        let (config, data, exe) = (
            lines[lines.len() - 3],
            lines[lines.len() - 2],
            lines[lines.len() - 1],
        );
        assert_eq!(config, "/daemon/pidash", "rc file's PIDASH_CONFIG_DIR won");
        assert_eq!(data, "/daemon/pidash/data", "rc file's PIDASH_DATA_DIR won");
        assert_eq!(
            exe,
            daemon_exe.to_string_lossy(),
            "PIDASH_RUNNER_EXE did not survive rc files"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn runner_bin_comes_before_managed_prepend() {
        // A managed run has both the bundled-CLI prepend and the daemon pin;
        // the daemon's own pidash must resolve first.
        let decoy_dir = tempfile::tempdir().expect("tempdir for decoy bin");
        marker_pidash(decoy_dir.path(), "decoy-pidash");
        let daemon_dir = tempfile::tempdir().expect("tempdir for daemon bin");
        marker_pidash(daemon_dir.path(), "daemon-pidash");
        let managed_dir = tempfile::tempdir().expect("tempdir for managed bin");
        marker_pidash(managed_dir.path(), "managed-pidash");
        let home = hostile_home(decoy_dir.path());
        let daemon_exe = daemon_dir.path().join("pidash");

        let env = AgentEnv {
            path_prepend: Some(managed_dir.path().to_path_buf()),
            ..AgentEnv::default()
        };
        let stdout = RUN_ENV
            .scope(pinned_run_env(daemon_dir.path(), &daemon_exe), async {
                let mut cmd = login_shell_command_with_env(
                    "sh",
                    &["-c", "command -v pidash; printf '%s\\n' \"$PATH\""],
                    None,
                    &env,
                );
                cmd.env("HOME", home.path());
                cmd.env_remove("BASH_ENV");
                cmd.kill_on_drop(true);
                probe_stdout(&mut cmd).await
            })
            .await;
        let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        let resolved = lines[lines.len() - 2];
        let path = lines[lines.len() - 1];
        assert_eq!(
            Path::new(resolved),
            daemon_exe.as_path(),
            "daemon pin did not resolve first; full stdout: {stdout}"
        );
        assert!(
            path.starts_with(&format!(
                "{}:{}:",
                daemon_dir.path().display(),
                managed_dir.path().display()
            )),
            "expected pin before managed prepend; PATH={path}"
        );
    }

    #[test]
    fn ensure_daemon_pidash_link_creates_and_refreshes_symlink() {
        let data_dir = tempfile::tempdir().expect("tempdir for data dir");
        let dir = ensure_daemon_pidash_link(data_dir.path());
        assert_eq!(dir, daemon_bin_dir(data_dir.path()));
        let link = dir.join(daemon_bin_name());
        let target = current_exe_canonical().expect("current exe resolves");
        assert_eq!(
            std::fs::read_link(&link).expect("pin symlink exists"),
            target,
            "pin does not point at the daemon binary"
        );
        // Steady state is a single read_link: no error, same result.
        let again = ensure_daemon_pidash_link(data_dir.path());
        assert_eq!(again, dir);
        assert_eq!(std::fs::read_link(&link).expect("pin still there"), target);
        // A stale link (e.g. from before a self-update) is replaced.
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink("/nonexistent/pidash", &link).expect("stale link");
        #[cfg(target_os = "windows")]
        std::os::windows::fs::symlink_file("C:\\nonexistent\\pidash.exe", &link)
            .expect("stale link");
        ensure_daemon_pidash_link(data_dir.path());
        assert_eq!(
            std::fs::read_link(&link).expect("pin recreated"),
            target,
            "stale pin was not refreshed"
        );
    }

    #[test]
    fn mismatch_warning_names_both_paths_and_version() {
        let running = PathBuf::from("/home/op/.local/bin/pidash");
        let daemon = PathBuf::from("/home/op/.local/lib/pidash-test/pidash");
        let msg =
            runner_exe_mismatch_message(&running, Some(&daemon), true).expect("mismatch warns");
        assert!(
            msg.lines().count() == 1,
            "warning must stay one line: {msg}"
        );
        assert!(
            msg.contains(running.to_str().unwrap()),
            "must name running binary: {msg}"
        );
        assert!(
            msg.contains(daemon.to_str().unwrap()),
            "must name daemon binary: {msg}"
        );
        assert!(
            msg.contains(crate::RUNNER_VERSION),
            "must name its version: {msg}"
        );
    }

    #[test]
    fn mismatch_warning_silent_for_match_unset_or_no_run() {
        let exe = std::env::current_exe().expect("current exe");
        // Same binary (canonically) is silent.
        assert_eq!(runner_exe_mismatch_message(&exe, Some(&exe), true), None);
        // No expected daemon binary, or no run id, is silent.
        assert_eq!(
            runner_exe_mismatch_message(&exe, None, true),
            None,
            "unset PIDASH_RUNNER_EXE must print nothing"
        );
        assert_eq!(
            runner_exe_mismatch_message(&PathBuf::from("/elsewhere/pidash"), Some(&exe), false),
            None,
            "outside an agent run must print nothing"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn model_token_is_read_from_file_not_config() {
        // The credential reaches the agent as an environment variable read
        // from a file at spawn time, so rotating the file applies to the next
        // run with no daemon restart and nothing lands in config.toml.
        let dir = tempfile::tempdir().expect("tempdir");
        let token_path = dir.path().join("model.token");
        std::fs::write(&token_path, "tok-first\n").expect("write token");

        let env = AgentEnv {
            model_token_file: Some(token_path.clone()),
            ..AgentEnv::default()
        };
        let read_back = |env: &AgentEnv| {
            let mut cmd = login_shell_command_with_env(
                "sh",
                &["-c", "printf '%s' \"$PIDASH_GATEWAY_TOKEN\""],
                None,
                env,
            );
            cmd.env_remove("BASH_ENV");
            cmd.kill_on_drop(true);
            cmd
        };

        let out = read_back(&env).output().await.expect("spawn");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("tok-first"),
            "token not exported; stdout={stdout}"
        );

        std::fs::write(&token_path, "tok-rotated\n").expect("rotate token");
        let out = read_back(&env).output().await.expect("spawn");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("tok-rotated"),
            "rotated token not picked up; stdout={stdout}"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn missing_token_file_leaves_variable_unset() {
        // A desktop that has not written a token yet must produce a loud
        // provider refusal, not a blank credential silently sent upstream.
        let env = AgentEnv {
            model_token_file: Some(PathBuf::from("/nonexistent/model.token")),
            ..AgentEnv::default()
        };
        let mut cmd = login_shell_command_with_env(
            "sh",
            &["-c", "printf '[%s]' \"${PIDASH_GATEWAY_TOKEN-unset}\""],
            None,
            &env,
        );
        cmd.env_remove("BASH_ENV");
        cmd.kill_on_drop(true);
        let out = cmd.output().await.expect("spawn");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("[unset]") || stdout.contains("[]"),
            "expected unset token, got {stdout}"
        );
    }

    #[tokio::test]
    async fn binary_version_reads_first_line() {
        let v = binary_version("echo", &AgentEnv::default()).await;
        // `echo --version` prints "--version" on most shells; the point is
        // that we get the first non-empty line back, trimmed.
        assert!(v.is_some(), "expected a version line from echo");
        assert!(!v.unwrap().contains('\n'));
    }

    #[tokio::test]
    async fn binary_version_none_for_missing_binary() {
        assert_eq!(
            binary_version("pidash-not-a-real-binary-xyz", &AgentEnv::default()).await,
            None
        );
    }
}
