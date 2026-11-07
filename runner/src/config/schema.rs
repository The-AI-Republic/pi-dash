use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

pub use pidash_ipc::protocol::EngineSignInState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub daemon: DaemonConfig,
    /// Per-instance runner configs. Length 1 in the current single-runner
    /// daemon; will grow once the cap is lifted (design.md §16).
    #[serde(default, rename = "runner")]
    pub runners: Vec<RunnerConfig>,
    /// CLI-side configuration consumed by the `pidash issue/comment/
    /// state/workspace` subcommands when invoked by the agent (or by the
    /// operator out-of-band). Optional so older configs still parse;
    /// missing means "fall back to environment variables".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<CliSection>,
    /// `[engine]`: the built-in agent engine. Empty (and not written) unless
    /// a developer overrides the engine binary.
    #[serde(default, skip_serializing_if = "EngineSection::is_default")]
    pub engine: EngineSection,
    /// `[auth]`: where the daemon and engine credentials are stored
    /// (`config::secrets`). Not written while it holds only defaults.
    #[serde(default, skip_serializing_if = "AuthSection::is_default")]
    pub auth: AuthSection,
}

/// Top-level `[engine]` table (`src/engine/locate.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineSection {
    /// Development override for the built-in engine executable, e.g. a local
    /// Codex build. When set it must exist; the installed
    /// `engine/<version>/` layouts next to `pidash` are then not searched.
    /// A leading `~/` is expanded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<PathBuf>,
    /// Parent of the per-project folders new engine runners work in
    /// (design §10). Unset means the default,
    /// `<daemon data>/engine/workdirs`. Changing it moves nothing: runners
    /// already set up keep their folder. Validated by
    /// `engine::workdir::validate_task_root`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_root: Option<PathBuf>,
}

/// `[auth] credentials_store`: where the daemon credentials (the `mt_`
/// tokens) and the engine credential live (design §4.3). Per-runner
/// refresh tokens always stay in their 0600 files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialsStoreMode {
    /// The OS keychain when it answers, else `config.toml` (0600).
    /// Plaintext tokens move into the keychain at the next sign-in.
    #[default]
    Auto,
    /// The OS keychain only; refuse to fall back to a file.
    Keyring,
    /// `config.toml` (0600), as builds before the keychain did.
    File,
}

impl CredentialsStoreMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Keyring => "keyring",
            Self::File => "file",
        }
    }
}

/// Top-level `[auth]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSection {
    #[serde(default)]
    pub credentials_store: CredentialsStoreMode,
    /// Written by the daemon, not the user: `"keyring"` when the tokens
    /// of `[cli]` are held in the OS keychain (the file then carries their
    /// workspaces and expiry dates but no secret).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets_in: Option<String>,
    /// Written by the daemon: the keychain items it holds, so items of a
    /// signed-out workspace can be deleted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyring_items: Vec<String>,
    /// In memory only: the keychain items that [`crate::config::secrets::hydrate`]
    /// could not read when this config was loaded. While it is not empty
    /// the config is missing secrets that are still in the keychain, so
    /// writing it is refused (it would drop or delete them).
    #[serde(skip)]
    pub unread_keyring_items: Vec<String>,
    /// In memory only: the part of `unread_keyring_items` the keychain can
    /// never return as it is (duplicate items, a value that is not text).
    /// Writes stay refused for them too, except from `pidash auth login`
    /// and `pidash auth logout`, which forget and replace them
    /// ([`crate::config::secrets::forget_unreadable`]).
    #[serde(skip)]
    pub broken_keyring_items: Vec<String>,
}

impl AuthSection {
    pub fn is_default(&self) -> bool {
        self.credentials_store == CredentialsStoreMode::default()
            && self.secrets_in.is_none()
            && self.keyring_items.is_empty()
    }

    /// Whether the `[cli]` secrets are in the keychain rather than this file.
    pub fn secrets_in_keyring(&self) -> bool {
        self.secrets_in.as_deref() == Some(SECRETS_IN_KEYRING)
    }
}

/// Value of `[auth] secrets_in` when the secrets are in the keychain.
pub const SECRETS_IN_KEYRING: &str = "keyring";

/// The engine credential (`ec_…`, design §4.3): one per machine, 14 days
/// from issue. Its only use is `POST /api/v1/runner/engine/model-token/`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineCredential {
    /// The raw `ec_` value. Empty in the file when the secret is in the
    /// keychain.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl EngineSection {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// Top-level `[cli]` table. Holds the daemon credentials (one dev-machine
/// `mt_` token per workspace) and the CLI's defaults. The CLI also reads
/// `cloud_url` from `[daemon]`.
///
/// # Workspaces and their tokens (PIDESKAPP-122, design §4.9)
///
/// One daemon serves every workspace the user signs in to on this
/// computer. The credentials are stored as:
///
/// ```toml
/// [cli]
/// token = "mt_…"            # the default workspace's token
/// workspace_slug = "acme"   # the default workspace
///
/// [[cli.workspaces]]        # every further workspace, one table each
/// slug = "beta"
/// token = "mt_…"
/// ```
///
/// `token` + `workspace_slug` is exactly the single-workspace format older
/// builds write, so an existing config loads unchanged and behaves as one
/// workspace. `pidash auth login --workspace <slug>` appends a
/// `[[cli.workspaces]]` entry instead of replacing the default; a login
/// without `--workspace` refreshes the default. The CRUD subcommands
/// (`pidash issue …`) keep presenting the default token.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CliSection {
    /// Auth token of the default workspace, presented to the cloud by the
    /// CRUD subcommands. Optional so a partial config (URL + workspace, no
    /// token) still parses; the CLI errors with a clear "no token" message
    /// when it's missing at command time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// The default workspace: the one `token` belongs to. `pidash runner
    /// add` reads this as the default for `--workspace` when the caller
    /// doesn't pass one explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_slug: Option<String>,

    /// Local fallback project for non-interactive CLI and agent use.
    /// This can be a project UUID or workspace-scoped identifier. Workspace
    /// context files still take precedence for agent workflows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_project: Option<String>,

    /// Email of the account the daemon is signed in as, recorded by
    /// `pidash auth login`. Informational (reported by the IPC `Hello`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_email: Option<String>,

    /// Cloud user id of the signed-in account, when login could read it.
    /// Informational (reported by the IPC `Hello`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_user_id: Option<String>,

    /// Daemon credentials of every workspace other than the default one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<WorkspaceCredential>,

    /// The machine's engine credential (design §4.3), issued by `pidash
    /// auth login` or `pidash login --engine`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_credential: Option<EngineCredential>,

    /// Local mirror of the server's engine sign-in state for this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_sign_in_state: Option<EngineSignInState>,
}

