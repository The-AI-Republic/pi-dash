//! D-17 authentication guards (api layer, stage 5).
//!
//! Ports the guard units of
//! `apps/api/pi_dash/authentication/views/cli/device.py` (flow constants,
//! `DeviceCodeStartThrottle`, `_verification_uri`, the six endpoint gate
//! triples) and the auth shape of the sixteen OAuth views
//! (`authentication/views/app+space/{google,github,gitlab,gitea}.py`).
//!
//! * [`guards`] — constants, throttle spec, URI builder, device matrix,
//!   OAuth view shape (PIDASHCONV-331, AUTHOAUTH-F9).
//! * [`login`] — shared `django.contrib.auth.login()` session semantics
//!   for the four providers (PIDASHCONV-1357).
//! * [`device_flow`] — device start / approve / token-poll handlers +
//!   route registration (PIDASHCONV-342, AUTHOAUTH-F12).
//! * [`device_session`] — workspaces list, machine-token exchange, revoke
//!   handlers + routes (PIDASHCONV-343, AUTHOAUTH-F12).
//! * [`oauth_gitea`] — Gitea app + space initiate/callback handlers +
//!   routes (PIDASHCONV-341, AUTHOAUTH-F10/F11).
//! * [`oauth_github`] — GitHub app + space initiate/callback handlers +
//!   routes (PIDASHCONV-336, AUTHOAUTH-F10/F11 github rows).
//! * [`oauth_gitlab`] — GitLab app + space initiate/callback handlers +
//!   routes (PIDASHCONV-339, AUTHOAUTH-F10/F11).
//! * [`oauth_google`] — Google app + space initiate/callback handlers +
//!   routes (PIDASHCONV-335, AUTHOAUTH-F10/F11).
//!
//! Each provider mounts its own router under the Auth group arm.
//!
//! [`routes`] merges both ported device families; merges keep both sides.
//! Provider families expose their own sub-routers, mounted on the Auth
//! group arm in `overlay.rs` (PIDASHCONV-341 precedent).
//!
//! # Wiring note
//!
//! The crate root declares `pub mod auth_oauth;` (one-line wiring in the
//! port PR, following the PIDASHCONV-284 precedent).

pub mod device_flow;
pub mod device_session;
pub mod guards;
pub mod login;
pub mod oauth_gitea;
pub mod oauth_github;
pub mod oauth_gitlab;
pub mod oauth_google;

pub use guards::{
    device_endpoint_guard, device_start_cache_key, device_start_throttle_rate,
    initiate_session_keys, verification_uri, DeviceEndpointGuard, OauthProvider,
    DEVICE_CODE_MIN_POLL_GAP_SECS, DEVICE_CODE_POLL_INTERVAL_SECS, DEVICE_CODE_START_MAX_RETRIES,
    DEVICE_CODE_TTL_SECS, DEVICE_ENDPOINT_GUARDS, DEVICE_START_THROTTLE_RATE_STR,
    DEVICE_START_THROTTLE_SCOPE, OAUTH_APP_INITIATE_SESSION_KEYS, OAUTH_CALLBACK_SESSION_KEYS,
    OAUTH_SPACE_GITEA_INITIATE_SESSION_KEYS, OAUTH_SPACE_INITIATE_SESSION_KEYS, OAUTH_VIEW_BASE,
    OAUTH_VIEW_METHOD,
};

/// Routes for the ported device families (PIDASHCONV-342 + PIDASHCONV-343).
pub fn routes() -> axum::Router<crate::state::AppState> {
    device_flow::routes().merge(device_session::routes())
}

/// Shared `delete_old_avatar` row write (`adapter/base.py:244-248`,
/// PIDASHCONV-1321): soft-delete the asset the way
/// `SoftDeleteModel.delete(soft=True)` does (`db/mixins.py:72-78` —
/// `deleted_at` stamp through a full `save()`, so `updated_at` moves and,
/// with the anonymous callback user, both audit columns NULL), enqueue
/// `soft_delete_related_objects("db", "fileasset", pk, using=None)`
/// best-effort (the intake-handler precedent), then clear the user's FK +
/// avatar string. Django order (asset delete — stamp AND enqueue — before
/// the user save) is safe here because the row write is an UPDATE, not a
/// DELETE: no deferred FK can fire. A failed asset write returns with the
/// user untouched (Django's `except` around the whole body).
pub(crate) async fn soft_delete_avatar_asset(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    asset_id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) {
    if sqlx::query(
        r#"UPDATE "file_assets"
           SET "deleted_at" = $1, "updated_at" = $1,
               "created_by_id" = NULL, "updated_by_id" = NULL
           WHERE "id" = $2"#,
    )
    .bind(now)
    .bind(asset_id)
    .execute(pool)
    .await
    .is_err()
    {
        return;
    }
    let mut kwargs = serde_json::Map::new();
    kwargs.insert("using".to_owned(), serde_json::Value::Null);
    let message = pidash_jobs::TaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            serde_json::Value::String("db".to_owned()),
            serde_json::Value::String("fileasset".to_owned()),
            serde_json::Value::String(asset_id.to_string()),
        ],
        kwargs,
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "avatar soft-delete enqueue failed; response stands");
    }
    let _ = sqlx::query(
        r#"UPDATE "users" SET "avatar_asset_id" = NULL, "avatar" = '' WHERE "id" = $1"#,
    )
    .bind(user_id)
    .execute(pool)
    .await;
}
