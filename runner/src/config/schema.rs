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
    /// Claude Code agent settings. Missing section falls back to
    /// `ClaudeCodeSection::default()` so existing `config.toml` files
    /// (written before Claude Code support) still parse. Only consulted
    /// when `agent.kind == claude_code`.
    #[serde(default)]
    pub claude_code: ClaudeCodeSection,
    /// Which agent CLI the daemon drives for assigned runs. Defaults to
    /// `codex` so existing deployments are unaffected.
    #[serde(default)]
    pub agent: AgentSection,
    /// Missing section falls back to the `ApprovalPolicySection::default()` so
    /// a minimal `config.toml` doesn't have to spell out every knob.
    #[serde(default)]
    pub approval_policy: ApprovalPolicySection,
    /// Missing section falls back to the `LoggingSection::default()`.
    #[serde(default)]
    pub logging: LoggingSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerSection {
    pub name: String,
    pub cloud_url: String,
    /// Slug of the workspace this runner is bound to. Populated from the
    /// register response at `pidash configure` time. `Option` so an older
    /// `config.toml` (written before this field existed) still parses; new
    /// CRUD subcommands hard-error if it's missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSection {
    pub working_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSection {
    pub binary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// Runner-wide agent selector. Wrapped in its own section (rather than
/// hoisted onto `RunnerSection`) so the file layout mirrors the per-agent
/// `[codex]` / `[claude_code]` tables: `[agent]` with `kind = "..."`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSection {
    #[serde(default)]
    pub kind: AgentKind,
}

/// Which agent CLI the runner drives for assigned runs. Serialised as a
/// lowercase string (`"codex"` / `"claude_code"`) so the config file stays
/// human-friendly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// OpenAI Codex via `codex app-server`. Default for backward compatibility.
    #[default]
    Codex,
    /// Anthropic Claude Code via `claude --print --output-format stream-json`.
    ClaudeCode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeSection {
    /// Per-field default so a partial `[claude_code]` block (e.g. only
    /// `model_default = "..."`) still parses. Without this, declaring the
    /// section at all would require spelling out `binary = "claude"`.
    #[serde(default = "default_claude_binary")]
    pub binary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_default: Option<String>,
}

fn default_claude_binary() -> String {
    "claude".to_string()
}

impl Default for ClaudeCodeSection {
    fn default() -> Self {
        Self {
            binary: default_claude_binary(),
            model_default: None,
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
        // `auto_approve_readonly_shell` defaults OFF: the heuristic accepts
        // any command whose first token is in the readonly list, which
        // includes things like `cat ~/.ssh/id_rsa` and `find / -perm -4000`.
        // Users can opt in once they've narrowed the allowlist.
        Self {
            auto_approve_readonly_shell: false,
            auto_approve_workspace_writes: false,
            auto_approve_network: false,
            allowlist_commands: vec![
                "ls".into(),
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
    /// Public REST API token (`X-Api-Key`) for `/api/v1/`. Issued by the
    /// cloud alongside `runner_secret` and used by future `pidash` CRUD
    /// subcommands. `None` for installs enrolled before the cloud started
    /// minting these; a follow-up `pidash login` will retrofit them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
    pub issued_at: DateTime<Utc>,
}