/// One `[[cli.workspaces]]` entry: a workspace and its dev-machine token.
/// `token` is empty in the file when the secret is in the keychain.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceCredential {
    pub slug: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
}

impl CliSection {
    /// Token for `workspace`, the slug of a runner block or of a
    /// machine-control command.
    ///
    /// - `None` (a runner block written before blocks carried a slug)
    ///   answers the default token, as older builds did.
    /// - A slug answers the default token when it is the default
    ///   workspace, else its `[[cli.workspaces]]` entry.
    /// - A config that has a token but never recorded which workspace it
    ///   belongs to (very old installs) answers that token for every
    ///   slug, which is how those installs behaved.
    pub fn token_for_workspace(&self, workspace: Option<&str>) -> Option<&str> {
        let default = self.token.as_deref().filter(|t| !t.is_empty());
        let Some(slug) = workspace.filter(|s| !s.is_empty()) else {
            return default;
        };
        if self.workspace_slug.as_deref() == Some(slug) {
            return default;
        }
        if let Some(entry) = self
            .workspaces
            .iter()
            .find(|w| w.slug == slug && !w.token.is_empty())
        {
            return Some(entry.token.as_str());
        }
        if self.workspace_slug.is_none() {
            return default;
        }
        None
    }

    /// Every `(workspace, token)` pair held, default first. The default
    /// pair's workspace is `None` when the config never recorded it.
    pub fn workspace_tokens(&self) -> Vec<(Option<String>, String)> {
        let mut out: Vec<(Option<String>, String)> = Vec::new();
        if let Some(token) = self.token.as_deref().filter(|t| !t.is_empty()) {
            out.push((self.workspace_slug.clone(), token.to_string()));
        }
        for entry in &self.workspaces {
            if entry.token.is_empty() || entry.slug.is_empty() {
                continue;
            }
            if out
                .iter()
                .any(|(slug, _)| slug.as_deref() == Some(entry.slug.as_str()))
            {
                continue;
            }
            out.push((Some(entry.slug.clone()), entry.token.clone()));
        }
        out
    }

    /// Slugs of every workspace this install holds a token for.
    pub fn workspace_slugs(&self) -> Vec<String> {
        self.workspace_tokens()
            .into_iter()
            .filter_map(|(slug, _)| slug)
            .collect()
    }

    /// Store `token` for `slug`.
    ///
    /// `as_default = true` (a login without `--workspace`) makes `slug`
    /// the default workspace, replacing the default pair as older builds
    /// did. `as_default = false` (`pidash auth login --workspace <slug>`)
    /// adds the workspace: it replaces the default pair only when there is
    /// none yet or `slug` already is the default, and otherwise upserts a
    /// `[[cli.workspaces]]` entry, leaving the default untouched. Either
    /// way no workspace is listed twice.
    pub fn set_workspace_token(&mut self, slug: &str, token: &str, as_default: bool) {
        let no_default = self.token.as_deref().is_none_or(str::is_empty)
            || self.workspace_slug.as_deref().is_none_or(str::is_empty);
        if as_default || no_default || self.workspace_slug.as_deref() == Some(slug) {
            self.token = Some(token.to_string());
            self.workspace_slug = Some(slug.to_string());
            self.workspaces.retain(|w| w.slug != slug);
            return;
        }
        match self.workspaces.iter_mut().find(|w| w.slug == slug) {
            Some(entry) => entry.token = token.to_string(),
            None => self.workspaces.push(WorkspaceCredential {
                slug: slug.to_string(),
                token: token.to_string(),
            }),
        }
    }

    /// Forget `slug`'s token. Removing the default workspace promotes the
    /// first `[[cli.workspaces]]` entry, so the CRUD subcommands keep a
    /// token while any workspace is signed in. Returns whether anything
    /// was removed.
    pub fn remove_workspace_token(&mut self, slug: &str) -> bool {
        let before = self.workspaces.len();
        self.workspaces.retain(|w| w.slug != slug);
        let mut removed = self.workspaces.len() != before;
        if self.workspace_slug.as_deref() == Some(slug) && self.token.is_some() {
            self.token = None;
            removed = true;
            if !self.workspaces.is_empty() {
                let next = self.workspaces.remove(0);
                self.token = Some(next.token);
                self.workspace_slug = Some(next.slug);
            }
        }
        removed
    }

    /// Forget every workspace token (`pidash auth logout`). The default
    /// workspace binding is kept as the hint for the next login.
    pub fn clear_all_tokens(&mut self) {
        self.token = None;
        self.workspaces.clear();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub cloud_url: String,
    /// Stable local identity for this dev machine. Minted once by the CLI and
    /// sent to the cloud on `pidash runner add`; host_label stays display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_machine_id: Option<Uuid>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_retention_days")]
    pub log_retention_days: u32,
    /// Opt into the per-active-run observability snapshot on the poll
    /// status. Default off; new fields ride only when this is true.
    /// See `.ai_design/runner_agent_bridge/design.md` §4.2.
    #[serde(default)]
    pub agent_observability_v1: bool,
    /// Passive-kill activity watchdog for agent runs (`[daemon.agent_watchdog]`).
    /// Missing section falls back to the 24h-unhealthy / 48h-kill defaults.
    #[serde(default)]
    pub agent_watchdog: AgentWatchdogConfig,
    /// Auto-update policy. When `true` (default), the daemon swaps the
    /// on-disk `pidash` binary in place whenever the cloud announces a
    /// newer `latest_runner_version` in the welcome frame. The running
    /// daemon is never disturbed — the new code only takes effect on
    /// the next natural restart. When `false`, the daemon surfaces an
    /// advisory in `pidash status` / TUI and waits for the operator to
    /// run `pidash update --restart` (or `pidash update` to defer restart).
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
}

fn default_auto_update() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_retention_days() -> u32 {
    14
}

/// Thresholds for the supervisor's passive-kill activity watchdog.
///
/// A run is never killed for being quiet for a while: a live agent can sit
/// silent for hours inside a long tool call (an integration suite, a
/// migration), and the runner cannot tell that apart from a frozen agent.
/// Provable deaths (the agent process exiting, a terminal agent error, the
/// turn ending) are handled immediately elsewhere. These thresholds are only
/// the backstop for a run that never proves itself dead:
///
/// - after `unhealthy_after_hours` with no agent event the run is reported
///   unhealthy to the cloud, but keeps running;
/// - after `kill_after_hours` with no agent event the run is failed.
///
/// Any agent event resets both clocks and clears the unhealthy mark.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWatchdogConfig {
    #[serde(default = "default_unhealthy_after_hours")]
    pub unhealthy_after_hours: u64,
    #[serde(default = "default_kill_after_hours")]
    pub kill_after_hours: u64,
}

