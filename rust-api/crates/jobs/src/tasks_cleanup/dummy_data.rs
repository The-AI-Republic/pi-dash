//! `create_dummy_data` background task (D-09, stage 5).
//!
//! Port of `apps/api/pi_dash/bgtasks/dummy_data_task.py:1-553` (the whole
//! file, one topological closure). Counts, sample sizes, the state literal
//! and the fifteen-step order live in the services planning kernel
//! ([`plan`]); this module owns drawing (OS-seeded for `random`, seed-0
//! for the `Faker.seed(0)` sections), fake strings, SQL execution and the
//! worker [`Registry`][crate::worker::Registry] wiring.
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * One statement per row, autocommit, in Python call order — the task
//!   has no `atomic()` block, so there is no transaction here either.
//! * UUID primary keys are generated client-side before insert, as Django
//!   does, so link rows reference the same ids on both backends.
//! * `bulk_create(..., ignore_conflicts=True)` is `ON CONFLICT DO NOTHING`;
//!   the batch sizes (1000/100) only packetize and are not ported.
//! * `ProjectBaseModel.save()` derives `workspace` from the project
//!   (`project.py:309-311`): the intake row carries the project's
//!   workspace, exactly as `get_or_create` persists it.
//! * Fake strings (names, colors, texts, dates) come from a local
//!   generator, not Python Faker: the fixture records counts, seeds,
//!   ranges and row shapes only, never fake values.
//! * Ported bugs, all kept: the `cycle_count + 1` off-by-one, the
//!   `create_issue_parent` no-write, the empty-cycle `randint` failure,
//!   unseeded `random` (OS-seeded here), and the empty/oversized-sample
//!   failures. A handler error requeues within the Celery retry budget,
//!   then parks the row as failed.
//!
//! [`plan`]: pidash_services::tasks_cleanup::dummy_data

use std::collections::HashSet;
use std::sync::Arc;

use chrono::{Datelike, NaiveDate};
use pidash_db::entity::{
    cycle_issues, cycles, intake_issues, intakes, issue_activities, issue_labels, issue_sequences,
    issues, labels, module_issues, modules, page_labels, pages, project_members, project_pages,
    project_user_properties, projects, states, users, workspaces,
};
use pidash_db::RequestContext;
use pidash_services::app_project::ser_project::next_work_item_sequence;
use pidash_services::tasks_cleanup::dummy_data as plan;
use pidash_types::{UserId, WorkspaceId};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, Set};
use serde_json::Map;

use crate::queue::JobRow;
use crate::task_message::TaskMessage;
use crate::worker::{Handler, Registry, Verdict};

/// Celery wire name, exactly as `.delay()` calls it.
pub const TASK_NAME: &str = plan::TASK_NAME;

/// Error text mirroring Django's `DoesNotExist` for the two lookups.
const NO_WORKSPACE: &str = "Workspace matching query does not exist.";
const NO_USER: &str = "User matching query does not exist.";

/// The `.delay()` equivalent: a first-attempt Celery protocol v2 message
/// with the eight positional args in signature order.
pub fn delay_message(args: &plan::CreateDummyDataArgs) -> TaskMessage {
    TaskMessage::new(TASK_NAME, args.wire_args(), Map::new())
}

/// Install the worker handler owning [`TASK_NAME`]. Malformed payloads fail
/// permanently; execution failures requeue within the retry budget.
pub fn register(registry: &mut Registry, pool: sqlx::PgPool) {
    let handler: Handler = Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            let args = match job_args(&job) {
                Ok(args) => args,
                Err(error) => return Ok(Verdict::Fail { error }),
            };
            match PgDummyData::new(pool).run(&args).await {
                Ok(()) => Ok(Verdict::Ack),
                Err(error) => Err(error),
            }
        })
    });
    registry.register(TASK_NAME, handler);
}

/// Parse the positional Celery args array off a claimed row.
fn job_args(job: &JobRow) -> Result<plan::CreateDummyDataArgs, String> {
    match &job.args {
        serde_json::Value::Array(args) => plan::CreateDummyDataArgs::from_job_args(args),
        _ => Err("create_dummy_data args must be a JSON array".to_owned()),
    }
}

/// Inclusive `random.randint(lo, hi)` over the unseeded RNG.
fn randint(rng: &mut StdRng, lo: u32, hi: u32) -> u32 {
    rng.random_range(lo..=hi)
}

/// `random.sample(population, k)` as index positions; oversized or negative
/// requests raise in Python, so they fail here too.
fn sample_positions(rng: &mut StdRng, population: usize, k: usize) -> Result<Vec<usize>, String> {
    if k > population {
        return Err(format!(
            "sample larger than population ({k} > {population})"
        ));
    }
    let mut positions: Vec<usize> = (0..population).collect();
    positions.shuffle(rng);
    positions.truncate(k);
    Ok(positions)
}

/// Fresh `Faker.seed(0)` section: deterministic within the run, mirroring
/// the re-seed before labels/cycles/modules/pages/issues (`:127,:147,`
/// `:193,:223,:269`). Values never match Python Faker — only the seeding
/// structure is ported.
fn seeded_fake() -> StdRng {
    StdRng::seed_from_u64(0)
}

const FIRST_NAMES: [&str; 20] = [
    "Ava", "Liam", "Maya", "Noah", "Zoe", "Ethan", "Ruby", "Owen", "Ivy", "Lucas", "Nina", "Kai",
    "Elsa", "Finn", "Tara", "Hugo", "Lena", "Milo", "Sara", "Theo",
];
const LAST_NAMES: [&str; 20] = [
    "Sharma", "Garcia", "Kim", "Novak", "Silva", "Haddad", "Berg", "Costa", "Weber", "Ali",
    "Tanaka", "Muller", "Rossi", "Dubois", "Khan", "Larsen", "Moreau", "Petrov", "Sato", "Lund",
];
const COLOR_NAMES: [&str; 20] = [
    "red", "teal", "amber", "indigo", "coral", "slate", "olive", "mauve", "azure", "ochre", "plum",
    "jade", "rust", "sage", "cobalt", "sand", "wine", "mint", "clay", "steel",
];
const WORDS: [&str; 48] = [
    "alpha", "bravo", "charter", "delta", "launch", "orbit", "pixel", "quarter", "relay", "signal",
    "tango", "urban", "vector", "window", "yellow", "zephyr", "anchor", "bridge", "canyon",
    "drift", "ember", "forest", "glacier", "harbor", "island", "jungle", "kernel", "lagoon",
    "meadow", "north", "ocean", "prairie", "quarry", "ridge", "summit", "trail", "umbra", "valley",
    "willow", "xenon", "yonder", "zeal", "bloom", "crest", "dune", "epoch", "flint", "grove",
];

