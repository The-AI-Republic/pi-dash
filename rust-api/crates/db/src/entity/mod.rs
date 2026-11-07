//! SeaORM entities for every OSS table (ORM-01, PIDASHCONV-1519; the full set
//! since the raw-SQL migration, so reads and writes can all move onto them).
//!
//! Generated with `sea-orm-cli generate entity` (1.1.20) against a database
//! migrated by `apps/api` at OSS `main`. The first 20 tables were generated at
//! Django `0a6fb8eca`; the agent-run set and every other table on 2026-11-04
//! from a database migrated by the `v2026.11.03` release (`runner.0033`).
//! Every base table in `public` is covered except Django's own (`django_*`,
//! `auth_*`) and the private overlay's `cloud_*` tables:
//!
//! ```sh
//! export DATABASE_URL=postgresql://<user>@127.0.0.1:<port>/<migrated-db>
//! TABLES=$(psql "$DATABASE_URL" -Atc "SELECT string_agg(table_name, ',') FROM \
//!   information_schema.tables WHERE table_schema = 'public' \
//!   AND table_type = 'BASE TABLE' AND table_name !~ '^(django_|auth_|cloud_)'")
//! sea-orm-cli generate entity --database-url "$DATABASE_URL" \
//!   --output-dir <staging-dir> --tables "$TABLES"
//! ```
//!
//! Regenerate into a staging directory, then:
//!
//! 1. Copy over only the files for tables that have no entity yet.
//! 2. For every table that already has one, do not copy: diff the staging
//!    file against the committed one and port only real schema changes
//!    (new or changed columns), keeping the hand edits listed below.
//! 3. Re-apply the cleanups below to the newly copied files. The compiler
//!    flags some of them (relations into `auth_*` and the `has_many` edges
//!    whose reverse is deleted fail with unresolved paths or unsatisfied
//!    `Related` bounds); the parity test flags the `unique`, `has_one` and
//!    coverage ones; the rest are on this list only.
//! 4. Run the parity test against the migrated database, then `cargo fmt`
//!    and clippy.
//!
//! The cleanups:
//!
//! * These `belongs_to` relations (and their `Related` impls) stay deleted
//!   (they were outside the original §4 set and nothing joins them yet):
//!   `assistant_thread` / `assistant_turn` relations of `assistant_message`,
//!   `estimate_points` / `issue_types` / `pod` of `issues`, `dev_machine` of
//!   `machine_token`, `estimates` / `file_assets` of `projects`,
//!   `file_assets` of `users` (two) and of `workspaces`.
//! * The agent-run read set (`agent_run`, `runner`, `runner_live_state`,
//!   added for `pidash_services::app_issues::agent_runs`) keeps only the
//!   edges that reader joins: `agent_run` -> `runner` (`runner_id`),
//!   `runner` -> `runner_live_state` and `runner_live_state` -> `runner`.
//!   Every other edge the
//!   generator emits for these three tables is deleted: the out-of-set
//!   `pod`, `scheduler_bindings` and `dev_machine` relations, and the
//!   in-set ones a reader would trip over (`agent_run` has two `runner`
//!   edges and two `users` edges, so a `Related` impl for either would be
//!   ambiguous). The `runner` relation of `runner_session` stays deleted
//!   too: nothing joins it. The `issues` / `workspaces` edges are not
//!   kept: nothing joins through them.
//! * A `#[sea_orm(unique)]` the generator infers from a *partial* unique
//!   index (one with a `WHERE`, so the column is not unique) is deleted:
//!   `agent_run.work_item_id`, `git_repository_bindings.project_id`,
//!   `github_repository_syncs.project_id`, `labels.name`, `loop_jobs.slug`,
//!   `loop_user_preferences.user_id`, `machine_session.dev_machine_id`,
//!   `pod.project_id`, `projects.workspace_id` and
//!   `runner_session.runner_id`. Find them on regeneration with the
//!   single-column `pg_index` rows where `indisunique AND indpred IS NOT NULL`;
//!   the parity test fails on any `unique` flag without a full unique index.
//!   The matching reverse edges are `has_many`, not the generated `has_one`:
//!   `workspaces` -> `projects` and `dev_machine` -> `machine_session`.
//! * Save hooks are not generated. Django `save()` has side effects on
//!   every `BaseModel` (actor stamps), `ProjectBaseModel` and
//!   `WorkspaceBaseModel` (workspace derived from the project) row, and its
//!   own logic on: `agent_run`, `cycles`, `description_versions`,
//!   `descriptions`, `draft_issues`, `issue_comments`, `issue_views`,
//!   `issues`, `labels`, `modules`, `page_versions`, `pages`, `pod`,
//!   `project_members`, `projects`, `runner`, `states`, `stickies`,
//!   `user_favorites` and `users`. Only `issues`, `issue_comments`,
//!   `descriptions` and `users` have hand-written `ActiveModelBehavior`
//!   impls (and the domain write services); a write moved onto any other
//!   entity must port its model's `save()` first.
//! * The two Postgres `inet` columns (`api_activity_logs.ip_address`,
//!   `device_sessions.ip_address`) carry `select_as = "text"` and
//!   `save_as = "inet"`, so they decode into and bind from a `String`.
//! * In the tables added on 2026-11-04 (everything outside the first 20
//!   and the agent-run set), relations are the generator's, minus:
//!   `users_groups` -> `auth_group` and `users_user_permissions` ->
//!   `auth_permission` (Django tables, outside the set), and the `has_many`
//!   edges whose reverse `belongs_to` the cleanups above delete from an
//!   existing entity: `assistant_thread` -> `assistant_message`,
//!   `dev_machine` -> `machine_token` / `runner`, `estimate_points` /
//!   `issue_types` -> `issues`, and `pod` -> `agent_run` / `runner` /
//!   `issues`. A migration that needs one of these joins adds both sides.
//! * This module doc is restored on top of the regenerated `mod.rs`.
//! * The `#[cfg(test)] mod parity;` declaration at the bottom of this file
//!   is restored (the parity test itself is hand-written).
//! * The empty `impl ActiveModelBehavior for ActiveModel {}` the generator
//!   restores in `users.rs` is deleted again: the hand-written impl lives
//!   in the (untouched) `users_hooks.rs`, and keeping both fails the build
//!   with duplicate impls (loud, by design).
//! * The `pub mod issue_hooks;` declaration is restored, and the empty
//!   `impl ActiveModelBehavior for ActiveModel {}` the generator emits in
//!   `issues.rs` is deleted again — the real `Issue.save()` hooks live in
//!   the hand-written `issue_hooks` module, and the compiler rejects the
//!   duplicate impl.
//! * Same for `pub mod comment_hooks;`: the generator-emitted empty impls
//!   in `issue_comments.rs` and `descriptions.rs` are deleted again.
//!
//! Django stays the schema owner: after a Django sync the entities are
//! regenerated and the `parity` test proves they still match
//! `information_schema`.
//!
//! `SeaORM` Entity files below, @generated by sea-orm-codegen 1.1.20

