//! Workspace seed worker task (D-09, stage 5).
//!
//! Port of `apps/api/pi_dash/bgtasks/workspace_seed_task.py:504-570`
//! (`@shared_task`, no decorator options — default ack-on-success, Celery
//! `max_retries = 3` / `default_retry_delay = 180s` per the jobs plane).
//! The pure planners live in
//! [`pidash_services::tasks_cleanup::workspace_seed`]; this module owns the
//! SQL, the [`SeedStore`] trait the fakes replay, [`run`] (the orchestration
//! at `:543-564`), [`delay_message`] (the `.delay()` equivalent), and
//! [`register`] (the worker [`Registry`][crate::worker::Registry] entry).
//!
//! Every write runs under an explicit
//! [`pidash_db::context::RequestContext`] whose actor is the seed bot user
//! created at `:522-532` — the jobs-plane form of
//! `save(created_by_id=bot_user.id, disable_auto_set_user=True)`.
//! `updated_by` stays NULL on every seeded row except `Page` and
//! `ProjectPage`, which pass `updated_by_id=bot_user.id` (`:370-371,
//! :384`) — mirrored per table below.
//!
//! SQL mirrors the Django ORM statement for statement, in source order:
//!
//! * bot `User.objects.create :522` — `email.lower().strip()` (`user.py`),
//!   `password = make_password(uuid4hex)` (pbkdf2_sha256/600000), all other
//!   columns at their model defaults.
//! * `WorkspaceMember.objects.create :535` (role 20, `company_role=""`).
//! * `create_project_and_member`: `WorkspaceMember.objects.filter(
//!   workspace).values("member_id", "role")` (`:87`, `-created_at` model
//!   ordering); `Project.save` side effects — `identifier.strip().upper()`,
//!   timezone copied from the workspace, first-project `is_default` with
//!   the atomic demote-others update (`project.py:255-298`);
//!   `ProjectMember` + `ProjectUserProperty` bulk inserts with the FIXED
//!   display literals and the model-default JSON (`get_default_props`/
//!   `get_default_filters`/`get_default_preferences`).
//! * `create_project_states/labels`: max-queries through the model managers
//!   (`StateManager` excludes triage) then inserts.
//! * `create_cycles`: `UPCOMING` chains off
//!   `Cycle.objects.filter(project).order_by("-end_date").first()` (`:418`);
//!   min-sort insert scope is the default manager.
//! * `create_modules`: per-index `now + index*2d` dates stored as DATEs
//!   (Django `DateField` truncates the datetimes at `:460-461`).
//! * `create_project_issues`: `Issue.save` advisory xact lock keyed by
//!   `convert_uuid_to_integer(project.id)` (`sha256(str(uuid))[:8]` signed
//!   big-endian, `utils/uuid.py:19-26`); the auto `IssueSequence` row from
//!   `save` (NULL audit, workspace from project) PLUS the explicit
//!   `IssueSequence.objects.create` row (bot audit) — two rows per issue,
//!   ported as-is; `assigned_pod` resolves through
//!   `Pod.default_for_project_id` (`runner/models.py:174-176`, NULL when no
//!   default pod); `completed_at` from the state's group.
//! * `create_views`: `query` is `{}` for the seed rows (empty filters).
//! * `create_pages`: `ProjectPage` link rows for `PROJECT`-type pages with
//!   a `project_id`.
//! * Failure re-raises after logging (`:568-570`): [`run`] returns `Err`,
//!   and [`register`] maps it to a handler error (requeue while the Celery
//!   retry budget lasts, then park as failed).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use pidash_db::context::RequestContext;
use pidash_services::tasks_cleanup::workspace_seed as logic;

use crate::celery::CeleryTaskMessage;
use crate::worker::{Handler, Registry, Verdict};

/// Celery wire name: the default `<module>.<function>` name Celery derives
/// for `workspace_seed` in `pi_dash/bgtasks/workspace_seed_task.py`.
pub const TASK_NAME: &str = "pi_dash.bgtasks.workspace_seed_task.workspace_seed";

/// Bot membership role (`WorkspaceMember.objects.create ... role=20
/// :535-540`).
pub const BOT_WORKSPACE_ROLE: i16 = 20;

/// One workspace-membership row as read at `:87`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMemberRow {
    pub member_id: uuid::Uuid,
    pub role: i16,
}

/// Workspace columns the task reads (`:84,522,260`): name for the project
/// name/identifier, timezone for the `Project.save` copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRow {
    pub name: String,
    pub timezone: String,
}

/// Parameters for one project insert, after the `Project.save` pure steps.
#[derive(Debug, Clone)]
pub struct ProjectInsert {
    pub name: String,
    pub identifier: String,
    pub timezone: String,
    pub is_default: bool,
    pub description: String,
    pub network: i16,
    pub cover_image: Option<String>,
    pub logo_props: Value,
    pub default_agent_executor: String,
}

/// Parameters for one state insert.
#[derive(Debug, Clone)]
pub struct StateInsert {
    pub name: String,
    pub color: String,
    pub group: String,
    pub is_default: bool,
    pub sequence: f64,
    pub slug: String,
}

/// Parameters for one label insert.
#[derive(Debug, Clone)]
pub struct LabelInsert {
    pub name: String,
    pub color: String,
    pub sort_order: f64,
}

/// Parameters for one cycle insert.
#[derive(Debug, Clone)]
pub struct CycleInsert {
    pub name: String,
    pub timezone: String,
    pub start_date: DateTime<Utc>,
    pub end_date: DateTime<Utc>,
    pub sort_order: f64,
}

/// Parameters for one module insert.
#[derive(Debug, Clone)]
pub struct ModuleInsert {
    pub name: String,
    pub description: String,
    pub status: String,
    pub start_date: chrono::NaiveDate,
    pub target_date: chrono::NaiveDate,
    pub sort_order: f64,
}

/// Parameters for one issue insert.
#[derive(Debug, Clone)]
pub struct IssueInsert {
    pub name: String,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub priority: String,
    pub sequence_id: i64,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub state_id: uuid::Uuid,
    pub assigned_pod_id: Option<uuid::Uuid>,
    pub rest: Map<String, Value>,
}

/// Parameters for one page insert.
#[derive(Debug, Clone)]
pub struct PageInsert {
    pub name: String,
    pub access: i16,
    pub description_json: Value,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub description_binary: Option<Vec<u8>>,
}

/// Parameters for one view insert.
#[derive(Debug, Clone)]
pub struct ViewInsert {
    pub name: String,
    pub description: String,
    pub access: i16,
    pub filters: Value,
    pub query: Value,
    pub display_filters: Value,
    pub display_properties: Value,
    pub rich_filters: Value,
    pub sort_order: f64,
    pub rest: Map<String, Value>,
}