/// Upper bound on either watchdog threshold (one year). Far beyond any real
/// run, and keeps `Instant + kill_after` well clear of overflow.
pub const MAX_AGENT_WATCHDOG_HOURS: u64 = 24 * 365;

fn default_unhealthy_after_hours() -> u64 {
    24
}

fn default_kill_after_hours() -> u64 {
    48
}

impl Default for AgentWatchdogConfig {
    fn default() -> Self {
        Self {
            unhealthy_after_hours: default_unhealthy_after_hours(),
            kill_after_hours: default_kill_after_hours(),
        }
    }
}

impl AgentWatchdogConfig {
    pub fn unhealthy_after(self) -> Duration {
        Duration::from_secs(self.unhealthy_after_hours.saturating_mul(3600))
    }

    pub fn kill_after(self) -> Duration {
        Duration::from_secs(self.kill_after_hours.saturating_mul(3600))
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            cloud_url: String::new(),
            dev_machine_id: None,
            log_level: default_log_level(),
            log_retention_days: default_retention_days(),
            agent_observability_v1: false,
            agent_watchdog: AgentWatchdogConfig::default(),
            auto_update: default_auto_update(),
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
    /// Cursor Agent settings. Missing section falls back to
    /// `CursorAgentSection::default()` so existing `config.toml` files (written
    /// before Cursor support) still parse. Only consulted when
    /// `agent.kind == cursor_agent`.
    #[serde(default)]
    pub cursor_agent: CursorAgentSection,
    /// OpenClaw settings. Missing section falls back to
    /// `OpenClawSection::default()` so existing `config.toml` files (written
    /// before OpenClaw support) still parse. Only consulted when
    /// `agent.kind == open_claw`.
    #[serde(default)]
    pub openclaw: OpenClawSection,
    /// Grok settings. Missing section falls back to `GrokSection::default()`
    /// so existing `config.toml` files (written before Grok support) still
    /// parse. Only consulted when `agent.kind == grok`.
    #[serde(default)]
    pub grok: GrokSection,
    /// Muse Code settings. Missing section falls back to
    /// `MuseCodeSection::default()` so existing `config.toml` files (written
    /// before Muse Code support) still parse. Only consulted when
    /// `agent.kind == muse_code`.
    #[serde(default)]
    pub muse_code: MuseCodeSection,
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
    /// Managed-runner only: private `CODEX_HOME` this runner's agent uses.
    ///
    /// When set, the daemon exports it for the agent process so the bundled
    /// engine reads Pi Dash's configuration instead of `~/.codex`, and the
    /// user's own Codex install (if any) is neither read nor written. `None`
    /// for user-enrolled runners, which keeps their behaviour byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_home: Option<PathBuf>,
    /// Managed-runner only: directory prepended to the agent's `PATH`.
    ///
    /// Carries the bundled `pidash` CLI so the agent's `pidash issue|comment|
    /// workpad` calls resolve to the app's copy rather than whatever is on the
    /// user's `PATH` — or nothing at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prepend: Option<PathBuf>,
    /// Managed-runner only: file holding the short-lived model credential.
    ///
    /// Read once per agent spawn rather than cached, so a token the desktop
    /// rotated between runs takes effect without restarting the daemon. Never
    /// stored in this config, and never passed on a command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_token_file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_default: Option<String>,
    /// Reasoning-effort tier passed to codex `turn/start` (`low` / `medium`
    /// / `high` / `xhigh`). `None` omits the field so codex applies the
    /// model's own default effort. Only meaningful alongside `model_default`
    /// — `pidash runner add --reasoning-effort` writes this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_default: Option<String>,
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
            codex_home: None,
            path_prepend: None,
            model_token_file: None,
            model_default: None,
            effort_default: None,
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
    /// Cursor Agent via `cursor-agent --print --output-format stream-json`.
    CursorAgent,
    /// OpenClaw via the headless ACP client `acpx`
    /// (`acpx --format json openclaw exec`), which drives OpenClaw over the
    /// Agent Client Protocol.
    OpenClaw,
    /// xAI Grok via its native ACP server (`grok agent stdio`), which the
    /// runner drives directly over the Agent Client Protocol.
    Grok,
    /// Meta Muse Code via its headless one-shot mode
    /// (`muse exec --json --prompt-file <PATH>`), whose JSONL event stream the
    /// runner drives one turn per subprocess.
    MuseCode,
    /// The built-in engine (`pidash-agent-engine`, the open-source Codex
    /// shipped with the CLI; design §5). Driven by the Codex driver; the
    /// daemon supplies the binary, a private `CODEX_HOME` and the model
    /// token itself, so the block holds no paths.
    Engine,
}

impl AgentKind {
    /// Human-facing name for prompts and operator-facing messages, e.g.
    /// the missing-agent reminder `pidash runner add` prints.
    pub fn display_name(self) -> &'static str {
        match self {
            AgentKind::Codex => "Codex",
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::CursorAgent => "Cursor",
            AgentKind::OpenClaw => "OpenClaw",
            AgentKind::Grok => "Grok",
            AgentKind::MuseCode => "Muse Code",
            AgentKind::Engine => "Built-in engine",
        }
    }

