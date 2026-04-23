//! Agent-spawn helper.
//!
//! The runner lives inside a daemon started by `systemd --user` on Linux
//! and `launchd` on macOS. Both managers expose a stripped `PATH` that does
//! not include anything the user's shell rc adds (nvm, pyenv, asdf, brew
//! on Linux, …). A plain `Command::new("claude")` therefore fails with
//! `ENOENT` on machines where `claude` works interactively.
//!
//! We sidestep this by wrapping every agent spawn in a login+interactive
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

use std::path::Path;
use tokio::process::Command;

const SHELL_SCRIPT: &str =
    r#"[ -n "${PIDASH_AGENT_CWD-}" ] && cd -- "$PIDASH_AGENT_CWD"; exec "$@""#;

/// Build a [`Command`] that runs `program args…` through a login+interactive
/// bash. See the module docs for why `-i` is required alongside `-l`.
///
/// `cwd`, when `Some`, is both applied to the outer bash (via `current_dir`)
/// and re-asserted inside the script after rc files run, so `.bashrc`'s
/// `cd` side effects can't clobber the agent's starting directory.
///
/// The returned command still needs the caller's usual stdio and
/// `kill_on_drop` wiring before being spawned.
pub fn login_shell_command(program: &str, args: &[&str], cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new("bash");
    // Pin the message locale so `is_benign_login_shell_warning` matches
    // regardless of the operator's LANG / LC_ALL.
    cmd.env("LC_MESSAGES", "C");
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
        cmd.env("PIDASH_AGENT_CWD", cwd.as_os_str());
    }
    cmd.arg("-ilc").arg(SHELL_SCRIPT).arg("bash").arg(program);
    cmd.args(args);
    cmd
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

    #[test]
    fn pins_lc_messages_to_c_for_stable_warning_strings() {
        // The stderr filter is exact-match English; pinning LC_MESSAGES=C
        // keeps it working under localized hosts.
        let cmd = login_shell_command("claude", &[], None);
        assert_eq!(env_value(&cmd, "LC_MESSAGES"), Some(OsString::from("C")));
    }

    #[test]
    fn cwd_none_does_not_set_pidash_agent_cwd() {
        let cmd = login_shell_command("claude", &["--version"], None);
        assert_eq!(env_value(&cmd, "PIDASH_AGENT_CWD"), None);
    }

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
}
