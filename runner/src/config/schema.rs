use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub daemon: DaemonConfig,
    /// Per-instance runner configs. Length 1 in the current single-runner
    /// daemon; will grow once the cap is lifted (design.md §16).
    #[serde(default, rename = "runner")]
    pub runners: Vec<RunnerConfig>,
    /// Work directories shared by runners. Each entry owns one canonical git
    /// clone on this machine plus a pool of git worktrees (the "desks" runs
    /// execute in). N runners may reference the same work dir by name; the
    /// pool bounds how many of them execute concurrently. See
    /// `.ai_design/worktree_pooling/design.md`. Empty by default so configs
    /// written before worktree pooling still parse — a runner with no
    /// `workdir` reference falls back to its legacy `workspace.working_dir`.
    #[serde(default, rename = "workdir")]
    pub workdirs: Vec<WorkdirConfig>,
    /// CLI-side configuration consumed by the `pidash issue/comment/
    /// state/workspace` subcommands when invoked by the agent (or by the
    /// operator out-of-band). Optional so older configs still parse;
    /// missing means "fall back to environment variables".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<CliSection>,
}

/// Top-level `[cli]` table. Holds the auth token the `pidash` CRUD
/// subcommands present to the cloud. The CLI also reads `cloud_url`
/// from `[daemon]` and `workspace_slug` from the primary `[[runner]]`
/// — there's deliberately no duplication of those values here.
///
/// For v1 the token is populated manually by the operator (or by a
/// future `pidash login` flow). A per-run, per-issue scoped token
/// minted by the cloud is the next step; see
/// `.ai_design/agent_cli_auth/` (TBD).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CliSection {
    /// Auth token presented to the cloud as `Authorization: Bearer …`
    /// by the CRUD subcommands. Optional so a partial config (URL +
    /// workspace, no token) still parses; the CLI errors with a clear
    /// "no token" message when it's missing at command time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Workspace this host is bound to. The Pi Dash CLI on a dev host
    /// is single-workspace-per-install in v1: `pidash auth login`
    /// resolves it (auto-picks if the user belongs to one workspace,
    /// prompts if they belong to several) and persists it here.
    /// `pidash runner add` reads this as the default for `--workspace`
    /// when the caller doesn't pass one explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_slug: Option<String>,

    /// Local fallback project for non-interactive CLI and agent use.
    /// This can be a project UUID or workspace-scoped identifier. Workspace
    /// context files still take precedence for agent workflows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_project: Option<String>,
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

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            cloud_url: String::new(),
            dev_machine_id: None,
            log_level: default_log_level(),
            log_retention_days: default_retention_days(),
            agent_observability_v1: false,
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
    /// Name of the `[[workdir]]` this runner executes in. When set, the
    /// daemon leases a git worktree from that work dir's pool for each run
    /// instead of running the agent directly in `workspace.working_dir`;
    /// this is what lets N runners (e.g. a codex runner and a claude_code
    /// runner) share one repo checkout. `None` preserves the legacy
    /// single-dir behavior (`workspace.working_dir` used directly). See
    /// `.ai_design/worktree_pooling/design.md` §7.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
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
    /// Missing section falls back to `ApprovalPolicySection::default()` so
    /// a minimal `config.toml` doesn't have to spell out every knob.
    #[serde(default)]
    pub approval_policy: ApprovalPolicySection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSection {
    pub working_dir: PathBuf,
}

/// Default pool size for a freshly-added work dir. Two desks lets a single
/// repo serve two concurrent runs (e.g. two agents) out of the box without
/// the operator having to think about capacity. See design §4.2.
pub const DEFAULT_POOL_SIZE: usize = 2;

/// Soft ceiling on `pool_size`. Values above this are accepted but warned
/// about — a desk cap, not an abuse cap (the `MAX_RUNNERS_PER_DAEMON` cap
/// still governs how many runners exist). See design §7.
pub const POOL_SIZE_WARN_ABOVE: usize = 16;

