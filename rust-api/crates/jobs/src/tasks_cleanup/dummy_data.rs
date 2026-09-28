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
use pidash_db::RequestContext;
use pidash_services::tasks_cleanup::dummy_data as plan;
use pidash_types::{UserId, WorkspaceId};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use serde_json::Map;

use crate::celery::CeleryTaskMessage;
use crate::queue::JobRow;
use crate::worker::{Handler, Registry, Verdict};

/// Celery wire name, exactly as `.delay()` calls it.
pub const TASK_NAME: &str = plan::TASK_NAME;

/// Error text mirroring Django's `DoesNotExist` for the two lookups.
const NO_WORKSPACE: &str = "Workspace matching query does not exist.";
const NO_USER: &str = "User matching query does not exist.";

/// The `.delay()` equivalent: a first-attempt Celery protocol v2 message
/// with the eight positional args in signature order.
pub fn delay_message(args: &plan::CreateDummyDataArgs) -> CeleryTaskMessage {
    CeleryTaskMessage::new(TASK_NAME, args.wire_args(), Map::new())
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

/// Postgres executor for the dummy-data task.
pub struct PgDummyData {
    pool: sqlx::PgPool,
}

impl PgDummyData {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
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
        let workspace_id: Option<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        let user_id: Option<String> =
            sqlx::query_scalar(r#"SELECT id::text FROM users WHERE email = $1"#)
                .bind(email)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;
        match (workspace_id, user_id) {
            (Some(workspace_id), Some(user_id)) => Ok((workspace_id, user_id)),
            (None, _) => Err(NO_WORKSPACE.to_owned()),
            (_, None) => Err(NO_USER.to_owned()),
        }
    }

    /// `create_project` (`:44-60`): the creator row carries only
    /// project/member/role — no workspace, no sort order.
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
        let identifier: String = name.chars().take(keep).collect::<String>().to_uppercase();
        let now = chrono::Utc::now();
        let project_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"INSERT INTO projects
                (id, workspace_id, name, identifier, created_by_id, intake_view, created_at, updated_at)
                VALUES ($1::uuid, $2::uuid, $3, $4, $5::uuid, TRUE, $6, $7)"#,
        )
        .bind(&project_id)
        .bind(workspace_id)
        .bind(&full_name)
        .bind(&identifier)
        .bind(user_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        sqlx::query(
            r#"INSERT INTO project_members (id, project_id, member_id, role, created_at, updated_at)
                VALUES ($1::uuid, $2::uuid, $3::uuid, $4, $5, $6)"#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&project_id)
        .bind(user_id)
        .bind(plan::PROJECT_MEMBER_ROLE)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
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
        let user_ids: Vec<String> =
            sqlx::query_scalar(r#"SELECT id::text FROM users WHERE email = ANY($1)"#)
                .bind(members.to_vec())
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?;
        let now = chrono::Utc::now();
        for member_id in &user_ids {
            let sort_order = f64::from(randint(rng, 0, 65535));
            sqlx::query(
                r#"INSERT INTO project_members
                    (id, project_id, workspace_id, member_id, role, sort_order, created_at, updated_at)
                    VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5, $6, $7, $8)
                    ON CONFLICT DO NOTHING"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(project_id)
            .bind(workspace_id)
            .bind(member_id)
            .bind(plan::PROJECT_MEMBER_ROLE)
            .bind(sort_order)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
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
        let now = chrono::Utc::now();
        for state in plan::DEFAULT_STATES {
            sqlx::query(
                r#"INSERT INTO states
                    (id, name, color, project_id, sequence, workspace_id, "group", "default", created_by_id, created_at, updated_at)
                    VALUES ($1::uuid, $2, $3, $4::uuid, $5, $6::uuid, $7, $8, $9::uuid, $10, $11)"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(state.name)
            .bind(state.color)
            .bind(project_id)
            .bind(state.sequence)
            .bind(workspace_id)
            .bind(state.group)
            .bind(state.default)
            .bind(user_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
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
        for _ in 0..plan::LABEL_ROWS {
            let sort_order = f64::from(randint(rng, 0, 65535));
            sqlx::query(
                r#"INSERT INTO labels
                    (id, name, color, project_id, workspace_id, created_by_id, sort_order, created_at, updated_at)
                    VALUES ($1::uuid, $2, $3, $4::uuid, $5::uuid, $6::uuid, $7, $8, $9)
                    ON CONFLICT DO NOTHING"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(fake_color_name(&mut fake))
            .bind(fake_hex_color(&mut fake))
            .bind(project_id)
            .bind(workspace_id)
            .bind(user_id)
            .bind(sort_order)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
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
        for (name, start, end, sort_order) in &rows {
            sqlx::query(
                r#"INSERT INTO cycles
                    (id, name, owned_by_id, sort_order, start_date, end_date, project_id, workspace_id, created_at, updated_at)
                    VALUES ($1::uuid, $2, $3::uuid, $4, $5, $6, $7::uuid, $8::uuid, $9, $10)
                    ON CONFLICT DO NOTHING"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(name)
            .bind(user_id)
            .bind(*sort_order)
            .bind(start.map(midnight))
            .bind(end.map(midnight))
            .bind(project_id)
            .bind(workspace_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
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
        for _ in 0..plan::module_row_count(module_count) {
            let start = if randint(rng, 0, 1) == 0 {
                None
            } else {
                Some(fake_date_this_year(&mut fake, today))
            };
            let end = start.map(|day| fake_date_between(&mut fake, day, year_end(today)));
            sqlx::query(
                r#"INSERT INTO modules
                    (id, name, sort_order, start_date, target_date, project_id, workspace_id, created_at, updated_at)
                    VALUES ($1::uuid, $2, $3, $4, $5, $6::uuid, $7::uuid, $8, $9)
                    ON CONFLICT DO NOTHING"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(fake_name(&mut fake))
            .bind(f64::from(randint(rng, 0, 65535)))
            .bind(start)
            .bind(end)
            .bind(project_id)
            .bind(workspace_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
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
        for _ in 0..plan::page_row_count(pages_count) {
            let text = fake_text(&mut fake, 60000);
            let page_id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                r#"INSERT INTO pages
                    (id, name, workspace_id, owned_by_id, access, color, description_html, is_locked, created_at, updated_at)
                    VALUES ($1::uuid, $2, $3::uuid, $4::uuid, $5, $6, $7, FALSE, $8, $9)
                    ON CONFLICT DO NOTHING"#,
            )
            .bind(&page_id)
            .bind(fake_name(&mut fake))
            .bind(workspace_id)
            .bind(user_id)
            .bind(randint(rng, 0, 1) as i16)
            .bind(fake_hex_color(&mut fake))
            .bind(format!("<p>{text}</p>"))
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
            sqlx::query(
                r#"INSERT INTO project_pages (id, page_id, project_id, workspace_id, created_at, updated_at)
                    VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5, $6)"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&page_id)
            .bind(project_id)
            .bind(workspace_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
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
        let label_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM labels WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let page_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT p.id::text FROM pages p
                JOIN project_pages pp ON pp.page_id = p.id AND pp.project_id = $1::uuid
                WHERE p.deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let take = plan::half_sample_size(pages_count)?;
        let max_draw = plan::per_row_draw_max(label_ids.len())?;
        let now = chrono::Utc::now();
        for position in sample_positions(rng, page_ids.len(), take)? {
            let draw = randint(rng, 0, max_draw as u32) as usize;
            for label_position in sample_positions(rng, label_ids.len(), draw)? {
                sqlx::query(
                    r#"INSERT INTO page_labels (id, page_id, label_id, workspace_id, created_at, updated_at)
                        VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5, $6)
                        ON CONFLICT DO NOTHING"#,
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&page_ids[position])
                .bind(&label_ids[label_position])
                .bind(workspace_id)
                .bind(now)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(db_error)?;
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
        let state_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM states
                WHERE workspace_id = $1::uuid AND project_id = $2::uuid
                AND "group" != $3 AND deleted_at IS NULL"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(plan::TRIAGE_GROUP)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let creator_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT member_id::text FROM project_members
                WHERE workspace_id = $1::uuid AND project_id = $2::uuid AND deleted_at IS NULL"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        if state_ids.is_empty() || creator_ids.is_empty() {
            return Err("randint(0, -1) on an empty state or creator pool".to_owned());
        }
        let max_sequence: Option<i64> = sqlx::query_scalar(
            r#"SELECT MAX(sequence) FROM issue_sequences WHERE project_id = $1::uuid"#,
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        let mut last_id = plan::next_sequence(max_sequence);
        let probe_state = &state_ids[randint(rng, 0, (state_ids.len() - 1) as u32) as usize];
        let max_sort: Option<f64> = sqlx::query_scalar(
            r#"SELECT MAX(sort_order) FROM issues WHERE project_id = $1::uuid AND state_id = $2::uuid"#,
        )
        .bind(project_id)
        .bind(probe_state)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
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
            let issue_id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                r#"INSERT INTO issues
                    (id, state_id, project_id, workspace_id, name, description_html, description_stripped,
                     sequence_id, sort_order, start_date, target_date, priority, created_by_id, created_at, updated_at)
                    VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13::uuid, $14, $15)
                    ON CONFLICT DO NOTHING"#,
            )
            .bind(&issue_id)
            .bind(&state_ids[randint(rng, 0, (state_ids.len() - 1) as u32) as usize])
            .bind(project_id)
            .bind(workspace_id)
            .bind(plan::truncate_code_points(&text, 254))
            .bind(format!("<p>{text}</p>"))
            .bind(&text)
            .bind(last_id as i32)
            .bind(sort_order)
            .bind(start)
            .bind(end)
            .bind(plan::PRIORITIES[randint(rng, 0, 4) as usize])
            .bind(&creator_ids[randint(rng, 0, (creator_ids.len() - 1) as u32) as usize])
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
            sqlx::query(
                r#"INSERT INTO issue_sequences (id, issue_id, sequence, project_id, workspace_id, created_at, updated_at)
                    VALUES ($1::uuid, $2::uuid, $3, $4::uuid, $5::uuid, $6, $7)"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&issue_id)
            .bind(last_id)
            .bind(project_id)
            .bind(workspace_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
            sqlx::query(
                r#"INSERT INTO issue_activities
                    (id, issue_id, actor_id, project_id, workspace_id, comment, verb, created_by_id, created_at, updated_at)
                    VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::uuid, $6, $7, $8::uuid, $9, $10)"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&issue_id)
            .bind(user_id)
            .bind(project_id)
            .bind(workspace_id)
            .bind(plan::ISSUE_ACTIVITY_COMMENT)
            .bind(plan::ISSUE_ACTIVITY_VERB)
            .bind(user_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
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
        let intake_id: Option<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM intakes
                WHERE project_id = $1::uuid AND name = $2 AND is_default AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .bind(plan::INTAKE_NAME)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        let intake_id = match intake_id {
            Some(intake_id) => intake_id,
            None => {
                let intake_id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now();
                sqlx::query(
                    r#"INSERT INTO intakes (id, name, project_id, workspace_id, is_default, created_at, updated_at)
                        VALUES ($1::uuid, $2, $3::uuid, $4::uuid, TRUE, $5, $6)"#,
                )
                .bind(&intake_id)
                .bind(plan::INTAKE_NAME)
                .bind(project_id)
                .bind(workspace_id)
                .bind(now)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(db_error)?;
                intake_id
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
            sqlx::query(
                r#"INSERT INTO intake_issues
                    (id, issue_id, intake_id, status, snoozed_till, source, workspace_id, project_id, created_at, updated_at)
                    VALUES ($1::uuid, $2::uuid, $3::uuid, $4, $5, $6, $7::uuid, $8::uuid, $9, $10)"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(issue_id)
            .bind(&intake_id)
            .bind(status)
            .bind(snoozed_till)
            .bind(plan::SOURCE_IN_APP)
            .bind(workspace_id)
            .bind(project_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
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
        let assignee_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT member_id::text FROM project_members
                WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let issue_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM issues WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let take = plan::half_sample_size(issue_count)?;
        let max_draw = plan::per_row_draw_max(assignee_ids.len())?;
        let now = chrono::Utc::now();
        for position in sample_positions(rng, issue_ids.len(), take)? {
            let draw = randint(rng, 0, max_draw as u32) as usize;
            for assignee_position in sample_positions(rng, assignee_ids.len(), draw)? {
                sqlx::query(
                    r#"INSERT INTO issue_assignees
                        (id, issue_id, assignee_id, project_id, workspace_id, created_at, updated_at)
                        VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::uuid, $6, $7)
                        ON CONFLICT DO NOTHING"#,
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&issue_ids[position])
                .bind(&assignee_ids[assignee_position])
                .bind(project_id)
                .bind(workspace_id)
                .bind(now)
                .bind(now)
                .execute(&self.pool)
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
        let label_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM labels WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let issue_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM issues WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let now = chrono::Utc::now();
        for issue_id in &issue_ids {
            let mut shuffled = label_ids.clone();
            shuffled.shuffle(rng);
            let draw = randint(rng, 0, plan::MULTI_LINK_DRAW_MAX as u32) as usize;
            for label_id in sample_positions(rng, shuffled.len(), draw)? {
                sqlx::query(
                    r#"INSERT INTO issue_labels
                        (id, issue_id, label_id, project_id, workspace_id, created_at, updated_at)
                        VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::uuid, $6, $7)
                        ON CONFLICT DO NOTHING"#,
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(issue_id)
                .bind(&shuffled[label_id])
                .bind(project_id)
                .bind(workspace_id)
                .bind(now)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(db_error)?;
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
        let cycle_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM cycles WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        if cycle_ids.is_empty() {
            return Err("randint(0, -1) with no cycles for cycle issues".to_owned());
        }
        let issue_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM issues WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let take = plan::half_sample_size(issue_count)?;
        let now = chrono::Utc::now();
        for position in sample_positions(rng, issue_ids.len(), take)? {
            let cycle = &cycle_ids[randint(rng, 0, (cycle_ids.len() - 1) as u32) as usize];
            sqlx::query(
                r#"INSERT INTO cycle_issues
                    (id, cycle_id, issue_id, project_id, workspace_id, created_at, updated_at)
                    VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::uuid, $6, $7)
                    ON CONFLICT DO NOTHING"#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(cycle)
            .bind(&issue_ids[position])
            .bind(project_id)
            .bind(workspace_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
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
        let module_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM modules WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let issue_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id::text FROM issues WHERE project_id = $1::uuid AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let now = chrono::Utc::now();
        for issue_id in &issue_ids {
            let mut shuffled = module_ids.clone();
            shuffled.shuffle(rng);
            let draw = randint(rng, 0, plan::MULTI_LINK_DRAW_MAX as u32) as usize;
            for module_position in sample_positions(rng, shuffled.len(), draw)? {
                sqlx::query(
                    r#"INSERT INTO module_issues
                        (id, module_id, issue_id, project_id, workspace_id, created_at, updated_at)
                        VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::uuid, $6, $7)
                        ON CONFLICT DO NOTHING"#,
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&shuffled[module_position])
                .bind(issue_id)
                .bind(project_id)
                .bind(workspace_id)
                .bind(now)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(db_error)?;
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
