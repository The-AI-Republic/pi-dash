//! Seed helpers for the read-surface live tests.
//!
//! Mirrors the contract-harness minimal rows
//! (`contract-tests/_harness/factory.py`, `db.py`, `seed.py`): every
//! `NOT NULL` column without a database default is bound explicitly.
//! Each seed mints fresh ids, so tests share one scratch database
//! without colliding.

use sqlx::PgPool;
use uuid::Uuid;

/// Connect to the scratch database, or `None` (skip) when
/// `DATABASE_URL` is unset — the `entity::parity` live-db pattern.
pub(crate) async fn pool() -> Option<PgPool> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        }
    };
    Some(
        PgPool::connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL"),
    )
}

/// One seeded world: user + profile + workspace + project + state +
/// issue + comment + asset, all cross-linked.
pub(crate) struct World {
    pub user_id: Uuid,
    pub user_email: String,
    pub profile_id: Uuid,
    pub workspace_id: Uuid,
    pub workspace_slug: String,
    pub project_id: Uuid,
    pub project_identifier: String,
    pub state_id: Uuid,
    pub issue_id: Uuid,
    pub sequence_id: i32,
    pub comment_id: Uuid,
    pub asset_id: Uuid,
}

pub(crate) async fn seed_world(pool: &PgPool) -> World {
    let user_id = Uuid::new_v4();
    let profile_id = Uuid::new_v4();
    let workspace_id = Uuid::new_v4();
    let slug = format!("ws-{}", workspace_id.simple());
    let project_id = Uuid::new_v4();
    let state_id = Uuid::new_v4();
    let issue_id = Uuid::new_v4();
    let comment_id = Uuid::new_v4();
    let asset_id = Uuid::new_v4();
    let username = format!("seed-{}", user_id.simple());
    let email = format!("{}@example.com", username);
    sqlx::query(
        "INSERT INTO users (id, password, username, email, first_name, last_name, avatar,\
         date_joined, created_at, updated_at, last_location, created_location,\
         is_superuser, is_managed, is_password_expired, is_active, is_staff,\
         is_email_verified, is_password_autoset, token, user_timezone,\
         last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,\
         is_bot, display_name, is_email_valid, is_password_reset_required)\
         VALUES ($1,'pbkdf2_sha256$600000$x$y',$2,$3,'Seed','User','',\
         now(), now(), now(), '','', false,false,false,true,false,false,false,'','UTC',\
         '','','email','',false,'Seed User',false,false)",
    )
    .bind(user_id)
    .bind(&username)
    .bind(&email)
    .execute(pool)
    .await
    .expect("seed user");
    sqlx::query(
        "INSERT INTO profiles (id, created_at, updated_at, theme, is_tour_completed,\
         onboarding_step, is_onboarded, billing_address_country, has_billing_address,\
         company_name, user_id, is_mobile_onboarded, mobile_onboarding_step,\
         mobile_timezone_auto_set, language, is_smooth_cursor_enabled, start_of_the_week,\
         is_app_rail_docked, background_color, goals, has_marketing_email_consent,\
         is_navigation_tour_completed, is_subscribed_to_changelog, notification_view_mode,\
         product_tour, settings)\
         VALUES ($1, now(), now(), '{}', false, '{}', false, 'INDIA', false, '', $2,\
         false, '{}', false, 'en', false, 1, true, '#ffffff', '{}', false, false, false,\
         'full', '{}', '{}')",
    )
    .bind(profile_id)
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed profile");
    sqlx::query(
        "INSERT INTO workspaces (id, name, slug, owner_id, timezone, background_color,\
         created_at, updated_at) VALUES ($1,'Seed WS',$2,$3,'UTC','#ffffff', now(), now())",
    )
    .bind(workspace_id)
    .bind(&slug)
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed workspace");
    sqlx::query(
        "INSERT INTO projects (id, created_at, updated_at, name, description, network,\
         identifier, workspace_id, cycle_view, module_view, issue_views_view, page_view,\
         intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,\
         is_issue_type_enabled, guest_view_all_features, timezone, members_can_edit_states,\
         repo_url, base_branch, agent_default_interval_seconds, agent_default_max_ticks,\
         agent_ticking_enabled, is_default, agent_review_default_interval_seconds,\
         default_agent_executor, agent_test_default_interval_seconds)\
         VALUES ($1, now(), now(), 'Seed Project', '', 0, 'SEED', $2, true, true, true, true,\
         true, 0, 0, '{}', false, false, false, 'UTC', false, '', '', 0, 0, false, false,\
         0, '', 0)",
    )
    .bind(project_id)
    .bind(workspace_id)
    .execute(pool)
    .await
    .expect("seed project");
    sqlx::query(
        "INSERT INTO states (id, created_at, updated_at, name, description, color, slug,\
         project_id, workspace_id, sequence, \"group\", \"default\", is_triage)\
         VALUES ($1, now(), now(), 'Seed Backlog', '', '#ff0000', 'seed-backlog', $2, $3,\
         1, 'backlog', true, false)",
    )
    .bind(state_id)
    .bind(project_id)
    .bind(workspace_id)
    .execute(pool)
    .await
    .expect("seed state");
    sqlx::query(
        "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority,\
         sequence_id, project_id, state_id, workspace_id, description_html, sort_order,\
         is_draft, git_work_branch, workpad, complexity_score)\
         VALUES ($1, now(), now(), 'Seed issue', '{}', 'high', 101, $2, $3, $4, '', 65535,\
         false, '', '', 0)",
    )
    .bind(issue_id)
    .bind(project_id)
    .bind(state_id)
    .bind(workspace_id)
    .execute(pool)
    .await
    .expect("seed issue");
    sqlx::query(
        "INSERT INTO issue_comments (id, workspace_id, project_id, comment_stripped,\
         comment_json, comment_html, attachments, labels, issue_id, actor_id, access,\
         speaker_type, speaker_label, created_at, updated_at)\
         VALUES ($1,$2,$3,'seed comment','{}','<p>seed comment</p>','{}','{}',$4,$5,\
         'EXTERNAL','human','',now(),now())",
    )
    .bind(comment_id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(issue_id)
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed comment");
    sqlx::query(
        "INSERT INTO file_assets (id, attributes, asset, entity_type, is_deleted,\
         is_archived, size, is_uploaded, created_at, updated_at, workspace_id)\
         VALUES ($1, '{}', 'seed/avatar.png', 'USER_AVATAR', false, false, 0, false,\
         now(), now(), $2)",
    )
    .bind(asset_id)
    .bind(workspace_id)
    .execute(pool)
    .await
    .expect("seed asset");
    World {
        user_id,
        user_email: email,
        profile_id,
        workspace_id,
        workspace_slug: slug,
        project_id,
        project_identifier: "SEED".to_owned(),
        state_id,
        issue_id,
        sequence_id: 101,
        comment_id,
        asset_id,
    }
}