/// One shared work directory: a single canonical git clone on this machine
/// plus a pool of git worktrees that runs execute in. See
/// `.ai_design/worktree_pooling/design.md` §3–§4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkdirConfig {
    /// Stable name runners reference via `runner.workdir`. Unique per config.
    pub name: String,
    /// The canonical clone. Holds the shared object database; agents never
    /// execute here (worktrees are leased from the pool instead). Typically
    /// the directory the operator already had a runner pointed at.
    pub path: PathBuf,
    /// Maximum concurrent leases (worktrees). Defaults to `DEFAULT_POOL_SIZE`.
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,
    /// How a worktree is cleaned when returned to the pool. See `CleanMode`.
    #[serde(default)]
    pub clean_mode: CleanMode,
    /// Globs preserved across cleans in `allowlist` mode (e.g.
    /// `["node_modules/**", ".env"]`). Ignored for other modes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keep_paths: Vec<String>,
    /// Optional command run once when a worktree is first created (and again
    /// after every `full` clean) — e.g. `pnpm install`. Provisions
    /// gitignored setup like `.env` and dependency installs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_command: Option<String>,
    /// Where worktrees are materialized. `None` => `data_dir/worktrees/<name>`.
    /// Override to keep worktrees on the same filesystem as a huge repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktrees_dir: Option<PathBuf>,
}

fn default_pool_size() -> usize {
    DEFAULT_POOL_SIZE
}

impl WorkdirConfig {
    /// `true` when `pool_size` exceeds the soft ceiling (operator-facing warn,
    /// not a hard error).
    pub fn pool_size_is_large(&self) -> bool {
        self.pool_size > POOL_SIZE_WARN_ABOVE
    }
}

/// How a leased worktree is scrubbed when a run finishes and the desk returns
/// to the pool. See design §4.4.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum CleanMode {
    /// `git reset --hard` + `git clean -fd`: drop tracked changes and stray
    /// untracked files, but KEEP gitignored files (`node_modules`, caches,
    /// `.env`). Warm pools — cheap reuse even on huge repos. Default.
    #[default]
    KeepIgnored,
    /// Like `full`, but preserve `keep_paths` globs. Warm where it matters,
    /// pristine everywhere else.
    Allowlist,
    /// `git reset --hard` + `git clean -fdx`: pristine but cold. The next
    /// lease re-runs `setup_command`.
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSection {
    pub binary: String,
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
}

impl AgentKind {
    /// How long the supervisor's stall watchdog waits for a fresh
    /// bridge event before declaring the agent stuck. Tuned per agent
    /// because the protocols have very different "natural quiet"
    /// envelopes:
    ///
    /// - **Codex** emits `codex/event/token_count` continuously while
    ///   the model thinks, so prolonged silence is genuinely abnormal.
    ///   5 minutes catches real stalls without false-positive on a
    ///   long but live token stream.
    /// - **Claude Code** can be silent for the full duration of a
    ///   single tool call (e.g. a long `Bash` running tests or a
    ///   build), and the stream-json transport doesn't expose
    ///   intra-tool progress. 15 minutes accommodates real-world
    ///   tool calls without hanging cancellation forever on a truly
    ///   stuck process.
    pub fn stall_timeout(self) -> Duration {
        match self {
            AgentKind::Codex => Duration::from_secs(5 * 60),
            // Cursor Agent, like Claude Code, can be silent for the full
            // duration of a single tool call (a long build or test run) with no
            // intra-tool progress on the stream-json transport. Use the same
            // 15-minute envelope so a live-but-quiet tool call isn't killed.
            AgentKind::ClaudeCode | AgentKind::CursorAgent => Duration::from_secs(15 * 60),
        }
    }

