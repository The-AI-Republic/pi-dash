//! Auto-install a runner's selected agent CLI via the vendor's *own* official
//! install script (PDASHOSS01-155).
//!
//! When `pidash runner add` registers a runner whose agent binary isn't present
//! on this dev machine, we install it for the operator by invoking the vendor's
//! official installer — never a bundled/vendored copy. The command table lives
//! in [`crate::config::schema::AgentKind::official_installer`] so adding a new
//! agent means adding one row there; this module is only the *execution* half:
//! run the command with output streamed to the terminal, then re-detect the
//! binary and record its absolute path in the runner's config (the installer
//! usually drops the binary in a dir like `~/.local/bin` that isn't yet on this
//! process's PATH, so we resolve the real path rather than trust PATH).
//!
//! Everything here is best-effort: `pidash runner add` must still succeed even
//! if the install fails. On any failure we fall back to the previous behaviour
//! — print and open the vendor's install page — and leave `pidash doctor`
//! reporting the agent as missing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use std::io::IsTerminal;
use uuid::Uuid;

use crate::config::file;
use crate::config::schema::{AgentKind, RunnerConfig};
use crate::util::paths::Paths;

/// Ensure the runner's selected agent CLI is installed on this machine.
///
/// Called at the tail of `pidash runner add`, after enrollment and service
/// setup have succeeded. Non-fatal by design — every path returns `()`; the
/// binary only has to exist by the time the daemon spawns a run, and
/// `pidash doctor` re-checks it.
///
/// Flow:
///   1. If the binary already runs, do nothing.
///   2. If `skip_install` (the `--skip-agent-install` opt-out), just remind +
///      open the install page.
///   3. Otherwise run the vendor's official installer (streamed), then
///      re-detect the binary, resolve its absolute path, and persist it to the
///      runner's agent-section `binary` config.
///   4. On any failure, fall back to opening the install page.
pub async fn ensure_agent_installed(
    paths: &Paths,
    runner_id: Uuid,
    agent: AgentKind,
    skip_install: bool,
) {
    let binary = agent.default_binary();
    if crate::util::shell::binary_runs_version(binary).await {
        return;
    }

    let name = agent.display_name();
    println!();
    println!("⚠ Pi Dash could not run the {name} CLI (`{binary}`) on this machine.");
    println!(
        "  This runner drives {name}, so its runs will fail until `{binary}` is installed."
    );

    if skip_install {
        println!("  --skip-agent-install was set, so Pi Dash won't install it for you.");
        open_install_page(agent);
        return;
    }

    // Pick the vendor installer for this agent + platform, or fall back.
    let installer = match agent.official_installer() {
        Some(i) => i,
        None => {
            println!("  Pi Dash has no verified official installer wired up for {name}.");
            open_install_page(agent);
            return;
        }
    };
    let cmd = match installer.command_for_host() {
        Some(c) => c,
        None => {
            println!("  {name} has no official one-line installer for this platform.");
            open_install_page(agent);
            return;
        }
    };

    println!(
        "  Installing {name} with its official installer (see {}).",
        installer.doc_url
    );
    println!("  Running: {cmd}");
    println!();

    if let Err(e) = run_install_command(cmd).await {
        println!();
        println!("  {name} install failed: {e:#}");
        open_install_page(agent);
        return;
    }

    // Re-detect: the installer usually drops the binary in a dir like
    // ~/.local/bin that isn't on this process's PATH yet, so resolve the
    // absolute path rather than assume PATH is updated.
    match resolve_installed_binary(binary).await {
        Some(path) => {
            println!();
            println!("  ✓ Installed {name}: {}", path.display());
            match persist_binary_path(paths, runner_id, agent, &path) {
                Ok(()) => println!("    Recorded its path in this runner's config."),
                Err(e) => println!(
                    "    (Installed, but recording the path in config failed: {e:#}. \
                     `pidash doctor` will still find it if it's on PATH.)"
                ),
            }
            // Installers place the binary but never authenticate it — remind the
            // operator of the one login step still required before runs start.
            if let Some(hint) = agent.post_install_login_hint() {
                println!("    One step left before runs start — log in: {hint}");
            }
        }
        None => {
            println!();
            println!(
                "  {name} installer finished but `{binary}` still isn't resolvable from here."
            );
            println!("  Open a new terminal so your PATH picks it up, then run `pidash doctor`.");
            open_install_page(agent);
        }
    }
}

