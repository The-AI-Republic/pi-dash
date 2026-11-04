//! `information_schema` parity test for the generated entities.
//!
//! For each covered table this compares the entity's declared table name,
//! column names, column types and nullability with a database migrated by
//! `apps/api`, and fails on any drift. Django stays the schema owner: after
//! a Django sync the entities are regenerated (see `super`'s module doc and
//! `rust-api/README.md`) and this test proves the regen matches.
//!
//! The test needs a migrated database: like every other live-db test in the
//! workspace it skips when `DATABASE_URL` is unset (plain `cargo test`, and
//! post-merge CI, have no Postgres) and runs for real otherwise. Type
//! lengths are compared only when the entity pins them: `sea-orm-cli` drops
//! `varchar(n)` lengths (bare `String` infers `StringLen::None`), so those
//! compare the `varchar` family only, while a pinned `StringLen::N(n)`
//! compares the length too.

use std::collections::BTreeMap;

use sea_orm::strum::IntoEnumIterator;
use sea_orm::{ColumnTrait, EntityName, EntityTrait, IdenStatic};
use sea_query::{ColumnType, RcOrArc, StringLen};

/// One `information_schema.columns` row: name, udt, char length,
/// numeric precision, numeric scale, `YES`/`NO` nullability.
type InfoSchemaRow = (
    String,
    String,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    String,
);

/// One `information_schema.columns` row for a covered table.
struct DbColumn {
    udt: String,
    char_len: Option<i32>,
    num_prec: Option<i32>,
    num_scale: Option<i32>,
    nullable: bool,
}

/// The Postgres type key an entity [`ColumnType`] maps to.
///
/// Lengths are `Some` only when the entity pins them; `None` skips that
/// comparison (the generator drops `varchar(n)` lengths).
struct ExpectedType {
    udt: String,
    char_len: Option<u32>,
    num_prec: Option<u32>,
    num_scale: Option<u32>,
}

impl ExpectedType {
    fn plain(udt: &str) -> Self {
        Self {
            udt: udt.to_owned(),
            char_len: None,
            num_prec: None,
            num_scale: None,
        }
    }
}