    /// Human-facing name for prompts and operator-facing messages, e.g.
    /// the missing-agent reminder `pidash runner add` prints.
    pub fn display_name(self) -> &'static str {
        match self {
            AgentKind::Codex => "Codex",
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::CursorAgent => "Cursor",
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

        // Validate work-dir entities and every runner's reference to one.
        self.validate_workdirs()?;

        // Workspace collisions: exact-match and nested-path. Two LEGACY
        // runners (no `workdir` reference) sharing a working directory will
        // trample each other's git state; refusing to start at config load is
        // dramatically cheaper than diagnosing the corrupted runs after the
        // fact. Pooled runners (those that reference a `[[workdir]]`) are
        // EXEMPT — sharing a checkout is the whole point of pooling, and the
        // worktree pool isolates them. Their canonical-clone collisions are
        // checked at the work-dir level by `validate_workdirs`.
        for (i, a) in self.runners.iter().enumerate() {
            if a.workdir.is_some() {
                continue;
            }
            for b in self.runners.iter().skip(i + 1) {
                if b.workdir.is_some() {
                    continue;
                }
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

    /// Validate `[[workdir]]` entities and the runner references to them
    /// (design §7). Hard errors only — the daemon refuses to start.
    fn validate_workdirs(&self) -> Result<(), ConfigError> {
        // Unique names; pool_size floor.
        for (i, a) in self.workdirs.iter().enumerate() {
            if a.pool_size < 1 {
                return Err(ConfigError::PoolSizeTooSmall {
                    workdir: a.name.clone(),
                });
            }
            for b in self.workdirs.iter().skip(i + 1) {
                if a.name == b.name {
                    return Err(ConfigError::DuplicateWorkdirName {
                        name: a.name.clone(),
                    });
                }
            }
        }

        // Equal / nested path collisions across (and within) work dirs. Each
        // work dir "claims" its canonical clone path plus any explicit
        // worktrees_dir; any overlap between claimed trees — including a
        // worktrees_dir nested inside a canonical clone (design §4.1) — would
        // corrupt git state exactly as two legacy runners sharing a dir would.
        // (Default worktrees_dirs live under `data_dir/worktrees/<name>` and
        // cannot collide: names are unique.)
        let collide = |x: &PathBuf, y: &PathBuf| x == y || x.starts_with(y) || y.starts_with(x);
        let claimed = |w: &WorkdirConfig| -> Vec<PathBuf> {
            let mut v = vec![w.path.clone()];
            if let Some(wt) = &w.worktrees_dir {
                v.push(wt.clone());
            }
            v
        };
        for (i, a) in self.workdirs.iter().enumerate() {
            // Own worktrees_dir inside (or containing) the own canonical clone.
            if let Some(wa) = &a.worktrees_dir
                && collide(&a.path, wa)
            {
                return Err(ConfigError::WorkdirPathCollision {
                    workdir_a: a.name.clone(),
                    path_a: a.path.display().to_string(),
                    workdir_b: a.name.clone(),
                    path_b: wa.display().to_string(),
                });
            }
            for b in self.workdirs.iter().skip(i + 1) {
                for pa in claimed(a) {
                    for pb in claimed(b) {
                        if collide(&pa, &pb) {
                            return Err(ConfigError::WorkdirPathCollision {
                                workdir_a: a.name.clone(),
                                path_a: pa.display().to_string(),
                                workdir_b: b.name.clone(),
                                path_b: pb.display().to_string(),
                            });
                        }
                    }
                }
            }
        }

        // Legacy runners (no `workdir` reference) must not run inside any work
        // dir's claimed tree: an agent executing directly in a canonical clone
        // holds its branch lock permanently and tramples pool state.
        for r in &self.runners {
            if r.workdir.is_some() {
                continue;
            }
            let rp = &r.workspace.working_dir;
            for w in &self.workdirs {
                for wp in claimed(w) {
                    if collide(rp, &wp) {
                        return Err(ConfigError::RunnerWorkdirCollision {
                            runner: r.name.clone(),
                            path: rp.display().to_string(),
                            workdir: w.name.clone(),
                            workdir_path: wp.display().to_string(),
                        });
                    }
                }
            }
        }

        // Every runner.workdir reference must name an existing work dir.
        for r in &self.runners {
            if let Some(name) = r.workdir.as_deref()
                && !self.workdirs.iter().any(|w| w.name == name)
            {
                return Err(ConfigError::UnknownWorkdir {
                    runner: r.name.clone(),
                    workdir: name.to_string(),
                });
            }
        }

        Ok(())
    }

    /// The work dir a runner executes in, by reference. `None` for legacy
    /// runners (no pool — they run directly in `workspace.working_dir`).
    pub fn workdir_for(&self, runner: &RunnerConfig) -> Option<&WorkdirConfig> {
        let name = runner.workdir.as_deref()?;
        self.workdirs.iter().find(|w| w.name == name)
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
    DuplicateWorkdirName {
        name: String,
    },
    PoolSizeTooSmall {
        workdir: String,
    },
    WorkdirPathCollision {
        workdir_a: String,
        path_a: String,
        workdir_b: String,
        path_b: String,
    },
    UnknownWorkdir {
        runner: String,
        workdir: String,
    },
    RunnerWorkdirCollision {
        runner: String,
        path: String,
        workdir: String,
        workdir_path: String,
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
            ConfigError::DuplicateWorkdirName { name } => write!(
                f,
                "configuration error: two [[workdir]] blocks share the name {name:?}. \
                 Each work dir must have a unique name."
            ),
            ConfigError::PoolSizeTooSmall { workdir } => write!(
                f,
                "configuration error: work dir {workdir:?} has pool_size < 1. \
                 A work dir needs at least one worktree to run anything."
            ),
            ConfigError::WorkdirPathCollision {
                workdir_a,
                path_a,
                workdir_b,
                path_b,
            } => write!(
                f,
                "configuration error: work dirs {workdir_a:?} ({path_a:?}) and \
                 {workdir_b:?} ({path_b:?}) have equal or nested paths. Their git \
                 trees would collide; use disjoint paths."
            ),
            ConfigError::UnknownWorkdir { runner, workdir } => write!(
                f,
                "configuration error: runner {runner:?} references workdir \
                 {workdir:?}, but no [[workdir]] block with that name exists. \
                 Add the work dir or fix the reference (see \
                 `pidash workdir add`)."
            ),
            ConfigError::RunnerWorkdirCollision {
                runner,
                path,
                workdir,
                workdir_path,
            } => write!(
                f,
                "configuration error: runner {runner:?} works directly in \
                 {path:?}, which overlaps work dir {workdir:?} ({workdir_path:?}). \
                 An agent executing inside a pooled work dir's tree would trample \
                 its git state; bind the runner to the work dir instead \
                 (`pidash runner add --workdir {workdir}`) or move one of the paths."
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
            workdir: None,
            agent: Default::default(),
            codex: Default::default(),
            claude_code: Default::default(),
            cursor_agent: Default::default(),
            approval_policy: Default::default(),
        }
    }

    fn config_with(runners: Vec<RunnerConfig>) -> Config {
        config_with_workdirs(runners, vec![])
    }

    fn config_with_workdirs(runners: Vec<RunnerConfig>, workdirs: Vec<WorkdirConfig>) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://x".into(),
                dev_machine_id: None,
                log_level: "info".into(),
                log_retention_days: 14,
                agent_observability_v1: false,
                auto_update: true,
            },
            runners,
            workdirs,
            cli: None,
        }
    }

    fn workdir(name: &str, path: &str) -> WorkdirConfig {
        WorkdirConfig {
            name: name.into(),
            path: PathBuf::from(path),
            pool_size: DEFAULT_POOL_SIZE,
            clean_mode: CleanMode::default(),
            keep_paths: vec![],
            setup_command: None,
            worktrees_dir: None,
        }
    }

    /// A runner bound to a named work dir (pooled). Its `workspace.working_dir`
    /// is a vestige; the pool path is what actually gets used.
    fn pooled_runner(name: &str, workdir_name: &str) -> RunnerConfig {
        let mut r = runner(name, "/vestige");
        r.workdir = Some(workdir_name.into());
        r
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
    }

    #[test]
    fn agent_kind_install_page_urls_are_https() {
        // These are opened in the operator's browser and printed verbatim;
        // a typo'd or non-https URL is a user-facing defect.
        for kind in [AgentKind::Codex, AgentKind::ClaudeCode] {
            let url = kind.install_page_url();
            assert!(url.starts_with("https://"), "{kind:?} url not https: {url}");
            assert!(!kind.display_name().is_empty());
        }
    }

    // ---- Worktree pooling: work-dir config validation (design §7) ----

    #[test]
    fn validate_accepts_two_pooled_runners_sharing_one_workdir() {
        // The motivating case: a codex runner and a claude_code runner on one
        // repo. Both reference the same work dir; sharing is allowed.
        let cfg = config_with_workdirs(
            vec![
                pooled_runner("codex", "main"),
                pooled_runner("fable", "main"),
            ],
            vec![workdir("main", "/work/main")],
        );
        cfg.validate().unwrap();
    }

    #[test]
    fn validate_rejects_runner_referencing_unknown_workdir() {
        let cfg = config_with_workdirs(
            vec![pooled_runner("codex", "ghost")],
            vec![workdir("main", "/work/main")],
        );
        let err = cfg.validate().unwrap_err();
        match err {
            ConfigError::UnknownWorkdir { runner, workdir } => {
                assert_eq!(runner, "codex");
                assert_eq!(workdir, "ghost");
            }
            other => panic!("expected UnknownWorkdir, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_duplicate_workdir_name() {
        let cfg = config_with_workdirs(
            vec![],
            vec![workdir("main", "/work/a"), workdir("main", "/work/b")],
        );
        assert!(matches!(
            cfg.validate().unwrap_err(),
            ConfigError::DuplicateWorkdirName { .. }
        ));
    }

    #[test]
    fn validate_rejects_nested_workdir_paths() {
        let cfg = config_with_workdirs(
            vec![],
            vec![workdir("outer", "/work"), workdir("inner", "/work/sub")],
        );
        let err = cfg.validate().unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, ConfigError::WorkdirPathCollision { .. }));
        assert!(msg.contains("outer"), "message: {msg}");
        assert!(msg.contains("inner"), "message: {msg}");
    }

    #[test]
    fn validate_rejects_pool_size_zero() {
        let mut w = workdir("main", "/work/main");
        w.pool_size = 0;
        let cfg = config_with_workdirs(vec![], vec![w]);
        assert!(matches!(
            cfg.validate().unwrap_err(),
            ConfigError::PoolSizeTooSmall { .. }
        ));
    }

    #[test]
    fn validate_rejects_legacy_runner_inside_workdir_tree() {
        // A legacy runner executing directly in (or under) a pooled work dir's
        // canonical clone would trample the pool's git state.
        let cfg = config_with_workdirs(
            vec![runner("legacy", "/work/main/sub")],
            vec![workdir("main", "/work/main")],
        );
        let err = cfg.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::RunnerWorkdirCollision { .. }),
            "expected RunnerWorkdirCollision, got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_worktrees_dir_inside_canonical_clone() {
        // worktrees nested inside the canonical clone (design §4.1 hazard).
        let mut w = workdir("main", "/work/main");
        w.worktrees_dir = Some(PathBuf::from("/work/main/.worktrees"));
        let cfg = config_with_workdirs(vec![], vec![w]);
        assert!(matches!(
            cfg.validate().unwrap_err(),
            ConfigError::WorkdirPathCollision { .. }
        ));
    }