/// Run one install command with its output streamed straight to the operator's
/// terminal. macOS/Linux/WSL run it in a login `bash` (so profile PATH
/// additions like Homebrew are visible); Windows runs it in PowerShell.
async fn run_install_command(cmd: &str) -> Result<()> {
    let mut command = if cfg!(target_os = "windows") {
        let mut c = tokio::process::Command::new("powershell");
        c.arg("-NoProfile").arg("-Command").arg(cmd);
        c
    } else {
        let mut c = tokio::process::Command::new("bash");
        // `-l` sources the operator's profile so `curl` / `npm` resolve exactly
        // as they do in their interactive shell. stdio is inherited (the
        // default), so the vendor installer streams directly to the terminal.
        c.arg("-lc").arg(cmd);
        c
    };
    let status = command
        .status()
        .await
        .with_context(|| format!("spawning install command: {cmd}"))?;
    if !status.success() {
        anyhow::bail!("install command exited with {status}");
    }
    Ok(())
}

/// Find the absolute path of `binary` after an install, without trusting this
/// process's PATH (the installer's bin dir may only be wired into a *new*
/// shell). Tries, in order:
///   1. a fresh login shell's `command -v` — picks up any PATH line the
///      installer appended to the operator's shell rc;
///   2. the well-known per-user bin dirs the vendor scripts drop binaries into.
pub async fn resolve_installed_binary(binary: &str) -> Option<PathBuf> {
    if let Some(p) = which_via_login_shell(binary).await {
        return Some(p);
    }
    candidate_install_paths(binary)
        .into_iter()
        .find(|p| p.is_file())
}

