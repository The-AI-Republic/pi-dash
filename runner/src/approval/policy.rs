use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::cloud::protocol::{ApprovalDecision, ApprovalKind};
use crate::config::schema::ApprovalPolicySection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    AutoAccept,
    AutoDecline,
    Ask,
}

impl Decision {
    pub fn into_cloud(self) -> Option<ApprovalDecision> {
        match self {
            Self::AutoAccept => Some(ApprovalDecision::Accept),
            Self::AutoDecline => Some(ApprovalDecision::Decline),
            Self::Ask => None,
        }
    }
}

pub struct Policy<'a> {
    pub config: &'a ApprovalPolicySection,
    pub workspace_root: &'a Path,
}

impl<'a> Policy<'a> {
    pub fn new(config: &'a ApprovalPolicySection, workspace_root: &'a Path) -> Self {
        Self {
            config,
            workspace_root,
        }
    }

    pub fn evaluate(&self, kind: ApprovalKind, payload: &serde_json::Value) -> Decision {
        // Denylist wins.
        if let Some(cmd) = extract_command(payload) {
            if self
                .config
                .denylist_commands
                .iter()
                .any(|d| command_matches(&cmd, d))
            {
                return Decision::AutoDecline;
            }
            if self
                .config
                .allowlist_commands
                .iter()
                .any(|a| command_matches(&cmd, a))
            {
                return Decision::AutoAccept;
            }
            if self.config.auto_approve_readonly_shell && is_readonly_shell(&cmd) {
                return Decision::AutoAccept;
            }
        }
        match kind {
            ApprovalKind::FileChange => {
                if self.config.auto_approve_workspace_writes
                    && let Some(path) = payload.get("path").and_then(|v| v.as_str())
                    && path_under_workspace(path, self.workspace_root)
                {
                    return Decision::AutoAccept;
                }
                Decision::Ask
            }
            ApprovalKind::NetworkAccess => {
                if self.config.auto_approve_network {
                    Decision::AutoAccept
                } else {
                    Decision::Ask
                }
            }
            _ => Decision::Ask,
        }
    }
}

fn extract_command(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
}

fn command_matches(cmd: &str, pattern: &str) -> bool {
    let c = cmd.trim();
    let p = pattern.trim();
    if let Some(stripped) = p.strip_suffix(' ').and_then(|s| s.strip_suffix('*')) {
        // Pattern like `git ` (after stripping `*`) — require a word boundary.
        c.starts_with(stripped)
            && (c.len() == stripped.len() || c[stripped.len()..].starts_with(' '))
    } else if let Some(stripped) = p.strip_suffix('*') {
        // Bare `git*` — require either exact match or a space after the prefix
        // so `git pushy` does not match `git push*`.
        c.starts_with(stripped)
            && (c.len() == stripped.len() || c[stripped.len()..].starts_with(' '))
    } else {
        c == p || c.starts_with(&format!("{p} "))
    }
}