    #[test]
    fn validate_rejects_worktrees_dir_colliding_with_other_workdir_path() {
        let mut a = workdir("a", "/work/a");
        a.worktrees_dir = Some(PathBuf::from("/work/b/wt"));
        let b = workdir("b", "/work/b");
        let cfg = config_with_workdirs(vec![], vec![a, b]);
        assert!(matches!(
            cfg.validate().unwrap_err(),
            ConfigError::WorkdirPathCollision { .. }
        ));
    }

    #[test]
    fn pooled_runners_exempt_from_legacy_working_dir_collision() {
        // Two pooled runners share the SAME vestigial workspace.working_dir
        // (`/vestige`), which would trip the legacy DuplicateWorkingDir check.
        // Because they reference a work dir, that check must be skipped.
        let cfg = config_with_workdirs(
            vec![
                pooled_runner("codex", "main"),
                pooled_runner("fable", "main"),
            ],
            vec![workdir("main", "/work/main")],
        );
        // Must not error on the vestigial working_dir collision.
        cfg.validate().unwrap();
    }

    #[test]
    fn legacy_runners_still_collide_on_shared_working_dir() {
        // A legacy (non-pooled) runner pair sharing a dir must still be
        // rejected — pooling doesn't loosen the guarantee for them.
        let cfg = config_with(vec![
            runner("a", "/work/shared"),
            runner("b", "/work/shared"),
        ]);
        assert!(matches!(
            cfg.validate().unwrap_err(),
            ConfigError::DuplicateWorkingDir { .. }
        ));
    }