/// Ask a fresh login shell where `binary` resolves *now*. Returns the path only
/// when it's absolute and points at an existing file.
async fn which_via_login_shell(binary: &str) -> Option<PathBuf> {
    let out = if cfg!(target_os = "windows") {
        tokio::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(format!(
                "(Get-Command {binary} -ErrorAction SilentlyContinue).Source"
            ))
            .output()
            .await
            .ok()?
    } else {
        // `command -v -- "$0"` prints the resolved path; `$0` is the first
        // positional after the `-c` script, so the binary name is passed as an
        // argument rather than interpolated into the script text.
        tokio::process::Command::new("bash")
            .arg("-lc")
            .arg(r#"command -v -- "$0""#)
            .arg(binary)
            .output()
            .await
            .ok()?
    };
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let path = line.lines().find(|l| !l.trim().is_empty())?.trim();
    let path = PathBuf::from(path);
    if path.is_absolute() && path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Well-known per-user directories the supported vendor installers drop their
/// binary into, in priority order. Used as a fallback when a fresh login shell
/// can't yet resolve the binary (PATH not wired up until a new terminal).
fn candidate_install_paths(binary: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(target_os = "windows") {
        if let Some(home) = std::env::var_os("USERPROFILE") {
            let base = PathBuf::from(home).join(".local").join("bin");
            paths.push(base.join(format!("{binary}.exe")));
            paths.push(base.join(binary));
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        // claude, codex, and cursor-agent all install here today.
        paths.push(home.join(".local").join("bin").join(binary));
    }
    paths
}

/// Write the resolved absolute path into the runner's agent-section `binary`
/// config so the daemon runs the freshly installed binary regardless of PATH.
fn persist_binary_path(
    paths: &Paths,
    runner_id: Uuid,
    agent: AgentKind,
    path: &Path,
) -> Result<()> {
    file::mutate_config(paths, |cfg| {
        if let Some(runner) = cfg.runners.iter_mut().find(|r| r.runner_id == runner_id) {
            set_runner_agent_binary(runner, agent, path);
        }
        Ok(())
    })
    .map(|_| ())
}

/// Set the `binary` field of the config section that matches `agent`.
pub fn set_runner_agent_binary(runner: &mut RunnerConfig, agent: AgentKind, path: &Path) {
    let p = path.display().to_string();
    match agent {
        AgentKind::Codex => runner.codex.binary = p,
        AgentKind::ClaudeCode => runner.claude_code.binary = p,
        AgentKind::CursorAgent => runner.cursor_agent.binary = p,
        AgentKind::OpenClaw => runner.openclaw.binary = p,
        AgentKind::Grok => runner.grok.binary = p,
        AgentKind::MuseCode => runner.muse_code.binary = p,
    }
}

/// Print the vendor install page and, when attached to an interactive
/// terminal, open it in the operator's browser. This is the fallback whenever
/// the auto-install path can't run or fails — the same behaviour
/// `pidash runner add` had before PDASHOSS01-155.
fn open_install_page(agent: AgentKind) {
    let url = agent.install_page_url();
    println!("  Install it yourself from: {url}");
    if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
        match crate::util::browser::open_url(url) {
            Ok(()) => println!("  (Opened the install page in your default browser.)"),
            Err(_) => println!("  (Open the link above to install, then re-run if needed.)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        AgentKind, ClaudeCodeSection, CodexSection, CursorAgentSection, GrokSection,
        MuseCodeSection, OpenClawSection, RunnerConfig, WorkspaceSection,
    };
    use std::path::PathBuf;

    fn blank_runner() -> RunnerConfig {
        RunnerConfig {
            name: "solo".into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: Some("WS".into()),
            project_slug: Some("PRJ".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: PathBuf::from("/tmp/pi-dash-agent-install-test"),
            },
            agent: Default::default(),
            codex: CodexSection::default(),
            claude_code: ClaudeCodeSection::default(),
            cursor_agent: CursorAgentSection::default(),
            openclaw: OpenClawSection::default(),
            grok: GrokSection::default(),
            muse_code: MuseCodeSection::default(),
            approval_policy: Default::default(),
        }
    }

    #[test]
    fn set_runner_agent_binary_targets_the_right_section() {
        let path = PathBuf::from("/home/dev/.local/bin/claude");

        let mut r = blank_runner();
        set_runner_agent_binary(&mut r, AgentKind::ClaudeCode, &path);
        assert_eq!(r.claude_code.binary, "/home/dev/.local/bin/claude");
        // Only the matching section is touched.
        assert_eq!(r.codex.binary, CodexSection::default().binary);
        assert_eq!(r.cursor_agent.binary, CursorAgentSection::default().binary);

        let mut r = blank_runner();
        set_runner_agent_binary(&mut r, AgentKind::Codex, &PathBuf::from("/opt/codex"));
        assert_eq!(r.codex.binary, "/opt/codex");
        assert_eq!(r.claude_code.binary, ClaudeCodeSection::default().binary);

        let mut r = blank_runner();
        set_runner_agent_binary(
            &mut r,
            AgentKind::CursorAgent,
            &PathBuf::from("/opt/cursor-agent"),
        );
        assert_eq!(r.cursor_agent.binary, "/opt/cursor-agent");

        let mut r = blank_runner();
        set_runner_agent_binary(&mut r, AgentKind::MuseCode, &PathBuf::from("/opt/muse"));
        assert_eq!(r.muse_code.binary, "/opt/muse");
    }

    #[test]
    fn candidate_install_paths_are_absolute_and_named_for_the_binary() {
        // Regardless of platform, every candidate must be an absolute path that
        // ends in the requested binary name — that's what we probe on disk.
        // Guard the env-dependent branch: only assert when the relevant home
        // var is set on this host.
        let home_set = if cfg!(target_os = "windows") {
            std::env::var_os("USERPROFILE").is_some()
        } else {
            std::env::var_os("HOME").is_some()
        };
        let cands = candidate_install_paths("claude");
        if home_set {
            assert!(!cands.is_empty(), "expected at least one candidate path");
        }
        for p in cands {
            assert!(p.is_absolute(), "candidate not absolute: {}", p.display());
            let name = p.file_name().unwrap().to_string_lossy();
            assert!(
                name == "claude" || name == "claude.exe",
                "candidate not named for the binary: {}",
                p.display()
            );
        }
    }

    #[tokio::test]
    async fn resolve_installed_binary_finds_a_present_binary() {
        // A binary that's genuinely on PATH must resolve to an absolute path.
        let binary = if cfg!(target_os = "windows") {
            "cargo"
        } else {
            "sh"
        };
        let resolved = resolve_installed_binary(binary).await;
        assert!(
            resolved.is_some(),
            "expected to resolve a present binary {binary:?}"
        );
        let p = resolved.unwrap();
        assert!(p.is_absolute(), "resolved path not absolute: {}", p.display());
    }

    #[tokio::test]
    async fn resolve_installed_binary_none_for_missing_binary() {
        assert!(
            resolve_installed_binary("pidash-no-such-agent-binary-xyz")
                .await
                .is_none()
        );
    }
}
