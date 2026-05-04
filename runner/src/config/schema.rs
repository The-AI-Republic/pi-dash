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
    /// Opt into the per-active-run observability snapshot on the poll
    /// status. Default off; new fields ride only when this is true.
    /// See `.ai_design/runner_agent_bridge/design.md` §4.2.
    #[serde(default)]
    pub agent_observability_v1: bool,
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
            agent_observability_v1: false,
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
    /// Identifier of the project this runner serves (e.g. `WEB`,
    /// `firstdream-API`). Populated from the registration response.
    /// One runner ↔ one project; cannot be changed without re-registering.
    /// See `.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md`
    /// §7.3. `Option` for back-compat with configs written before this
    /// field existed; `Config::validate()` rejects empty values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_slug: Option<String>,
    /// Pod id assigned by the cloud at registration. Informational only —
    /// the source of truth is the cloud-side runner row. Stamped here so
    /// `pidash status` can show it without an extra REST call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_id: Option<Uuid>,
    pub workspace: WorkspaceSection,
    /// Which agent CLI the daemon drives for assigned runs. Defaults to
    /// `codex` so existing deployments are unaffected.
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
        // Leave model unset: when `model_default` is None we omit the `model`
        // field from `thread/start` and `turn/start`, so codex picks its own
        // default. Hardcoding `gpt-5-codex` here forced every fresh runner
        // onto a model that's unavailable on ChatGPT-account auth — runs
        // would 400 from the OpenAI side before doing any work.
        Self {
            binary: "codex".to_string(),
            model_default: None,
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

/// Long-lived credentials a daemon needs to talk to the cloud.
///
/// One Connection per dev machine. Set on first ``pidash connect`` and
/// reused across runner adds/removes. Mode 0600 on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// Cloud Connection id. Sent on every WS upgrade in
    /// ``X-Connection-Id``.
    pub connection_id: Uuid,
    /// Long-lived bearer secret presented in
    /// ``Authorization: Bearer …`` on every WS connect. Never echoed.
    pub connection_secret: String,
    /// Editable label echoed by the cloud at enrollment time. Mirrors
    /// ``Connection.name``; surfaced in ``pidash status`` and the TUI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_name: Option<String>,
    /// Public REST API token (``X-Api-Key``) for ``/api/v1/`` CRUD
    /// commands. Optional so a daemon can be enrolled with WS-only access
    /// today and pick up an api_token via ``pidash login`` later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
    pub issued_at: DateTime<Utc>,
}

/// Hard cap on the number of runner instances per daemon. See `design.md`
/// §16. Both daemon-side validation and cloud-side enforcement use this
/// number; the cloud rejects `Hello` beyond it as well.
pub const MAX_RUNNERS_PER_DAEMON: usize = 50;

impl Config {
    /// First configured runner, or `None` for a freshly enrolled connection
    /// that has no runners yet. Callers that need a panic-on-absent
    /// invariant should validate first via [`Config::validate`] which
    /// surfaces a user-facing error.
    pub fn primary_runner(&self) -> Option<&RunnerConfig> {
        self.runners.first()
    }

    pub fn primary_runner_mut(&mut self) -> Option<&mut RunnerConfig> {
        self.runners.first_mut()
    }