    /// Default CLI binary name for this agent — the executable name a
    /// fresh runner is configured with (see `CodexSection::default` /
    /// `ClaudeCodeSection::default`) and the one `pidash runner add`
    /// probes for presence on the dev machine.
    pub fn default_binary(self) -> &'static str {
        match self {
            AgentKind::Codex => "codex",
            AgentKind::ClaudeCode => "claude",
            AgentKind::CursorAgent => "cursor-agent",
            // OpenClaw is driven through the `acpx` ACP client, so that's the
            // binary the runner invokes and `pidash runner add` probes for.
            AgentKind::OpenClaw => "acpx",
            // Grok is its own ACP server (`grok agent stdio`), so the runner
            // invokes `grok` directly.
            AgentKind::Grok => "grok",
            // Muse Code ships as the `muse` CLI; the runner drives `muse exec`.
            AgentKind::MuseCode => "muse",
            AgentKind::Engine => crate::engine::ENGINE_BINARY_NAME,
        }
    }

    /// Official install / setup page for this agent's CLI. `pidash runner
    /// add` prints this and opens it in the operator's browser when the
    /// agent binary is missing, so they can install it before runs start.
    pub fn install_page_url(self) -> &'static str {
        match self {
            AgentKind::Codex => "https://github.com/openai/codex",
            AgentKind::ClaudeCode => "https://docs.claude.com/en/docs/claude-code/setup",
            AgentKind::CursorAgent => "https://cursor.com/download",
            AgentKind::OpenClaw => "https://github.com/openclaw/acpx",
            AgentKind::Grok => "https://x.ai/cli",
            AgentKind::MuseCode => "https://developer.meta.com/ai/products/muse-code/",
            AgentKind::Engine => "https://github.com/The-AI-Republic/pi-dash",
        }
    }

    /// The agent's *official* install command(s), or `None` when the vendor
    /// ships no scriptable installer we've verified (in which case
    /// `pidash runner add` falls back to opening [`install_page_url`]).
    ///
    /// This is the single data table the issue (PDASHOSS01-155) calls for:
    /// each agent that has an installer contributes exactly one [`AgentInstaller`]
    /// row here, so adding a new agent kind means adding a row — not editing
    /// several match arms across the codebase. **Key principle:** we never
    /// vendor, mirror, pin, or ship an agent binary; we only invoke the vendor's
    /// own script at runtime, so pidash always tracks the vendor's latest
    /// supported build. Only official vendor domains are allowed here.
    ///
    /// Every command below was verified against the vendor's current docs
    /// (linked per row). Re-verify before changing any command.
    pub fn official_installer(self) -> Option<AgentInstaller> {
        match self {
            // Anthropic Claude Code — https://code.claude.com/docs/en/setup
            AgentKind::ClaudeCode => Some(AgentInstaller {
                unix: Some("curl -fsSL https://claude.ai/install.sh | bash"),
                windows: Some("irm https://claude.ai/install.ps1 | iex"),
                doc_url: "https://code.claude.com/docs/en/setup",
            }),
            // OpenAI Codex — https://learn.chatgpt.com/docs/codex/cli
            // The standalone installer is macOS/Linux only; on Windows the
            // official method is the npm package (requires Node.js).
            AgentKind::Codex => Some(AgentInstaller {
                unix: Some("curl -fsSL https://chatgpt.com/codex/install.sh | sh"),
                windows: Some("npm install -g @openai/codex"),
                doc_url: "https://learn.chatgpt.com/docs/codex/cli",
            }),
            // Cursor CLI — https://cursor.com/docs/cli/installation
            AgentKind::CursorAgent => Some(AgentInstaller {
                unix: Some("curl https://cursor.com/install -fsS | bash"),
                windows: Some("irm 'https://cursor.com/install?win32=true' | iex"),
                doc_url: "https://cursor.com/docs/cli/installation",
            }),
            // OpenClaw (acpx), Grok, and Muse Code do not (yet) publish a
            // verified official one-line installer, so we keep the
            // open-install-page fallback for these agents rather than guess a
            // command. Add a row here once a vendor script is confirmed.
            AgentKind::OpenClaw | AgentKind::Grok | AgentKind::MuseCode => None,
            // Ships with pidash (`pidash engine install`), never fetched
            // from a vendor.
            AgentKind::Engine => None,
        }
    }

    /// The one login step still required after a successful install, phrased as
    /// a command the operator can run. Installers place the binary but never
    /// authenticate it, so `pidash runner add` prints this so the runner can
    /// actually pick up work. `None` means auth is via an env var / no explicit
    /// login command, in which case the doctor's per-agent auth hint covers it.
    pub fn post_install_login_hint(self) -> Option<&'static str> {
        match self {
            AgentKind::ClaudeCode => Some("claude  (then follow the /login prompts)"),
            AgentKind::Codex => Some("codex login"),
            AgentKind::CursorAgent => Some("cursor-agent login  (or set CURSOR_API_KEY)"),
            AgentKind::OpenClaw => None,
            AgentKind::Grok => Some("set XAI_API_KEY  (or run `grok` and log in)"),
            AgentKind::MuseCode => Some("set META_API_KEY"),
            AgentKind::Engine => Some("pidash login --engine"),
        }
    }
}

/// One agent's official installer command(s), returned by
/// [`AgentKind::official_installer`]. The command strings are shown to the
/// operator verbatim *and* executed, so they must be exactly the vendor's
/// documented one-liner. `unix` covers macOS/Linux/WSL; `windows` is run in
/// PowerShell. A `None` for a platform means "no verified installer on this
/// platform" and the caller falls back to opening the install page there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentInstaller {
    /// Command run in a login shell on macOS/Linux/WSL, or `None` if the vendor
    /// has no scriptable macOS/Linux installer.
    pub unix: Option<&'static str>,
    /// Command run in PowerShell on Windows, or `None` if the vendor has no
    /// scriptable Windows installer.
    pub windows: Option<&'static str>,
    /// Vendor documentation URL the command was verified against.
    pub doc_url: &'static str,
}

impl AgentInstaller {
    /// The install command for the host platform this binary is running on:
    /// [`windows`](Self::windows) on Windows, [`unix`](Self::unix) elsewhere.
    pub fn command_for_host(&self) -> Option<&'static str> {
        if cfg!(target_os = "windows") {
            self.windows
        } else {
            self.unix
        }
    }
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
pub struct CursorAgentSection {
    /// Per-field default so a partial `[cursor_agent]` block (e.g. only
    /// `model_default = "..."`) still parses. Without this, declaring the
    /// section at all would require spelling out `binary = "cursor-agent"`.
    #[serde(default = "default_cursor_binary")]
    pub binary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_default: Option<String>,
}

fn default_cursor_binary() -> String {
    "cursor-agent".to_string()
}