/// `fake.name()`: two drawn words, like `"Maya Novak"`.
fn fake_name(rng: &mut StdRng) -> String {
    format!(
        "{} {}",
        FIRST_NAMES[rng.random_range(0..FIRST_NAMES.len())],
        LAST_NAMES[rng.random_range(0..LAST_NAMES.len())]
    )
}

/// `fake.color_name()`.
fn fake_color_name(rng: &mut StdRng) -> String {
    COLOR_NAMES[rng.random_range(0..COLOR_NAMES.len())].to_owned()
}

/// `fake.hex_color()`: lowercase `#rrggbb`.
fn fake_hex_color(rng: &mut StdRng) -> String {
    format!("#{:06x}", rng.random_range(0..0x100_0000u32))
}

/// `fake.text(max_nb_chars)`: words joined to at most `max_chars` chars.
fn fake_text(rng: &mut StdRng, max_chars: usize) -> String {
    let mut out = String::new();
    while out.len() < max_chars {
        let word = WORDS[rng.random_range(0..WORDS.len())];
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.chars().take(max_chars).collect()
}

/// `fake.date_this_year()`: a uniform day from Jan 1 to today.
fn fake_date_this_year(rng: &mut StdRng, today: NaiveDate) -> NaiveDate {
    let start = NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("Jan 1 exists");
    let span = today.signed_duration_since(start).num_days().max(0);
    start + chrono::Duration::days(rng.random_range(0..=span))
}

/// `fake.date_between_dates(date_start, date_end)`, inclusive both ends.
fn fake_date_between(rng: &mut StdRng, start: NaiveDate, end: NaiveDate) -> NaiveDate {
    let span = end.signed_duration_since(start).num_days().max(0);
    start + chrono::Duration::days(rng.random_range(0..=span))
}

/// Last day of the current year, the `date_end` of the cycle/module windows.
fn year_end(today: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(today.year(), 12, 31).expect("Dec 31 exists")
}

/// Midnight UTC for the `timestamptz` cycle/intake columns: Django coerces
/// the Faker `date` to midnight on write.
fn midnight(date: NaiveDate) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_naive_utc_and_offset(
        date.and_hms_opt(0, 0, 0).expect("midnight exists"),
        chrono::Utc,
    )
}

type DbResult<T> = Result<T, String>;

fn db_error(error: sqlx::Error) -> String {
    error.to_string()
}

fn sea_err(error: sea_orm::DbErr) -> String {
    error.to_string()
}

fn do_nothing() -> OnConflict {
    OnConflict::new().do_nothing().to_owned()
}

/// The task carries ids as `uuid::text` strings (its own statements cast
/// them back); the domain writers take `Uuid`.
fn parse_uuid(id: &str) -> DbResult<uuid::Uuid> {
    uuid::Uuid::parse_str(id).map_err(|error| error.to_string())
}

/// `get_default_agent_executor` (`core/agent_execution.py:48-55`, #2903):
/// always `local_runner`; the setting is ignored (reported by the
/// `managed.W001` system check, not refused).
fn default_agent_executor() -> String {
    "local_runner".to_owned()
}

/// Postgres executor for the dummy-data task.
pub struct PgDummyData {
    pool: sqlx::PgPool,
}

impl PgDummyData {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    fn db(&self) -> sea_orm::DatabaseConnection {
        pidash_db::pool::sea_orm_connection(&self.pool)
    }

    /// The fifteen steps of `create_dummy_data` (`:488-553`) with one
    /// OS-seeded RNG threaded through every unseeded `random.*` draw, as
    /// the shared module state is in Python.
    pub async fn run(&self, args: &plan::CreateDummyDataArgs) -> Result<(), String> {
        let mut rng = StdRng::from_os_rng();
        let (workspace_id, user_id) = self.resolve_workspace_user(&args.slug, &args.email).await?;
        // Explicit request context on every write below, per the Porting
        // guide: the tenant is the resolved workspace, the actor the
        // resolved user.
        let ctx = RequestContext::new(
            WorkspaceId::from(workspace_id.as_str()),
            Some(UserId::from(user_id.as_str())),
        );
        let project_id = self
            .create_project(&ctx, &workspace_id, &user_id, &mut rng)
            .await?;
        self.create_project_members(&ctx, &project_id, &workspace_id, &args.members, &mut rng)
            .await?;
        self.create_states(&ctx, &project_id, &workspace_id, &user_id)
            .await?;
        self.create_labels(&ctx, &project_id, &workspace_id, &user_id, &mut rng)
            .await?;
        self.create_cycles(
            &ctx,
            &project_id,
            &workspace_id,
            &user_id,
            args.cycle_count,
            &mut rng,
        )
        .await?;
        self.create_modules(
            &ctx,
            &project_id,
            &workspace_id,
            args.module_count,
            &mut rng,
        )
        .await?;
        self.create_pages(
            &ctx,
            &project_id,
            &workspace_id,
            &user_id,
            args.pages_count,
            &mut rng,
        )
        .await?;
        self.create_page_labels(&ctx, &project_id, &workspace_id, args.pages_count, &mut rng)
            .await?;
        self.create_issues(
            &ctx,
            &project_id,
            &workspace_id,
            &user_id,
            args.issue_count,
            &mut rng,
        )
        .await?;
        self.create_intake_issues(
            &ctx,
            &project_id,
            &workspace_id,
            &user_id,
            args.intake_issue_count,
            &mut rng,
        )
        .await?;
        plan::parent_plan(args.issue_count)?;
        // create_issue_parent persists nothing (ported bug): no SQL here.
        self.create_issue_assignees(&ctx, &project_id, &workspace_id, args.issue_count, &mut rng)
            .await?;
        self.create_issue_labels(&ctx, &project_id, &workspace_id, &mut rng)
            .await?;
        self.create_cycle_issues(&ctx, &project_id, &workspace_id, args.issue_count, &mut rng)
            .await?;
        self.create_module_issues(&ctx, &project_id, &workspace_id, &mut rng)
            .await?;
        Ok(())
    }
}

