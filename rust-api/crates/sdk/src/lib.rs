#![forbid(unsafe_code)]

//! The one public surface the cloud overlay may depend on.
//!
//! `pidash-sdk` contains no logic, only `pub use` re-exports grouped into
//! coarse modules. The overlay (`private-pi-dash/rust-cloud`) depends on
//! this crate; every other OSS path is internal and may change without
//! notice (see "Overlay surface" in `rust-api/README.md`).
//!
//! Seeded by OVL-02 (PIDASHCONV-1540) with every OSS path the overlay
//! imports at private `main` `68a7486` (scan of `use pidash_*` and
//! `pidash_*::` over `rust-cloud/**/*.rs`; brace groups expanded, qualified
//! method calls and enum variants folded to their parent item, brace-prefix
//! module hits excluded). OVL-03 (PIDASHCONV-1541)..OVL-07 (PIDASHCONV-1545)
//! add their new public items here as they land.
//!
//! Adding an export: add the `pub use` line below, the matching
//! `module::Name` entry in `tests/surface.rs`, and the
//! `pidash_sdk::module::Name` line (sorted) in `SURFACE.txt`. The surface
//! test fails when the three drift apart; removing or renaming an export
//! therefore shows up as a reviewed diff, never as a surprise overlay
//! breakage.

/// Serve/worker composition: router builders, the overlay hook, the edge
/// handle, the middleware stack with the session store, and worker
/// registration. OVL-03 (PIDASHCONV-1541) added
/// `ServeBuilder`/`WorkerBuilder` (`ServeParts`/`BuiltWorker`) here;
/// OVL-04 (PIDASHCONV-1542)..OVL-07 (PIDASHCONV-1545) extend it.
pub mod startup {
    pub use pidash_api::build_router_with_overlay;
    pub use pidash_api::edge::proxy;
    pub use pidash_api::edge::EdgeHandle; // also imported by the overlay as `pidash_api::EdgeHandle`.
    pub use pidash_api::stack;
    pub use pidash_api::startup::{BuiltWorker, ServeBuilder, ServeParts, WorkerBuilder};
    pub use pidash_api::CorsConfig;
    pub use pidash_api::EdgeFlags;
    pub use pidash_api::LoggingConfig;
    pub use pidash_api::Overlay;
    pub use pidash_api::PgSessionStore;
    pub use pidash_api::QueueSink;
    pub use pidash_api::SecurityConfig;
    pub use pidash_api::SessionConfig;
}

/// Shared state and seam traits: `AppState`, the desktop request gate,
/// and the services extension seams the overlay implements.
pub mod state {
    pub use pidash_api::desktop_gate::header_value;
    pub use pidash_api::desktop_gate::DesktopCredential;
    pub use pidash_api::desktop_gate::DesktopRequestGate;
    pub use pidash_api::state::AppState; // also imported by the overlay as `pidash_api::AppState`.
    pub use pidash_services::extensions::AgentModelProfile;
    pub use pidash_services::extensions::AssistantModelSeam;
    pub use pidash_services::extensions::CloudAgentToolsetsSeam;
    pub use pidash_services::extensions::CredentialError;
    pub use pidash_services::extensions::CredentialUnavailable;
    pub use pidash_services::extensions::ExtraToolset;
    pub use pidash_services::extensions::NoExtraToolsets;
    pub use pidash_services::extensions::SeamFuture;
    pub use pidash_services::extensions::TitleError;
    pub use pidash_services::extensions::TitleRequest;
    pub use pidash_services::extensions::ToolsetsResolution;
    pub use pidash_services::extensions::ToolsetsRun;
    pub use pidash_services::extensions::ToolsetsUnavailable;
}

/// ORM write services (`pidash_services::domain`). Empty: ORM-02
/// (PIDASHCONV-1520) populates it. Until then the overlay must not
/// write OSS tables directly.
pub mod domain {}