pub mod prelude;

pub mod accounts;
pub mod agent_chat_approval;
pub mod agent_chat_event;
pub mod agent_chat_message;
pub mod agent_chat_session;
pub mod agent_run;
pub mod agent_run_approval;
pub mod agent_run_event;
pub mod agent_run_tool_call;
pub mod analytic_views;
pub mod api_activity_logs;
pub mod api_tokens;
pub mod assistant_event;
pub mod assistant_mcp_server;
pub mod assistant_message;
pub mod assistant_thread;
pub mod assistant_turn;
pub mod assistant_user_llm_config;
pub mod assistant_user_stt_config;
pub mod changelogs;
pub mod chat_message_dedupe;
pub mod cli_device_codes;
pub mod comment_hooks;
pub mod comment_reactions;
pub mod cycle_issues;
pub mod cycle_user_properties;
pub mod cycles;
pub mod deploy_boards;
pub mod description_versions;
pub mod descriptions;
pub mod dev_machine;
pub mod device_sessions;
pub mod devices;
pub mod draft_issue_assignees;
pub mod draft_issue_cycles;
pub mod draft_issue_labels;
pub mod draft_issue_modules;
pub mod draft_issues;
pub mod email_notification_logs;
pub mod engine_credential;
pub mod estimate_points;
pub mod estimates;
pub mod exporters;
pub mod file_assets;
pub mod git_code_review_links;
pub mod git_comment_syncs;
pub mod git_issue_syncs;
pub mod git_provider_accounts;
pub mod git_repositories;
pub mod git_repository_bindings;
pub mod git_webhook_deliveries;
pub mod git_webhook_registrations;
pub mod github_app_install_sessions;
pub mod github_app_installations;
pub mod github_comment_syncs;
pub mod github_issue_syncs;
pub mod github_pull_request_links;
pub mod github_repositories;
pub mod github_repository_syncs;
pub mod github_webhook_deliveries;
pub mod importers;
pub mod instance_admins;
pub mod instance_configurations;
pub mod instances;
pub mod intake_issues;
pub mod intakes;
pub mod integrations;
pub mod issue_activities;
pub mod issue_agent_ticker;
pub mod issue_assignees;
pub mod issue_attachments;
pub mod issue_blockers;
pub mod issue_comments;
pub mod issue_description_versions;
pub mod issue_hooks;
pub mod issue_labels;
pub mod issue_links;
pub mod issue_mentions;
pub mod issue_reactions;
pub mod issue_relations;
pub mod issue_sequences;
pub mod issue_subscribers;
pub mod issue_types;
pub mod issue_versions;
pub mod issue_views;
pub mod issue_votes;
pub mod issue_worklogs;
pub mod issues;
pub mod labels;
pub mod loop_jobs;
pub mod loop_targets;
pub mod loop_user_preferences;
pub mod machine_session;
pub mod machine_token;
pub mod module_issues;
pub mod module_links;
pub mod module_members;
pub mod module_user_properties;
pub mod modules;
pub mod notifications;
pub mod page_labels;
pub mod page_logs;
pub mod page_versions;
pub mod pages;
pub mod pod;
pub mod profiles;
pub mod project_deploy_boards;
pub mod project_identifiers;
pub mod project_issue_types;
pub mod project_member_invites;
pub mod project_members;
pub mod project_pages;
pub mod project_public_members;
pub mod project_user_properties;
pub mod project_webhooks;
pub mod projects;
pub mod prompt_section_override;
pub mod prompt_template;
pub mod run_message_dedupe;
pub mod runner;
pub mod runner_force_refresh;
pub mod runner_live_state;
pub mod runner_session;
pub mod scheduler_bindings;
pub mod schedulers;
pub mod sessions;
pub mod slack_project_syncs;
pub mod social_login_connections;
pub mod states;
pub mod stickies;
pub mod teams;
pub mod user_favorites;
pub mod user_notification_preferences;
pub mod user_recent_visits;
pub mod users;
pub mod users_groups;
pub mod users_user_permissions;
pub mod webhook_logs;
pub mod webhooks;
pub mod workspace_home_preferences;
pub mod workspace_integrations;
pub mod workspace_join_requests;
pub mod workspace_member_invites;
pub mod workspace_members;
pub mod workspace_themes;
pub mod workspace_user_links;
pub mod workspace_user_preferences;
pub mod workspace_user_properties;
pub mod workspaces;

// Hand-written (ORM-07): the `users` hook impl. Re-apply after every
// regeneration, which rewrites this file (and restores the empty
// `ActiveModelBehavior` impl in `users.rs`, which the checklist above
// deletes — forgetting that fails the build with duplicate impls).
mod users_hooks;

// Hand-written (ORM-01): the `information_schema` parity test. Re-apply
// after every regeneration, which rewrites this file.
#[cfg(test)]
mod parity;
