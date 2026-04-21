use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

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
/// directly onto an existing feature branch. Called before the Codex process
/// spawns when an issue specifies `git_work_branch`.
pub async fn checkout_work_branch(path: &Path, branch: &str) -> Result<()> {
    // Reject shell metacharacters / flag-like values up front. Branches with
    // these characters cannot be valid git refs, and passing them to git would
    // either error cryptically or (worst case) let a crafted ref smuggle flags
    // through as `git` command options.
    if branch.is_empty()
        || branch.starts_with('-')
        || branch.chars().any(|c| c.is_control() || c == ' ')
    {
        anyhow::bail!("invalid work branch name: {branch:?}");
    }

    git_output(path, &["fetch", "origin", branch])
        .await
        .with_context(|| format!("git fetch origin {branch}"))?;
    // `git checkout -B <branch> <start_point>` always lands us on a local
    // branch that points at `origin/<branch>`, creating it if necessary.
    let remote_ref = format!("origin/{branch}");
    git_output(path, &["checkout", "-B", branch, remote_ref.as_str()])
        .await
        .with_context(|| format!("git checkout {branch}"))?;
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