/// Reads and render: issue shapes and serializers, workspace, project
/// and search models, and auth-session shapes. OVL-05 (PIDASHCONV-1543)
/// added the `pidash_services::read` root API here item for item.
pub mod read {
    pub use pidash_services::app_issues::pr_links::attach_pull_request;
    pub use pidash_services::app_issues::pr_links::AttachError;
    pub use pidash_services::app_issues::pr_links::CreatePrLinkOutcome;
    pub use pidash_services::app_issues::pr_links::InstallationRow;
    pub use pidash_services::app_issues::pr_links::InvalidPullRequestURL;
    pub use pidash_services::app_issues::pr_links::IssueNotFound;
    pub use pidash_services::app_issues::pr_links::NewPrLink;
    pub use pidash_services::app_issues::pr_links::NewReviewLink;
    pub use pidash_services::app_issues::pr_links::PrLinkRow;
    pub use pidash_services::app_issues::pr_links::PrLinksStore;
    pub use pidash_services::app_issues::pr_links::PullRequestAlreadyLinked;
    pub use pidash_services::app_issues::pr_links::ReviewLinkRow;
    pub use pidash_services::app_issues::pr_links::ReviewMirrorFields;
    pub use pidash_services::app_issues::pr_links::StoreError;
    pub use pidash_services::app_issues::pr_links::INSTALLATION_LOOKUP_SQL;
    pub use pidash_services::app_issues::pr_links::ISSUE_EXISTS_SQL;
    pub use pidash_services::app_issues::pr_links::PR_LINK_INSERT_SQL;
    pub use pidash_services::app_issues::pr_links::PR_LINK_LOOKUP_SQL;
    pub use pidash_services::app_issues::pr_links::PR_LINK_SOFT_DELETE_SQL;
    pub use pidash_services::app_issues::pr_links::REVIEW_LINK_INSERT_SQL;
    pub use pidash_services::app_issues::pr_links::REVIEW_LINK_LOOKUP_SQL;
    pub use pidash_services::app_issues::pr_links::REVIEW_LINK_SOFT_DELETE_SQL;
    pub use pidash_services::app_issues::pr_links::REVIEW_LINK_UPDATE_SQL;
    pub use pidash_services::app_issues::repoint_sql;
    pub use pidash_services::app_issues::serialize_iso_datetime;
    pub use pidash_services::app_issues::serializers_create::DEFAULT_ASSIGNEE_EXISTS_SQL;
    pub use pidash_services::app_issues::serializers_create::STATE_EXISTS_SQL;
    pub use pidash_services::app_issues::serializers_detail::serialize_drf_datetime; // also imported by the overlay as `pidash_services::app_issues::serialize_drf_datetime`.
    pub use pidash_services::app_issues::RepointTarget;
    pub use pidash_services::app_issues::ASSIGNEE_IDS_SQL;
    pub use pidash_services::app_issues::LABEL_IDS_SQL;
    pub use pidash_services::app_issues::REPOINT_ORDER;
    pub use pidash_services::app_project::queries::normalize_resolve_identifier;
    pub use pidash_services::app_views_search::fts;
    pub use pidash_services::app_workspace::models_user::profile;
    pub use pidash_services::app_workspace::models_user::user;
    pub use pidash_services::app_workspace::models_workspace::workspace;
    pub use pidash_services::app_workspace::models_workspace::workspace_member;
    pub use pidash_services::app_workspace::tasks::workspace_seed_emit;
    pub use pidash_services::app_workspace::tasks::WORKSPACE_SEED_TASK;
    pub use pidash_services::auth_session::password::drf_datetime;
    pub use pidash_services::auth_session::shapes::base_host; // also imported by the overlay as `pidash_services::auth_session::base_host`.
    pub use pidash_services::auth_session::shapes::HostSettings; // also imported by the overlay as `pidash_services::auth_session::HostSettings`.
    pub use pidash_services::read::comment_is_synced;
    pub use pidash_services::read::comment_url;
    pub use pidash_services::read::decode_issue;
    pub use pidash_services::read::expand_estimate_point;
    pub use pidash_services::read::expand_parent;
    pub use pidash_services::read::expand_project;
    pub use pidash_services::read::expand_state;
    pub use pidash_services::read::expand_user;
    pub use pidash_services::read::expand_workspace;
    pub use pidash_services::read::fetch_asset;
    pub use pidash_services::read::fetch_assignee_ids;
    pub use pidash_services::read::fetch_assignee_rows;
    pub use pidash_services::read::fetch_blocker_rows;
    pub use pidash_services::read::fetch_blocker_summary;
    pub use pidash_services::read::fetch_expanded_labels;
    pub use pidash_services::read::fetch_grouped_rows;
    pub use pidash_services::read::fetch_issue_for_write;
    pub use pidash_services::read::fetch_label_ids;
    pub use pidash_services::read::fetch_profile;
    pub use pidash_services::read::fetch_user;
    pub use pidash_services::read::render_actor_detail;
    pub use pidash_services::read::render_comment;
    pub use pidash_services::read::render_comment_reaction_row;
    pub use pidash_services::read::render_decoded;
    pub use pidash_services::read::render_state;
    pub use pidash_services::read::render_write_response;
    pub use pidash_services::read::requested_names;
    pub use pidash_services::read::resolve_workpad_state;
    pub use pidash_services::read::state_row_from;
    pub use pidash_services::read::workpad_stripped;
    pub use pidash_services::read::workspace_of;
    pub use pidash_services::read::workspace_row;
    pub use pidash_services::read::workspace_slug;
    pub use pidash_services::read::ActorDetailView;
    pub use pidash_services::read::AssetRow;
    pub use pidash_services::read::BlockerRows;
    pub use pidash_services::read::CommentReactionView;
    pub use pidash_services::read::CommentRow;
    pub use pidash_services::read::CommentView;
    pub use pidash_services::read::DbError;
    pub use pidash_services::read::DecodedIssue;
    pub use pidash_services::read::Error;
    pub use pidash_services::read::IssueFlatView;
    pub use pidash_services::read::OwnedAssigneeRow;
    pub use pidash_services::read::OwnedRelationItem;
    pub use pidash_services::read::ProfileRow;
    pub use pidash_services::read::ProjectLiteView;
    pub use pidash_services::read::RenderRequest;
    pub use pidash_services::read::StateRow;
    pub use pidash_services::read::StateRowTuple;
    pub use pidash_services::read::UserShape;
    pub use pidash_services::read::WorkspaceLiteView;
    pub use pidash_services::read::ISSUE_EXTRA_COLS;
    pub use pidash_services::read::ISSUE_SELECT_COLS;
    pub use pidash_services::v1_work_items::shape_issue;
    pub use pidash_services::v1_work_items::shape_issue::issue_url;
    pub use pidash_services::v1_work_items::shape_issue::plan_create;
    pub use pidash_services::v1_work_items::shape_issue::render_issue;
    pub use pidash_services::v1_work_items::shape_issue::run_validate;
    pub use pidash_services::v1_work_items::shape_issue::web_base_url;
    pub use pidash_services::v1_work_items::shape_issue::BlockerSummary;
    pub use pidash_services::v1_work_items::shape_issue::CreatePlan;
    pub use pidash_services::v1_work_items::shape_issue::DescriptionHtmlFacts;
    pub use pidash_services::v1_work_items::shape_issue::HtmlCheck;
    pub use pidash_services::v1_work_items::shape_issue::HtmlOutcome;
    pub use pidash_services::v1_work_items::shape_issue::IssueRow;
    pub use pidash_services::v1_work_items::shape_issue::RenderError;
    pub use pidash_services::v1_work_items::shape_issue::RepresentationInput;
    pub use pidash_services::v1_work_items::shape_issue::SummaryItem;
    pub use pidash_services::v1_work_items::shape_issue::ValidateError;
    pub use pidash_services::v1_work_items::shape_issue::ValidateInput;
    pub use pidash_services::v1_work_items::shape_labels::render_workpad;
    pub use pidash_services::v1_work_items::shape_labels::validate_workpad_write;
    pub use pidash_services::v1_work_items::shape_labels::WorkpadRow;
    pub use pidash_services::v1_work_items::shape_labels::WorkpadWriteInput;
    pub use pidash_types::v1_assets::file_asset::asset_url;
    pub use pidash_types::v1_assets::file_asset::is_description_asset_type;
}