/// Map an entity [`ColumnType`] to its Postgres `udt_name`, following the
/// `sea-query` Postgres backend's DDL table (`table.rs`,
/// `prepare_column_type`). Returns `None` for types Postgres cannot store
/// ([`ColumnType::Year` is `unimplemented!` there) so the test fails loudly
/// instead of silently passing an unmapped type.
fn expected_type(col: &ColumnType) -> Option<ExpectedType> {
    match col {
        // Bare `char` is `character(1)` in Postgres.
        ColumnType::Char(len) => Some(ExpectedType {
            udt: "bpchar".to_owned(),
            char_len: Some(len.unwrap_or(1)),
            num_prec: None,
            num_scale: None,
        }),
        // Bare `varchar` (what `None` and `Max` both render as) carries no
        // length in `information_schema`.
        ColumnType::String(StringLen::N(len)) => Some(ExpectedType {
            udt: "varchar".to_owned(),
            char_len: Some(*len),
            num_prec: None,
            num_scale: None,
        }),
        ColumnType::String(_) => Some(ExpectedType::plain("varchar")),
        ColumnType::Text => Some(ExpectedType::plain("text")),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned => Some(ExpectedType::plain("int2")),
        ColumnType::Integer | ColumnType::Unsigned => Some(ExpectedType::plain("int4")),
        ColumnType::BigInteger | ColumnType::BigUnsigned => Some(ExpectedType::plain("int8")),
        ColumnType::Float => Some(ExpectedType::plain("float4")),
        ColumnType::Double => Some(ExpectedType::plain("float8")),
        ColumnType::Decimal(precision) => Some(ExpectedType {
            udt: "numeric".to_owned(),
            char_len: None,
            num_prec: precision.map(|(prec, _)| prec),
            num_scale: precision.map(|(_, scale)| scale),
        }),
        ColumnType::DateTime | ColumnType::Timestamp => Some(ExpectedType::plain("timestamp")),
        ColumnType::TimestampWithTimeZone => Some(ExpectedType::plain("timestamptz")),
        ColumnType::Time => Some(ExpectedType::plain("time")),
        ColumnType::Date => Some(ExpectedType::plain("date")),
        // Postgres has no `year` column type.
        ColumnType::Year => None,
        ColumnType::Interval(_, _) => Some(ExpectedType::plain("interval")),
        ColumnType::Binary(_) | ColumnType::VarBinary(_) | ColumnType::Blob => {
            Some(ExpectedType::plain("bytea"))
        }
        // `character_maximum_length` covers bit strings too.
        ColumnType::Bit(len) => Some(ExpectedType {
            udt: "bit".to_owned(),
            char_len: Some(len.unwrap_or(1)),
            num_prec: None,
            num_scale: None,
        }),
        ColumnType::VarBit(len) => Some(ExpectedType {
            udt: "varbit".to_owned(),
            char_len: Some(*len),
            num_prec: None,
            num_scale: None,
        }),
        ColumnType::Boolean => Some(ExpectedType::plain("bool")),
        // Postgres `money` takes no precision; only the family compares.
        ColumnType::Money(_) => Some(ExpectedType::plain("money")),
        ColumnType::Json => Some(ExpectedType::plain("json")),
        ColumnType::JsonBinary => Some(ExpectedType::plain("jsonb")),
        ColumnType::Uuid => Some(ExpectedType::plain("uuid")),
        ColumnType::Custom(name) => Some(ExpectedType::plain(&name.to_string())),
        ColumnType::Enum { name, .. } => Some(ExpectedType::plain(&name.to_string())),
        // Postgres names an array type `_<element>`.
        ColumnType::Array(inner) => {
            let inner = expected_type(inner)?;
            if inner.udt.starts_with('_') {
                return None;
            }
            Some(ExpectedType::plain(&format!("_{}", inner.udt)))
        }
        ColumnType::Vector(_) => Some(ExpectedType::plain("vector")),
        ColumnType::Cidr => Some(ExpectedType::plain("cidr")),
        ColumnType::Inet => Some(ExpectedType::plain("inet")),
        ColumnType::MacAddr => Some(ExpectedType::plain("macaddr")),
        ColumnType::LTree => Some(ExpectedType::plain("ltree")),
        // `ColumnType` is non-exhaustive: a future variant fails loudly.
        _ => None,
    }
}

