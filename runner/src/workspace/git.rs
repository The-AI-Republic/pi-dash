use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use uuid::Uuid;

use crate::cloud::protocol::WorkspaceState;

pub async fn clone(url: &str, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating parent dir {parent:?}"))?;
    }
    // `--` separator is required: a hostile `url` like `--upload-pack=...` would
    // otherwise be interpreted as a flag and execute arbitrary commands.
    let out = Command::new("git")
        .arg("clone")
        .arg("--")
        .arg(url)
        .arg(target)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("spawning git clone")?;
    if !out.status.success() {
        anyhow::bail!(
            "git clone failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

pub fn is_empty_dir(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut rd) => rd.next().is_none(),
        Err(_) => false,
    }
}

pub async fn workspace_state(path: &Path) -> Result<WorkspaceState> {
    let branch = git_output(path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .ok();
    let head = git_output(path, &["rev-parse", "HEAD"]).await.ok();
    let status = git_output(path, &["status", "--porcelain"]).await?;
    let dirty = !status.trim().is_empty();
    Ok(WorkspaceState {
        branch,
        head,
        dirty,
    })
}

/// Fetch from origin and check out the given branch so the agent can commit
/// directly onto an existing feature branch. Called before the agent process
/// spawns when an issue specifies `git_work_branch`.
///
/// Resets the local branch to `origin/<branch>` unless the local ref is
/// strictly ahead of origin (e.g. a WIP commit whose push failed) —
/// resetting then would silently destroy the only copy of that work.
///
/// (Removing this platform-side checkout is scoped to PDASHOSS01-136.)
pub async fn checkout_work_branch(path: &Path, branch: &str) -> Result<()> {
    validate_branch_name(branch)?;

    git_output(path, &["fetch", "origin", branch])
        .await
        .with_context(|| format!("git fetch origin {branch}"))?;
    let remote_ref = format!("origin/{branch}");
    // Preserve a local branch that is strictly ahead of origin (salvage
    // commits whose push failed live there). `merge-base --is-ancestor`
    // exits 0 when origin/<branch> is an ancestor of the local ref.
    let local_exists = git_output(path, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
        .await
        .is_ok();
    if local_exists
        && git_output(path, &["merge-base", "--is-ancestor", remote_ref.as_str(), branch]).await.is_ok()
    {
        git_output(path, &["checkout", branch])
            .await
            .with_context(|| format!("git checkout {branch}"))?;
        return Ok(());
    }
    if local_exists {
        // Diverged local ref: about to be reset to origin. The old tip stays
        // reachable via the reflog; record it so operators can recover.
        if let Ok(old) = git_output(path, &["rev-parse", branch]).await {
            tracing::warn!(%branch, old_tip = %old, "local branch diverged from origin; resetting (old tip kept in reflog)");
        }
    }
    // `git checkout -B <branch> <start_point>` always lands us on a local
    // branch that points at `origin/<branch>`, creating it if necessary.
    git_output(path, &["checkout", "-B", branch, remote_ref.as_str()])
        .await
        .with_context(|| format!("git checkout {branch}"))?;
    Ok(())
}

/// The branch HEAD is on, or `None` when detached.
pub async fn current_branch(worktree: &Path) -> Result<Option<String>> {
    let name = git_output(worktree, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    if name == "HEAD" { Ok(None) } else { Ok(Some(name)) }
}

/// Scrub a working directory between runs, per `mode`:
/// - `KeepIgnored`: `reset --hard` + `clean -fd` (keep gitignored files).
/// - `Allowlist`: like `Full` but `--exclude`s each `keep_paths` glob.
/// - `Full`: `reset --hard` + `clean -fdx` (pristine).
pub async fn reset_clean(
    worktree: &Path,
    mode: crate::config::schema::CleanMode,
    keep_paths: &[String],
) -> Result<()> {
    use crate::config::schema::CleanMode;
    git_output(worktree, &["reset", "--hard"])
        .await
        .context("git reset --hard")?;
    match mode {
        CleanMode::KeepIgnored => {
            git_output(worktree, &["clean", "-fd"])
                .await
                .context("git clean -fd")?;
        }
        CleanMode::Full => {
            git_output(worktree, &["clean", "-fdx"])
                .await
                .context("git clean -fdx")?;
        }
        CleanMode::Allowlist => {
            // Build `clean -fdx -e <glob> -e <glob> …`. Reject globs that look
            // like flags so a malicious config can't smuggle one past `git`.
            let mut args: Vec<String> =
                vec!["clean".into(), "-fdx".into()];
            for g in keep_paths {
                if g.starts_with('-') || g.contains(['\n', '\0']) {
                    anyhow::bail!("invalid keep_paths glob: {g:?}");
                }
                args.push("-e".into());
                args.push(g.clone());
            }
            let argref: Vec<&str> = args.iter().map(String::as_str).collect();
            git_output(worktree, &argref)
                .await
                .context("git clean -fdx (allowlist)")?;
        }
    }
    Ok(())
}

/// Best-effort salvage of a dirty working directory. Commits everything as a
/// WIP commit and tries to push it.
///
/// The commit lands on whatever branch HEAD is actually on — the truthful
/// location of the work, even if the agent switched branches mid-run. When
/// HEAD is detached (so a plain commit would be orphaned), the WIP is parked on
/// a dedicated `pidash/salvage/<holder>` branch instead; no existing branch ref
/// is ever moved. The commit is left on the local branch even if the push
/// fails. Returns `Ok(Some(sha))` if a WIP commit was made, `Ok(None)` if the
/// tree was clean (nothing to salvage).
///
/// (Removing this platform-side salvage is scoped to PDASHOSS01-136.)
pub async fn salvage_wip(worktree: &Path, holder: Uuid, label: &str) -> Result<Option<String>> {
    // Nothing to do if the tree is clean.
    let status = git_output(worktree, &["status", "--porcelain"]).await?;
    if status.trim().is_empty() {
        return Ok(None);
    }
    let target = match current_branch(worktree).await? {
        Some(b) => b,
        None => {
            // Detached HEAD: park the WIP on a fresh salvage branch. Never
            // `checkout -B` an existing branch here — that would move a ref
            // that may point at someone else's work.
            let name = format!("pidash/salvage/{holder}");
            git_output(worktree, &["checkout", "-b", &name])
                .await
                .with_context(|| format!("git checkout -b {name} (salvage)"))?;
            name
        }
    };
    git_output(worktree, &["add", "-A"])
        .await
        .context("git add -A (salvage)")?;
    let msg = format!("wip(pidash): salvaged working state from {label}");
    // `--no-verify` so a repo's commit hooks can't block salvage; `-q` quiets.
    git_output(worktree, &["commit", "-q", "--no-verify", "-m", &msg])
        .await
        .context("git commit (salvage)")?;
    let sha = git_output(worktree, &["rev-parse", "HEAD"]).await.ok();
    // Push is best-effort: salvage's value is the local commit; the push is a
    // convenience. A push failure (auth/network) must not block desk recycle.
    // Push `HEAD:` explicitly so the remote ref updated is the branch the
    // commit actually sits on, not a stale local ref of the same name.
    let refspec = format!("HEAD:refs/heads/{target}");
    if let Err(e) = git_output(worktree, &["push", "origin", &refspec]).await {
        tracing::warn!(branch = %target, error = %e, "salvage push failed; WIP commit kept locally");
    }
    Ok(sha)
}

/// Injection-level validation: reject names that could be mistaken for a git
/// flag or contain characters that don't belong in a branch name. Does not
/// enforce the full `git check-ref-format` rules — git itself catches those
/// when the command runs and surfaces the error via `WorkspaceSetup`.
fn validate_branch_name(branch: &str) -> Result<()> {
    if branch.is_empty()
        || branch.starts_with('-')
        || branch.chars().any(|c| c.is_control() || c == ' ')
    {
        anyhow::bail!("invalid work branch name: {branch:?}");
    }
    Ok(())
}

async fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty() {
        assert!(validate_branch_name("").is_err());
    }

    #[test]
    fn rejects_leading_dash() {
        // `-rf`, `--upload-pack=...` could otherwise smuggle a flag past `git`.
        assert!(validate_branch_name("-rf").is_err());
        assert!(validate_branch_name("--upload-pack=evil").is_err());
    }

    #[test]
    fn rejects_whitespace_and_control_chars() {
        assert!(validate_branch_name(" ").is_err());
        assert!(validate_branch_name("feat/with space").is_err());
        assert!(validate_branch_name("feat\tname").is_err());
        assert!(validate_branch_name("feat\nname").is_err());
        assert!(validate_branch_name("feat\0name").is_err());
    }

    #[test]
    fn accepts_normal_branch_names() {
        assert!(validate_branch_name("main").is_ok());
        assert!(validate_branch_name("feat/pinned-branch").is_ok());
        assert!(validate_branch_name("release/1.2.3").is_ok());
        assert!(validate_branch_name("user/jdoe/fix_42").is_ok());
    }
}
