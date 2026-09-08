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

/// Re-assert Pi Dash's environment *after* the login shell's rc files have run.
///
/// `bash -ilc` sources `~/.bash_profile` / `~/.bashrc`, which happens after the
/// values we set with `Command::env` are applied — so a user with
/// `export CODEX_HOME=...` or a `PATH` rewrite in their profile would silently
/// win over the daemon. Passing our values under `PIDASH_*` names and
/// re-exporting them here is the same trick the working directory has always
/// used, extended to the managed runner's environment.
///
/// Every clause is a no-op when its `PIDASH_*` variable is unset, so a
/// user-enrolled runner produces exactly the command line it always did.
#[cfg(not(target_os = "windows"))]
const SHELL_SCRIPT: &str = r#"[ -n "${PIDASH_AGENT_CWD-}" ] && cd -- "$PIDASH_AGENT_CWD"
[ -n "${PIDASH_CODEX_HOME-}" ] && export CODEX_HOME="$PIDASH_CODEX_HOME"
[ -n "${PIDASH_AGENT_PATH_PREPEND-}" ] && export PATH="$PIDASH_AGENT_PATH_PREPEND:$PATH"
[ -n "${PIDASH_AGENT_CONFIG_DIR-}" ] && export PIDASH_CONFIG_DIR="$PIDASH_AGENT_CONFIG_DIR"
[ -n "${PIDASH_AGENT_DATA_DIR-}" ] && export PIDASH_DATA_DIR="$PIDASH_AGENT_DATA_DIR"
[ -n "${PIDASH_MODEL_TOKEN_FILE-}" ] && [ -r "$PIDASH_MODEL_TOKEN_FILE" ] &&   export PIDASH_GATEWAY_TOKEN="$(cat "$PIDASH_MODEL_TOKEN_FILE")"
exec "$@""#;

/// Pi Dash-controlled environment for a spawned agent process.
///
/// All fields are `None` for a user-enrolled runner, which is what keeps that
/// path byte-identical. The managed runner fills them from its `CodexSection`
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
            !joined.iter().any(|a| a.contains("model.token")
                && !a.contains("PIDASH_MODEL_TOKEN_FILE")),
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