    /// Validate the loaded config before the daemon starts. Returns a
    /// `ConfigError` with a user-facing message on the first violation;
    /// the daemon refuses to start with this message rather than booting
    /// into a state that will silently corrupt data later.
    ///
    /// See `design.md` §9 for the rule set.
    ///
    /// A zero-runner config is valid: a freshly enrolled connection may
    /// hold the WS open before any runners have been added. The daemon
    /// stays connected and the user runs ``pidash runner add`` later.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.runners.len() > MAX_RUNNERS_PER_DAEMON {
            return Err(ConfigError::TooManyRunners {
                count: self.runners.len(),
                cap: MAX_RUNNERS_PER_DAEMON,
            });
        }

        // Every runner must declare its project. The wire path
        // (register/, register-under-token/) requires a project at
        // registration, so an in-memory RunnerConfig that lacks one
        // means the file was hand-edited or written by a pre-refactor
        // CLI. Refuse to start so dispatch can't silently route into
        // the wrong project. See
        // .ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md
        // §7.3.
        for r in &self.runners {
            match r.project_slug.as_deref() {
                Some(s) if !s.trim().is_empty() => {}
                _ => {
                    return Err(ConfigError::MissingProjectSlug {
                        runner: r.name.clone(),
                    });
                }
            }
        }

        // Duplicate name / runner_id detection. O(n²) is fine at n≤50.
        for (i, a) in self.runners.iter().enumerate() {
            for b in self.runners.iter().skip(i + 1) {
                if a.name == b.name {
                    return Err(ConfigError::DuplicateName {
                        name: a.name.clone(),
                    });
                }
                if a.runner_id == b.runner_id {
                    return Err(ConfigError::DuplicateRunnerId { id: a.runner_id });
                }
            }
        }

        // Workspace collisions: exact-match and nested-path. Two runners
        // sharing a working directory will trample each other's git state;
        // refusing to start at config load is dramatically cheaper than
        // diagnosing the corrupted runs after the fact.
        for (i, a) in self.runners.iter().enumerate() {
            for b in self.runners.iter().skip(i + 1) {
                let ap = &a.workspace.working_dir;
                let bp = &b.workspace.working_dir;
                if ap == bp {
                    return Err(ConfigError::DuplicateWorkingDir {
                        runner_a: a.name.clone(),
                        runner_b: b.name.clone(),
                        path: ap.display().to_string(),
                    });
                }
                if ap.starts_with(bp) || bp.starts_with(ap) {
                    return Err(ConfigError::NestedWorkingDir {
                        runner_a: a.name.clone(),
                        path_a: ap.display().to_string(),
                        runner_b: b.name.clone(),
                        path_b: bp.display().to_string(),
                    });
                }
            }
        }

        Ok(())
    }
}