impl PgDummyData {
    /// `Workspace.objects.get(slug=...)` + `User.objects.get(email=...)`
    /// (`:499-502`); missing rows raise, as `get()` does.
    async fn resolve_workspace_user(&self, slug: &str, email: &str) -> DbResult<(String, String)> {
        let db = self.db();
        let workspace_id: Option<uuid::Uuid> = workspaces::Entity::find()
            .filter(workspaces::Column::Slug.eq(slug.to_owned()))
            .filter(workspaces::Column::DeletedAt.is_null())
            .select_only()
            .columns([workspaces::Column::Id])
            .into_tuple()
            .one(&db)
            .await
            .map_err(sea_err)?;
        let user_id: Option<uuid::Uuid> = users::Entity::find()
            .filter(users::Column::Email.eq(email.to_owned()))
            .select_only()
            .columns([users::Column::Id])
            .into_tuple()
            .one(&db)
            .await
            .map_err(sea_err)?;
        match (workspace_id, user_id) {
            (Some(workspace_id), Some(user_id)) => {
                Ok((workspace_id.to_string(), user_id.to_string()))
            }
            (None, _) => Err(NO_WORKSPACE.to_owned()),
            (_, None) => Err(NO_USER.to_owned()),
        }
    }

    /// `create_project` (`:44-60`): the creator call passes only
    /// project/member/role — `save()` derives the workspace, fills
    /// the model defaults, and nulls the audit user (no request
    /// user in a task); it also inherits the workspace timezone
    /// and defaults the first project.
    async fn create_project(
        &self,
        _ctx: &RequestContext,
        workspace_id: &str,
        user_id: &str,
        rng: &mut StdRng,
    ) -> DbResult<String> {
        let mut fake = StdRng::from_os_rng();
        let name = fake_name(&mut fake);
        let suffix = uuid::Uuid::new_v4().simple().to_string()[..5].to_owned();
        let full_name = format!("{name}_{suffix}");
        let name_chars = name.chars().count();
        let (lo, hi) = plan::identifier_bounds(name_chars).map_err(|_| {
            format!(
                "randint(2, {}) with empty range",
                name_chars.saturating_sub(1)
            )
        })?;
        let keep = randint(rng, lo, hi) as usize;
        let identifier: String = name
            .chars()
            .take(keep)
            .collect::<String>()
            .trim()
            .to_uppercase();
        // `Project.save()` on create: the timezone inherits the
        // workspace's (the task never passes one), and the first
        // live project in the workspace becomes the default (the
        // demote-others update then matches zero rows).
        let workspace_uuid = parse_uuid(workspace_id)?;
        let db = self.db();
        let timezone: String = workspaces::Entity::find_by_id(workspace_uuid)
            .select_only()
            .columns([workspaces::Column::Timezone])
            .into_tuple()
            .one(&db)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| NO_WORKSPACE.to_owned())?;
        let has_default: bool = projects::Entity::find()
            .filter(projects::Column::WorkspaceId.eq(workspace_uuid))
            .filter(projects::Column::IsDefault.eq(true))
            .filter(projects::Column::DeletedAt.is_null())
            .select_only()
            .column(projects::Column::Id)
            .limit(1)
            .into_tuple::<uuid::Uuid>()
            .one(&db)
            .await
            .map_err(sea_err)?
            .is_some();
        use pidash_services::domain::projects::{insert_demo_project, DemoProject};
        let now = chrono::Utc::now();
        let default_executor = default_agent_executor();
        let project_uuid = insert_demo_project(
            &db,
            &DemoProject {
                workspace_id: workspace_uuid,
                name: &full_name,
                identifier: &identifier,
                is_default: !has_default,
                timezone: &timezone,
                default_agent_executor: &default_executor,
                now,
            },
        )
        .await
        .map_err(db_error)?;
        let project_id = project_uuid.to_string();
        // `create_default_pod_for_new_project` (`runner/signals.py:31-66`):
        // the `post_save` handler fires during `Project.objects.create`,
        // before the member rows below. Best-effort + idempotent — never
        // fails the task. Lead/assignee are `None` on this path (the task
        // passes neither), so `created_by` stamps `NULL`, like Django.
        pidash_services::runner_runs::signals::create_default_pod_for_new_project(
            &self.pool,
            &pidash_services::runner_runs::signals::NewProject {
                id: project_uuid,
                workspace_id: workspace_uuid,
                identifier: identifier.clone(),
                created: true,
                project_lead: None,
                default_assignee: None,
            },
        )
        .await;
        // `ProjectMember.objects.create` runs the `save()` hook: the
        // property row persists BEFORE the member row, and `save()`
        // nulls the audit user on both.
        let member_props = pidash_db::ops::users::project_member_props_json();
        let member_prefs = pidash_db::ops::users::project_preferences_json();
        let user_uuid = parse_uuid(user_id)?;
        let min_sort: Option<f64> = project_user_properties::Entity::find()
            .filter(project_user_properties::Column::WorkspaceId.eq(workspace_uuid))
            .filter(project_user_properties::Column::UserId.eq(user_uuid))
            .filter(project_user_properties::Column::DeletedAt.is_null())
            .select_only()
            .column_as(
                Expr::col(project_user_properties::Column::SortOrder).min(),
                "min_sort",
            )
            .into_tuple::<Option<f64>>()
            .one(&db)
            .await
            .map_err(sea_err)?
            .flatten();
        project_user_properties::Entity::insert(project_user_properties::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            workspace_id: Set(workspace_uuid),
            project_id: Set(project_uuid),
            user_id: Set(user_uuid),
            filters: Set(pidash_db::ops::users::property_filters_json()),
            display_filters: Set(pidash_db::ops::users::property_display_filters_json()),
            display_properties: Set(pidash_db::ops::users::property_display_properties_json()),
            rich_filters: Set(serde_json::json!({})),
            preferences: Set(member_prefs.clone()),
            sort_order: Set(min_sort.map(|min| min - 10000.0).unwrap_or(65535.0)),
            deleted_at: Set(None),
            created_by_id: Set(None),
            updated_by_id: Set(None),
        })
        .exec_without_returning(&db)
        .await
        .map_err(sea_err)?;
        use pidash_services::domain::projects::members::{add_demo, DemoProjectMember};
        add_demo(
            &db,
            &DemoProjectMember {
                project_id: project_uuid,
                workspace_id: workspace_uuid,
                member_id: user_uuid,
                role: plan::PROJECT_MEMBER_ROLE,
                sort_order: 65535.0,
                view_props: &member_props,
                preferences: &member_prefs,
                now,
                ignore_conflicts: false,
            },
        )
        .await
        .map_err(db_error)?;
        Ok(project_id)
    }

    /// `create_project_members` (`:63-79`): role 20, random sort, conflicts
    /// ignored.
    async fn create_project_members(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        members: &[String],
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let db = self.db();
        let user_ids: Vec<uuid::Uuid> = users::Entity::find()
            .filter(users::Column::Email.is_in(members.to_vec()))
            .select_only()
            .columns([users::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        let now = chrono::Utc::now();
        let member_props = pidash_db::ops::users::project_member_props_json();
        let member_prefs = pidash_db::ops::users::project_preferences_json();
        use pidash_services::domain::projects::members::{add_demo, DemoProjectMember};
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        for member_id in &user_ids {
            let sort_order = f64::from(randint(rng, 0, 65535));
            add_demo(
                &db,
                &DemoProjectMember {
                    project_id: project_uuid,
                    workspace_id: workspace_uuid,
                    member_id: *member_id,
                    role: plan::PROJECT_MEMBER_ROLE,
                    sort_order,
                    view_props: &member_props,
                    preferences: &member_prefs,
                    now,
                    ignore_conflicts: true,
                },
            )
            .await
            .map_err(db_error)?;
        }
        Ok(())
    }

    /// `create_states` (`:82-123`): the five-row literal, no conflict clause.
    async fn create_states(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        user_id: &str,
    ) -> DbResult<()> {
        use pidash_services::domain::projects::states::{add_demo, DemoState};
        let now = chrono::Utc::now();
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let created_by = parse_uuid(user_id)?;
        let db = self.db();
        for state in plan::DEFAULT_STATES {
            add_demo(
                &db,
                &DemoState {
                    project_id: project_uuid,
                    workspace_id: workspace_uuid,
                    created_by,
                    name: state.name,
                    color: state.color,
                    sequence: state.sequence,
                    group: state.group,
                    default: state.default,
                    now,
                },
            )
            .await
            .map_err(db_error)?;
        }
        Ok(())
    }

    /// `create_labels` (`:126-143`): seeded Faker, 50 rows, conflicts ignored.
    async fn create_labels(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        user_id: &str,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let mut fake = seeded_fake();
        let now = chrono::Utc::now();
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let user_uuid = parse_uuid(user_id)?;
        let db = self.db();
        for _ in 0..plan::LABEL_ROWS {
            // Draw order (sort, name, color) matches the old bind order.
            let sort_order = f64::from(randint(rng, 0, 65535));
            let name = fake_color_name(&mut fake);
            let color = fake_hex_color(&mut fake);
            labels::Entity::insert(labels::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                name: Set(name),
                color: Set(color),
                project_id: Set(Some(project_uuid)),
                workspace_id: Set(workspace_uuid),
                created_by_id: Set(Some(user_uuid)),
                sort_order: Set(sort_order),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                description: Set(String::new()),
                deleted_at: Set(None),
                updated_by_id: Set(None),
                parent_id: Set(None),
                external_id: Set(None),
                external_source: Set(None),
            })
            .on_conflict(do_nothing())
            .exec_without_returning(&db)
            .await
            .map_err(sea_err)?;
        }
        Ok(())
    }

    /// `create_cycles` (`:146-188`): the `<=` loop keeps the off-by-one —
    /// `cycle_count + 1` rows. Regenerated ends ignore `date_start`, as in
    /// Python (`:174` calls `date_this_year()` unconditionally).
    async fn create_cycles(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        user_id: &str,
        cycle_count: i64,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let mut fake = seeded_fake();
        let today = chrono::Utc::now().date_naive();
        let mut used: HashSet<(Option<NaiveDate>, Option<NaiveDate>)> = HashSet::new();
        let mut rows: Vec<(String, Option<NaiveDate>, Option<NaiveDate>, f64)> = Vec::new();
        while (rows.len() as i64) <= cycle_count {
            let start = if randint(rng, 0, 1) == 0 {
                None
            } else {
                Some(fake_date_this_year(&mut fake, today))
            };
            let mut end = start.map(|day| fake_date_between(&mut fake, day, year_end(today)));
            while start.is_some() && (end <= start || used.contains(&(start, end))) {
                end = Some(fake_date_this_year(&mut fake, today));
            }
            if start.is_some() && end.is_some() {
                used.insert((start, end));
            }
            rows.push((
                fake_name(&mut fake),
                start,
                end,
                f64::from(randint(rng, 0, 65535)),
            ));
        }
        let now = chrono::Utc::now();
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let user_uuid = parse_uuid(user_id)?;
        let db = self.db();
        for (name, start, end, sort_order) in &rows {
            cycles::Entity::insert(cycles::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                name: Set(name.clone()),
                owned_by_id: Set(user_uuid),
                sort_order: Set(*sort_order),
                start_date: Set(start.map(|day| midnight(day).into())),
                end_date: Set(end.map(|day| midnight(day).into())),
                project_id: Set(project_uuid),
                workspace_id: Set(workspace_uuid),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                description: Set(String::new()),
                view_props: Set(serde_json::json!({})),
                progress_snapshot: Set(serde_json::json!({})),
                logo_props: Set(serde_json::json!({})),
                timezone: Set("UTC".to_owned()),
                version: Set(1),
                created_by_id: Set(None),
                updated_by_id: Set(None),
                external_id: Set(None),
                external_source: Set(None),
                archived_at: Set(None),
                deleted_at: Set(None),
            })
            .on_conflict(do_nothing())
            .exec_without_returning(&db)
            .await
            .map_err(sea_err)?;
        }
        Ok(())
    }

    /// `create_modules` (`:191-218`): `range(module_count)` rows.
    async fn create_modules(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        module_count: i64,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let mut fake = seeded_fake();
        let today = chrono::Utc::now().date_naive();
        let now = chrono::Utc::now();
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let db = self.db();
        for _ in 0..plan::module_row_count(module_count) {
            let start = if randint(rng, 0, 1) == 0 {
                None
            } else {
                Some(fake_date_this_year(&mut fake, today))
            };
            let end = start.map(|day| fake_date_between(&mut fake, day, year_end(today)));
            // Draw order (name, sort) matches the old bind order.
            let name = fake_name(&mut fake);
            let sort_order = f64::from(randint(rng, 0, 65535));
            modules::Entity::insert(modules::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                name: Set(name),
                sort_order: Set(sort_order),
                start_date: Set(start),
                target_date: Set(end),
                project_id: Set(project_uuid),
                workspace_id: Set(workspace_uuid),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                description: Set(String::new()),
                status: Set("planned".to_owned()),
                view_props: Set(serde_json::json!({})),
                logo_props: Set(serde_json::json!({})),
                description_text: Set(None),
                description_html: Set(None),
                created_by_id: Set(None),
                lead_id: Set(None),
                updated_by_id: Set(None),
                external_id: Set(None),
                external_source: Set(None),
                archived_at: Set(None),
                deleted_at: Set(None),
            })
            .on_conflict(do_nothing())
            .exec_without_returning(&db)
            .await
            .map_err(sea_err)?;
        }
        Ok(())
    }

    /// `create_pages` (`:221-247`): pages plus one `ProjectPage` link each —
    /// the link insert has no conflict clause, as in Python.
    async fn create_pages(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        user_id: &str,
        pages_count: i64,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let mut fake = seeded_fake();
        let now = chrono::Utc::now();
        let workspace_uuid = parse_uuid(workspace_id)?;
        let user_uuid = parse_uuid(user_id)?;
        let project_uuid = parse_uuid(project_id)?;
        let db = self.db();
        for _ in 0..plan::page_row_count(pages_count) {
            let text = fake_text(&mut fake, 60000);
            let page_uuid = uuid::Uuid::new_v4();
            // Draw order (name, access, color) matches the old bind order.
            let name = fake_name(&mut fake);
            let access = randint(rng, 0, 1) as i16;
            let color = fake_hex_color(&mut fake);
            pages::Entity::insert(pages::ActiveModel {
                id: Set(page_uuid),
                name: Set(name),
                workspace_id: Set(workspace_uuid),
                owned_by_id: Set(user_uuid),
                access: Set(access),
                color: Set(color),
                description_html: Set(format!("<p>{text}</p>")),
                is_locked: Set(false),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                description_json: Set(serde_json::json!({})),
                view_props: Set(serde_json::json!({"full_width": false})),
                logo_props: Set(serde_json::json!({})),
                is_global: Set(false),
                sort_order: Set(65535.0),
                description_stripped: Set(None),
                created_by_id: Set(None),
                updated_by_id: Set(None),
                archived_at: Set(None),
                parent_id: Set(None),
                description_binary: Set(None),
                deleted_at: Set(None),
                moved_to_page: Set(None),
                moved_to_project: Set(None),
                external_id: Set(None),
                external_source: Set(None),
            })
            .on_conflict(do_nothing())
            .exec_without_returning(&db)
            .await
            .map_err(sea_err)?;
            project_pages::Entity::insert(project_pages::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                page_id: Set(page_uuid),
                project_id: Set(project_uuid),
                workspace_id: Set(workspace_uuid),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                created_by_id: Set(None),
                updated_by_id: Set(None),
                deleted_at: Set(None),
            })
            .exec_without_returning(&db)
            .await
            .map_err(sea_err)?;
        }
        Ok(())
    }

    /// `create_page_labels` (`:249-264`): `int(pages_count / 2)` sampled
    /// pages, each drawing at most `len(labels) - 1` labels — never all.
    async fn create_page_labels(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        pages_count: i64,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let db = self.db();
        let label_ids: Vec<uuid::Uuid> = labels::Entity::find()
            .filter(labels::Column::ProjectId.eq(project_uuid))
            .filter(labels::Column::DeletedAt.is_null())
            .select_only()
            .columns([labels::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        // The pages-for-project JOIN as two steps (no relation edge
        // guaranteed): linked rows preserve duplicate links exactly.
        let linked: Vec<uuid::Uuid> = project_pages::Entity::find()
            .filter(project_pages::Column::ProjectId.eq(project_uuid))
            .select_only()
            .columns([project_pages::Column::PageId])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        let live: HashSet<uuid::Uuid> = if linked.is_empty() {
            HashSet::new()
        } else {
            pages::Entity::find()
                .filter(pages::Column::Id.is_in(linked.clone()))
                .filter(pages::Column::DeletedAt.is_null())
                .select_only()
                .columns([pages::Column::Id])
                .into_tuple()
                .all(&db)
                .await
                .map_err(sea_err)?
                .into_iter()
                .collect()
        };
        let page_ids: Vec<uuid::Uuid> = linked.into_iter().filter(|id| live.contains(id)).collect();
        let take = plan::half_sample_size(pages_count)?;
        let max_draw = plan::per_row_draw_max(label_ids.len())?;
        let now = chrono::Utc::now();
        for position in sample_positions(rng, page_ids.len(), take)? {
            let draw = randint(rng, 0, max_draw as u32) as usize;
            for label_position in sample_positions(rng, label_ids.len(), draw)? {
                page_labels::Entity::insert(page_labels::ActiveModel {
                    id: Set(uuid::Uuid::new_v4()),
                    page_id: Set(page_ids[position]),
                    label_id: Set(label_ids[label_position]),
                    workspace_id: Set(workspace_uuid),
                    created_at: Set(now.into()),
                    updated_at: Set(now.into()),
                    created_by_id: Set(None),
                    updated_by_id: Set(None),
                    deleted_at: Set(None),
                })
                .on_conflict(do_nothing())
                .exec_without_returning(&db)
                .await
                .map_err(sea_err)?;
            }
        }
        Ok(())
    }

    /// `create_issues` (`:267-355`): the max-sort probe draws from a random
    /// state before the loop (`:288-290`); sequences and activities follow
    /// every issue batch, as in Python.
    async fn create_issues(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        user_id: &str,
        issue_count: i64,
        rng: &mut StdRng,
    ) -> DbResult<Vec<String>> {
        let mut fake = seeded_fake();
        let today = chrono::Utc::now().date_naive();
        let workspace_uuid = parse_uuid(workspace_id)?;
        let project_uuid = parse_uuid(project_id)?;
        let user_uuid = parse_uuid(user_id)?;
        let db = self.db();
        let state_ids: Vec<uuid::Uuid> = states::Entity::find()
            .filter(states::Column::WorkspaceId.eq(workspace_uuid))
            .filter(states::Column::ProjectId.eq(project_uuid))
            .filter(states::Column::Group.ne(plan::TRIAGE_GROUP.to_owned()))
            .filter(states::Column::DeletedAt.is_null())
            .select_only()
            .columns([states::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        let creator_ids: Vec<uuid::Uuid> = project_members::Entity::find()
            .filter(project_members::Column::WorkspaceId.eq(workspace_uuid))
            .filter(project_members::Column::ProjectId.eq(project_uuid))
            .filter(project_members::Column::DeletedAt.is_null())
            .select_only()
            .columns([project_members::Column::MemberId])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        if state_ids.is_empty() || creator_ids.is_empty() {
            return Err("randint(0, -1) on an empty state or creator pool".to_owned());
        }
        let max_sequence: Option<i64> = issue_sequences::Entity::find()
            .filter(issue_sequences::Column::ProjectId.eq(project_uuid))
            .select_only()
            .column_as(
                Expr::col(issue_sequences::Column::Sequence).max(),
                "max_sequence",
            )
            .into_tuple::<Option<i64>>()
            .one(&db)
            .await
            .map_err(sea_err)?
            .flatten();
        let mut last_id = next_work_item_sequence(max_sequence);
        let probe_state = state_ids[randint(rng, 0, (state_ids.len() - 1) as u32) as usize];
        let max_sort: Option<f64> = issues::Entity::find()
            .filter(issues::Column::ProjectId.eq(project_uuid))
            .filter(issues::Column::StateId.eq(probe_state))
            .select_only()
            .column_as(Expr::col(issues::Column::SortOrder).max(), "max_sort")
            .into_tuple::<Option<f64>>()
            .one(&db)
            .await
            .map_err(sea_err)?
            .flatten();
        use pidash_services::domain::issues::{
            insert_demo_issue, insert_demo_issue_sequence, DemoIssue, DemoIssueSequence,
        };
        let mut sort_order = plan::initial_sort_order(max_sort);
        let now = chrono::Utc::now();
        let mut issue_ids = Vec::new();
        for _ in 0..plan::issue_row_count(issue_count) {
            let start = if randint(rng, 0, 1) == 0 {
                None
            } else {
                Some(fake_date_this_year(&mut fake, today))
            };
            let end = start.map(|day| fake_date_between(&mut fake, day, year_end(today)));
            let text = fake_text(&mut fake, 3000);
            let description_html = format!("<p>{text}</p>");
            // Draw order (state, priority, creator) matches the old
            // bind order.
            let state_id = state_ids[randint(rng, 0, (state_ids.len() - 1) as u32) as usize];
            let priority = plan::PRIORITIES[randint(rng, 0, 4) as usize];
            let creator_id = creator_ids[randint(rng, 0, (creator_ids.len() - 1) as u32) as usize];
            let state_str = state_id.to_string();
            let creator_str = creator_id.to_string();
            let issue_id = insert_demo_issue(
                &db,
                &DemoIssue {
                    state_id: &state_str,
                    project_id,
                    workspace_id,
                    created_by: &creator_str,
                    name: plan::truncate_code_points(&text, 254),
                    description_html: &description_html,
                    description_stripped: &text,
                    sequence_id: last_id as i32,
                    sort_order,
                    start_date: start,
                    target_date: end,
                    priority,
                    now,
                },
            )
            .await
            .map_err(db_error)?;
            insert_demo_issue_sequence(
                &db,
                &DemoIssueSequence {
                    issue_id: &issue_id,
                    project_id,
                    workspace_id,
                    sequence: last_id,
                    now,
                },
            )
            .await
            .map_err(db_error)?;
            let issue_uuid = parse_uuid(&issue_id)?;
            issue_activities::Entity::insert(issue_activities::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                issue_id: Set(Some(issue_uuid)),
                actor_id: Set(Some(user_uuid)),
                project_id: Set(project_uuid),
                workspace_id: Set(workspace_uuid),
                comment: Set(plan::ISSUE_ACTIVITY_COMMENT.to_owned()),
                verb: Set(plan::ISSUE_ACTIVITY_VERB.to_owned()),
                created_by_id: Set(Some(user_uuid)),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                attachments: Set(Vec::new()),
                field: Set(None),
                old_value: Set(None),
                new_value: Set(None),
                issue_comment_id: Set(None),
                updated_by_id: Set(None),
                new_identifier: Set(None),
                old_identifier: Set(None),
                epoch: Set(None),
                deleted_at: Set(None),
            })
            .exec_without_returning(&db)
            .await
            .map_err(sea_err)?;
            sort_order =
                plan::advance_sort_order(sort_order, randint(rng, 0, plan::SORT_ADVANCE_DRAW_MAX));
            last_id += 1;
            issue_ids.push(issue_id);
        }
        Ok(issue_ids)
    }

    /// `create_intake_issues` (`:358-375`): a second `create_issues` call
    /// (Faker re-seeded inside it), then every returned issue linked to the
    /// default intake — `get_or_create` derives the workspace from the
    /// project, as `ProjectBaseModel.save()` does.
    async fn create_intake_issues(
        &self,
        ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        user_id: &str,
        intake_issue_count: i64,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let issue_ids = self
            .create_issues(
                ctx,
                project_id,
                workspace_id,
                user_id,
                intake_issue_count,
                rng,
            )
            .await?;
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let db = self.db();
        let found: Option<uuid::Uuid> = intakes::Entity::find()
            .filter(intakes::Column::ProjectId.eq(project_uuid))
            .filter(intakes::Column::Name.eq(plan::INTAKE_NAME.to_owned()))
            .filter(intakes::Column::IsDefault.eq(true))
            .filter(intakes::Column::DeletedAt.is_null())
            .select_only()
            .columns([intakes::Column::Id])
            .into_tuple()
            .one(&db)
            .await
            .map_err(sea_err)?;
        let intake_id: uuid::Uuid = match found {
            Some(intake_id) => intake_id,
            None => {
                let intake_uuid = uuid::Uuid::new_v4();
                let now = chrono::Utc::now();
                intakes::Entity::insert(intakes::ActiveModel {
                    id: Set(intake_uuid),
                    name: Set(plan::INTAKE_NAME.to_owned()),
                    project_id: Set(project_uuid),
                    workspace_id: Set(workspace_uuid),
                    is_default: Set(true),
                    created_at: Set(now.into()),
                    updated_at: Set(now.into()),
                    description: Set(String::new()),
                    view_props: Set(serde_json::json!({})),
                    logo_props: Set(serde_json::json!({})),
                    created_by_id: Set(None),
                    updated_by_id: Set(None),
                    deleted_at: Set(None),
                })
                .exec_without_returning(&db)
                .await
                .map_err(sea_err)?;
                intake_uuid
            }
        };
        let now = chrono::Utc::now();
        for issue_id in &issue_ids {
            let status = plan::INTAKE_STATUSES[randint(rng, 0, 4) as usize];
            let snoozed_till = if plan::snoozed_for_status(status) {
                Some(now + chrono::Duration::days(i64::from(randint(rng, 1, 30))))
            } else {
                None
            };
            intake_issues::Entity::insert(intake_issues::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                issue_id: Set(parse_uuid(issue_id)?),
                intake_id: Set(intake_id),
                status: Set(status),
                snoozed_till: Set(snoozed_till.map(|t| t.into())),
                source: Set(Some(plan::SOURCE_IN_APP.to_owned())),
                workspace_id: Set(workspace_uuid),
                project_id: Set(project_uuid),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                extra: Set(serde_json::json!({})),
                created_by_id: Set(None),
                updated_by_id: Set(None),
                duplicate_to_id: Set(None),
                external_id: Set(None),
                external_source: Set(None),
                deleted_at: Set(None),
                source_email: Set(None),
            })
            .exec_without_returning(&db)
            .await
            .map_err(sea_err)?;
        }
        Ok(())
    }

    /// `create_issue_assignees` (`:391-413`): `int(issue_count / 2)` sampled
    /// issues, each drawing at most `len(assignees) - 1` members.
    async fn create_issue_assignees(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        issue_count: i64,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let project_uuid = parse_uuid(project_id)?;
        let db = self.db();
        let assignee_ids: Vec<String> = project_members::Entity::find()
            .filter(project_members::Column::ProjectId.eq(project_uuid))
            .filter(project_members::Column::DeletedAt.is_null())
            .select_only()
            .columns([project_members::Column::MemberId])
            .into_tuple::<uuid::Uuid>()
            .all(&db)
            .await
            .map_err(sea_err)?
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        let issue_ids: Vec<String> = issues::Entity::find()
            .filter(issues::Column::ProjectId.eq(project_uuid))
            .filter(issues::Column::DeletedAt.is_null())
            .select_only()
            .columns([issues::Column::Id])
            .into_tuple::<uuid::Uuid>()
            .all(&db)
            .await
            .map_err(sea_err)?
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        use pidash_services::domain::issues::{insert_demo_issue_assignee, DemoIssueAssignee};
        let take = plan::half_sample_size(issue_count)?;
        let max_draw = plan::per_row_draw_max(assignee_ids.len())?;
        let now = chrono::Utc::now();
        for position in sample_positions(rng, issue_ids.len(), take)? {
            let draw = randint(rng, 0, max_draw as u32) as usize;
            for assignee_position in sample_positions(rng, assignee_ids.len(), draw)? {
                insert_demo_issue_assignee(
                    &db,
                    &DemoIssueAssignee {
                        issue_id: &issue_ids[position],
                        assignee_id: &assignee_ids[assignee_position],
                        project_id,
                        workspace_id,
                        now,
                    },
                )
                .await
                .map_err(db_error)?;
            }
        }
        Ok(())
    }

    /// `create_issue_labels` (`:416-436`): every project issue, shuffled
    /// labels, `randint(0, 5)` links each — the commented-out sampling stays
    /// out, as in Python.
    async fn create_issue_labels(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let db = self.db();
        let label_ids: Vec<uuid::Uuid> = labels::Entity::find()
            .filter(labels::Column::ProjectId.eq(project_uuid))
            .filter(labels::Column::DeletedAt.is_null())
            .select_only()
            .columns([labels::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        let issue_ids: Vec<uuid::Uuid> = issues::Entity::find()
            .filter(issues::Column::ProjectId.eq(project_uuid))
            .filter(issues::Column::DeletedAt.is_null())
            .select_only()
            .columns([issues::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        let now = chrono::Utc::now();
        for issue_id in &issue_ids {
            let mut shuffled = label_ids.clone();
            shuffled.shuffle(rng);
            let draw = randint(rng, 0, plan::MULTI_LINK_DRAW_MAX as u32) as usize;
            for label_id in sample_positions(rng, shuffled.len(), draw)? {
                issue_labels::Entity::insert(issue_labels::ActiveModel {
                    id: Set(uuid::Uuid::new_v4()),
                    issue_id: Set(*issue_id),
                    label_id: Set(shuffled[label_id]),
                    project_id: Set(project_uuid),
                    workspace_id: Set(workspace_uuid),
                    created_at: Set(now.into()),
                    updated_at: Set(now.into()),
                    created_by_id: Set(None),
                    updated_by_id: Set(None),
                    deleted_at: Set(None),
                })
                .on_conflict(do_nothing())
                .exec_without_returning(&db)
                .await
                .map_err(sea_err)?;
            }
        }
        Ok(())
    }

    /// `create_cycle_issues` (`:439-454`): one random cycle per sampled
    /// issue; no cycles raises, as `randint(0, -1)` does in Python.
    async fn create_cycle_issues(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        issue_count: i64,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let db = self.db();
        let cycle_ids: Vec<uuid::Uuid> = cycles::Entity::find()
            .filter(cycles::Column::ProjectId.eq(project_uuid))
            .filter(cycles::Column::DeletedAt.is_null())
            .select_only()
            .columns([cycles::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        if cycle_ids.is_empty() {
            return Err("randint(0, -1) with no cycles for cycle issues".to_owned());
        }
        let issue_ids: Vec<uuid::Uuid> = issues::Entity::find()
            .filter(issues::Column::ProjectId.eq(project_uuid))
            .filter(issues::Column::DeletedAt.is_null())
            .select_only()
            .columns([issues::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        let take = plan::half_sample_size(issue_count)?;
        let now = chrono::Utc::now();
        for position in sample_positions(rng, issue_ids.len(), take)? {
            let cycle = cycle_ids[randint(rng, 0, (cycle_ids.len() - 1) as u32) as usize];
            cycle_issues::Entity::insert(cycle_issues::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                cycle_id: Set(cycle),
                issue_id: Set(issue_ids[position]),
                project_id: Set(project_uuid),
                workspace_id: Set(workspace_uuid),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                created_by_id: Set(None),
                updated_by_id: Set(None),
                deleted_at: Set(None),
            })
            .on_conflict(do_nothing())
            .exec_without_returning(&db)
            .await
            .map_err(sea_err)?;
        }
        Ok(())
    }

    /// `create_module_issues` (`:457-484`): every issue, shuffled modules,
    /// `randint(0, 5)` links each; a nonzero draw on no modules raises.
    async fn create_module_issues(
        &self,
        _ctx: &RequestContext,
        project_id: &str,
        workspace_id: &str,
        rng: &mut StdRng,
    ) -> DbResult<()> {
        let project_uuid = parse_uuid(project_id)?;
        let workspace_uuid = parse_uuid(workspace_id)?;
        let db = self.db();
        let module_ids: Vec<uuid::Uuid> = modules::Entity::find()
            .filter(modules::Column::ProjectId.eq(project_uuid))
            .filter(modules::Column::DeletedAt.is_null())
            .select_only()
            .columns([modules::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        let issue_ids: Vec<uuid::Uuid> = issues::Entity::find()
            .filter(issues::Column::ProjectId.eq(project_uuid))
            .filter(issues::Column::DeletedAt.is_null())
            .select_only()
            .columns([issues::Column::Id])
            .into_tuple()
            .all(&db)
            .await
            .map_err(sea_err)?;
        let now = chrono::Utc::now();
        for issue_id in &issue_ids {
            let mut shuffled = module_ids.clone();
            shuffled.shuffle(rng);
            let draw = randint(rng, 0, plan::MULTI_LINK_DRAW_MAX as u32) as usize;
            for module_position in sample_positions(rng, shuffled.len(), draw)? {
                module_issues::Entity::insert(module_issues::ActiveModel {
                    id: Set(uuid::Uuid::new_v4()),
                    module_id: Set(shuffled[module_position]),
                    issue_id: Set(*issue_id),
                    project_id: Set(project_uuid),
                    workspace_id: Set(workspace_uuid),
                    created_at: Set(now.into()),
                    updated_at: Set(now.into()),
                    created_by_id: Set(None),
                    updated_by_id: Set(None),
                    deleted_at: Set(None),
                })
                .on_conflict(do_nothing())
                .exec_without_returning(&db)
                .await
                .map_err(sea_err)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> plan::CreateDummyDataArgs {
        plan::CreateDummyDataArgs {
            slug: "ws".to_owned(),
            email: "owner@example.com".to_owned(),
            members: vec!["a@example.com".to_owned()],
            issue_count: 4,
            cycle_count: 2,
            module_count: 2,
            pages_count: 4,
            intake_issue_count: 2,
        }
    }

    #[test]
    fn wire_name_matches_python_task() {
        assert_eq!(
            TASK_NAME,
            "pi_dash.bgtasks.dummy_data_task.create_dummy_data"
        );
    }

    #[test]
    fn default_executor_follows_validated_env() {
        // #2903: DEFAULT_AGENT_EXECUTOR is ignored; the default is
        // always local_runner.
        let saved = std::env::var("DEFAULT_AGENT_EXECUTOR").ok();
        std::env::remove_var("DEFAULT_AGENT_EXECUTOR");
        assert_eq!(default_agent_executor(), "local_runner");
        std::env::set_var("DEFAULT_AGENT_EXECUTOR", "cloud_agent");
        assert_eq!(default_agent_executor(), "local_runner");
        std::env::set_var("DEFAULT_AGENT_EXECUTOR", "bogus");
        assert_eq!(default_agent_executor(), "local_runner");
        match saved {
            Some(value) => std::env::set_var("DEFAULT_AGENT_EXECUTOR", value),
            None => std::env::remove_var("DEFAULT_AGENT_EXECUTOR"),
        }
    }

    #[test]
    fn delay_message_carries_positional_payload() {
        let message = delay_message(&args());
        assert_eq!(message.task, TASK_NAME);
        assert_eq!(message.args.len(), 8);
        assert_eq!(message.args[0], serde_json::Value::String("ws".to_owned()));
        assert_eq!(message.args[3], serde_json::Value::from(4));
        assert!(message.kwargs.is_empty());
    }

    #[test]
    fn job_args_reject_non_array_and_bad_arity() {
        let row = JobRow {
            id: 1,
            celery_id: "c".to_owned(),
            task: TASK_NAME.to_owned(),
            args: serde_json::json!({"slug": "ws"}),
            kwargs: serde_json::Value::Object(Map::new()),
            queue: "q".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: chrono::Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: chrono::Utc::now(),
            last_error: None,
        };
        assert!(job_args(&row).is_err());
        let mut bad = row;
        bad.args = serde_json::json!(["only-slug"]);
        assert!(job_args(&bad).is_err());
    }

    #[tokio::test]
    async fn registry_owns_task_after_register() {
        let mut registry = Registry::new();
        assert!(!registry.owns(TASK_NAME));
        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:5432/postgres").expect("lazy pool");
        register(&mut registry, pool);
        assert!(registry.owns(TASK_NAME));
    }

    #[test]
    fn seaorm_states_filter_excludes_triage() {
        use sea_orm::{DbBackend, QueryTrait};
        let ws = uuid::Uuid::from_u128(1);
        let proj = uuid::Uuid::from_u128(2);
        let sql = states::Entity::find()
            .filter(states::Column::WorkspaceId.eq(ws))
            .filter(states::Column::ProjectId.eq(proj))
            .filter(states::Column::Group.ne("triage".to_owned()))
            .filter(states::Column::DeletedAt.is_null())
            .select_only()
            .columns([states::Column::Id])
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("\"group\" <>"), "triage excluded: {sql}");
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(!sql.contains("MAX("), "id list, not aggregate: {sql}");
    }

    #[test]
    fn seaorm_max_sequence_shape() {
        use sea_orm::{DbBackend, QueryTrait};
        let proj = uuid::Uuid::from_u128(3);
        let sql = issue_sequences::Entity::find()
            .filter(issue_sequences::Column::ProjectId.eq(proj))
            .select_only()
            .column_as(
                Expr::col(issue_sequences::Column::Sequence).max(),
                "max_sequence",
            )
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("MAX(\"sequence\")"), "{sql}");
        assert!(sql.contains("max_sequence"), "single aliased output: {sql}");
    }

    #[test]
    fn seaorm_label_insert_has_on_conflict() {
        use sea_orm::{DbBackend, EntityTrait, QueryTrait};
        let now = chrono::Utc::now();
        let id = uuid::Uuid::from_u128(1);
        let ws = uuid::Uuid::from_u128(2);
        let proj = uuid::Uuid::from_u128(3);
        let user = uuid::Uuid::from_u128(4);
        let stmt = labels::Entity::insert(labels::ActiveModel {
            id: Set(id),
            name: Set("n".to_owned()),
            color: Set("c".to_owned()),
            project_id: Set(Some(proj)),
            workspace_id: Set(ws),
            created_by_id: Set(Some(user)),
            sort_order: Set(1.0),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            description: Set(String::new()),
            deleted_at: Set(None),
            updated_by_id: Set(None),
            parent_id: Set(None),
            external_id: Set(None),
            external_source: Set(None),
        })
        .on_conflict(do_nothing())
        .build(DbBackend::Postgres);
        let sql = stmt.to_string();
        assert!(sql.contains("INSERT INTO \"labels\""), "{sql}");
        assert!(sql.contains("DO NOTHING"), "idempotent insert: {sql}");
    }

    #[test]
    fn samplers_reject_oversized_requests_like_python() {
        let mut rng = StdRng::seed_from_u64(7);
        assert!(sample_positions(&mut rng, 2, 3).is_err());
        assert_eq!(sample_positions(&mut rng, 3, 0).expect("empty").len(), 0);
        let picked = sample_positions(&mut rng, 4, 2).expect("pick");
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn fake_strings_stay_within_shapes() {
        let mut rng = StdRng::seed_from_u64(0);
        assert!(fake_name(&mut rng).contains(' '));
        let color = fake_hex_color(&mut rng);
        assert!(color.starts_with('#') && color.len() == 7);
        let text = fake_text(&mut rng, 60);
        assert!(text.chars().count() <= 60 && !text.is_empty());
        let today = NaiveDate::from_ymd_opt(2026, 9, 28).expect("date");
        let day = fake_date_this_year(&mut rng, today);
        assert!(day <= today && day >= NaiveDate::from_ymd_opt(2026, 1, 1).expect("jan"));
        assert_eq!(fake_date_between(&mut rng, today, today), today);
    }
}