impl Default for CursorAgentSection {
    fn default() -> Self {
        Self {
            binary: default_cursor_binary(),
            model_default: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawSection {
    /// The `acpx` ACP client binary the runner shells out to. Per-field
    /// default so a partial `[openclaw]` block (e.g. only `model_default`)
    /// still parses without spelling out `binary = "acpx"`.
    #[serde(default = "default_openclaw_binary")]
    pub binary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_default: Option<String>,
}

fn default_openclaw_binary() -> String {
    "acpx".to_string()
}

impl Default for OpenClawSection {
    fn default() -> Self {
        Self {
            binary: default_openclaw_binary(),
            model_default: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrokSection {
    /// The `grok` CLI binary the runner spawns as an ACP server (`grok agent
    /// stdio`). Per-field default so a partial `[grok]` block (e.g. only
    /// `model_default`) still parses without spelling out `binary = "grok"`.
    #[serde(default = "default_grok_binary")]
    pub binary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_default: Option<String>,
}

fn default_grok_binary() -> String {
    "grok".to_string()
}

impl Default for GrokSection {
    fn default() -> Self {
        Self {
            binary: default_grok_binary(),
            model_default: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuseCodeSection {
    /// The `muse` CLI binary the runner drives as `muse exec --json`. Per-field
    /// default so a partial `[muse_code]` block (e.g. only `model_default`)
    /// still parses without spelling out `binary = "muse"`.
    #[serde(default = "default_muse_binary")]
    pub binary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_default: Option<String>,
}

fn default_muse_binary() -> String {
    "muse".to_string()
}

impl Default for MuseCodeSection {
    fn default() -> Self {
        Self {
            binary: default_muse_binary(),
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

/// Normalize a path for *physical-identity* comparison. When the path exists we
/// `canonicalize` it, resolving symlinks and (on case-insensitive filesystems)
/// casing so two spellings of one directory — a symlinked `~/repo`, or `C:\repo`
/// vs `c:\repo` — compare equal. When it does not yet exist (a canonical clone
/// the daemon creates lazily, or a unit-test fixture path) we fall back to
/// lexical absolutization, preserving the old behavior. Every "same or nested
/// working dir" collision check routes through this so config validation and
/// `runner add`'s auto-pool dedup agree on what "the same repo" means.
pub(crate) fn canonical_for_compare(path: &Path) -> PathBuf {
    std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

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
        let watchdog = self.daemon.agent_watchdog;
        if watchdog.unhealthy_after_hours == 0
            || watchdog.kill_after_hours < watchdog.unhealthy_after_hours
            || watchdog.kill_after_hours > MAX_AGENT_WATCHDOG_HOURS
        {
            return Err(ConfigError::InvalidAgentWatchdog {
                unhealthy_after_hours: watchdog.unhealthy_after_hours,
                kill_after_hours: watchdog.kill_after_hours,
            });
        }

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

        // Managed Codex fields are all-or-nothing (see IncompleteManagedCodex).
        for r in &self.runners {
            let present = [
                ("codex_home", r.codex.codex_home.is_some()),
                ("path_prepend", r.codex.path_prepend.is_some()),
                ("model_token_file", r.codex.model_token_file.is_some()),
            ];
            let set_count = present.iter().filter(|(_, has)| *has).count();
            if set_count != 0 && set_count != present.len() {
                let missing: Vec<&str> = present
                    .iter()
                    .filter(|(_, has)| !*has)
                    .map(|(name, _)| *name)
                    .collect();
                return Err(ConfigError::IncompleteManagedCodex {
                    runner: r.name.clone(),
                    missing: missing.join(", "),
                });
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

        // Workspace collisions: exact-match and nested-path. Each runner owns
        // exactly one working directory, exclusive to it within the workspace;
        // two runners sharing (or nesting) a working directory would trample
        // each other's git state, so refuse to start at config load rather than
        // diagnose the corrupted runs after the fact. This applies to every
        // runner — isolation is by construction, not by policy.
        for (i, a) in self.runners.iter().enumerate() {
            for b in self.runners.iter().skip(i + 1) {
                let ap = &a.workspace.working_dir;
                let bp = &b.workspace.working_dir;
                let (apc, bpc) = (canonical_for_compare(ap), canonical_for_compare(bp));
                if apc == bpc {
                    return Err(ConfigError::DuplicateWorkingDir {
                        runner_a: a.name.clone(),
                        runner_b: b.name.clone(),
                        path: ap.display().to_string(),
                    });
                }
                if apc.starts_with(&bpc) || bpc.starts_with(&apc) {
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
    /// A managed runner declared some but not all of the managed Codex fields.
    ///
    /// They are a set: without `codex_home` the engine would read the user's
    /// personal config, without `path_prepend` the agent could not find the
    /// bundled CLI, and without `model_token_file` it has no credential. A
    /// partial set is always a bug in whatever wrote the file, so refuse to
    /// start rather than run in a half-configured state.
    IncompleteManagedCodex {
        runner: String,
        missing: String,
    },
    /// `[daemon.agent_watchdog]` thresholds that cannot both hold: a zero
    /// unhealthy threshold, or a kill threshold below the unhealthy one.
    InvalidAgentWatchdog {
        unhealthy_after_hours: u64,
        kill_after_hours: u64,
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
            ConfigError::IncompleteManagedCodex { runner, missing } => write!(
                f,
                "configuration error: runner {runner:?} sets some managed Codex                  fields but is missing {missing}. These are written together by                  the Pi Dash desktop app; a partial set means the config was                  hand-edited or a write was interrupted. Re-open the project in                  Pi Dash Desktop to rewrite it."
            ),
            ConfigError::InvalidAgentWatchdog {
                unhealthy_after_hours,
                kill_after_hours,
            } => write!(
                f,
                "configuration error: [daemon.agent_watchdog] has \
                 unhealthy_after_hours = {unhealthy_after_hours} and \
                 kill_after_hours = {kill_after_hours}. unhealthy_after_hours \
                 must be at least 1, kill_after_hours must not be lower \
                 than unhealthy_after_hours, and neither may exceed \
                 {MAX_AGENT_WATCHDOG_HOURS} (one year)."
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
            cursor_agent: Default::default(),
            openclaw: Default::default(),
            grok: Default::default(),
            muse_code: Default::default(),
            approval_policy: Default::default(),
        }
    }

    fn config_with(runners: Vec<RunnerConfig>) -> Config {
        Config {
            engine: Default::default(),
            auth: Default::default(),
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                dev_machine_id: None,
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
                agent_watchdog: Default::default(),
                auto_update: true,
            },
            runners,
            cli: None,
        }
    }

    #[test]
    fn engine_section_is_omitted_by_default_and_round_trips_an_override() {
        let cfg = config_with(vec![]);
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(!text.contains("[engine]"), "{text}");
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.engine, EngineSection::default());

        let mut cfg = config_with(vec![]);
        cfg.engine.binary = Some(PathBuf::from("/opt/codex/bin/codex"));
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(text.contains("[engine]"), "{text}");
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(
            back.engine.binary,
            Some(PathBuf::from("/opt/codex/bin/codex"))
        );
    }

    #[test]
    fn agent_watchdog_defaults_to_24h_unhealthy_and_48h_kill() {
        let watchdog = AgentWatchdogConfig::default();
        assert_eq!(watchdog.unhealthy_after(), Duration::from_secs(24 * 3600));
        assert_eq!(watchdog.kill_after(), Duration::from_secs(48 * 3600));
    }

    #[test]
    fn agent_watchdog_section_parses_and_fills_missing_fields() {
        let daemon: DaemonConfig = toml::from_str(
            r#"
                cloud_url = "https://x"
                [agent_watchdog]
                kill_after_hours = 72
            "#,
        )
        .unwrap();
        assert_eq!(daemon.agent_watchdog.unhealthy_after_hours, 24);
        assert_eq!(daemon.agent_watchdog.kill_after_hours, 72);

        let daemon: DaemonConfig = toml::from_str(r#"cloud_url = "https://x""#).unwrap();
        assert_eq!(daemon.agent_watchdog, AgentWatchdogConfig::default());
    }

    #[test]
    fn validate_rejects_inconsistent_agent_watchdog() {
        for (unhealthy, kill) in [
            (0, 48),
            (24, 12),
            (24, MAX_AGENT_WATCHDOG_HOURS + 1),
            (u64::MAX, u64::MAX),
        ] {
            let mut cfg = config_with(vec![]);
            cfg.daemon.agent_watchdog = AgentWatchdogConfig {
                unhealthy_after_hours: unhealthy,
                kill_after_hours: kill,
            };
            assert!(matches!(
                cfg.validate(),
                Err(ConfigError::InvalidAgentWatchdog { .. })
            ));
        }

        let mut cfg = config_with(vec![]);
        cfg.daemon.agent_watchdog = AgentWatchdogConfig {
            unhealthy_after_hours: 24,
            kill_after_hours: 24,
        };
        cfg.validate().unwrap();
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

    #[cfg(unix)]
    #[test]
    fn canonical_for_compare_resolves_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("repo");
        std::fs::create_dir(&real).unwrap();
        let link = tmp.path().join("repo-link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        // A directory and a symlink pointing at it normalize to one path.
        assert_eq!(canonical_for_compare(&real), canonical_for_compare(&link));
        // A path that doesn't exist yet still normalizes deterministically
        // (lexical fallback) so it stays comparable with itself.
        let ghost = tmp.path().join("not-created-yet");
        assert_eq!(canonical_for_compare(&ghost), canonical_for_compare(&ghost));
    }

    #[cfg(unix)]
    #[test]
    fn validate_rejects_legacy_runners_sharing_a_dir_via_symlink() {
        // Two spellings of one physical directory — a real path and a symlink
        // to it — must collide, or two legacy runners would silently trample
        // each other's git state. Lexical comparison missed this; the
        // canonicalize-based check catches it.
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("repo");
        std::fs::create_dir(&real).unwrap();
        let link = tmp.path().join("repo-link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let cfg = config_with(vec![
            runner("a", real.to_str().unwrap()),
            runner("b", link.to_str().unwrap()),
        ]);
        let err = cfg.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::DuplicateWorkingDir { .. }),
            "expected symlinked working dirs to collide, got {err:?}"
        );
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

    #[test]
    fn agent_kind_default_binary_matches_section_defaults() {
        // `pidash runner add` probes `default_binary()` for the agent it's
        // about to configure; it must equal the binary the fresh runner is
        // actually written with, or the probe checks the wrong executable.
        assert_eq!(
            AgentKind::Codex.default_binary(),
            CodexSection::default().binary
        );
        assert_eq!(
            AgentKind::ClaudeCode.default_binary(),
            ClaudeCodeSection::default().binary
        );
        assert_eq!(
            AgentKind::CursorAgent.default_binary(),
            CursorAgentSection::default().binary
        );
        assert_eq!(
            AgentKind::OpenClaw.default_binary(),
            OpenClawSection::default().binary
        );
        assert_eq!(
            AgentKind::Grok.default_binary(),
            GrokSection::default().binary
        );
        assert_eq!(
            AgentKind::MuseCode.default_binary(),
            MuseCodeSection::default().binary
        );
    }

    #[test]
    fn agent_kind_install_page_urls_are_https() {
        // These are opened in the operator's browser and printed verbatim;
        // a typo'd or non-https URL is a user-facing defect.
        for kind in [
            AgentKind::Codex,
            AgentKind::ClaudeCode,
            AgentKind::CursorAgent,
            AgentKind::OpenClaw,
            AgentKind::Grok,
            AgentKind::MuseCode,
        ] {
            let url = kind.install_page_url();
            assert!(url.starts_with("https://"), "{kind:?} url not https: {url}");
            assert!(!kind.display_name().is_empty());
        }
    }

    #[test]
    fn official_installers_only_use_https_and_official_domains() {
        // The install commands are printed verbatim and executed, so a typo'd
        // scheme or a non-vendor host is a serious defect. Assert every
        // command begins with the vendor's https one-liner and cites an https
        // doc URL, and that no command references a non-official domain.
        let ok_hosts = [
            "claude.ai",
            "code.claude.com",
            "chatgpt.com",
            "learn.chatgpt.com",
            "cursor.com",
            "@openai/codex", // npm package name, not a URL host
        ];
        for kind in [
            AgentKind::Codex,
            AgentKind::ClaudeCode,
            AgentKind::CursorAgent,
            AgentKind::OpenClaw,
            AgentKind::Grok,
            AgentKind::MuseCode,
        ] {
            let Some(installer) = kind.official_installer() else {
                continue;
            };
            assert!(
                installer.doc_url.starts_with("https://"),
                "{kind:?} doc_url not https: {}",
                installer.doc_url
            );
            for cmd in [installer.unix, installer.windows].into_iter().flatten() {
                // Any http:// (non-TLS) reference is forbidden.
                assert!(
                    !cmd.contains("http://"),
                    "{kind:?} command uses plaintext http: {cmd}"
                );
                // Any URL in the command must point at an official host.
                for tok in cmd.split_whitespace() {
                    if tok.contains("https://") {
                        assert!(
                            ok_hosts.iter().any(|h| tok.contains(h)),
                            "{kind:?} command references non-official host: {tok}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn agents_with_verified_installers_have_them_others_fall_back() {
        // The three agents whose vendor scripts we verified must expose an
        // installer; the rest deliberately return None so `runner add` keeps
        // the open-install-page fallback.
        assert!(AgentKind::ClaudeCode.official_installer().is_some());
        assert!(AgentKind::Codex.official_installer().is_some());
        assert!(AgentKind::CursorAgent.official_installer().is_some());
        assert!(AgentKind::OpenClaw.official_installer().is_none());
        assert!(AgentKind::Grok.official_installer().is_none());
        assert!(AgentKind::MuseCode.official_installer().is_none());
    }

    #[test]
    fn command_for_host_picks_the_running_platform() {
        // Platform selection must match the host: Windows → PowerShell command,
        // everything else → the unix curl|sh command.
        let claude = AgentKind::ClaudeCode.official_installer().unwrap();
        let picked = claude.command_for_host().unwrap();
        if cfg!(target_os = "windows") {
            assert_eq!(picked, claude.windows.unwrap());
            assert!(picked.contains("install.ps1"), "win cmd should be ps1");
        } else {
            assert_eq!(picked, claude.unix.unwrap());
            assert!(picked.contains("install.sh"), "unix cmd should be sh");
        }
    }

    #[test]
    fn installer_agents_carry_a_login_hint() {
        // Every agent with an installer must tell the operator how to log in,
        // since installers never authenticate the agent.
        for kind in [
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::CursorAgent,
        ] {
            assert!(
                kind.post_install_login_hint().is_some(),
                "{kind:?} should carry a post-install login hint"
            );
        }
    }

    // ---- One exclusive work dir per runner (PDASHOSS01-134) ----

    #[test]
    fn every_runner_collides_on_a_shared_working_dir() {
        // Isolation is by construction now: any two runners that resolve to the
        // same working directory are rejected at config load. There is no
        // longer a pooled-runner exemption to loosen this.
        let cfg = config_with(vec![
            runner("codex", "/work/shared"),
            runner("fable", "/work/shared"),
        ]);
        assert!(matches!(
            cfg.validate().unwrap_err(),
            ConfigError::DuplicateWorkingDir { .. }
        ));
    }

    #[test]
    fn config_toml_has_no_workdir_table() {
        // The `[[workdir]]` pool concept is gone; a config must not serialize
        // a workdirs vec because it no longer exists on `Config`.
        let cfg = config_with(vec![runner("main", "/work/main")]);
        let out = toml::to_string(&cfg).expect("serialize");
        assert!(!out.contains("[[workdir]]"), "out: {out}");
    }

    // -----------------------------------------------------------------
    // Managed Codex fields (.ai_design/managed_runner/design.md §9.3, §12.3)
    // -----------------------------------------------------------------

    fn managed_runner(name: &str) -> RunnerConfig {
        let mut r = runner(name, "/work/managed");
        r.codex.binary = "/app/bin/pidash-agent-engine".into();
        r.codex.codex_home = Some(PathBuf::from("/app/managed/codex-home"));
        r.codex.path_prepend = Some(PathBuf::from("/app/bin"));
        r.codex.model_token_file = Some(PathBuf::from("/app/managed/runtime/model.token"));
        r
    }

    #[test]
    fn managed_codex_fields_default_to_none() {
        // A user-enrolled runner must be untouched by this feature: absent
        // fields mean the agent inherits the operator's environment exactly
        // as it always has.
        let section = CodexSection::default();
        assert!(section.codex_home.is_none());
        assert!(section.path_prepend.is_none());
        assert!(section.model_token_file.is_none());
        assert!(config_with(vec![runner("a", "/work/a")]).validate().is_ok());
    }

    #[test]
    fn validate_accepts_complete_managed_codex_set() {
        assert!(
            config_with(vec![managed_runner("desktop-host")])
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn validate_rejects_partial_managed_codex_set() {
        // Half a set is always a bug in whatever wrote the file. Starting
        // anyway would run the bundled engine against the *user's* Codex
        // config, or with no credential at all — both worse than refusing.
        for drop_field in ["codex_home", "path_prepend", "model_token_file"] {
            let mut r = managed_runner("desktop-host");
            match drop_field {
                "codex_home" => r.codex.codex_home = None,
                "path_prepend" => r.codex.path_prepend = None,
                _ => r.codex.model_token_file = None,
            }
            let err = config_with(vec![r]).validate().unwrap_err();
            match &err {
                ConfigError::IncompleteManagedCodex { runner, missing } => {
                    assert_eq!(runner, "desktop-host");
                    assert!(
                        missing.contains(drop_field),
                        "error should name the missing field, got {missing:?}"
                    );
                }
                other => panic!("expected IncompleteManagedCodex for {drop_field}, got {other:?}"),
            }
            // And the message must tell the operator what to do about it.
            assert!(
                err.to_string().contains("Pi Dash Desktop"),
                "message should point at the app that owns this config: {err}"
            );
        }
    }

    #[test]
    fn managed_and_manual_runners_coexist() {
        // A user may run the desktop app and a hand-installed runner on the
        // same machine; neither validation nor the other's config is affected.
        let cfg = config_with(vec![
            managed_runner("desktop-host"),
            runner("manual", "/work/manual"),
        ]);
        assert!(cfg.validate().is_ok(), "{:?}", cfg.validate());
    }

    #[test]
    fn managed_codex_fields_round_trip_through_toml() {
        // The desktop writes this file and the daemon reads it back; a serde
        // rename or skip_serializing_if mistake would silently drop the
        // managed paths and send the engine at ~/.codex.
        let cfg = config_with(vec![managed_runner("desktop-host")]);
        let text = toml::to_string(&cfg).expect("serialise");
        let back: Config = toml::from_str(&text).expect("deserialise");
        let codex = &back.runners[0].codex;
        assert_eq!(
            codex.codex_home.as_deref(),
            Some(Path::new("/app/managed/codex-home"))
        );
        assert_eq!(codex.path_prepend.as_deref(), Some(Path::new("/app/bin")));
        assert_eq!(
            codex.model_token_file.as_deref(),
            Some(Path::new("/app/managed/runtime/model.token"))
        );
        assert_eq!(codex.binary, "/app/bin/pidash-agent-engine");
    }

    #[test]
    fn manual_runner_toml_omits_managed_fields() {
        // skip_serializing_if keeps existing config.toml files byte-stable,
        // so upgrading the runner does not rewrite every operator's config.
        let cfg = config_with(vec![runner("manual", "/work/manual")]);
        let text = toml::to_string(&cfg).expect("serialise");
        assert!(!text.contains("codex_home"), "unexpected key in:\n{text}");
        assert!(!text.contains("path_prepend"), "unexpected key in:\n{text}");
        assert!(
            !text.contains("model_token_file"),
            "unexpected key in:\n{text}"
        );
    }

    // ---- Workspaces and their tokens (PIDESKAPP-122) -------------------

    const LEGACY_SINGLE_TOKEN_CONFIG: &str = r#"
version = 2

[daemon]
cloud_url = "https://cloud.example"
dev_machine_id = "1e6a7a56-0000-0000-0000-000000000001"

[[runner]]
name = "main"
runner_id = "1e6a7a56-0000-0000-0000-0000000000aa"
workspace_slug = "acme"
project_slug = "WEB"

[runner.workspace]
working_dir = "/tmp/pidash-legacy-main"

[cli]
token = "mt_legacy"
workspace_slug = "acme"
default_project = "WEB"
"#;

    #[test]
    fn legacy_single_token_config_loads_unchanged_as_one_workspace() {
        let cfg: Config = toml::from_str(LEGACY_SINGLE_TOKEN_CONFIG).unwrap();
        cfg.validate().unwrap();
        let cli = cfg.cli.as_ref().unwrap();
        assert!(cli.workspaces.is_empty());
        assert_eq!(cli.workspace_slugs(), vec!["acme"]);
        assert_eq!(cli.token_for_workspace(Some("acme")), Some("mt_legacy"));
        assert_eq!(
            cli.token_for_workspace(cfg.runners[0].workspace_slug.as_deref()),
            Some("mt_legacy"),
            "the runner of the one workspace gets the one token"
        );
        assert_eq!(cli.token_for_workspace(None), Some("mt_legacy"));
        assert_eq!(cli.token_for_workspace(Some("other")), None);

        // Writing it back adds no new keys, so older builds still read it.
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(!text.contains("workspaces"), "unexpected key in:\n{text}");
        assert!(!text.contains("account_"), "unexpected key in:\n{text}");
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.cli, cfg.cli);
    }

    #[test]
    fn token_without_a_recorded_workspace_serves_every_workspace() {
        let cli = CliSection {
            token: Some("mt_unbound".into()),
            ..Default::default()
        };
        assert_eq!(cli.token_for_workspace(Some("acme")), Some("mt_unbound"));
        assert_eq!(cli.token_for_workspace(None), Some("mt_unbound"));
        assert_eq!(
            cli.workspace_tokens(),
            vec![(None, "mt_unbound".to_string())]
        );
    }

    #[test]
    fn per_workspace_credentials_round_trip_through_toml() {
        let text = format!(
            "{LEGACY_SINGLE_TOKEN_CONFIG}\n[[cli.workspaces]]\nslug = \"beta\"\ntoken = \"mt_beta\"\n"
        );
        let cfg: Config = toml::from_str(&text).unwrap();
        let cli = cfg.cli.as_ref().unwrap();
        assert_eq!(cli.token_for_workspace(Some("acme")), Some("mt_legacy"));
        assert_eq!(cli.token_for_workspace(Some("beta")), Some("mt_beta"));
        assert_eq!(cli.workspace_slugs(), vec!["acme", "beta"]);
        let back: Config = toml::from_str(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
        assert_eq!(back.cli, cfg.cli);
    }

    #[test]
    fn login_with_workspace_adds_instead_of_replacing() {
        let mut cli = CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            ..Default::default()
        };
        cli.set_workspace_token("beta", "mt_beta", false);
        assert_eq!(cli.token.as_deref(), Some("mt_acme"), "default kept");
        assert_eq!(cli.workspace_slug.as_deref(), Some("acme"));
        assert_eq!(cli.token_for_workspace(Some("beta")), Some("mt_beta"));

        // Re-login of an added workspace updates its entry in place.
        cli.set_workspace_token("beta", "mt_beta_2", false);
        assert_eq!(cli.workspaces.len(), 1);
        assert_eq!(cli.token_for_workspace(Some("beta")), Some("mt_beta_2"));

        // `--workspace <default>` refreshes the default pair.
        cli.set_workspace_token("acme", "mt_acme_2", false);
        assert_eq!(cli.token.as_deref(), Some("mt_acme_2"));
        assert_eq!(cli.workspaces.len(), 1);

        // A login without --workspace rebinds the default, never listing
        // a workspace twice.
        cli.set_workspace_token("beta", "mt_beta_3", true);
        assert_eq!(cli.workspace_slug.as_deref(), Some("beta"));
        assert_eq!(cli.token.as_deref(), Some("mt_beta_3"));
        assert!(cli.workspaces.is_empty());
    }

    #[test]
    fn first_login_with_workspace_becomes_the_default() {
        let mut cli = CliSection::default();
        cli.set_workspace_token("acme", "mt_acme", false);
        assert_eq!(cli.workspace_slug.as_deref(), Some("acme"));
        assert_eq!(cli.token.as_deref(), Some("mt_acme"));
        assert!(cli.workspaces.is_empty());
    }

    #[test]
    fn removing_the_default_workspace_promotes_the_next() {
        let mut cli = CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            workspaces: vec![WorkspaceCredential {
                slug: "beta".into(),
                token: "mt_beta".into(),
            }],
            ..Default::default()
        };
        assert!(cli.remove_workspace_token("acme"));
        assert_eq!(cli.workspace_slug.as_deref(), Some("beta"));
        assert_eq!(cli.token.as_deref(), Some("mt_beta"));
        assert!(cli.workspaces.is_empty());
        assert!(!cli.remove_workspace_token("gamma"));
        cli.clear_all_tokens();
        assert!(cli.workspace_tokens().is_empty());
    }
}