/// Operational flows: orchestration, dispatch tools, runners, the
/// scheduler, user settings, and cloud ops commands.
pub mod ops {
    pub use pidash_db::runner_sessions::outbox::OutboxError;
    pub use pidash_jobs::dispatch::dispatch_waiting;
    pub use pidash_jobs::orchestration::complete_project_move_handoff_inline;
    pub use pidash_jobs::orchestration::drain_publish_effects;
    pub use pidash_jobs::orchestration::fire_pending_entry;
    pub use pidash_jobs::orchestration::pending_entry_ticker_for;
    pub use pidash_jobs::orchestration::post_run_orchestration;
    pub use pidash_jobs::orchestration::post_run_orchestration_in_tx;
    pub use pidash_jobs::orchestration::run_work_item_id;
    pub use pidash_jobs::orchestration::HandoffInlineDeps;
    pub use pidash_jobs::orchestration::HandoffStore;
    pub use pidash_jobs::runner_runs::emit_terminal_effects;
    pub use pidash_jobs::tasks_ticker::scan::emit_fire_tick;
    pub use pidash_ops::connect_primary;
    pub use pidash_ops::run;
    pub use pidash_ops::OpsCommand;
    pub use pidash_services::app_issues::SourceIssueRow;
    pub use pidash_services::dispatch::admission::RedisAdmissionCache;
    pub use pidash_services::dispatch::tools::advisory_lock_key;
    pub use pidash_services::dispatch::tools::py_stripped;
    pub use pidash_services::dispatch::tools::saved_sequence_id;
    pub use pidash_services::dispatch::tools::saved_sort_order;
    pub use pidash_services::dispatch::tools::CREATE_ADVISORY_LOCK_SQL;
    pub use pidash_services::dispatch::tools::CREATE_DEFAULT_POD_SQL;
    pub use pidash_services::dispatch::tools::CREATE_SAVE_DEFAULT_STATE_SQL;
    pub use pidash_services::dispatch::tools::CREATE_SAVE_FALLBACK_STATE_SQL;
    pub use pidash_services::dispatch::tools::CREATE_SEQ_SCAN_SQL;
    pub use pidash_services::dispatch::tools::CREATE_SORT_ORDER_MAX_SQL;
    pub use pidash_services::dispatch::LlmProfile;
    pub use pidash_services::domain::identity::agent_system_user_id;
    pub use pidash_services::domain::identity::GetAgentUserError;
    pub use pidash_services::domain::issues::move_to_project;
    pub use pidash_services::domain::issues::CreationDeps;
    pub use pidash_services::domain::issues::MoveDomainOutcome;
    pub use pidash_services::domain::issues::MoveInput;
    pub use pidash_services::domain::issues::MovePostCommitHook;
    pub use pidash_services::domain::issues::MoveRejection;
    pub use pidash_services::integrations::adapters_github::ClientAuth;
    pub use pidash_services::integrations::adapters_github::GitHubAdapter;
    pub use pidash_services::integrations::adapters_github::GithubClient;
    pub use pidash_services::integrations::adapters_github::GithubError;
    pub use pidash_services::integrations::github_client::LiveGithubClient;
    pub use pidash_services::integrations::github_client::PoolPrStore;
    pub use pidash_services::integrations::github_client::PrLinksSurface;
    pub use pidash_services::integrations::github_client::TransportContext;
    pub use pidash_services::integrations::github_client::TransportGuard;
    pub use pidash_services::orchestration::blockers;
    pub use pidash_services::orchestration::blockers::has_open_blockers_sql;
    pub use pidash_services::orchestration::blockers::summary_list;
    pub use pidash_services::orchestration::blockers::summary_sql;
    pub use pidash_services::orchestration::blockers::BlockerRow;
    pub use pidash_services::orchestration::clock::create_ticker_sql;
    pub use pidash_services::orchestration::clock::effective_interval_seconds;
    pub use pidash_services::orchestration::clock::lock_ticker_sql;
    pub use pidash_services::orchestration::clock::reconcile;
    pub use pidash_services::orchestration::clock::ClockIssue;
    pub use pidash_services::orchestration::clock::ClockWrite;
    pub use pidash_services::orchestration::clock::ProjectClockPolicy;
    pub use pidash_services::orchestration::clock::CREATE_TICKER_PARAMS;
    pub use pidash_services::orchestration::clock::LOCK_TICKER_PARAMS;
    pub use pidash_services::orchestration::clock::SAVE_CLOCK_PARAMS;
    pub use pidash_services::orchestration::clock::SAVE_CLOCK_SQL;
    pub use pidash_services::orchestration::creation::fallback_creator_select;
    pub use pidash_services::orchestration::creation::latest_prior_run_sql;
    pub use pidash_services::orchestration::creation::run_select_sql;
    pub use pidash_services::orchestration::creation::select_parent_for_next_run;
    pub use pidash_services::orchestration::creation::CreateDispatchRequest;
    pub use pidash_services::orchestration::creation::ParentCandidate;
    pub use pidash_services::orchestration::creation::RunView;
    pub use pidash_services::orchestration::creation::StateView;
    pub use pidash_services::orchestration::creation::PROJECT_SELECT_SQL;
    pub use pidash_services::orchestration::creation::TICKER_RESUME_SELECT_SQL;
    pub use pidash_services::orchestration::dispatch::LogLine;
    pub use pidash_services::orchestration::dispatch::LIVE_ASSIGNEE_CANDIDATES_SQL;
    pub use pidash_services::runner_enroll::fire::fire_plan;
    pub use pidash_services::runner_enroll::fire::EffectBundle;
    pub use pidash_services::runner_enroll::fire::FireOp;
    pub use pidash_services::runner_enroll::revoke;
    pub use pidash_services::runner_enroll::revoke::RevokeError;
    pub use pidash_services::runner_enroll::revoke::RevokeOutcome;
    pub use pidash_services::runner_enroll::revoke::RevokeStore;
    pub use pidash_services::runner_runs::chat::next_event_seq;
    pub use pidash_services::runner_runs::chat::next_message_seq;
    pub use pidash_services::runner_runs::finalization;
    pub use pidash_services::runner_runs::LifecycleEffect;
    pub use pidash_services::runner_runs::SetValue;
    pub use pidash_services::runner_sessions::live_drains::drain_for_runner_by_id;
    pub use pidash_services::runner_sessions::live_drains::drain_pod_by_id;
    pub use pidash_services::runner_sessions::live_drains::DrainError;
    pub use pidash_services::runner_sessions::live_drains::PoolPubsubStore;
    pub use pidash_services::runner_sessions::pubsub::send_to_runner;
    pub use pidash_services::scheduler::builtins;
    pub use pidash_services::user_settings::base_settings_schema;
    pub use pidash_services::user_settings::default_for;
    pub use pidash_services::user_settings::extend_settings_schema;
    pub use pidash_services::user_settings::get_setting;
    pub use pidash_services::user_settings::merge_settings;
    pub use pidash_services::user_settings::validate_settings_patch;
    pub use pidash_services::user_settings::CeUserSettings;
    pub use pidash_services::user_settings::SettingsSchema;
    pub use pidash_services::user_settings::UserSettingsSchema;
    pub use pidash_types::orchestration::is_ticking_state;
    pub use pidash_types::orchestration::StateRef;
    pub use pidash_types::orchestration::TickerDecision;
    pub use pidash_types::orchestration::TickerEvent;
    pub use pidash_types::runner_runs::AgentRunStatus;
    pub use pidash_types::runner_runs::AgentRunTrigger;
    pub use pidash_types::runner_runs::TERMINAL_RUN_STATUSES;
}

