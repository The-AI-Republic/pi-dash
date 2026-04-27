use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub daemon: DaemonConfig,
    /// Per-instance runner configs. Length 1 in the current single-runner
    /// daemon; will grow once the cap is lifted (design.md §16).
    #[serde(default, rename = "runner")]
    pub runners: Vec<RunnerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub cloud_url: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_retention_days")]
    pub log_retention_days: u32,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_retention_days() -> u32 {
    14
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            cloud_url: String::new(),
            log_level: default_log_level(),
            log_retention_days: default_retention_days(),
        }
    }
}

/// One configured runner instance. The daemon currently hosts exactly one
/// (`Vec<RunnerConfig>` length 1); multi-runner support arrives in a later
/// phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    pub name: String,
    pub runner_id: Uuid,
    /// Slug of the workspace this runner is bound to. Populated from the
    /// register response at `pidash configure` time. `Option` so an older
    /// config still parses; new CRUD subcommands hard-error if it's missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_slug: Option<String>,
    pub workspace: WorkspaceSection,
    #[serde(default)]
    pub agent: AgentSection,
    #[serde(default)]
    pub codex: CodexSection,
    /// Claude Code agent settings. Missing section falls back to
    /// `ClaudeCodeSection::default()` so existing `config.toml` files
    /// (written before Claude Code support) still parse. Only consulted
    /// when `agent.kind == claude_code`.
    #[serde(default)]
    pub claude_code: ClaudeCodeSection,
    /// Missing section falls back to `ApprovalPolicySection::default()` so
    /// a minimal `config.toml` doesn't have to spell out every knob.
    #[serde(default)]
    pub approval_policy: ApprovalPolicySection,
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

/// Per-runner agent selector. Wrapped in its own section (rather than
/// hoisted onto `RunnerConfig`) so the file layout mirrors the per-agent
/// `[runner.codex]` / `[runner.claude_code]` tables: `[runner.agent]` with
/// `kind = "..."`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSection {
    #[serde(default)]
    pub kind: AgentKind,
}

/// Which agent CLI the runner drives for assigned runs. Serialised as a
/// lowercase string (`"codex"` / `"claude_code"`) so the config file stays
/// human-friendly. `ValueEnum` lets `--agent` accept the kebab-case spelling
/// on the CLI (`codex`, `claude-code`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, clap::ValueEnum)]
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
pub struct Credentials {
    /// Token (machine credential) — authenticates the WS connection. The
    /// design's eventual shape (one token per machine, owns N runners) is
    /// surfaced as an `Option` here while cloud is still on v1 wire auth;
    /// it'll become required once cloud ships v2 and the `runner_secret`
    /// path retires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<TokenCredentials>,
    pub runner_id: Uuid,
    pub runner_secret: String,
    /// Public REST API token (`X-Api-Key`) for `/api/v1/`. Issued by the
    /// cloud alongside `runner_secret` and used by `pidash` CRUD subcommands.
    /// `None` for installs enrolled before the cloud started minting these;
    /// a follow-up `pidash login` will retrofit them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
    pub issued_at: DateTime<Utc>,
}

/// Per-machine token credential. See `design.md` §5.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCredentials {
    pub token_id: Uuid,
    /// Bearer secret. Mode 0600 on disk; never echoed to logs.
    pub token_secret: String,
    /// User-supplied label, shown in `pidash status` and the cloud UI's
    /// connections list.
    pub title: String,
}

impl Config {
    /// Convenience accessor for the (currently unique) runner config.
    /// Panics if the runners list is empty — daemon startup validates that
    /// at least one runner is configured before we get here. Used in
    /// single-runner code paths that need to read the active runner's
    /// fields without picking up the multi-runner indexing churn yet.
    pub fn primary_runner(&self) -> &RunnerConfig {
        self.runners
            .first()
            .expect("config.runners must contain at least one entry")
    }

    /// Mutable counterpart to [`primary_runner`]. Same panic contract.
    pub fn primary_runner_mut(&mut self) -> &mut RunnerConfig {
        self.runners
            .first_mut()
            .expect("config.runners must contain at least one entry")
    }
}
