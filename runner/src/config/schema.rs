use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub runner: RunnerSection,
    pub workspace: WorkspaceSection,
    pub codex: CodexSection,
    pub approval_policy: ApprovalPolicySection,
    pub logging: LoggingSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerSection {
    pub name: String,
    pub cloud_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSection {
    pub working_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSection {
    pub binary: String,
    pub model_default: Option<String>,
}

impl Default for CodexSection {
    fn default() -> Self {
        Self {
            binary: "codex".to_string(),
            model_default: Some("gpt-5-codex".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPolicySection {
    pub auto_approve_readonly_shell: bool,
    pub auto_approve_workspace_writes: bool,
    pub auto_approve_network: bool,
    pub allowlist_commands: Vec<String>,
    pub denylist_commands: Vec<String>,
}

impl Default for ApprovalPolicySection {
    fn default() -> Self {
        Self {
            auto_approve_readonly_shell: true,
            auto_approve_workspace_writes: false,
            auto_approve_network: false,
            allowlist_commands: vec![
                "ls".into(),
                "cat".into(),
                "pwd".into(),
                "git status".into(),
                "git diff".into(),
                "git log".into(),
                "git branch".into(),
            ],
            denylist_commands: vec!["rm -rf /".into(), "git push".into(), "sudo".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSection {
    pub level: String,
    pub retention_days: u32,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            retention_days: 14,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub runner_id: Uuid,
    pub runner_secret: String,
    pub issued_at: DateTime<Utc>,
}