/// HTTP and Python-compat plumbing: the serializer kernel, `py_*`
/// repr/format helpers, response headers, CSRF/token/session auth,
/// and OAuth provider helpers. OVL-04 (PIDASHCONV-1542) added the
/// `pidash_compat` canonical modules below.
pub mod compat {
    pub use pidash_api::auth_oauth::oauth_google::is_secure_request;
    pub use pidash_api::auth_session::password::password_routes;
    pub use pidash_api::auth_session::render_csrf_failure;
    pub use pidash_api::license::json_response;
    pub use pidash_api::license::MODEL_BACKEND;
    pub use pidash_api::middleware::RequestSession;
    pub use pidash_api::middleware::SessionHandle;
    pub use pidash_api::response_headers::header_value;
    pub use pidash_api::runner_runs::run_endpoints::drain_lifecycle_effects;
    pub use pidash_api::runner_runs::LivePorts;
    pub use pidash_api::runner_runs::PortError;
    pub use pidash_api::runner_runs::RunnerPorts;
    pub use pidash_api::serializer::render_datetime;
    pub use pidash_api::serializer::render_datetime_in;
    pub use pidash_api::space::sanitize::sanitize_html;
    pub use pidash_api::space::sanitize::strip_tags;
    pub use pidash_api::space::sanitize::Sanitize;
    pub use pidash_api::v1_work_items::handlers_actions::gate_fn;
    pub use pidash_api::v1_work_items::handlers_actions::profile_fn;
    pub use pidash_api::v1_work_items::handlers_actions::LiveMatcher;
    pub use pidash_api::v1_work_items::handlers_core::lxml_roundtrip;
    pub use pidash_auth::csrf;
    pub use pidash_auth::csrf::check_token_format;
    pub use pidash_auth::csrf::mask_secret;
    pub use pidash_auth::csrf::mask_secret_with;
    pub use pidash_auth::csrf::new_secret;
    pub use pidash_auth::csrf::tokens_match;
    pub use pidash_auth::csrf::unmask_token;
    pub use pidash_auth::csrf::CsrfError;
    pub use pidash_auth::csrf::CSRF_TOKEN_LENGTH;
    pub use pidash_auth::session::cookie_value;
    pub use pidash_auth::session::http_date;
    pub use pidash_auth::token::classify_token;
    pub use pidash_auth::token::hash_token;
    pub use pidash_auth::token::validate_api_token;
    pub use pidash_auth::token::validate_machine_token_static;
    pub use pidash_auth::token::ApiTokenRow;
    pub use pidash_auth::token::MachineTokenRow;
    pub use pidash_auth::token::TokenKind;
    pub use pidash_auth::token::API_KEY_HEADER;
    pub use pidash_compat::datetime;
    pub use pidash_compat::db;
    pub use pidash_compat::filter;
    pub use pidash_compat::http;
    // Moved from `pidash_api::license` by OVL-04 (same value, canonical home).
    pub use pidash_compat::http::SESSION_AUTH_HASH_SALT;
    pub use pidash_compat::py;
    pub use pidash_compat::response;
    pub use pidash_compat::validate;
    pub use pidash_jobs::json_compact::write_compact_array;
    pub use pidash_jobs::json_compact::write_compact_map;
    pub use pidash_jobs::py_repr;
    pub use pidash_jobs::pyformat::dict_repr;
    pub use pidash_services::auth_oauth::providers::json_truthy;
    pub use pidash_services::auth_oauth::providers::urlencode;
}