/// True iff `path` resolves *under* `workspace_root` after canonicalisation.
/// Rejects `..` traversal and symlink escape (canonicalize follows links).
/// If either path can't be canonicalized (e.g., the file doesn't exist yet),
/// fall back to a strict lexical check that refuses any `..` component.
fn path_under_workspace(path: &str, workspace_root: &Path) -> bool {
    let candidate = Path::new(path);
    let canonical_root = workspace_root.canonicalize().ok();
    if let (Ok(c), Some(r)) = (candidate.canonicalize(), canonical_root.as_ref()) {
        return c.starts_with(r);
    }
    // The file may not exist yet (codex creating a new file). Walk parents.
    let mut probe = candidate.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            return false;
        }
    }
    let Some(parent_canon) = probe.canonicalize().ok() else {
        return false;
    };
    let Some(root_canon) = canonical_root else {
        return false;
    };
    if !parent_canon.starts_with(&root_canon) {
        return false;
    }
    // Tail component(s) must not contain `..`.
    let tail = candidate.strip_prefix(&probe).unwrap_or(Path::new(""));
    !tail
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Rough heuristic: treat commands whose first token is a known read-only
/// tool as auto-approvable under `auto_approve_readonly_shell`.
///
/// SECURITY: this is a coarse check — `cat ~/.ssh/id_rsa` would match. Only
/// safe to enable when the runner is confined to a workspace it doesn't
/// share with sensitive files, and the user accepts that read-only does not
/// imply low-impact.
fn is_readonly_shell(cmd: &str) -> bool {
    const READONLY: &[&str] = &[
        "ls", "cat", "head", "tail", "wc", "grep", "rg", "find", "pwd", "which", "whoami", "file",
        "stat", "echo", "printf", "env", "date",
    ];
    let first = cmd.split_whitespace().next().unwrap_or("");
    READONLY.contains(&first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ApprovalPolicySection {
        ApprovalPolicySection::default()
    }

    #[test]
    fn denylist_wins_over_allowlist() {
        let mut c = cfg();
        c.allowlist_commands.push("git push".into());
        c.denylist_commands.push("git push".into());
        let p = Policy::new(&c, Path::new("/"));
        let decision = p.evaluate(
            ApprovalKind::CommandExecution,
            &serde_json::json!({ "command": "git push" }),
        );
        assert_eq!(decision, Decision::AutoDecline);
    }

    #[test]
    fn allowlist_auto_accepts() {
        let c = cfg();
        let p = Policy::new(&c, Path::new("/"));
        let d = p.evaluate(
            ApprovalKind::CommandExecution,
            &serde_json::json!({ "command": "ls /tmp" }),
        );
        assert_eq!(d, Decision::AutoAccept);
    }

    #[test]
    fn file_change_outside_workspace_asks() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = cfg();
        c.auto_approve_workspace_writes = true;
        let p = Policy::new(&c, tmp.path());
        let d = p.evaluate(
            ApprovalKind::FileChange,
            &serde_json::json!({ "path": "/etc/passwd" }),
        );
        assert_eq!(d, Decision::Ask);
    }

    #[test]
    fn file_change_inside_workspace_accepts_when_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = cfg();
        c.auto_approve_workspace_writes = true;
        let p = Policy::new(&c, tmp.path());
        let target = tmp.path().join("src/lib.rs");
        let d = p.evaluate(
            ApprovalKind::FileChange,
            &serde_json::json!({ "path": target.to_str().unwrap() }),
        );
        assert_eq!(d, Decision::AutoAccept);
    }

    #[test]
    fn file_change_with_dotdot_traversal_asks() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = cfg();
        c.auto_approve_workspace_writes = true;
        let p = Policy::new(&c, tmp.path());
        // Path lexically appears under workspace but escapes via `..`.
        let target = tmp.path().join("inner/../../../../etc/passwd");
        let d = p.evaluate(
            ApprovalKind::FileChange,
            &serde_json::json!({ "path": target.to_str().unwrap() }),
        );
        assert_eq!(d, Decision::Ask);
    }

    #[test]
    fn readonly_shell_auto_accepts_when_enabled() {
        let mut c = cfg();
        c.auto_approve_readonly_shell = true; // off by default now
        let p = Policy::new(&c, Path::new("/"));
        let d = p.evaluate(
            ApprovalKind::CommandExecution,
            &serde_json::json!({ "command": "grep -R foo src/" }),
        );
        assert_eq!(d, Decision::AutoAccept);
    }

    #[test]
    fn glob_pattern_word_boundary() {
        let mut c = cfg();
        c.denylist_commands.clear(); // default denylists `git push`
        c.allowlist_commands = vec!["git push*".into()];
        let p = Policy::new(&c, Path::new("/"));
        // `git pushy` must NOT match `git push*` — there's no word boundary.
        let d = p.evaluate(
            ApprovalKind::CommandExecution,
            &serde_json::json!({ "command": "git pushy" }),
        );
        assert_eq!(d, Decision::Ask);
        // But `git push origin` should match.
        let d = p.evaluate(
            ApprovalKind::CommandExecution,
            &serde_json::json!({ "command": "git push origin" }),
        );
        assert_eq!(d, Decision::AutoAccept);
    }
}
