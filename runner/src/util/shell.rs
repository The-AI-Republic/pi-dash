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
//! The script we pass to `-c` is just `exec "$@"`: `exec` replaces bash
//! with the target (so the spawned PID and any signals we deliver hit the
//! agent, not a lingering shell), and `"$@"` preserves our structured argv
//! with no shell re-parsing of agent flags.
//!
//! Running an interactive bash without a controlling TTY makes bash emit
//! two diagnostic lines to stderr during startup:
//!   `bash: cannot set terminal process group (-1): Inappropriate ioctl for device`
//!   `bash: no job control in this shell`
//! Those lines are harmless (bash just can't install a foreground job
//! group), but they're emitted before our `-c` script runs so they can't be
//! silenced from inside the script. Callers should filter them out of any
//! captured stderr via [`is_benign_login_shell_warning`].

use tokio::process::Command;

/// Build a [`Command`] that runs `program args…` through a login+interactive
/// bash. See the module docs for why `-i` is required alongside `-l`.
///
/// The returned command still needs the caller's usual stdio / cwd /
/// `kill_on_drop` wiring before being spawned.
pub fn login_shell_command(program: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new("bash");
    cmd.arg("-ilc").arg(r#"exec "$@""#).arg("bash").arg(program);
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

    fn argv(cmd: &Command) -> Vec<OsString> {
        cmd.as_std().get_args().map(|a| a.to_os_string()).collect()
    }

    #[test]
    fn wraps_program_and_args_in_login_bash() {
        let cmd = login_shell_command("claude", &["--print", "--model", "sonnet-4"]);
        assert_eq!(cmd.as_std().get_program(), "bash");
        assert_eq!(
            argv(&cmd),
            vec![
                OsString::from("-ilc"),
                OsString::from(r#"exec "$@""#),
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
        let cmd = login_shell_command("codex", &[]);
        assert_eq!(
            argv(&cmd),
            vec![
                OsString::from("-ilc"),
                OsString::from(r#"exec "$@""#),
                OsString::from("bash"),
                OsString::from("codex"),
            ]
        );
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
        // split or re-parsed by the shell. stderr is merged into stdout so
        // the test passes regardless of whether bash emits its no-tty
        // warnings (it does under `cargo test`, it doesn't under a tty).
        let mut cmd =
            login_shell_command("printf", &["%s\n", "first arg", "second-arg", "has=equal"]);
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
}