/// Assistant runtime: the turn pipeline, the model runtime,
/// crypto/MCP/title/SSRF helpers, and assistant config + event stores.
/// OVL-07 (PIDASHCONV-1545) added the live engine steps, the history
/// codec, the config/probe/retry services, and the remaining event
/// shapes.
pub mod assistant {
    pub use pidash_api::assistant::agent_profile::is_desktop_from_extension;
    pub use pidash_api::assistant::common::actor;
    pub use pidash_api::assistant::common::base_url_blocked;
    pub use pidash_api::assistant::common::body_get;
    pub use pidash_api::assistant::common::crypto_config;
    pub use pidash_api::assistant::common::crypto_config_from_settings;
    pub use pidash_api::assistant::common::crypto_failure;
    pub use pidash_api::assistant::common::decrypt_api_key;
    pub use pidash_api::assistant::common::encrypt_api_key;
    pub use pidash_api::assistant::common::isoformat;
    pub use pidash_api::assistant::common::ok_false;
    pub use pidash_api::assistant::common::ok_false_detail;
    pub use pidash_api::assistant::common::ok_true;
    pub use pidash_api::assistant::common::parse_body;
    pub use pidash_api::assistant::common::pool_of;
    pub use pidash_api::assistant::common::py_scalar_display;
    pub use pidash_api::assistant::common::request_content_type;
    pub use pidash_api::assistant::common::run_char_field;
    pub use pidash_api::assistant::common::validate_choice;
    pub use pidash_api::assistant::common::validate_django_url;
    pub use pidash_api::assistant::common::BodyField;
    pub use pidash_api::assistant::common::BodyMap;
    pub use pidash_api::assistant::common::CharFieldRules;
    pub use pidash_api::assistant::common::Failure;
    pub use pidash_api::assistant::common::ParsedBody;
    pub use pidash_api::assistant::events::py_dumps;
    pub use pidash_api::assistant::governor;
    pub use pidash_api::assistant::governor::Governor;
    pub use pidash_api::assistant::kms::HttpKmsTransport;
    pub use pidash_api::assistant::llm_config::validate_put;
    pub use pidash_api::assistant::model_seam::CeAssistantSeam;
    pub use pidash_api::assistant::throttles;
    pub use pidash_api::assistant::throttles::spec_for;
    pub use pidash_api::assistant::throttles::ThrottlePoint;
    pub use pidash_api::assistant::throttles::ThrottleSpec;
    pub use pidash_api::assistant::throttles::Throttled;
    pub use pidash_api::assistant::throttles::LLM_TEST_THROTTLE;
    pub use pidash_api::turn::history::dump_blob;
    pub use pidash_api::turn::history::dump_message;
    pub use pidash_api::turn::history::now_stamp;
    pub use pidash_api::turn::history::parse_blob;
    pub use pidash_api::turn::history::parse_retry_details;
    pub use pidash_api::turn::history::retry_message;
    pub use pidash_api::turn::history::CodecError;
    pub use pidash_api::turn::history::RetryErrorDetail;
    pub use pidash_api::turn::history::RetryLocSeg;
    pub use pidash_api::turn::history::RunIds;
    pub use pidash_api::turn::history::TurnFinish;
    pub use pidash_api::turn::history::TurnMessage;
    pub use pidash_api::turn::history::TurnOutcome;
    pub use pidash_api::turn::history::TurnRequestPart;
    pub use pidash_api::turn::history::TurnResponsePart;
    pub use pidash_api::turn::history::TurnRetryContent;
    pub use pidash_api::turn::history::TurnState;
    pub use pidash_api::turn::live::create_message;
    pub use pidash_api::turn::live::mcp_max_servers;
    pub use pidash_api::turn::live::mcp_timeouts;
    pub use pidash_api::turn::live::publish_event;
    pub use pidash_api::turn::live::LiveStream;
    pub use pidash_api::turn::live::LiveToolset;
    pub use pidash_api::turn::live::LiveTurnSeam;
    pub use pidash_api::turn::mcp_client::CallResult;
    pub use pidash_api::turn::mcp_client::McpContent;
    pub use pidash_api::turn::mcp_client::McpError;
    pub use pidash_api::turn::mcp_client::McpToolDef;
    pub use pidash_api::turn::mcp_client::McpValue;
    pub use pidash_api::turn::mcp_client::ResilientSession;
    pub use pidash_db::assistant::event_queries::decode_history_blobs;
    pub use pidash_db::assistant::event_queries::history_limit;
    pub use pidash_db::assistant::event_queries::load_history_sql;
    pub use pidash_db::assistant::event_queries::message_envelope_json;
    pub use pidash_db::assistant::event_queries::next_event_seq_sql;
    pub use pidash_db::assistant::event_queries::next_message_seq_sql;
    pub use pidash_db::assistant::event_queries::publish_frame;
    pub use pidash_db::assistant::event_queries::serialize_event_json;
    pub use pidash_db::assistant::event_queries::truncate_kind;
    pub use pidash_db::assistant::event_queries::EventParts;
    pub use pidash_db::assistant::event_queries::MessageParts;
    pub use pidash_db::assistant::event_queries::HISTORY_STATUS;
    pub use pidash_db::assistant::models::assistant_mcp_server;
    pub use pidash_db::assistant::models::assistant_mcp_server::AssistantMCPServer;
    pub use pidash_db::assistant::models::has_secret;
    pub use pidash_db::assistant::models::user_llm_config;
    pub use pidash_db::assistant::models::user_llm_config::UserLLMConfig;
    pub use pidash_db::assistant::models::ThreadKind;
    pub use pidash_jobs::assistant::classify_provider_error;
    pub use pidash_jobs::assistant::register_assistant_tasks;
    pub use pidash_jobs::assistant::register_turn_handler;
    pub use pidash_jobs::assistant::run_turn::cancel_key;
    pub use pidash_jobs::assistant::run_turn::cancel_turn_update;
    pub use pidash_jobs::assistant::run_turn::clear_active_turn_update;
    pub use pidash_jobs::assistant::run_turn::context_eligible;
    pub use pidash_jobs::assistant::run_turn::drive_turn;
    pub use pidash_jobs::assistant::run_turn::lock_turn_select;
    pub use pidash_jobs::assistant::run_turn::mark_running_update;
    pub use pidash_jobs::assistant::run_turn::skipped_payload;
    pub use pidash_jobs::assistant::run_turn::thread_lock_select;
    pub use pidash_jobs::assistant::run_turn::truncate_code;
    pub use pidash_jobs::assistant::run_turn::truncate_detail;
    pub use pidash_jobs::assistant::run_turn::AgentError;
    pub use pidash_jobs::assistant::run_turn::DriveResult;
    pub use pidash_jobs::assistant::run_turn::ModelError;
    pub use pidash_jobs::assistant::run_turn::SeamError;
    pub use pidash_jobs::assistant::run_turn::SkippedServer;
    pub use pidash_jobs::assistant::run_turn::StreamItem;
    pub use pidash_jobs::assistant::run_turn::ToolsetHandle;
    pub use pidash_jobs::assistant::run_turn::TurnContext;
    pub use pidash_jobs::assistant::run_turn::TurnSeam;
    pub use pidash_jobs::assistant::run_turn::TurnUsage;
    pub use pidash_jobs::assistant::run_turn::EVENT_ASSISTANT_DELTA;
    pub use pidash_jobs::assistant::run_turn::EVENT_MESSAGE_COMPLETED;
    pub use pidash_jobs::assistant::run_turn::EVENT_MESSAGE_CREATED;
    pub use pidash_jobs::assistant::run_turn::EVENT_TOOL_SERVERS_SKIPPED;
    pub use pidash_jobs::assistant::run_turn::EVENT_TURN_CANCELLED;
    pub use pidash_jobs::assistant::run_turn::EVENT_TURN_COMPLETED;
    pub use pidash_jobs::assistant::run_turn::EVENT_TURN_FAILED;
    pub use pidash_jobs::assistant::run_turn::EVENT_TURN_STARTED;
    pub use pidash_jobs::assistant::run_turn::KIND_ASSISTANT;
    pub use pidash_jobs::assistant::run_turn::KIND_ERROR;
    pub use pidash_jobs::assistant::run_turn::MSG_CANCELLED;
    pub use pidash_jobs::assistant::run_turn::MSG_COMPLETED;
    pub use pidash_jobs::assistant::run_turn::MSG_FAILED;
    pub use pidash_jobs::assistant::run_turn::MSG_STREAMING;
    pub use pidash_jobs::assistant::run_turn::TABLE_EVENT;
    pub use pidash_jobs::assistant::run_turn::TABLE_MESSAGE;
    pub use pidash_jobs::assistant::run_turn::TURN_HARD_LIMIT_SECS;
    pub use pidash_jobs::assistant::run_turn::TURN_SOFT_LIMIT_SECS;
    pub use pidash_jobs::assistant::RUN_TURN_TASK;
    pub use pidash_jobs::assistant::SWEEP_TASK;
    pub use pidash_jobs::oss_worker::model_runtime::github::installation_token;
    pub use pidash_jobs::oss_worker::model_runtime::github::GithubAppContext;
    pub use pidash_jobs::oss_worker::model_runtime::github::GithubFailure;
    pub use pidash_jobs::oss_worker::model_runtime::github::GithubRestClient;
    pub use pidash_jobs::oss_worker::model_runtime::github::CLIENT_USER_AGENT;
    pub use pidash_jobs::oss_worker::model_runtime::keys::KeyResolver;
    pub use pidash_jobs::oss_worker::model_runtime::provider::Lane;
    pub use pidash_jobs::oss_worker::model_runtime::provider::ProviderError;
    pub use pidash_services::assistant::config::delete_config;
    pub use pidash_services::assistant::config::get_config;
    pub use pidash_services::assistant::config::put_config;
    pub use pidash_services::assistant::config::ConfigDeps;
    pub use pidash_services::assistant::config::ConfigError;
    pub use pidash_services::assistant::config::LlmConfigRow;
    pub use pidash_services::assistant::config::ValidatedPut;
    pub use pidash_services::assistant::crypto;
    pub use pidash_services::assistant::crypto::decrypt;
    pub use pidash_services::assistant::crypto::encrypt;
    pub use pidash_services::assistant::crypto::CryptoConfig;
    pub use pidash_services::assistant::crypto::CryptoError;
    pub use pidash_services::assistant::crypto::KmsTransport;
    pub use pidash_services::assistant::crypto::KmsTransportError;
    pub use pidash_services::assistant::llm;
    pub use pidash_services::assistant::llm::resolve_byok_model;
    pub use pidash_services::assistant::llm::ModelRef;
    pub use pidash_services::assistant::mcp;
    pub use pidash_services::assistant::mcp::build_toolset_spec;
    pub use pidash_services::assistant::mcp::ResilientState;
    pub use pidash_services::assistant::mcp::ToolsetSpec;
    pub use pidash_services::assistant::probe::classify_probe;
    pub use pidash_services::assistant::probe::probe_provider;
    pub use pidash_services::assistant::probe::ProbeOutcome;
    pub use pidash_services::assistant::retry::retry_after_seconds;
    pub use pidash_services::assistant::retry::should_retry_status;
    pub use pidash_services::assistant::retry::sleep_for_retry;
    pub use pidash_services::assistant::retry::INITIAL_RETRY_DELAY_SECS;
    pub use pidash_services::assistant::retry::MAX_RETRIES;
    pub use pidash_services::assistant::retry::MAX_RETRY_DELAY_SECS;
    pub use pidash_services::assistant::retry::RETRY_AFTER_MAX_SECS;
    pub use pidash_services::assistant::seams::agent_model_credential_for_user;
    pub use pidash_services::assistant::ssrf::is_blocked;
    pub use pidash_services::assistant::ssrf::SystemResolver;
    pub use pidash_services::assistant::title::clean_title;
    pub use pidash_services::assistant::title::content_to_text;
    pub use pidash_services::assistant::title::strip_reasoning_blocks;
    pub use pidash_services::assistant::title::TITLE_MAX_LEN;
    pub use pidash_services::assistant::title::TITLE_MAX_OUTPUT_TOKENS;
    pub use pidash_services::assistant::title::TITLE_SYSTEM_PROMPT;
    pub use pidash_services::domain::assistant::append_event;
    pub use pidash_services::domain::assistant::complete_turn;
    pub use pidash_services::domain::assistant::fail_turn;
    pub use pidash_services::domain::assistant::finalize_open_messages;
    pub use pidash_services::domain::assistant::prune_turn_deltas;
    pub use pidash_services::domain::assistant::NewEvent;
    pub use pidash_types::assistant::errors::AssistantError;
    pub use pidash_types::assistant::serializers;
    pub use pidash_types::assistant::serializers::PROVIDER_ANTHROPIC;
    pub use pidash_types::assistant::serializers::PROVIDER_OPENAI_COMPATIBLE;
}