    #[test]
    fn workdir_defaults_pool_size_and_clean_mode() {
        let w = workdir("main", "/work/main");
        assert_eq!(w.pool_size, DEFAULT_POOL_SIZE);
        assert_eq!(w.clean_mode, CleanMode::KeepIgnored);
        assert!(!w.pool_size_is_large());
    }

    #[test]
    fn workdir_for_resolves_reference() {
        let cfg = config_with_workdirs(
            vec![pooled_runner("codex", "main")],
            vec![workdir("main", "/work/main")],
        );
        let r = &cfg.runners[0];
        let w = cfg.workdir_for(r).expect("workdir resolves");
        assert_eq!(w.name, "main");
        assert_eq!(w.path, PathBuf::from("/work/main"));
        // A legacy runner resolves to None.
        let legacy = runner("legacy", "/work/legacy");
        assert!(cfg.workdir_for(&legacy).is_none());
    }

    #[test]
    fn workdir_toml_roundtrip_with_defaults_omitted() {
        // A minimal [[workdir]] (name + path only) parses, and serializing a
        // default config omits empty keep_paths / None options.
        let toml_in = r#"
            version = 2
            [daemon]
            cloud_url = "https://x"
            [[workdir]]
            name = "main"
            path = "/work/main"
        "#;
        let cfg: Config = toml::from_str(toml_in).expect("parse");
        assert_eq!(cfg.workdirs.len(), 1);
        assert_eq!(cfg.workdirs[0].pool_size, DEFAULT_POOL_SIZE);
        assert_eq!(cfg.workdirs[0].clean_mode, CleanMode::KeepIgnored);

        let out = toml::to_string(&cfg).expect("serialize");
        assert!(out.contains("[[workdir]]"), "out: {out}");
        // keep_paths is empty → omitted; setup_command None → omitted.
        assert!(!out.contains("keep_paths"), "out: {out}");
        assert!(!out.contains("setup_command"), "out: {out}");
    }
}