/// Compare entity `E`'s columns with `table` in `information_schema`, and
/// its `unique` flags with the table's single-column unique indexes. Returns
/// one line per drifted column.
async fn entity_drift<E>(pool: &sqlx::PgPool, table: &str) -> Vec<String>
where
    E: EntityTrait,
    E::Column: IntoEnumIterator,
{
    let mut expected = BTreeMap::new();
    let mut unique = Vec::new();
    for col in E::Column::iter() {
        let name = col.as_str().to_owned();
        let def = col.def();
        let col_type = def.get_column_type();
        let exp = expected_type(col_type)
            .unwrap_or_else(|| panic!("{table}.{name}: unmapped entity type {col_type:?}"));
        if def.is_unique() {
            unique.push(name.clone());
        }
        expected.insert(name, (exp, def.is_null()));
    }

    let rows: Vec<InfoSchemaRow> = sqlx::query_as(
        "SELECT column_name, udt_name, character_maximum_length, \
             numeric_precision, numeric_scale, is_nullable \
             FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 \
             ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("information_schema query");
    if rows.is_empty() {
        return vec![format!(
            "table {table} missing from information_schema on the migrated database"
        )];
    }
    let mut db = BTreeMap::new();
    for (name, udt, char_len, num_prec, num_scale, is_nullable) in rows {
        db.insert(
            name,
            DbColumn {
                udt,
                char_len,
                num_prec,
                num_scale,
                nullable: is_nullable == "YES",
            },
        );
    }

    let mut diffs = Vec::new();
    for (name, (exp, nullable)) in &expected {
        match db.get(name) {
            None => diffs.push(format!(
                "column {table}.{name} in entity but missing from database"
            )),
            Some(found) => {
                if found.udt != exp.udt {
                    diffs.push(format!(
                        "column {table}.{name} type drift: entity {} but database {}",
                        exp.udt, found.udt
                    ));
                }
                if let Some(len) = exp.char_len {
                    if found.char_len != Some(len as i32) {
                        diffs.push(format!(
                            "column {table}.{name} length drift: entity {len} but database {:?}",
                            found.char_len
                        ));
                    }
                }
                if let Some(prec) = exp.num_prec {
                    if found.num_prec != Some(prec as i32) {
                        diffs.push(format!(
                            "column {table}.{name} precision drift: entity {prec} but database {:?}",
                            found.num_prec
                        ));
                    }
                }
                if let Some(scale) = exp.num_scale {
                    if found.num_scale != Some(scale as i32) {
                        diffs.push(format!(
                            "column {table}.{name} scale drift: entity {scale} but database {:?}",
                            found.num_scale
                        ));
                    }
                }
                if found.nullable != *nullable {
                    diffs.push(format!(
                        "column {table}.{name} nullability drift: entity {} but database {}",
                        if *nullable { "NULL" } else { "NOT NULL" },
                        if found.nullable { "NULL" } else { "NOT NULL" },
                    ));
                }
            }
        }
    }
    for name in db.keys() {
        if !expected.contains_key(name) {
            diffs.push(format!(
                "column {table}.{name} in database but missing from entity"
            ));
        }
    }
    // A `unique` flag must stand for a full (non-partial) single-column
    // unique index: the generator also reads a partial one (`WHERE ...`) as a
    // column constraint, which would make a one-to-many edge look one-to-one.
    let fully_unique: Vec<(String,)> = sqlx::query_as(
        "SELECT a.attname::text FROM pg_index i \
             JOIN pg_class t ON t.oid = i.indrelid \
             JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = i.indkey[0] \
             WHERE t.relname = $1 AND t.relnamespace = 'public'::regnamespace \
             AND i.indisunique AND i.indnatts = 1 AND i.indpred IS NULL",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("pg_index query");
    for name in &unique {
        if !fully_unique.iter().any(|(column,)| column == name) {
            diffs.push(format!(
                "column {table}.{name} is `unique` in the entity but has no full \
                 single-column unique index (a partial index does not count)"
            ));
        }
    }
    diffs
}

/// One live test over every entity, sharing one connection: each entity's
/// declared table name is its module name, and its columns match
/// `information_schema`. All drift is reported together.
macro_rules! parity_tests {
    ($($entity:ident),+ $(,)?) => {
        #[tokio::test]
        async fn entities_match_information_schema() {
            let url = match std::env::var("DATABASE_URL") {
                Ok(url) => url,
                Err(_) => {
                    eprintln!("skipping live-db test: DATABASE_URL is not set");
                    return;
                }
            };
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL");
            let mut drift = Vec::new();
            $(
                let table = stringify!($entity);
                let declared = super::$entity::Entity.table_name();
                if declared != table {
                    drift.push(format!("entity {table} declares table_name {declared}"));
                }
                drift.extend(entity_drift::<super::$entity::Entity>(&pool, table).await);
            )+
            assert!(drift.is_empty(), "schema drift:\n{}", drift.join("\n"));
        }
    };
}

parity_tests!(
    accounts,
    agent_chat_approval,
    agent_chat_event,
    agent_chat_message,
    agent_chat_session,
    agent_run,
    agent_run_approval,
    agent_run_event,
    agent_run_tool_call,
    analytic_views,
    api_activity_logs,
    api_tokens,
    assistant_event,
    assistant_mcp_server,
    assistant_message,
    assistant_thread,
    assistant_turn,
    assistant_user_llm_config,
    assistant_user_stt_config,
    changelogs,
    chat_message_dedupe,
    cli_device_codes,
    comment_reactions,
    cycle_issues,
    cycle_user_properties,
    cycles,
    deploy_boards,
    description_versions,
    descriptions,
    dev_machine,
    device_sessions,
    devices,
    draft_issue_assignees,
    draft_issue_cycles,
    draft_issue_labels,
    draft_issue_modules,
    draft_issues,
    email_notification_logs,
    estimate_points,
    estimates,
    exporters,
    file_assets,
    git_code_review_links,
    git_comment_syncs,
    git_issue_syncs,
    git_provider_accounts,
    git_repositories,
    git_repository_bindings,
    git_webhook_deliveries,
    git_webhook_registrations,
    github_app_install_sessions,
    github_app_installations,
    github_comment_syncs,
    github_issue_syncs,
    github_pull_request_links,
    github_repositories,
    github_repository_syncs,
    github_webhook_deliveries,
    importers,
    instance_admins,
    instance_configurations,
    instances,
    intake_issues,
    intakes,
    integrations,
    issue_activities,
    issue_agent_ticker,
    issue_assignees,
    issue_attachments,
    issue_blockers,
    issue_comments,
    issue_description_versions,
    issue_labels,
    issue_links,
    issue_mentions,
    issue_reactions,
    issue_relations,
    issue_sequences,
    issue_subscribers,
    issue_types,
    issue_versions,
    issue_views,
    issue_votes,
    issue_worklogs,
    issues,
    labels,
    loop_jobs,
    loop_targets,
    loop_user_preferences,
    machine_session,
    machine_token,
    module_issues,
    module_links,
    module_members,
    module_user_properties,
    modules,
    notifications,
    page_labels,
    page_logs,
    page_versions,
    pages,
    pod,
    profiles,
    project_deploy_boards,
    project_identifiers,
    project_issue_types,
    project_member_invites,
    project_members,
    project_pages,
    project_public_members,
    project_user_properties,
    project_webhooks,
    projects,
    prompt_section_override,
    prompt_template,
    run_message_dedupe,
    runner,
    runner_force_refresh,
    runner_live_state,
    runner_session,
    scheduler_bindings,
    schedulers,
    sessions,
    slack_project_syncs,
    social_login_connections,
    states,
    stickies,
    teams,
    user_favorites,
    user_notification_preferences,
    user_recent_visits,
    users,
    users_groups,
    users_user_permissions,
    webhook_logs,
    webhooks,
    workspace_home_preferences,
    workspace_integrations,
    workspace_join_requests,
    workspace_member_invites,
    workspace_members,
    workspace_themes,
    workspace_user_links,
    workspace_user_preferences,
    workspace_user_properties,
    workspaces,
);

#[test]
fn inferred_string_compares_varchar_family_only() {
    let exp = expected_type(&ColumnType::String(StringLen::None)).expect("mapped");
    assert_eq!(exp.udt, "varchar");
    assert_eq!(exp.char_len, None);
}

#[test]
fn pinned_string_compares_length() {
    let exp = expected_type(&ColumnType::String(StringLen::N(30))).expect("mapped");
    assert_eq!(exp.udt, "varchar");
    assert_eq!(exp.char_len, Some(30));
}

#[test]
fn array_maps_to_underscore_element() {
    let varchar_array = ColumnType::Array(RcOrArc::new(ColumnType::String(StringLen::None)));
    let exp = expected_type(&varchar_array).expect("mapped");
    assert_eq!(exp.udt, "_varchar");
}

#[test]
fn year_is_unmapped() {
    assert!(expected_type(&ColumnType::Year).is_none());
}

/// The two `inet` columns read as text and write back as inet, so a `String`
/// decodes and binds (sqlx has no text <-> inet coercion).
#[test]
fn inet_columns_cast_through_text() {
    use sea_orm::{ActiveValue::Set, DbBackend, QueryTrait};
    let read = super::device_sessions::Entity::find()
        .build(DbBackend::Postgres)
        .to_string();
    assert!(
        read.contains(r#"CAST("device_sessions"."ip_address" AS "text")"#)
            || read.contains(r#"CAST("device_sessions"."ip_address" AS text)"#),
        "{read}"
    );
    let read = super::api_activity_logs::Entity::find()
        .build(DbBackend::Postgres)
        .to_string();
    assert!(
        read.contains(r#""ip_address" AS"#) && read.contains("CAST("),
        "{read}"
    );
    let insert = super::device_sessions::Entity::insert(super::device_sessions::ActiveModel {
        ip_address: Set(Some("10.0.0.1".to_owned())),
        ..Default::default()
    })
    .build(DbBackend::Postgres)
    .to_string();
    assert!(insert.contains("CAST('10.0.0.1' AS inet)"), "{insert}");
}