/// Worker, queue and task registry: handler registration, schedules,
/// and task-name constants.
pub mod jobs {
    pub use pidash_jobs::admission::RedisAdmissionCache;
    pub use pidash_jobs::default_schedule;
    pub use pidash_jobs::dispatch_execute::extra_toolsets_allowed;
    pub use pidash_jobs::integrations::git_sync::TASK_NAMES as GIT_SYNC_TASK_NAMES; // renamed: TASK_NAMES clashes across task modules.
    pub use pidash_jobs::integrations::github_signals::dispatch_completion_comment;
    pub use pidash_jobs::integrations::github_signals::dispatch_job;
    pub use pidash_jobs::integrations::github_signals::DispatchOutcome;
    pub use pidash_jobs::integrations::github_signals::DispatchTarget;
    pub use pidash_jobs::integrations::github_sync::TASK_NAMES as GITHUB_SYNC_TASK_NAMES; // renamed: TASK_NAMES clashes across task modules.
    pub use pidash_jobs::license::tasks::TASK_NAME;
    pub use pidash_jobs::loop_scan::TASK_NAMES as LOOP_SCAN_TASK_NAMES; // renamed: TASK_NAMES clashes across task modules.
    pub use pidash_jobs::orchestration::create_and_dispatch_run;
    pub use pidash_jobs::orchestration::preflight_eligibility_or_bounce;
    pub use pidash_jobs::orchestration::runner_seams::complete_project_move_handoff_live;
    pub use pidash_jobs::orchestration::StoreDeps;
    pub use pidash_jobs::queue::enqueue;
    pub use pidash_jobs::queue::ensure_schema;
    pub use pidash_jobs::queue::ensure_schema_sql;
    pub use pidash_jobs::queue::JobRow;
    pub use pidash_jobs::queue::NewJob;
    pub use pidash_jobs::register_oss_handlers;
    pub use pidash_jobs::runner_runs::TASK_NAMES as RUNNER_RUNS_TASK_NAMES; // renamed: TASK_NAMES clashes across task modules.
    pub use pidash_jobs::scheduler::run_scheduler;
    pub use pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK; // also imported by the overlay as `pidash_jobs::tasks_cleanup::SOFT_DELETE_TASK`.
    pub use pidash_jobs::tasks_cleanup::EXPORT_TASK_NAMES;
    pub use pidash_jobs::tasks_cleanup::RESTORE_TASK_NAME;
    pub use pidash_jobs::tasks_cleanup::TASK_DELETE_OLD_S3_LINK;
    pub use pidash_jobs::tasks_export::ANALYTIC_EXPORT_TASK;
    pub use pidash_jobs::tasks_export::ISSUE_EXPORT_TASK;
    pub use pidash_jobs::tasks_mail::NOTIFICATIONS_TASK;
    pub use pidash_jobs::tasks_mail::SEND_EMAIL_NOTIFICATION_TASK;
    pub use pidash_jobs::tasks_mail::STACK_EMAIL_NOTIFICATION_TASK;
    pub use pidash_jobs::tasks_ticker::scan::SCAN_DUE_TICKERS_TASK;
    pub use pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps;
    pub use pidash_jobs::tasks_webhooks::activity_dispatch::ISSUE_ACTIVITY_TASK; // also imported by the overlay as `pidash_jobs::tasks_webhooks::ISSUE_ACTIVITY_TASK`.
    pub use pidash_jobs::tasks_webhooks::webhook_fanout::MODEL_ACTIVITY_TASK_NAME;
    pub use pidash_jobs::tasks_webhooks::ARCHIVE_AND_CLOSE_TASK;
    pub use pidash_jobs::tasks_webhooks::PROCESS_LOGS_TASK;
    pub use pidash_jobs::tasks_webhooks::TRACK_EVENT_TASK;
    pub use pidash_jobs::worker::run_worker;
    pub use pidash_jobs::worker::Handler;
    pub use pidash_jobs::worker::Registry; // also imported by the overlay as `pidash_jobs::Registry`.
    pub use pidash_jobs::worker::Verdict;
    pub use pidash_jobs::OssWorkerCtx;
    pub use pidash_jobs::TaskMessage;
    pub use pidash_jobs::WorkerConfig;
    pub use pidash_types::dispatch::AgentExecutorKind;
}