/// User-facing config validation errors. Each variant's `Display` is the
/// message printed on stderr when the daemon refuses to start.
#[derive(Debug)]
pub enum ConfigError {
    NoRunners,
    TooManyRunners {
        count: usize,
        cap: usize,
    },
    DuplicateName {
        name: String,
    },
    DuplicateRunnerId {
        id: Uuid,
    },
    MissingProjectSlug {
        runner: String,
    },
    DuplicateWorkingDir {
        runner_a: String,
        runner_b: String,
        path: String,
    },
    NestedWorkingDir {
        runner_a: String,
        path_a: String,
        runner_b: String,
        path_b: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NoRunners => write!(
                f,
                "configuration error: no [[runner]] block in config.toml. \
                 Run `pidash runner add` to register a runner under this connection."
            ),
            ConfigError::TooManyRunners { count, cap } => write!(
                f,
                "configuration error: {count} runners configured, but the daemon \
                 supports at most {cap}. Remove [[runner]] blocks from config.toml \
                 (see design.md §16)."
            ),
            ConfigError::DuplicateName { name } => write!(
                f,
                "configuration error: two [[runner]] blocks share the name {name:?}. \
                 Each runner must have a unique name."
            ),
            ConfigError::DuplicateRunnerId { id } => write!(
                f,
                "configuration error: two [[runner]] blocks share runner_id {id}. \
                 Each runner must have a unique runner_id; this usually means \
                 config.toml was edited incorrectly — re-run `pidash runner add`."
            ),
            ConfigError::MissingProjectSlug { runner } => write!(
                f,
                "configuration error: runner {runner:?} has no project_slug. \
                 Every runner must declare its project (via `pidash runner add \
                 --project <SLUG>`); dispatch is project-scoped and a runner \
                 with no project would be unreachable."
            ),
            ConfigError::DuplicateWorkingDir {
                runner_a,
                runner_b,
                path,
            } => write!(
                f,
                "configuration error: runners {runner_a:?} and {runner_b:?} share \
                 working_dir {path:?}. Each runner must have its own working \
                 directory; concurrent git operations on the same tree corrupt \
                 state silently. Update one of them in config.toml."
            ),
            ConfigError::NestedWorkingDir {
                runner_a,
                path_a,
                runner_b,
                path_b,
            } => write!(
                f,
                "configuration error: runners {runner_a:?} ({path_a:?}) and \
                 {runner_b:?} ({path_b:?}) have nested working directories. One \
                 path is a prefix of the other; their git trees will collide. \
                 Use disjoint working directories."
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn runner(name: &str, working_dir: &str) -> RunnerConfig {
        RunnerConfig {
            name: name.into(),
            runner_id: Uuid::new_v4(),
            workspace_slug: None,
            project_slug: Some("TEST".into()),
            pod_id: None,
            workspace: WorkspaceSection {
                working_dir: PathBuf::from(working_dir),
            },
            agent: Default::default(),
            codex: Default::default(),
            claude_code: Default::default(),
            approval_policy: Default::default(),
        }
    }

    fn config_with(runners: Vec<RunnerConfig>) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
            },
            runners,
        }
    }

    #[test]
    fn validate_accepts_single_runner() {
        let cfg = config_with(vec![runner("main", "/work/main")]);
        cfg.validate().unwrap();
    }

    #[test]
    fn validate_accepts_zero_runners() {
        // A freshly enrolled connection has no runners yet — the daemon
        // still needs to come up so the user can `pidash runner add`.
        let cfg = config_with(vec![]);
        cfg.validate().unwrap();
    }

    #[test]
    fn validate_rejects_too_many_runners() {
        let runners: Vec<_> = (0..MAX_RUNNERS_PER_DAEMON + 1)
            .map(|i| runner(&format!("r{i}"), &format!("/work/r{i}")))
            .collect();
        let cfg = config_with(runners);
        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, ConfigError::TooManyRunners { .. }));
    }

    #[test]
    fn validate_rejects_duplicate_name() {
        let cfg = config_with(vec![runner("main", "/work/a"), runner("main", "/work/b")]);
        let err = cfg.validate().unwrap_err();
        match err {
            ConfigError::DuplicateName { name } => assert_eq!(name, "main"),
            other => panic!("expected DuplicateName, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_duplicate_runner_id() {
        let mut a = runner("a", "/work/a");
        let mut b = runner("b", "/work/b");
        let id = Uuid::new_v4();
        a.runner_id = id;
        b.runner_id = id;
        let cfg = config_with(vec![a, b]);
        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, ConfigError::DuplicateRunnerId { .. }));
    }

    #[test]
    fn validate_rejects_duplicate_working_dir() {
        let cfg = config_with(vec![
            runner("a", "/work/shared"),
            runner("b", "/work/shared"),
        ]);
        let err = cfg.validate().unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, ConfigError::DuplicateWorkingDir { .. }));
        // Error message must name both runners and the shared path so the
        // operator can find and fix the collision.
        assert!(msg.contains("\"a\""), "message: {msg}");
        assert!(msg.contains("\"b\""), "message: {msg}");
        assert!(msg.contains("/work/shared"), "message: {msg}");
    }

    #[test]
    fn validate_rejects_nested_working_dirs() {
        let cfg = config_with(vec![runner("outer", "/work"), runner("inner", "/work/sub")]);
        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, ConfigError::NestedWorkingDir { .. }));
    }

    #[test]
    fn validate_accepts_disjoint_sibling_working_dirs() {
        // Two siblings under the same parent that don't nest are fine.
        let cfg = config_with(vec![runner("a", "/work/main"), runner("b", "/work/side")]);
        cfg.validate().unwrap();
    }

    #[test]
    fn validate_rejects_missing_project_slug() {
        let mut r = runner("a", "/work/a");
        r.project_slug = None;
        let cfg = config_with(vec![r]);
        let err = cfg.validate().unwrap_err();
        match err {
            ConfigError::MissingProjectSlug { runner } => assert_eq!(runner, "a"),
            other => panic!("expected MissingProjectSlug, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_empty_project_slug() {
        let mut r = runner("a", "/work/a");
        r.project_slug = Some("   ".into());
        let cfg = config_with(vec![r]);
        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, ConfigError::MissingProjectSlug { .. }));
    }
}