/// Where the seed rows go. One method per read/insert family so fakes stay
/// trivial and the Postgres implementation stays the only SQL owner (the
/// [`crate::license::tasks::TraceDb`] pattern). Every `insert_*` takes the
/// request context explicitly and stamps `created_by` (and `updated_by`
/// for pages) from its actor.
///
/// Reads that Django scopes through soft-delete managers carry
/// `AND deleted_at IS NULL`; the state max additionally excludes
/// `group = 'triage'` (`StateManager`); the issue max/sort additionally
/// exclude triage-state, archived, project-archived and draft rows
/// (`IssueManager`).
pub trait SeedStore {
    /// `Workspace.objects.get(id=...)` — name + timezone (`:84,260,519`).
    fn workspace(
        &self,
        workspace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<WorkspaceRow>, String>> + Send;

    /// `WorkspaceMember.objects.filter(workspace).values("member_id",
    /// "role")` (`:87`), `-created_at` model ordering.
    fn workspace_members(
        &self,
        workspace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Vec<WorkspaceMemberRow>, String>> + Send;

    /// `Project.objects.filter(workspace, is_default=True,
    /// deleted_at__isnull=True).exists()` (`project.py:263-270`).
    fn default_project_exists(
        &self,
        workspace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<bool, String>> + Send;

    /// `State.objects.filter(project).aggregate(Max("sequence"))`
    /// (`state.py:136`): triage-excluded, soft-delete-filtered.
    fn max_state_sequence(
        &self,
        project_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<f64>, String>> + Send;

    /// `Label.objects.filter(project).aggregate(Max("sort_order"))`
    /// (`label.py:49`).
    fn max_label_sort_order(
        &self,
        project_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<f64>, String>> + Send;

    /// `Cycle.objects.filter(project).aggregate(Min("sort_order"))`
    /// (`cycle.py:89-91`).
    fn min_cycle_sort_order(
        &self,
        project_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<f64>, String>> + Send;

    /// `Module.objects.filter(project).aggregate(Min("sort_order"))`
    /// (`module.py`).
    fn min_module_sort_order(
        &self,
        project_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<f64>, String>> + Send;

    /// `IssueView.objects.filter(project).aggregate(Max("sort_order"))`
    /// (`view.py:86-88`).
    fn max_view_sort_order(
        &self,
        project_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<f64>, String>> + Send;

    /// `IssueSequence.objects.filter(project).aggregate(Max("sequence"))`
    /// under the per-project advisory xact lock (`issue.py:313-328`).
    fn max_issue_sequence(
        &self,
        project_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<i64>, String>> + Send;

    /// `Issue.objects.filter(project, state).aggregate(Max("sort_order"))`
    /// (`issue.py:335-337`): the plain soft-delete manager (`Issue.objects`
    /// is inherited from `SoftDeleteModel`, not the triage/archived/draft
    /// excluding `issue_objects = IssueManager()`), so only `deleted_at IS
    /// NULL` applies.
    fn max_issue_sort_order(
        &self,
        project_id: uuid::Uuid,
        state_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<f64>, String>> + Send;

    /// `Cycle.objects.filter(project).order_by("-end_date").first()` end
    /// date for `UPCOMING` chaining (`:418`).
    fn last_cycle_end(
        &self,
        project_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<DateTime<Utc>>, String>> + Send;

    /// The state's `group` for the `completed_at` branch (`issue.py:303`).
    fn state_group(
        &self,
        state_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send;

    /// `Pod.default_for_project_id` (`runner/models.py:174-176`): the
    /// default pod for the project, if one exists. `Pod.objects` is the
    /// `PodManager`, so soft-deleted pods are excluded.
    fn default_pod_for_project(
        &self,
        project_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<uuid::Uuid>, String>> + Send;

    /// `User.objects.create :522-532` — returns the bot user id.
    fn insert_bot_user(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// `WorkspaceMember.objects.create :535-540` — returns the row id.
    fn insert_workspace_member(
        &self,
        workspace_id: uuid::Uuid,
        member_id: uuid::Uuid,
        role: i16,
        company_role: &str,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// `Project(...)` + `save` (`:101-112`) with the demote-others update
    /// when `is_default` (`project.py:292-298`) — returns the project id.
    fn insert_project(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project: &ProjectInsert,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `ProjectMember` bulk row (`:115-126`).
    fn insert_project_member(
        &self,
        ctx: &RequestContext,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        member_id: uuid::Uuid,
        role: i16,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `ProjectUserProperty` bulk row (`:129-168`).
    fn insert_project_user_property(
        &self,
        ctx: &RequestContext,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        user_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `State` row (`:199-205`).
    fn insert_state(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        state: &StateInsert,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `Label` row (`:233-238`).
    fn insert_label(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        label: &LabelInsert,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `Cycle` row (`:426-435`).
    fn insert_cycle(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        cycle: &CycleInsert,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `Module` row (`:464-471`).
    fn insert_module(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        module: &ModuleInsert,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `Issue` row (`:285-292`) — returns the issue id. The
    /// `Issue.save` auto-`IssueSequence` row is a separate
    /// [`SeedStore::insert_save_sequence`] call so the double-row quirk
    /// stays visible.
    fn insert_issue(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        issue: &IssueInsert,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// The `Issue.save`-created `IssueSequence(issue, sequence=sequence_id)`
    /// row (`issue.py:343`): NULL audit, workspace from project.
    fn insert_save_sequence(
        &self,
        issue_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        sequence: i64,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// The explicit `IssueSequence.objects.create :293-298` row (bot
    /// audit, default `sequence = 1`).
    fn insert_issue_sequence(
        &self,
        ctx: &RequestContext,
        issue_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// `IssueActivity.objects.create :300-308` (epoch = run time).
    fn insert_issue_activity(
        &self,
        issue_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        actor_id: uuid::Uuid,
        epoch_secs: f64,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// `IssueLabel.objects.create :312-318`.
    fn insert_issue_label(
        &self,
        ctx: &RequestContext,
        issue_id: uuid::Uuid,
        label_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// `CycleIssue.objects.create :322-328`.
    fn insert_cycle_issue(
        &self,
        ctx: &RequestContext,
        issue_id: uuid::Uuid,
        cycle_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// `ModuleIssue.objects.create :333-339`.
    fn insert_module_issue(
        &self,
        ctx: &RequestContext,
        issue_id: uuid::Uuid,
        module_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `Page` row (`:361-375`).
    fn insert_page(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        page: &PageInsert,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// `ProjectPage` link (`:379-386`).
    fn insert_project_page(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        page_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;

    /// One `IssueView` row (`:493-500`).
    fn insert_view(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        view: &ViewInsert,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, String>> + Send;
}

/// Input for one [`run`]: the seed-data directory (`settings.SEED_DIR /
/// "data"`), the target workspace, the run clock (`timezone.now()`), fresh
/// bot secrets, and the resolved project-executor default.
#[derive(Debug, Clone)]
pub struct RunInput {
    pub seed_data_dir: PathBuf,
    pub workspace_id: uuid::Uuid,
    pub now: DateTime<Utc>,
    /// `uuid.uuid4().hex` for `make_password` (`:530`).
    pub password_hex: String,
    /// 22-char alphanumerics for the password salt (Django
    /// `get_random_string` alphabet).
    pub password_salt: String,
    /// `get_default_agent_executor()` (`core/agent_execution.py:26-31`).
    pub default_agent_executor: String,
}

impl RunInput {
    /// Build the per-run input exactly like the task does: fresh secrets
    /// plus the executor default.
    pub fn now_for(seed_data_dir: &Path, workspace_id: uuid::Uuid, now: DateTime<Utc>) -> Self {
        Self {
            seed_data_dir: seed_data_dir.to_owned(),
            workspace_id,
            now,
            password_hex: uuid::Uuid::new_v4().simple().to_string(),
            password_salt: uuid::Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(22)
                .collect(),
            default_agent_executor: default_agent_executor(),
        }
    }
}

/// `get_default_agent_executor` (`core/agent_execution.py:26-31`):
/// `DEFAULT_AGENT_EXECUTOR` (`settings/common.py:545`, itself
/// `get_config("DEFAULT_AGENT_EXECUTOR", "local_runner")`) validated
/// against the executor choices, else `local_runner`.
pub fn default_agent_executor() -> String {
    match std::env::var("DEFAULT_AGENT_EXECUTOR") {
        Ok(value)
            if matches!(
                value.as_str(),
                "local_runner" | "cloud_agent" | "managed_runner"
            ) =>
        {
            value
        }
        _ => "local_runner".to_owned(),
    }
}

/// What one [`run`] seeded, in orchestration order — the DB before/after
/// assertion surface.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SeedReport {
    pub projects: usize,
    pub states: usize,
    pub labels: usize,
    pub cycles: usize,
    pub modules: usize,
    pub issues: usize,
    pub views: usize,
    pub pages: usize,
    pub project_pages: usize,
    /// The `:274` missing-field error logs (ported bug 1 keeps processing).
    pub warnings: Vec<String>,
}

fn key_error(key: &str) -> String {
    format!("KeyError: {key}")
}

fn map_lookup(
    map: &HashMap<i64, uuid::Uuid>,
    seed_id: i64,
    what: &str,
) -> Result<uuid::Uuid, String> {
    map.get(&seed_id)
        .copied()
        .ok_or_else(|| key_error(&format!("{what} {seed_id}")))
}

fn seed_str<'a>(row: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

fn seed_f64(row: &Map<String, Value>, key: &str) -> Option<f64> {
    row.get(key).and_then(Value::as_f64)
}

/// Run the task (`workspace_seed :516-570`): bot user, membership, then
/// creators in `:543-564` order. Any failure aborts with the Python
/// exception's shape (`:568-570` re-raise) — the caller logs and returns
/// `Err`, and [`register`] turns it into a worker retry/park.
pub async fn run<S: SeedStore>(store: &S, input: &RunInput) -> Result<SeedReport, String> {
    let workspace_id = input.workspace_id;
    let workspace = store.workspace(workspace_id).await?.ok_or_else(|| {
        // `Workspace.objects.get` DoesNotExist.
        "Workspace matching query does not exist.".to_owned()
    })?;

    // Bot user + membership (`:521-540`).
    let bot_username = logic::bot_username(&workspace_id.to_string());
    let bot_email = logic::bot_email(&workspace_id.to_string());
    let password_hash = logic::bot_password_hash(&input.password_hex, &input.password_salt);
    let bot_id = store
        .insert_bot_user(
            &bot_username,
            &logic::normalize_user_email(&bot_email),
            &password_hash,
        )
        .await?;
    let ctx = RequestContext::new(
        pidash_types::WorkspaceId::from(workspace_id.to_string()),
        Some(pidash_types::UserId::from(bot_id.to_string())),
    );
    store
        .insert_workspace_member(workspace_id, bot_id, BOT_WORKSPACE_ROLE, "")
        .await?;

    let mut report = SeedReport::default();

    // Projects + members (`:543`, `create_project_and_member :71-173`).
    let members = store.workspace_members(workspace_id).await?;
    let mut project_map: HashMap<i64, uuid::Uuid> = HashMap::new();
    if let Some(rows) = logic::read_seed_file(&input.seed_data_dir, "projects.json") {
        let identifier = logic::project_identifier(&workspace.name);
        for mut row in rows {
            let row = row
                .as_object_mut()
                .ok_or_else(|| key_error("project row"))?;
            let seed_id = row
                .remove("id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("id"))?;
            row.remove("name");
            row.remove("identifier");
            let project_id = store
                .insert_project(
                    &ctx,
                    workspace_id,
                    &ProjectInsert {
                        name: workspace.name.clone(),
                        identifier: logic::normalize_identifier(&identifier),
                        timezone: workspace.timezone.clone(),
                        is_default: !store.default_project_exists(workspace_id).await?,
                        description: seed_str(row, "description").unwrap_or("").to_owned(),
                        network: row.get("network").and_then(Value::as_i64).unwrap_or(2) as i16,
                        cover_image: seed_str(row, "cover_image").map(str::to_owned),
                        logo_props: row
                            .get("logo_props")
                            .cloned()
                            .unwrap_or(Value::Object(Map::new())),
                        default_agent_executor: input.default_agent_executor.clone(),
                    },
                )
                .await?;
            for member in &members {
                store
                    .insert_project_member(
                        &ctx,
                        project_id,
                        workspace_id,
                        member.member_id,
                        member.role,
                    )
                    .await?;
                store
                    .insert_project_user_property(&ctx, project_id, workspace_id, member.member_id)
                    .await?;
            }
            project_map.insert(seed_id, project_id);
            report.projects += 1;
        }
    }

    // States (`:546`, `:176-208`).
    let mut state_map: HashMap<i64, uuid::Uuid> = HashMap::new();
    if let Some(rows) = logic::read_seed_file(&input.seed_data_dir, "states.json") {
        for value in rows {
            let mut row = value.as_object().cloned().unwrap_or_default();
            let seed_id = row
                .remove("id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("id"))?;
            let project_seed_id = row
                .remove("project_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("project_id"))?;
            let project_id = map_lookup(&project_map, project_seed_id, "project")?;
            let name = seed_str(&row, "name").unwrap_or("").to_owned();
            let sequence = logic::next_state_sequence(store.max_state_sequence(project_id).await?)
                .unwrap_or_else(|| seed_f64(&row, "sequence").unwrap_or(65_535.0));
            let state_id = store
                .insert_state(
                    &ctx,
                    workspace_id,
                    project_id,
                    &StateInsert {
                        slug: logic::slugify(&name),
                        name,
                        color: seed_str(&row, "color").unwrap_or("").to_owned(),
                        group: seed_str(&row, "group").unwrap_or("backlog").to_owned(),
                        is_default: row.get("default").and_then(Value::as_bool).unwrap_or(false),
                        sequence,
                    },
                )
                .await?;
            state_map.insert(seed_id, state_id);
            report.states += 1;
        }
    }

    // Labels (`:549`, `:211-242`).
    let mut label_map: HashMap<i64, uuid::Uuid> = HashMap::new();
    if let Some(rows) = logic::read_seed_file(&input.seed_data_dir, "labels.json") {
        for value in rows {
            let mut row = value.as_object().cloned().unwrap_or_default();
            let seed_id = row
                .remove("id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("id"))?;
            let project_seed_id = row
                .remove("project_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("project_id"))?;
            let project_id = map_lookup(&project_map, project_seed_id, "project")?;
            let sort_order =
                logic::next_label_sort_order(store.max_label_sort_order(project_id).await?)
                    .unwrap_or_else(|| seed_f64(&row, "sort_order").unwrap_or(65_535.0));
            let label_id = store
                .insert_label(
                    &ctx,
                    workspace_id,
                    project_id,
                    &LabelInsert {
                        name: seed_str(&row, "name").unwrap_or("").to_owned(),
                        color: seed_str(&row, "color").unwrap_or("").to_owned(),
                        sort_order,
                    },
                )
                .await?;
            label_map.insert(seed_id, label_id);
            report.labels += 1;
        }
    }

    // Cycles (`:552`, `:391-439`).
    let mut cycle_map: HashMap<i64, uuid::Uuid> = HashMap::new();
    if let Some(rows) = logic::read_seed_file(&input.seed_data_dir, "cycles.json") {
        for value in rows {
            let mut row = value.as_object().cloned().unwrap_or_default();
            let seed_id = row
                .remove("id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("id"))?;
            let project_seed_id = row
                .remove("project_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("project_id"))?;
            let project_id = map_lookup(&project_map, project_seed_id, "project")?;
            let cycle_type = seed_str(&row, "type").map(str::to_owned);
            row.remove("type");
            let last_end = store.last_cycle_end(project_id).await?;
            let plan = logic::plan_cycle_dates(cycle_type.as_deref(), last_end.is_some()).map_err(
                |e| match e {
                    logic::CycleSeedError::MissingType => key_error("'type'"),
                    // `NameError: name 'start_date' is not defined` at
                    // `Cycle(...) :426` for any other type value.
                    logic::CycleSeedError::UnboundDates(_) => {
                        "NameError: name 'start_date' is not defined".to_owned()
                    }
                },
            )?;
            let anchor_day = match plan.anchor {
                logic::CycleAnchor::Now => input.now,
                logic::CycleAnchor::LastCycleEnd => {
                    last_end.expect("UPCOMING-with-history always has an end")
                }
            };
            let start_date = anchor_day + chrono::Duration::days(plan.start_offset_days);
            let end_date = anchor_day + chrono::Duration::days(plan.end_offset_days);
            let sort_order =
                logic::next_cycle_sort_order(store.min_cycle_sort_order(project_id).await?)
                    .unwrap_or_else(|| seed_f64(&row, "sort_order").unwrap_or(65_535.0));
            let cycle_id = store
                .insert_cycle(
                    &ctx,
                    workspace_id,
                    project_id,
                    &CycleInsert {
                        name: seed_str(&row, "name").unwrap_or("").to_owned(),
                        timezone: seed_str(&row, "timezone").unwrap_or("UTC").to_owned(),
                        start_date,
                        end_date,
                        sort_order,
                    },
                )
                .await?;
            cycle_map.insert(seed_id, cycle_id);
            report.cycles += 1;
        }
    }

    // Modules (`:555`, `:442-474`).
    let mut module_map: HashMap<i64, uuid::Uuid> = HashMap::new();
    if let Some(rows) = logic::read_seed_file(&input.seed_data_dir, "modules.json") {
        for (index, value) in rows.into_iter().enumerate() {
            let mut row = value.as_object().cloned().unwrap_or_default();
            let seed_id = row
                .remove("id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("id"))?;
            let project_seed_id = row
                .remove("project_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("project_id"))?;
            let project_id = map_lookup(&project_map, project_seed_id, "project")?;
            let dates = logic::plan_module_dates(index);
            let today = input.now.date_naive();
            let sort_order =
                logic::next_cycle_sort_order(store.min_module_sort_order(project_id).await?)
                    .unwrap_or_else(|| seed_f64(&row, "sort_order").unwrap_or(65_535.0));
            let module_id = store
                .insert_module(
                    &ctx,
                    workspace_id,
                    project_id,
                    &ModuleInsert {
                        name: seed_str(&row, "name").unwrap_or("").to_owned(),
                        description: seed_str(&row, "description").unwrap_or("").to_owned(),
                        status: seed_str(&row, "status").unwrap_or("planned").to_owned(),
                        start_date: today + chrono::Duration::days(dates.start_offset_days),
                        target_date: today + chrono::Duration::days(dates.target_offset_days),
                        sort_order,
                    },
                )
                .await?;
            module_map.insert(seed_id, module_id);
            report.modules += 1;
        }
    }

    // Issues (`:558`, `:245-342`).
    if let Some(rows) = logic::read_seed_file(&input.seed_data_dir, "issues.json") {
        for value in rows {
            let row = value.as_object().cloned().unwrap_or_default();
            let (warnings, planned) = logic::plan_issue_seed(&row);
            report.warnings.extend(warnings);
            let plan = planned.map_err(|e| match e {
                logic::IssueSeedError::MissingKey(key) => key_error(&key),
            })?;
            // `Issue(**rest)`: unknown seed keys are `TypeError`.
            for key in plan.rest.keys() {
                if !matches!(
                    key.as_str(),
                    "name" | "priority" | "description_html" | "sequence_id" | "sort_order"
                ) {
                    return Err(format!(
                        "TypeError: Issue() got an unexpected keyword argument '{key}'"
                    ));
                }
            }
            let project_seed_id: i64 = plan
                .project_id
                .parse()
                .map_err(|_| key_error("project_id"))?;
            let state_seed_id: i64 = plan.state_id.parse().map_err(|_| key_error("state_id"))?;
            let project_id = map_lookup(&project_map, project_seed_id, "project")?;
            let state_id = map_lookup(&state_map, state_seed_id, "state")?;
            let group = store.state_group(state_id).await?.unwrap_or_default();
            let completed_at = if logic::issue_completed(&group) {
                Some(input.now)
            } else {
                None
            };
            let sequence_id =
                logic::next_issue_sequence_id(store.max_issue_sequence(project_id).await?);
            let sort_order = logic::next_issue_sort_order(
                store.max_issue_sort_order(project_id, state_id).await?,
            )
            .unwrap_or_else(|| {
                plan.rest
                    .get("sort_order")
                    .and_then(Value::as_f64)
                    .unwrap_or(65_535.0)
            });
            let stripped = plan
                .rest
                .get("description_html")
                .and_then(Value::as_str)
                .map(logic::strip_tags);
            let description_stripped = match plan.rest.get("description_html") {
                None => None,
                Some(Value::Null) => None,
                Some(Value::String(s)) if s.is_empty() => None,
                _ => stripped,
            };
            let assigned_pod_id = store.default_pod_for_project(project_id).await?;
            let issue_id = store
                .insert_issue(
                    &ctx,
                    workspace_id,
                    project_id,
                    &IssueInsert {
                        name: plan
                            .rest
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        description_html: plan
                            .rest
                            .get("description_html")
                            .and_then(Value::as_str)
                            .unwrap_or("<p></p>")
                            .to_owned(),
                        description_stripped,
                        priority: plan
                            .rest
                            .get("priority")
                            .and_then(Value::as_str)
                            .unwrap_or("none")
                            .to_owned(),
                        sequence_id,
                        sort_order,
                        completed_at,
                        state_id,
                        assigned_pod_id,
                        rest: plan.rest.clone(),
                    },
                )
                .await?;
            // `Issue.save` auto-row (ported double-row quirk) + explicit row.
            store
                .insert_save_sequence(issue_id, project_id, workspace_id, sequence_id)
                .await?;
            store
                .insert_issue_sequence(&ctx, issue_id, project_id, workspace_id)
                .await?;
            store
                .insert_issue_activity(
                    issue_id,
                    project_id,
                    workspace_id,
                    bot_id,
                    input.now.timestamp_millis() as f64 / 1000.0,
                )
                .await?;
            for label_seed_id in &plan.label_ids {
                let label_id = map_lookup(&label_map, *label_seed_id, "label")?;
                store
                    .insert_issue_label(&ctx, issue_id, label_id, project_id, workspace_id)
                    .await?;
            }
            // `if cycle_id:` — nonzero ints link (`:321`).
            if matches!(plan.cycle_id, Some(v) if v != 0) {
                let cycle_id = map_lookup(&cycle_map, plan.cycle_id.unwrap_or(0), "cycle")?;
                store
                    .insert_cycle_issue(&ctx, issue_id, cycle_id, project_id, workspace_id)
                    .await?;
            }
            // `if module_ids:` — non-empty lists link (`:331`).
            if matches!(&plan.module_ids, Some(ids) if !ids.is_empty()) {
                for module_seed_id in plan.module_ids.as_deref().unwrap_or(&[]) {
                    let module_id = map_lookup(&module_map, *module_seed_id, "module")?;
                    store
                        .insert_module_issue(&ctx, issue_id, module_id, project_id, workspace_id)
                        .await?;
                }
            }
            report.issues += 1;
        }
    }

    // Views (`:561`, `:477-500`).
    if let Some(rows) = logic::read_seed_file(&input.seed_data_dir, "views.json") {
        for value in rows {
            let row = value.as_object().cloned().unwrap_or_default();
            let (project_seed_id, rest) = logic::plan_view_seed(&row);
            let project_seed_id = project_seed_id.ok_or_else(|| key_error("project_id"))?;
            // `IssueView(**rest)`: unknown seed keys are `TypeError`.
            for key in rest.keys() {
                if !matches!(
                    key.as_str(),
                    "name"
                        | "description"
                        | "access"
                        | "filters"
                        | "display_filters"
                        | "display_properties"
                        | "rich_filters"
                        | "sort_order"
                ) {
                    return Err(format!(
                        "TypeError: IssueView() got an unexpected keyword argument '{key}'"
                    ));
                }
            }
            let project_id = map_lookup(&project_map, project_seed_id, "project")?;
            let filters = rest
                .get("filters")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let query = logic::view_query_value(&filters, input.now.date_naive())
                .map_err(|e| format!("issue_filters: {e}"))?;
            let sort_order =
                logic::next_view_sort_order(store.max_view_sort_order(project_id).await?)
                    .unwrap_or_else(|| {
                        rest.get("sort_order")
                            .and_then(Value::as_f64)
                            .unwrap_or(65_535.0)
                    });
            store
                .insert_view(
                    &ctx,
                    workspace_id,
                    project_id,
                    &ViewInsert {
                        name: rest
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        description: rest
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        // `access` model default is 1 (`view.py`).
                        access: rest.get("access").and_then(Value::as_i64).unwrap_or(1) as i16,
                        filters: rest
                            .get("filters")
                            .cloned()
                            .unwrap_or(Value::Object(Map::new())),
                        query,
                        display_filters: rest
                            .get("display_filters")
                            .cloned()
                            .unwrap_or_else(logic::issue_default_display_filters),
                        display_properties: rest
                            .get("display_properties")
                            .cloned()
                            .unwrap_or_else(logic::issue_default_display_properties),
                        rich_filters: rest
                            .get("rich_filters")
                            .cloned()
                            .unwrap_or(Value::Object(Map::new())),
                        sort_order,
                        rest,
                    },
                )
                .await?;
            report.views += 1;
        }
    }

    // Pages (`:564`, `:345-388`).
    if let Some(rows) = logic::read_seed_file(&input.seed_data_dir, "pages.json") {
        for value in rows {
            let mut row = value.as_object().cloned().unwrap_or_default();
            let _seed_id = row
                .remove("id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| key_error("id"))?;
            let description_html = row
                .get("description_html")
                .and_then(Value::as_str)
                .unwrap_or("<p></p>")
                .to_owned();
            let page_id = store
                .insert_page(
                    &ctx,
                    workspace_id,
                    &PageInsert {
                        name: row
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        access: row.get("access").and_then(Value::as_i64).unwrap_or(0) as i16,
                        // `page_seed.get("description_json", {})` plus the
                        // `or {}`: explicit null also stores `{}`.
                        description_json: match row.get("description_json") {
                            Some(Value::Object(map)) if !map.is_empty() => {
                                Value::Object(map.clone())
                            }
                            _ => Value::Object(Map::new()),
                        },
                        description_stripped: logic::description_stripped(Some(&description_html)),
                        description_html,
                        // Seed rows carry no binary payload
                        // (`page_seed.get("description_binary", None)`).
                        description_binary: None,
                    },
                )
                .await?;
            report.pages += 1;
            // `PROJECT`-type pages with a `project_id` link through
            // `ProjectPage` (`:378-386`).
            let is_project_page = row.get("type").and_then(Value::as_str) == Some("PROJECT")
                && row.get("project_id").and_then(Value::as_i64).is_some();
            if is_project_page {
                let project_seed_id = row.get("project_id").and_then(Value::as_i64).unwrap_or(0);
                let project_id = map_lookup(&project_map, project_seed_id, "project")?;
                store
                    .insert_project_page(&ctx, workspace_id, project_id, page_id)
                    .await?;
                report.project_pages += 1;
            }
        }
    }

    Ok(report)
}

/// `convert_uuid_to_integer` (`utils/uuid.py:19-26`): sha256 of the uuid
/// string, first 8 bytes as signed big-endian — the `Issue.save`
/// `pg_advisory_xact_lock` key (`issue.py:317-322`).
pub fn advisory_lock_key(project_id: uuid::Uuid) -> i64 {
    let digest = Sha256::digest(project_id.to_string().as_bytes());
    i64::from_be_bytes(digest[..8].try_into().expect("sha256 is 32 bytes"))
}

/// `get_default_props` (`db/models/workspace.py:22`): workspace-member
/// `view_props`/`default_props` (note: WITH `display_properties`, unlike
/// the project-level twin).
fn ws_default_props() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null,
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true, "layout": "list",
            "calendar_date_range": "",
        },
        "display_properties": {
            "assignee": true, "attachment_count": true, "created_on": true,
            "due_date": true, "estimate": true, "key": true, "labels": true,
            "link": true, "priority": true, "start_date": true,
            "state": true, "sub_issue_count": true, "updated_on": true,
        },
    })
}

/// `get_issue_props` (`db/models/workspace.py:110-111`).
fn ws_issue_props() -> Value {
    serde_json::json!({"subscribed": true, "assigned": true, "created": true, "all_issues": true})
}

/// `get_default_props` (`db/models/project.py:43`): project-member
/// `view_props`/`default_props` (WITHOUT `display_properties`).
fn project_default_props() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null,
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true, "layout": "list",
            "calendar_date_range": "",
        },
    })
}

/// `get_default_preferences` (`db/models/project.py:68`).
fn default_preferences() -> Value {
    serde_json::json!({"pages": {"block_display": true},
        "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}})
}

/// `get_default_filters` (`db/models/issue.py:50`).
fn default_issue_filters() -> Value {
    serde_json::json!({
        "priority": null, "state": null, "state_group": null,
        "assignees": null, "created_by": null, "labels": null,
        "start_date": null, "target_date": null, "subscriber": null,
    })
}

/// `get_view_props` (`db/models/page.py:19-20`).
fn page_view_props() -> Value {
    serde_json::json!({"full_width": false})
}

/// Postgres [`SeedStore`]: the exact Django query shapes. Timestamps are
/// one clock (`RunInput::now`) the way `auto_now_add`/`auto_now` stamp
/// statement time; uuids are v4 like the model default.
pub struct PgSeedStore {
    pool: sqlx::PgPool,
    now: DateTime<Utc>,
}

impl PgSeedStore {
    pub fn new(pool: sqlx::PgPool, now: DateTime<Utc>) -> Self {
        Self { pool, now }
    }

    fn db_err(e: sqlx::Error) -> String {
        e.to_string()
    }
}

impl SeedStore for PgSeedStore {
    async fn workspace(&self, workspace_id: uuid::Uuid) -> Result<Option<WorkspaceRow>, String> {
        // `Workspace.objects.get`: default soft-delete manager.
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT name, timezone FROM workspaces WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(row.map(|(name, timezone)| WorkspaceRow { name, timezone }))
    }

    async fn workspace_members(
        &self,
        workspace_id: uuid::Uuid,
    ) -> Result<Vec<WorkspaceMemberRow>, String> {
        // `.values("member_id", "role")`, `-created_at` model ordering.
        let rows: Vec<(uuid::Uuid, i16)> = sqlx::query_as(
            "SELECT member_id, role FROM workspace_members
             WHERE workspace_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(rows
            .into_iter()
            .map(|(member_id, role)| WorkspaceMemberRow { member_id, role })
            .collect())
    }

    async fn default_project_exists(&self, workspace_id: uuid::Uuid) -> Result<bool, String> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM projects
             WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL)",
        )
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(exists)
    }

    async fn max_state_sequence(&self, project_id: uuid::Uuid) -> Result<Option<f64>, String> {
        // `StateManager`: triage excluded.
        sqlx::query_scalar(
            "SELECT MAX(sequence) FROM states
             WHERE project_id = $1 AND deleted_at IS NULL AND \"group\" <> 'triage'",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)
    }

    async fn max_label_sort_order(&self, project_id: uuid::Uuid) -> Result<Option<f64>, String> {
        sqlx::query_scalar(
            "SELECT MAX(sort_order) FROM labels WHERE project_id = $1 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)
    }

    async fn min_cycle_sort_order(&self, project_id: uuid::Uuid) -> Result<Option<f64>, String> {
        sqlx::query_scalar(
            "SELECT MIN(sort_order) FROM cycles WHERE project_id = $1 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)
    }

    async fn min_module_sort_order(&self, project_id: uuid::Uuid) -> Result<Option<f64>, String> {
        sqlx::query_scalar(
            "SELECT MIN(sort_order) FROM modules WHERE project_id = $1 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)
    }

    async fn max_view_sort_order(&self, project_id: uuid::Uuid) -> Result<Option<f64>, String> {
        sqlx::query_scalar(
            "SELECT MAX(sort_order) FROM issue_views WHERE project_id = $1 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)
    }

    async fn max_issue_sequence(&self, project_id: uuid::Uuid) -> Result<Option<i64>, String> {
        // `SELECT pg_advisory_xact_lock` + max in one transaction, like
        // `Issue.save` (`issue.py:313-328`). The seeded project is brand
        // new, so no concurrent writer exists; the lock is parity, not
        // protection.
        let mut tx = self.pool.begin().await.map_err(Self::db_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(advisory_lock_key(project_id))
            .execute(&mut *tx)
            .await
            .map_err(Self::db_err)?;
        let max: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(sequence) FROM issue_sequences WHERE project_id = $1 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(Self::db_err)?;
        tx.commit().await.map_err(Self::db_err)?;
        Ok(max)
    }

    async fn max_issue_sort_order(
        &self,
        project_id: uuid::Uuid,
        state_id: uuid::Uuid,
    ) -> Result<Option<f64>, String> {
        // Plain `Issue.objects` scope: soft-delete only. (`issue_objects`
        // would exclude triage/archived/draft, but `Issue.save` queries
        // through `Issue.objects`.)
        sqlx::query_scalar(
            "SELECT MAX(sort_order) FROM issues
             WHERE project_id = $1 AND state_id = $2 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .bind(state_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)
    }

    async fn last_cycle_end(
        &self,
        project_id: uuid::Uuid,
    ) -> Result<Option<DateTime<Utc>>, String> {
        // `.order_by("-end_date").first()`: plain DESC, NULLS FIRST like
        // Django's ordering.
        Ok(sqlx::query_scalar(
            "SELECT end_date FROM cycles
             WHERE project_id = $1 AND deleted_at IS NULL ORDER BY end_date DESC LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::db_err)?
        .flatten())
    }

    async fn state_group(&self, state_id: uuid::Uuid) -> Result<Option<String>, String> {
        Ok(
            sqlx::query_scalar("SELECT \"group\" FROM states WHERE id = $1")
                .bind(state_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(Self::db_err)?
                .flatten(),
        )
    }

    async fn default_pod_for_project(
        &self,
        project_id: uuid::Uuid,
    ) -> Result<Option<uuid::Uuid>, String> {
        // `Pod.default_for_project_id` (`runner/models.py:174-176`):
        // `PodManager` excludes soft-deleted pods.
        Ok(
            sqlx::query_scalar("SELECT id FROM pod WHERE project_id = $1 AND is_default AND deleted_at IS NULL LIMIT 1")
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(Self::db_err)?
                .flatten(),
        )
    }

    async fn insert_bot_user(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> Result<uuid::Uuid, String> {
        // `User.objects.create :522-532` + `User.save` email/display
        // normalization; every other column at its model default.
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (password, last_login, id, username, mobile_number, email,
               display_name, first_name, last_name, avatar, avatar_asset_id, cover_image,
               cover_image_asset_id, date_joined, created_at, updated_at, last_location,
               created_location, is_superuser, is_managed, is_password_expired, is_active,
               is_staff, is_email_verified, is_password_autoset, token, user_timezone,
               last_active, last_login_time, last_logout_time, last_login_ip, last_logout_ip,
               last_login_medium, last_login_uagent, token_updated_at, is_bot, bot_type,
               is_email_valid, masked_at)
             VALUES ($1, NULL, $2, $3, NULL, $4,
               'Pi Dash', 'Pi Dash', '', '', NULL, NULL,
               NULL, $5, $5, $5, '',
               '', false, false, false, true,
               false, false, true, '', 'UTC',
               $5, NULL, NULL, '', '',
               'email', '', NULL, true, 'WORKSPACE_SEED',
               false, NULL)
             RETURNING id",
        )
        .bind(password_hash)
        .bind(uuid::Uuid::new_v4())
        .bind(username)
        .bind(email)
        .bind(self.now)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_workspace_member(
        &self,
        workspace_id: uuid::Uuid,
        member_id: uuid::Uuid,
        role: i16,
        company_role: &str,
    ) -> Result<uuid::Uuid, String> {
        // `.objects.create` without `created_by`: NULL audit.
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO workspace_members (created_at, updated_at, id, role, created_by_id,
               member_id, updated_by_id, workspace_id, company_role, view_props,
               default_props, issue_props, is_active, deleted_at, explored_features,
               getting_started_checklist, tips)
             VALUES ($1, $1, $2, $3, NULL,
               $4, NULL, $5, $6, $7,
               $7, $8, true, NULL, '{}',
               '{}', '{}')
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(role)
        .bind(member_id)
        .bind(workspace_id)
        .bind(company_role)
        .bind(sqlx::types::Json(ws_default_props()))
        .bind(sqlx::types::Json(ws_issue_props()))
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_project(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project: &ProjectInsert,
    ) -> Result<uuid::Uuid, String> {
        // `Project(...) :101-111` + `save` in one transaction: the
        // demote-others update (`project.py:292-298`) and the insert are
        // atomic, like `Project.save`'s `transaction.atomic` block.
        let actor = ctx_actor(ctx)?;
        let mut tx = self.pool.begin().await.map_err(Self::db_err)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO projects (created_at, updated_at, id, name, description,
               description_text, description_html, network, identifier, created_by_id,
               default_assignee_id, project_lead_id, updated_by_id, workspace_id, emoji,
               cycle_view, module_view, cover_image, issue_views_view, page_view,
               estimate_id, icon_prop, intake_view, archive_in, close_in, default_state_id,
               logo_props, archived_at, is_time_tracking_enabled, is_issue_type_enabled,
               deleted_at, guest_view_all_features, timezone, cover_image_asset_id,
               external_id, external_source, members_can_edit_states, repo_url, base_branch,
               agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled,
               is_default, agent_review_default_interval_seconds, default_agent_executor,
               agent_test_default_interval_seconds)
             VALUES ($1, $1, $2, $3, $4,
               NULL, NULL, $5, $6, $7,
               NULL, NULL, NULL, $8, NULL,
               true, true, $9, true, true,
               NULL, NULL, false, 0, 0, NULL,
               $10, NULL, false, false,
               NULL, false, $11, NULL,
               NULL, NULL, true, '', 'main',
               10800, 10, true,
               $12, 10800, $13,
               10800)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(&project.name)
        .bind(&project.description)
        .bind(project.network)
        .bind(&project.identifier)
        .bind(actor)
        .bind(workspace_id)
        .bind(project.cover_image.as_deref())
        .bind(sqlx::types::Json(project.logo_props.clone()))
        .bind(project.timezone.clone())
        .bind(project.is_default)
        .bind(project.default_agent_executor.clone())
        .fetch_one(&mut *tx)
        .await
        .map_err(Self::db_err)?;
        if project.is_default {
            sqlx::query(
                "UPDATE projects SET is_default = false
                 WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL AND id <> $2",
            )
            .bind(workspace_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(Self::db_err)?;
        }
        tx.commit().await.map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_project_member(
        &self,
        ctx: &RequestContext,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        member_id: uuid::Uuid,
        role: i16,
    ) -> Result<uuid::Uuid, String> {
        // One `bulk_create` row (`:115-126`): `bulk_create` skips
        // `ProjectMember.save`, so no auto `ProjectUserProperty` row here.
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO project_members (created_at, updated_at, id, comment, role,
               created_by_id, member_id, project_id, updated_by_id, workspace_id,
               view_props, default_props, sort_order, preferences, is_active, deleted_at)
             VALUES ($1, $1, $2, NULL, $3,
               $4, $5, $6, NULL, $7,
               $8, $8, 65535, $9, true, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(role)
        .bind(actor)
        .bind(member_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(sqlx::types::Json(project_default_props()))
        .bind(sqlx::types::Json(default_preferences()))
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_project_user_property(
        &self,
        ctx: &RequestContext,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        user_id: uuid::Uuid,
    ) -> Result<uuid::Uuid, String> {
        // One `bulk_create` row (`:129-168`) with the FIXED literals.
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO project_user_properties (created_at, updated_at, id,
               display_properties, created_by_id, project_id, updated_by_id, user_id,
               workspace_id, display_filters, filters, deleted_at, rich_filters,
               preferences, sort_order)
             VALUES ($1, $1, $2,
               $3, $4, $5, NULL, $6,
               $7, $8, $9, NULL, '{}',
               $10, 65535)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(sqlx::types::Json(logic::seed_display_properties()))
        .bind(actor)
        .bind(project_id)
        .bind(user_id)
        .bind(workspace_id)
        .bind(sqlx::types::Json(logic::seed_display_filters()))
        .bind(sqlx::types::Json(default_issue_filters()))
        .bind(sqlx::types::Json(default_preferences()))
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_state(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        state: &StateInsert,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO states (created_at, updated_at, id, name, description, color,
               slug, created_by_id, project_id, updated_by_id, workspace_id, sequence,
               \"group\", \"default\", external_id, external_source, is_triage, deleted_at)
             VALUES ($1, $1, $2, $3, '', $4,
               $5, $6, $7, NULL, $8, $9,
               $10, $11, NULL, NULL, false, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(&state.name)
        .bind(&state.color)
        .bind(&state.slug)
        .bind(actor)
        .bind(project_id)
        .bind(workspace_id)
        .bind(state.sequence)
        .bind(&state.group)
        .bind(state.is_default)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_label(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        label: &LabelInsert,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO labels (created_at, updated_at, id, name, description,
               created_by_id, project_id, updated_by_id, workspace_id, parent_id,
               color, sort_order, external_id, external_source, deleted_at)
             VALUES ($1, $1, $2, $3, '',
               $4, $5, NULL, $6, NULL,
               $7, $8, NULL, NULL, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(&label.name)
        .bind(actor)
        .bind(project_id)
        .bind(workspace_id)
        .bind(&label.color)
        .bind(label.sort_order)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_cycle(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        cycle: &CycleInsert,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO cycles (created_at, updated_at, id, name, description,
               start_date, end_date, created_by_id, owned_by_id, project_id,
               updated_by_id, workspace_id, view_props, sort_order, external_id,
               external_source, progress_snapshot, archived_at, logo_props,
               deleted_at, timezone, version)
             VALUES ($1, $1, $2, $3, '',
               $4, $5, $6, $6, $7,
               NULL, $8, '{}', $9, NULL,
               NULL, '{}', NULL, '{}',
               NULL, $10, 1)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(&cycle.name)
        .bind(cycle.start_date)
        .bind(cycle.end_date)
        .bind(actor)
        .bind(project_id)
        .bind(workspace_id)
        .bind(cycle.sort_order)
        .bind(&cycle.timezone)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_module(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        module: &ModuleInsert,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO modules (created_at, updated_at, id, name, description,
               description_text, description_html, start_date, target_date, status,
               created_by_id, lead_id, project_id, updated_by_id, workspace_id,
               view_props, sort_order, external_id, external_source, archived_at,
               logo_props, deleted_at)
             VALUES ($1, $1, $2, $3, $4,
               NULL, NULL, $5, $6, $7,
               $8, NULL, $9, NULL, $10,
               '{}', $11, NULL, NULL, NULL,
               '{}', NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(&module.name)
        .bind(&module.description)
        .bind(module.start_date)
        .bind(module.target_date)
        .bind(&module.status)
        .bind(actor)
        .bind(project_id)
        .bind(workspace_id)
        .bind(module.sort_order)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_issue(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        issue: &IssueInsert,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO issues (created_at, updated_at, id, name, description_json,
               priority, start_date, target_date, sequence_id, created_by_id, parent_id,
               project_id, state_id, updated_by_id, workspace_id, description_html,
               description_stripped, completed_at, sort_order, point, archived_at,
               is_draft, external_id, external_source, description_binary,
               estimate_point_id, type_id, deleted_at, git_work_branch, assigned_pod_id,
               workpad, created_via, agent_executor, complexity_score)
             VALUES ($1, $1, $2, $3, '{}',
               $4, NULL, NULL, $5, $6, NULL,
               $7, $8, NULL, $9, $10,
               $11, $12, $13, NULL, NULL,
               false, NULL, NULL, NULL,
               NULL, NULL, NULL, '', $14,
               '', NULL, NULL, 0)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(&issue.name)
        .bind(&issue.priority)
        .bind(issue.sequence_id)
        .bind(actor)
        .bind(project_id)
        .bind(issue.state_id)
        .bind(workspace_id)
        .bind(&issue.description_html)
        .bind(issue.description_stripped.as_deref())
        .bind(issue.completed_at)
        .bind(issue.sort_order)
        .bind(issue.assigned_pod_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_save_sequence(
        &self,
        issue_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        sequence: i64,
    ) -> Result<uuid::Uuid, String> {
        // The `Issue.save` row (`issue.py:343`): NULL audit, workspace set
        // from the project by `ProjectBaseModel.save`.
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO issue_sequences (created_at, updated_at, id, sequence, deleted,
               created_by_id, issue_id, project_id, updated_by_id, workspace_id, deleted_at)
             VALUES ($1, $1, $2, $3, false,
               NULL, $4, $5, NULL, $6, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(sequence)
        .bind(issue_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_issue_sequence(
        &self,
        ctx: &RequestContext,
        issue_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> Result<uuid::Uuid, String> {
        // The explicit `.objects.create` row (`:293-298`): bot audit,
        // default `sequence = 1`.
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO issue_sequences (created_at, updated_at, id, sequence, deleted,
               created_by_id, issue_id, project_id, updated_by_id, workspace_id, deleted_at)
             VALUES ($1, $1, $2, 1, false,
               $3, $4, $5, NULL, $6, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(actor)
        .bind(issue_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_issue_activity(
        &self,
        issue_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        actor_id: uuid::Uuid,
        epoch_secs: f64,
    ) -> Result<uuid::Uuid, String> {
        // `.objects.create :300-308` without `created_by`: NULL audit.
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO issue_activities (created_at, updated_at, id, verb, field,
               old_value, new_value, comment, attachments, created_by_id, issue_id,
               issue_comment_id, project_id, updated_by_id, workspace_id, actor_id,
               new_identifier, old_identifier, epoch, deleted_at)
             VALUES ($1, $1, $2, 'created', NULL,
               NULL, NULL, 'created the issue', '{}', NULL, $3,
               NULL, $4, NULL, $5, $6,
               NULL, NULL, $7, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(issue_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(actor_id)
        .bind(epoch_secs)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_issue_label(
        &self,
        ctx: &RequestContext,
        issue_id: uuid::Uuid,
        label_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO issue_labels (created_at, updated_at, id, created_by_id,
               issue_id, label_id, project_id, updated_by_id, workspace_id, deleted_at)
             VALUES ($1, $1, $2, $3,
               $4, $5, $6, NULL, $7, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(actor)
        .bind(issue_id)
        .bind(label_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_cycle_issue(
        &self,
        ctx: &RequestContext,
        issue_id: uuid::Uuid,
        cycle_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO cycle_issues (created_at, updated_at, id, created_by_id,
               cycle_id, issue_id, project_id, updated_by_id, workspace_id, deleted_at)
             VALUES ($1, $1, $2, $3,
               $4, $5, $6, NULL, $7, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(actor)
        .bind(cycle_id)
        .bind(issue_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_module_issue(
        &self,
        ctx: &RequestContext,
        issue_id: uuid::Uuid,
        module_id: uuid::Uuid,
        project_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO module_issues (created_at, updated_at, id, created_by_id,
               issue_id, module_id, project_id, updated_by_id, workspace_id, deleted_at)
             VALUES ($1, $1, $2, $3,
               $4, $5, $6, NULL, $7, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(actor)
        .bind(issue_id)
        .bind(module_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_page(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        page: &PageInsert,
    ) -> Result<uuid::Uuid, String> {
        // `Page(...) :361-373`: bot owns AND audits (`updated_by_id` set,
        // unlike every other seeded row).
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO pages (created_at, updated_at, id, name, description_json,
               description_html, description_stripped, access, created_by_id,
               owned_by_id, updated_by_id, workspace_id, color, archived_at, is_locked,
               parent_id, view_props, logo_props, description_binary, is_global,
               deleted_at, moved_to_page, moved_to_project, external_id,
               external_source, sort_order)
             VALUES ($1, $1, $2, $3, $4,
               $5, $6, $7, $8,
               $8, $8, $9, '', NULL, false,
               NULL, $10, '{}', NULL, false,
               NULL, NULL, NULL, NULL,
               NULL, 65535)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(&page.name)
        .bind(sqlx::types::Json(page.description_json.clone()))
        .bind(&page.description_html)
        .bind(page.description_stripped.as_deref())
        .bind(page.access)
        .bind(actor)
        .bind(workspace_id)
        .bind(sqlx::types::Json(page_view_props()))
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_project_page(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        page_id: uuid::Uuid,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO project_pages (created_at, updated_at, id, created_by_id,
               page_id, project_id, updated_by_id, workspace_id, deleted_at)
             VALUES ($1, $1, $2, $3,
               $4, $5, $3, $6, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(actor)
        .bind(page_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }

    async fn insert_view(
        &self,
        ctx: &RequestContext,
        workspace_id: uuid::Uuid,
        project_id: uuid::Uuid,
        view: &ViewInsert,
    ) -> Result<uuid::Uuid, String> {
        let actor = ctx_actor(ctx)?;
        let id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO issue_views (created_at, updated_at, id, name, description,
               query, access, filters, created_by_id, project_id, updated_by_id,
               workspace_id, display_filters, display_properties, sort_order,
               logo_props, is_locked, owned_by_id, deleted_at, rich_filters, archived_at)
             VALUES ($1, $1, $2, $3, $4,
               $5, $6, $7, $8, $9, NULL,
               $10, $11, $12, $13,
               '{}', false, $8, NULL, $14, NULL)
             RETURNING id",
        )
        .bind(self.now)
        .bind(uuid::Uuid::new_v4())
        .bind(&view.name)
        .bind(&view.description)
        .bind(sqlx::types::Json(view.query.clone()))
        .bind(view.access)
        .bind(sqlx::types::Json(view.filters.clone()))
        .bind(actor)
        .bind(project_id)
        .bind(workspace_id)
        .bind(sqlx::types::Json(view.display_filters.clone()))
        .bind(sqlx::types::Json(view.display_properties.clone()))
        .bind(view.sort_order)
        .bind(sqlx::types::Json(view.rich_filters.clone()))
        .fetch_one(&self.pool)
        .await
        .map_err(Self::db_err)?;
        Ok(id)
    }
}

/// The request-context actor as a uuid for `created_by` columns. The
/// context is built from the bot id inside [`run`], so this is infallible
/// by construction; a foreign context surfaces as an explicit error, never
/// a NULL audit.
fn ctx_actor(ctx: &RequestContext) -> Result<uuid::Uuid, String> {
    ctx.audit_actor()
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or_else(|| "workspace_seed: request context has no bot actor".to_owned())
}

/// The `.delay()` equivalent: a first-attempt Celery protocol v2 message
/// for `workspace_seed.delay(workspace_id)`, exactly what the call sites
/// publish.
pub fn delay_message(workspace_id: uuid::Uuid) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        TASK_NAME,
        vec![Value::String(workspace_id.to_string())],
        Map::new(),
    )
}

/// Install the worker handler owning [`TASK_NAME`]. A store failure (or a
/// seed-data `KeyError`/`NameError`/`TypeError`) propagates as a handler
/// error: the worker requeues with `DEFAULT_RETRY_DELAY_SECS` while the
/// Celery retry budget (`max_retries = 3`) lasts, then parks the row as
/// failed with this text. The bare `@shared_task` carries no ack overrides
/// (contract `test_task_options_parity` pins the empty option set).
pub fn register_workspace_seed(
    registry: &mut Registry,
    pool: sqlx::PgPool,
    seed_data_dir: PathBuf,
) {
    let handler: Handler = Arc::new(move |job: crate::queue::JobRow| {
        let pool = pool.clone();
        let seed_data_dir = seed_data_dir.clone();
        Box::pin(async move {
            let workspace_id: uuid::Uuid = job
                .args
                .as_array()
                .and_then(|args| args.first())
                .and_then(|v| v.as_str())
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .ok_or_else(|| "workspace_seed: args[0] must be a workspace uuid".to_owned())?;
            let now = Utc::now();
            let store = PgSeedStore::new(pool, now);
            let input = RunInput::now_for(&seed_data_dir, workspace_id, now);
            match run(&store, &input).await {
                Ok(report) => {
                    tracing::info!(
                        task = TASK_NAME,
                        workspace = %workspace_id,
                        projects = report.projects,
                        states = report.states,
                        labels = report.labels,
                        cycles = report.cycles,
                        modules = report.modules,
                        issues = report.issues,
                        views = report.views,
                        pages = report.pages,
                        "workspace seeded"
                    );
                    Ok(Verdict::Ack)
                }
                Err(error) => {
                    tracing::error!(task = TASK_NAME, workspace = %workspace_id, %error, "seed failed");
                    Err(error)
                }
            }
        })
    });
    registry.register(TASK_NAME, handler);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeStore {
        inner: Mutex<FakeInner>,
    }

    #[derive(Default)]
    struct FakeInner {
        events: Vec<String>,
        next_id: u128,
        workspace: Option<WorkspaceRow>,
        members: Vec<WorkspaceMemberRow>,
        default_exists: bool,
        state_groups: HashMap<uuid::Uuid, String>,
        maxes_f64: Option<f64>,
        max_seq: Option<i64>,
        last_end: Option<DateTime<Utc>>,
    }

    impl FakeStore {
        fn with_workspace(name: &str) -> Self {
            Self {
                inner: Mutex::new(FakeInner {
                    workspace: Some(WorkspaceRow {
                        name: name.to_owned(),
                        timezone: "UTC".to_owned(),
                    }),
                    ..Default::default()
                }),
            }
        }

        fn events(&self) -> Vec<String> {
            self.inner.lock().unwrap().events.clone()
        }

        fn minted(&self) -> Vec<uuid::Uuid> {
            // Every insert mints `Uuid::from_u128(counter)` in call order.
            let inner = self.inner.lock().unwrap();
            (1..=inner.next_id).map(uuid::Uuid::from_u128).collect()
        }
    }

    impl FakeInner {
        fn mint(&mut self, what: &str) -> uuid::Uuid {
            self.next_id += 1;
            let id = uuid::Uuid::from_u128(self.next_id);
            self.events.push(format!("{what} {id}"));
            id
        }
    }

    impl SeedStore for FakeStore {
        async fn workspace(&self, _id: uuid::Uuid) -> Result<Option<WorkspaceRow>, String> {
            Ok(self.inner.lock().unwrap().workspace.clone())
        }
        async fn workspace_members(
            &self,
            _id: uuid::Uuid,
        ) -> Result<Vec<WorkspaceMemberRow>, String> {
            Ok(self.inner.lock().unwrap().members.clone())
        }
        async fn default_project_exists(&self, _id: uuid::Uuid) -> Result<bool, String> {
            Ok(self.inner.lock().unwrap().default_exists)
        }
        async fn max_state_sequence(&self, _id: uuid::Uuid) -> Result<Option<f64>, String> {
            Ok(self.inner.lock().unwrap().maxes_f64)
        }
        async fn max_label_sort_order(&self, _id: uuid::Uuid) -> Result<Option<f64>, String> {
            Ok(self.inner.lock().unwrap().maxes_f64)
        }
        async fn min_cycle_sort_order(&self, _id: uuid::Uuid) -> Result<Option<f64>, String> {
            Ok(self.inner.lock().unwrap().maxes_f64)
        }
        async fn min_module_sort_order(&self, _id: uuid::Uuid) -> Result<Option<f64>, String> {
            Ok(self.inner.lock().unwrap().maxes_f64)
        }
        async fn max_view_sort_order(&self, _id: uuid::Uuid) -> Result<Option<f64>, String> {
            Ok(self.inner.lock().unwrap().maxes_f64)
        }
        async fn max_issue_sequence(&self, _id: uuid::Uuid) -> Result<Option<i64>, String> {
            Ok(self.inner.lock().unwrap().max_seq)
        }
        async fn max_issue_sort_order(
            &self,
            _project: uuid::Uuid,
            _state: uuid::Uuid,
        ) -> Result<Option<f64>, String> {
            Ok(self.inner.lock().unwrap().maxes_f64)
        }
        async fn last_cycle_end(&self, _id: uuid::Uuid) -> Result<Option<DateTime<Utc>>, String> {
            Ok(self.inner.lock().unwrap().last_end)
        }
        async fn state_group(&self, id: uuid::Uuid) -> Result<Option<String>, String> {
            Ok(self.inner.lock().unwrap().state_groups.get(&id).cloned())
        }
        async fn default_pod_for_project(
            &self,
            _id: uuid::Uuid,
        ) -> Result<Option<uuid::Uuid>, String> {
            Ok(None)
        }
        async fn insert_bot_user(
            &self,
            username: &str,
            email: &str,
            _hash: &str,
        ) -> Result<uuid::Uuid, String> {
            assert!(username.starts_with("bot_user_"), "bot username shape");
            assert!(email.ends_with("@example.com"), "bot email shape");
            Ok(self.inner.lock().unwrap().mint("bot"))
        }
        async fn insert_workspace_member(
            &self,
            _ws: uuid::Uuid,
            _member: uuid::Uuid,
            role: i16,
            company: &str,
        ) -> Result<uuid::Uuid, String> {
            assert_eq!((role, company), (BOT_WORKSPACE_ROLE, ""));
            Ok(self.inner.lock().unwrap().mint("ws_member"))
        }
        async fn insert_project(
            &self,
            ctx: &RequestContext,
            _ws: uuid::Uuid,
            project: &ProjectInsert,
        ) -> Result<uuid::Uuid, String> {
            assert!(
                ctx.actor_id().is_some(),
                "explicit request context on writes"
            );
            assert_eq!(project.identifier, project.identifier.to_uppercase());
            Ok(self.inner.lock().unwrap().mint("project"))
        }
        async fn insert_project_member(
            &self,
            ctx: &RequestContext,
            _p: uuid::Uuid,
            _ws: uuid::Uuid,
            _m: uuid::Uuid,
            _r: i16,
        ) -> Result<uuid::Uuid, String> {
            assert!(ctx.actor_id().is_some());
            Ok(self.inner.lock().unwrap().mint("project_member"))
        }
        async fn insert_project_user_property(
            &self,
            ctx: &RequestContext,
            _p: uuid::Uuid,
            _ws: uuid::Uuid,
            _u: uuid::Uuid,
        ) -> Result<uuid::Uuid, String> {
            assert!(ctx.actor_id().is_some());
            Ok(self.inner.lock().unwrap().mint("user_property"))
        }
        async fn insert_state(
            &self,
            _ctx: &RequestContext,
            _ws: uuid::Uuid,
            _p: uuid::Uuid,
            state: &StateInsert,
        ) -> Result<uuid::Uuid, String> {
            assert_eq!(state.slug, logic::slugify(&state.name));
            Ok(self.inner.lock().unwrap().mint("state"))
        }
        async fn insert_label(
            &self,
            _ctx: &RequestContext,
            _ws: uuid::Uuid,
            _p: uuid::Uuid,
            _label: &LabelInsert,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("label"))
        }
        async fn insert_cycle(
            &self,
            _ctx: &RequestContext,
            _ws: uuid::Uuid,
            _p: uuid::Uuid,
            cycle: &CycleInsert,
        ) -> Result<uuid::Uuid, String> {
            assert!(cycle.end_date > cycle.start_date);
            Ok(self.inner.lock().unwrap().mint("cycle"))
        }
        async fn insert_module(
            &self,
            _ctx: &RequestContext,
            _ws: uuid::Uuid,
            _p: uuid::Uuid,
            module: &ModuleInsert,
        ) -> Result<uuid::Uuid, String> {
            assert!(module.target_date > module.start_date);
            Ok(self.inner.lock().unwrap().mint("module"))
        }
        async fn insert_issue(
            &self,
            _ctx: &RequestContext,
            _ws: uuid::Uuid,
            _p: uuid::Uuid,
            _issue: &IssueInsert,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("issue"))
        }
        async fn insert_save_sequence(
            &self,
            _issue: uuid::Uuid,
            _p: uuid::Uuid,
            _ws: uuid::Uuid,
            _seq: i64,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("save_sequence"))
        }
        async fn insert_issue_sequence(
            &self,
            _ctx: &RequestContext,
            _issue: uuid::Uuid,
            _p: uuid::Uuid,
            _ws: uuid::Uuid,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("issue_sequence"))
        }
        async fn insert_issue_activity(
            &self,
            _issue: uuid::Uuid,
            _p: uuid::Uuid,
            _ws: uuid::Uuid,
            _actor: uuid::Uuid,
            _epoch: f64,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("activity"))
        }
        async fn insert_issue_label(
            &self,
            _ctx: &RequestContext,
            _issue: uuid::Uuid,
            _label: uuid::Uuid,
            _p: uuid::Uuid,
            _ws: uuid::Uuid,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("issue_label"))
        }
        async fn insert_cycle_issue(
            &self,
            _ctx: &RequestContext,
            _issue: uuid::Uuid,
            _cycle: uuid::Uuid,
            _p: uuid::Uuid,
            _ws: uuid::Uuid,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("cycle_issue"))
        }
        async fn insert_module_issue(
            &self,
            _ctx: &RequestContext,
            _issue: uuid::Uuid,
            _module: uuid::Uuid,
            _p: uuid::Uuid,
            _ws: uuid::Uuid,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("module_issue"))
        }
        async fn insert_page(
            &self,
            _ctx: &RequestContext,
            _ws: uuid::Uuid,
            _page: &PageInsert,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("page"))
        }
        async fn insert_project_page(
            &self,
            _ctx: &RequestContext,
            _ws: uuid::Uuid,
            _p: uuid::Uuid,
            _page: uuid::Uuid,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("project_page"))
        }
        async fn insert_view(
            &self,
            _ctx: &RequestContext,
            _ws: uuid::Uuid,
            _p: uuid::Uuid,
            _view: &ViewInsert,
        ) -> Result<uuid::Uuid, String> {
            Ok(self.inner.lock().unwrap().mint("view"))
        }
    }

    fn seed_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pidash-seed-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, body) in files {
            std::fs::write(dir.join(file), body).unwrap();
        }
        dir
    }

    fn mini_seed() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "projects.json",
                r##"[{"id": 1, "description": "", "network": 2, "cover_image": null, "logo_props": {}}]"##,
            ),
            (
                "states.json",
                r##"[{"id": 1, "project_id": 1, "name": "Backlog", "color": "#ccc", "group": "backlog", "default": true, "sequence": 15000},
                    {"id": 2, "project_id": 1, "name": "Done", "color": "#0c0", "group": "completed", "default": false, "sequence": 45000}]"##,
            ),
            (
                "labels.json",
                r##"[{"id": 1, "project_id": 1, "name": "bug", "color": "#f00", "sort_order": 65535}]"##,
            ),
            (
                "cycles.json",
                r##"[{"id": 1, "project_id": 1, "name": "C1", "timezone": "UTC", "type": "CURRENT", "sort_order": 65535},
                    {"id": 2, "project_id": 1, "name": "C2", "timezone": "UTC", "type": "UPCOMING", "sort_order": 65535}]"##,
            ),
            (
                "modules.json",
                r##"[{"id": 1, "project_id": 1, "name": "M1", "description": "", "status": "planned", "sort_order": 65535}]"##,
            ),
            (
                "issues.json",
                r##"[{"id": 1, "project_id": 1, "state_id": 1, "labels": [1], "name": "A",
                     "description_html": "<p>Hi</p>", "priority": "urgent", "sequence_id": 1, "sort_order": 1000,
                     "cycle_id": 1, "module_ids": [1]},
                    {"id": 2, "project_id": 1, "state_id": 2, "labels": [], "name": "B",
                     "description_html": "<p></p>", "priority": "none", "sequence_id": 2, "sort_order": 1000,
                     "cycle_id": null, "module_ids": null}]"##,
            ),
            (
                "views.json",
                r##"[{"id": 1, "project_id": 1, "name": "V", "description": "V", "access": 1,
                     "filters": {}, "display_filters": {}, "display_properties": {},
                     "rich_filters": {}, "sort_order": 75535}]"##,
            ),
            (
                "pages.json",
                r##"[{"id": 1, "name": "P1", "access": 0, "description_html": "<p>x</p>", "project_id": 1, "type": "PROJECT"},
                    {"id": 2, "name": "P2", "access": 0, "description_html": "<p>y</p>"}]"##,
            ),
        ]
    }

    fn input_for(dir: &Path, ws: uuid::Uuid) -> RunInput {
        RunInput {
            seed_data_dir: dir.to_owned(),
            workspace_id: ws,
            now: chrono::DateTime::from_timestamp(1_789_000_000, 0).unwrap(),
            password_hex: "00".repeat(16),
            password_salt: "s".repeat(22),
            default_agent_executor: "local_runner".to_owned(),
        }
    }

    #[tokio::test]
    async fn full_run_threads_maps_in_source_order() {
        let ws = uuid::Uuid::from_u128(900);
        let store = FakeStore::with_workspace("Acme Workspace");
        let dir = seed_dir("full", &mini_seed());
        let report = run(&store, &input_for(&dir, ws))
            .await
            .expect("mini seed runs");

        assert_eq!(
            report,
            SeedReport {
                projects: 1,
                states: 2,
                labels: 1,
                cycles: 2,
                modules: 1,
                issues: 2,
                views: 1,
                pages: 2,
                project_pages: 1,
                warnings: vec![],
            }
        );

        // Insert order mirrors `:543-564`: bot, membership, project (+0
        // members here), states, labels, cycles, modules, issues (each with
        // the double sequence rows + activity + links), views, pages.
        let events = store.events();
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| e.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "bot",
                "ws_member",
                "project",
                "state",
                "state",
                "label",
                "cycle",
                "cycle",
                "module",
                "issue",
                "save_sequence",
                "issue_sequence",
                "activity",
                "issue_label",
                "cycle_issue",
                "module_issue",
                "issue",
                "save_sequence",
                "issue_sequence",
                "activity",
                "view",
                // `ProjectPage` links inline per page (`:378-387` inside
                // the loop), so P1's link precedes P2's row.
                "page",
                "project_page",
                "page",
            ]
        );

        // Id-mapping is threaded: the project id minted first is the one
        // every later creator references (fake asserts shapes; the minted
        // sequence pins the threading).
        let minted = store.minted();
        assert_eq!(minted[0], uuid::Uuid::from_u128(1)); // bot
        assert_eq!(minted[2], uuid::Uuid::from_u128(3)); // project
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn missing_workspace_aborts_like_get() {
        let store = FakeStore {
            inner: Mutex::new(FakeInner::default()),
        };
        let dir = seed_dir("nows", &mini_seed());
        let err = run(&store, &input_for(&dir, uuid::Uuid::from_u128(1)))
            .await
            .expect_err("no workspace must fail");
        assert!(
            err.contains("Workspace matching query does not exist"),
            "{err}"
        );
        assert!(store.events().is_empty(), "nothing written before the read");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn unknown_cycle_type_is_name_error() {
        let ws = uuid::Uuid::from_u128(901);
        let store = FakeStore::with_workspace("Acme");
        let dir = seed_dir(
            "badcycle",
            &[
                ("projects.json", r##"[{"id": 1}]"##),
                (
                    "cycles.json",
                    r##"[{"id": 1, "project_id": 1, "name": "C", "type": "PAUSED"}]"##,
                ),
            ],
        );
        let err = run(&store, &input_for(&dir, ws))
            .await
            .expect_err("must fail");
        assert!(err.contains("NameError"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn missing_projects_cascades_to_key_error() {
        // No `projects.json` → empty map, and the states creator still
        // runs into `project_map[1]` (`KeyError`), like Python.
        let ws = uuid::Uuid::from_u128(902);
        let store = FakeStore::with_workspace("Acme");
        let dir = seed_dir(
            "noproj",
            &[(
                "states.json",
                r##"[{"id": 1, "project_id": 1, "name": "Backlog", "group": "backlog"}]"##,
            )],
        );
        let err = run(&store, &input_for(&dir, ws))
            .await
            .expect_err("must fail");
        assert!(err.contains("KeyError"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn issue_missing_field_warns_then_key_errors() {
        // Ported bug 1 end to end: the warning is recorded AND the row
        // still fails on the pop.
        let ws = uuid::Uuid::from_u128(903);
        let store = FakeStore::with_workspace("Acme");
        let dir = seed_dir(
            "badissue",
            &[
                ("projects.json", r##"[{"id": 1}]"##),
                (
                    "states.json",
                    r##"[{"id": 1, "project_id": 1, "name": "Backlog", "group": "backlog"}]"##,
                ),
                (
                    "issues.json",
                    r##"[{"id": 1, "project_id": 1, "state_id": 1, "name": "A",
                         "cycle_id": null, "module_ids": null}]"##,
                ),
            ],
        );
        let err = run(&store, &input_for(&dir, ws))
            .await
            .expect_err("must fail");
        assert!(err.contains("KeyError") && err.contains("labels"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn advisory_key_matches_python() {
        // Golden from `hashlib.sha256` (`utils/uuid.py:19-26`).
        let id = uuid::Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        assert_eq!(advisory_lock_key(id), 8400349069047396436);
    }

    #[test]
    fn executor_default_parses_like_django_settings() {
        assert_eq!(default_agent_executor(), "local_runner");
    }

    #[test]
    fn delay_message_carries_wire_name_and_workspace_arg() {
        let ws = uuid::Uuid::from_u128(42);
        let message = delay_message(ws);
        let debug = format!("{message:?}");
        assert!(debug.contains(TASK_NAME), "{debug}");
        assert!(debug.contains(&ws.to_string()), "{debug}");
        assert_eq!(
            TASK_NAME,
            "pi_dash.bgtasks.workspace_seed_task.workspace_seed"
        );
    }
}