/// Database layer: pools, settings/config, transactions, and query helpers.
pub mod db {
    pub use pidash_db::app_integrations::queries_github::DEFAULT_TIMEOUT_SECONDS;
    pub use pidash_db::app_integrations::queries_github::GITHUB_ACCEPT_HEADER;
    pub use pidash_db::app_integrations::queries_github::GITHUB_API_BASE;
    pub use pidash_db::app_integrations::queries_github::GITHUB_API_VERSION_HEADER;
    pub use pidash_db::app_pages::strip::ml_strip_tags;
    pub use pidash_db::app_project::models::project_member;
    pub use pidash_db::config::registry::ConfigRegistry;
    pub use pidash_db::config::registry::ConfigSource;
    pub use pidash_db::config::settings::CloudAgentSettings;
    pub use pidash_db::config::settings::ManagedRunnerSettings;
    pub use pidash_db::config::settings::RunnerSettings;
    pub use pidash_db::config::AssistantSettings;
    pub use pidash_db::config::NoOverlay;
    pub use pidash_db::config::Profile;
    pub use pidash_db::config::Settings;
    pub use pidash_db::config::SettingsOverlay;
    pub use pidash_db::dispatch::status::AgentRunStatus;
    pub use pidash_db::license::models::instance::TABLE;
    pub use pidash_db::license::models::instance_admin;
    pub use pidash_db::license::models::instance_configuration;
    pub use pidash_db::license::queries::admin_get_by_instance_user_sql;
    pub use pidash_db::license::queries::fetch_instance_first;
    pub use pidash_db::license::queries::map_instance_admin_row;
    pub use pidash_db::license::queries::map_instance_row;
    pub use pidash_db::license::queries::ADMIN_INSERT_SQL;
    pub use pidash_db::license::queries::ADMIN_PERMISSION_ROLE_GTE;
    pub use pidash_db::license::queries::INSTANCE_FIRST_SQL;
    pub use pidash_db::orchestration::querysets::fetch_is_workspace_member;
    pub use pidash_db::orchestration::runs::active_run_for;
    pub use pidash_db::orchestration::runs::latest_prior_run;
    pub use pidash_db::pool::Pools; // also imported by the overlay as `pidash_db::Pools`.
    pub use pidash_db::redis::RedisHandle;
    pub use pidash_db::runner_enroll::columns::machine_token;
    pub use pidash_db::runner_enroll::columns::runner;
    pub use pidash_db::runner_sessions::models::runner_session;
    pub use pidash_db::runner_sessions::outbox::schedule_stream_cleanup_for_runner;
    pub use pidash_db::soft_delete::DELETED_AT;
    pub use pidash_db::tasks_cleanup::cleanup_queries::MongoSink;
    pub use pidash_db::tasks_ticker::jitter_seconds;
    pub use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
    pub use pidash_db::tx::Transaction;
    pub use pidash_db::DbConfig;
}

/// Test-only helpers shared with the overlay. Empty: the seed
/// imports no test-only items; OVL-03 (PIDASHCONV-1541)..OVL-07
/// (PIDASHCONV-1545) add what the overlay harness needs.
pub mod testkit {}
