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

/// Compare entity `E`'s columns with `table` in `information_schema`.
/// Fails with one line per drifted column.
async fn assert_entity_parity<E>(pool: &sqlx::PgPool, table: &str)
where
    E: EntityTrait,
    E::Column: IntoEnumIterator,
{
    let mut expected = BTreeMap::new();
    for col in E::Column::iter() {
        let name = col.as_str().to_owned();
        let def = col.def();
        let col_type = def.get_column_type();
        let exp = expected_type(col_type)
            .unwrap_or_else(|| panic!("{table}.{name}: unmapped entity type {col_type:?}"));
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
    assert!(
        !rows.is_empty(),
        "table {table} missing from information_schema on the migrated database"
    );
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
    assert!(
        diffs.is_empty(),
        "schema drift on {table}:\n{}",
        diffs.join("\n")
    );
}

/// One parity test per covered table: pins the entity's declared table
/// name, then compares its columns with `information_schema`.
macro_rules! parity_test {
    ($test:ident, $entity:ident, $table:literal) => {
        #[tokio::test]
        async fn $test() {
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
            let entity = super::$entity::Entity;
            assert_eq!(entity.table_name(), $table, "entity table_name drift");
            assert_entity_parity::<super::$entity::Entity>(&pool, $table).await;
        }
    };
}

parity_test!(issues, issues, "issues");
parity_test!(
    agent_chat_approval,
    agent_chat_approval,
    "agent_chat_approval"
);
parity_test!(agent_chat_event, agent_chat_event, "agent_chat_event");
parity_test!(agent_chat_message, agent_chat_message, "agent_chat_message");
parity_test!(agent_chat_session, agent_chat_session, "agent_chat_session");
parity_test!(agent_run_approval, agent_run_approval, "agent_run_approval");
parity_test!(agent_run_event, agent_run_event, "agent_run_event");
parity_test!(
    agent_run_tool_call,
    agent_run_tool_call,
    "agent_run_tool_call"
);
parity_test!(analytic_views, analytic_views, "analytic_views");
parity_test!(api_activity_logs, api_activity_logs, "api_activity_logs");
parity_test!(assistant_event, assistant_event, "assistant_event");
parity_test!(
    assistant_mcp_server,
    assistant_mcp_server,
    "assistant_mcp_server"
);
parity_test!(assistant_thread, assistant_thread, "assistant_thread");
parity_test!(assistant_turn, assistant_turn, "assistant_turn");
parity_test!(
    assistant_user_stt_config,
    assistant_user_stt_config,
    "assistant_user_stt_config"
);
parity_test!(changelogs, changelogs, "changelogs");
parity_test!(
    chat_message_dedupe,
    chat_message_dedupe,
    "chat_message_dedupe"
);
parity_test!(cli_device_codes, cli_device_codes, "cli_device_codes");
parity_test!(comment_reactions, comment_reactions, "comment_reactions");
parity_test!(cycle_issues, cycle_issues, "cycle_issues");
parity_test!(
    cycle_user_properties,
    cycle_user_properties,
    "cycle_user_properties"
);
parity_test!(cycles, cycles, "cycles");
parity_test!(deploy_boards, deploy_boards, "deploy_boards");
parity_test!(
    description_versions,
    description_versions,
    "description_versions"
);
parity_test!(dev_machine, dev_machine, "dev_machine");
parity_test!(device_sessions, device_sessions, "device_sessions");
parity_test!(devices, devices, "devices");
parity_test!(
    draft_issue_assignees,
    draft_issue_assignees,
    "draft_issue_assignees"
);
parity_test!(draft_issue_cycles, draft_issue_cycles, "draft_issue_cycles");
parity_test!(draft_issue_labels, draft_issue_labels, "draft_issue_labels");
parity_test!(
    draft_issue_modules,
    draft_issue_modules,
    "draft_issue_modules"
);
parity_test!(draft_issues, draft_issues, "draft_issues");
parity_test!(
    email_notification_logs,
    email_notification_logs,
    "email_notification_logs"
);
parity_test!(estimate_points, estimate_points, "estimate_points");
parity_test!(estimates, estimates, "estimates");
parity_test!(exporters, exporters, "exporters");
parity_test!(file_assets, file_assets, "file_assets");
parity_test!(
    git_code_review_links,
    git_code_review_links,
    "git_code_review_links"
);
parity_test!(git_comment_syncs, git_comment_syncs, "git_comment_syncs");
parity_test!(git_issue_syncs, git_issue_syncs, "git_issue_syncs");
parity_test!(
    git_provider_accounts,
    git_provider_accounts,
    "git_provider_accounts"
);
parity_test!(git_repositories, git_repositories, "git_repositories");
parity_test!(
    git_repository_bindings,
    git_repository_bindings,
    "git_repository_bindings"
);
parity_test!(
    git_webhook_deliveries,
    git_webhook_deliveries,
    "git_webhook_deliveries"
);
parity_test!(
    git_webhook_registrations,
    git_webhook_registrations,
    "git_webhook_registrations"
);
parity_test!(
    github_app_install_sessions,
    github_app_install_sessions,
    "github_app_install_sessions"
);
parity_test!(
    github_app_installations,
    github_app_installations,
    "github_app_installations"
);
parity_test!(
    github_comment_syncs,
    github_comment_syncs,
    "github_comment_syncs"
);
parity_test!(github_issue_syncs, github_issue_syncs, "github_issue_syncs");
parity_test!(
    github_pull_request_links,
    github_pull_request_links,
    "github_pull_request_links"
);
parity_test!(
    github_repositories,
    github_repositories,
    "github_repositories"
);
parity_test!(
    github_repository_syncs,
    github_repository_syncs,
    "github_repository_syncs"
);
parity_test!(
    github_webhook_deliveries,
    github_webhook_deliveries,
    "github_webhook_deliveries"
);
parity_test!(importers, importers, "importers");
parity_test!(instance_admins, instance_admins, "instance_admins");
parity_test!(
    instance_configurations,
    instance_configurations,
    "instance_configurations"
);
parity_test!(instances, instances, "instances");
parity_test!(intake_issues, intake_issues, "intake_issues");
parity_test!(intakes, intakes, "intakes");
parity_test!(integrations, integrations, "integrations");
parity_test!(issue_activities, issue_activities, "issue_activities");
parity_test!(issue_agent_ticker, issue_agent_ticker, "issue_agent_ticker");
parity_test!(issue_attachments, issue_attachments, "issue_attachments");
parity_test!(issue_blockers, issue_blockers, "issue_blockers");
parity_test!(
    issue_description_versions,
    issue_description_versions,
    "issue_description_versions"
);
parity_test!(issue_labels, issue_labels, "issue_labels");
parity_test!(issue_links, issue_links, "issue_links");
parity_test!(issue_mentions, issue_mentions, "issue_mentions");
parity_test!(issue_reactions, issue_reactions, "issue_reactions");
parity_test!(issue_subscribers, issue_subscribers, "issue_subscribers");
parity_test!(issue_types, issue_types, "issue_types");
parity_test!(issue_versions, issue_versions, "issue_versions");
parity_test!(issue_views, issue_views, "issue_views");
parity_test!(issue_votes, issue_votes, "issue_votes");
parity_test!(issue_worklogs, issue_worklogs, "issue_worklogs");
parity_test!(labels, labels, "labels");
parity_test!(loop_jobs, loop_jobs, "loop_jobs");
parity_test!(loop_targets, loop_targets, "loop_targets");
parity_test!(
    loop_user_preferences,
    loop_user_preferences,
    "loop_user_preferences"
);
parity_test!(machine_session, machine_session, "machine_session");
parity_test!(module_issues, module_issues, "module_issues");
parity_test!(module_links, module_links, "module_links");
parity_test!(module_members, module_members, "module_members");
parity_test!(
    module_user_properties,
    module_user_properties,
    "module_user_properties"
);
parity_test!(modules, modules, "modules");
parity_test!(notifications, notifications, "notifications");
parity_test!(page_labels, page_labels, "page_labels");
parity_test!(page_logs, page_logs, "page_logs");
parity_test!(page_versions, page_versions, "page_versions");
parity_test!(pages, pages, "pages");
parity_test!(pod, pod, "pod");
parity_test!(
    project_deploy_boards,
    project_deploy_boards,
    "project_deploy_boards"
);
parity_test!(
    project_identifiers,
    project_identifiers,
    "project_identifiers"
);
parity_test!(
    project_issue_types,
    project_issue_types,
    "project_issue_types"
);
parity_test!(
    project_member_invites,
    project_member_invites,
    "project_member_invites"
);
parity_test!(project_pages, project_pages, "project_pages");
parity_test!(
    project_public_members,
    project_public_members,
    "project_public_members"
);
parity_test!(
    project_user_properties,
    project_user_properties,
    "project_user_properties"
);
parity_test!(project_webhooks, project_webhooks, "project_webhooks");
parity_test!(
    prompt_section_override,
    prompt_section_override,
    "prompt_section_override"
);
parity_test!(prompt_template, prompt_template, "prompt_template");
parity_test!(run_message_dedupe, run_message_dedupe, "run_message_dedupe");
parity_test!(
    runner_force_refresh,
    runner_force_refresh,
    "runner_force_refresh"
);
parity_test!(scheduler_bindings, scheduler_bindings, "scheduler_bindings");
parity_test!(schedulers, schedulers, "schedulers");
parity_test!(sessions, sessions, "sessions");
parity_test!(
    slack_project_syncs,
    slack_project_syncs,
    "slack_project_syncs"
);
parity_test!(
    social_login_connections,
    social_login_connections,
    "social_login_connections"
);
parity_test!(stickies, stickies, "stickies");
parity_test!(teams, teams, "teams");
parity_test!(user_favorites, user_favorites, "user_favorites");
parity_test!(user_recent_visits, user_recent_visits, "user_recent_visits");
parity_test!(users_groups, users_groups, "users_groups");
parity_test!(
    users_user_permissions,
    users_user_permissions,
    "users_user_permissions"
);
parity_test!(webhook_logs, webhook_logs, "webhook_logs");
parity_test!(webhooks, webhooks, "webhooks");
parity_test!(
    workspace_home_preferences,
    workspace_home_preferences,
    "workspace_home_preferences"
);
parity_test!(
    workspace_integrations,
    workspace_integrations,
    "workspace_integrations"
);
parity_test!(
    workspace_join_requests,
    workspace_join_requests,
    "workspace_join_requests"
);
parity_test!(
    workspace_member_invites,
    workspace_member_invites,
    "workspace_member_invites"
);
parity_test!(workspace_themes, workspace_themes, "workspace_themes");
parity_test!(
    workspace_user_links,
    workspace_user_links,
    "workspace_user_links"
);
parity_test!(
    workspace_user_preferences,
    workspace_user_preferences,
    "workspace_user_preferences"
);
parity_test!(
    workspace_user_properties,
    workspace_user_properties,
    "workspace_user_properties"
);
parity_test!(agent_run, agent_run, "agent_run");
parity_test!(runner, runner, "runner");
parity_test!(runner_live_state, runner_live_state, "runner_live_state");
parity_test!(issue_sequences, issue_sequences, "issue_sequences");
parity_test!(issue_assignees, issue_assignees, "issue_assignees");
parity_test!(issue_comments, issue_comments, "issue_comments");
parity_test!(descriptions, descriptions, "descriptions");
parity_test!(issue_relations, issue_relations, "issue_relations");
parity_test!(users, users, "users");
parity_test!(profiles, profiles, "profiles");
parity_test!(accounts, accounts, "accounts");
parity_test!(
    user_notification_preferences,
    user_notification_preferences,
    "user_notification_preferences"
);
parity_test!(workspaces, workspaces, "workspaces");
parity_test!(workspace_members, workspace_members, "workspace_members");
parity_test!(projects, projects, "projects");
parity_test!(project_members, project_members, "project_members");
parity_test!(states, states, "states");
parity_test!(api_tokens, api_tokens, "api_tokens");
parity_test!(machine_token, machine_token, "machine_token");
parity_test!(runner_session, runner_session, "runner_session");
parity_test!(
    assistant_user_llm_config,
    assistant_user_llm_config,
    "assistant_user_llm_config"
);
parity_test!(assistant_message, assistant_message, "assistant_message");

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
