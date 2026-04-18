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
                if self.config.auto_approve_workspace_writes {
                    if let Some(path) = payload.get("path").and_then(|v| v.as_str()) {
                        let p = Path::new(path);
                        if p.starts_with(self.workspace_root) {
                            return Decision::AutoAccept;
                        }
                    }
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
    if let Some(stripped) = p.strip_suffix("*") {
        c.starts_with(stripped.trim_end())
    } else {
        c == p || c.starts_with(&format!("{p} "))
    }
}

/// Rough heuristic: treat commands whose first token is a known read-only
/// tool as auto-approvable under `auto_approve_readonly_shell`.
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
    use std::path::PathBuf;

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
        let mut c = cfg();
        c.auto_approve_workspace_writes = true;
        let ws = PathBuf::from("/workspace");
        let p = Policy::new(&c, &ws);
        let d = p.evaluate(
            ApprovalKind::FileChange,
            &serde_json::json!({ "path": "/etc/passwd" }),
        );
        assert_eq!(d, Decision::Ask);
    }

    #[test]
    fn file_change_inside_workspace_accepts_when_enabled() {
        let mut c = cfg();
        c.auto_approve_workspace_writes = true;
        let ws = PathBuf::from("/workspace");
        let p = Policy::new(&c, &ws);
        let d = p.evaluate(
            ApprovalKind::FileChange,
            &serde_json::json!({ "path": "/workspace/src/lib.rs" }),
        );
        assert_eq!(d, Decision::AutoAccept);
    }

    #[test]
    fn readonly_shell_auto_accepts() {
        let c = cfg();
        let p = Policy::new(&c, Path::new("/"));
        let d = p.evaluate(
            ApprovalKind::CommandExecution,
            &serde_json::json!({ "command": "grep -R foo src/" }),
        );
        assert_eq!(d, Decision::AutoAccept);
    }
}
