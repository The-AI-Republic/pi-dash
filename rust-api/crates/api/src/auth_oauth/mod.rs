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

/// Shared `download_and_upload_avatar` (`adapter/base.py:139-218`,
/// PIDASHCONV-1323): provider download (10s timeout, `Content-Length`
/// pre-check, exact-match content-type allowlist, streaming size guard),
/// SigV4 PUT through the shared signer, HEAD metadata, `FileAsset` row.
/// ANY failure returns `None` so the caller falls back to the provider URL
/// (`except Exception: log_exception; return None`). `provider` names the
/// `attributes["name"]` (`{provider}-avatar.{extension}`); `max_bytes` is
/// `settings.DATA_UPLOAD_MAX_MEMORY_SIZE` (`FILE_SIZE_LIMIT`, default
/// `5242880`). `storage` / `scheme` / `host` mirror the
/// `S3Storage(request)` server-side call the avatar flow makes
/// (`base.py:198`, `is_server=False`). The google and gitea callbacks keep
/// their own older copies; github and gitlab share this one.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn download_and_upload_avatar(
    client: &reqwest::Client,
    pool: &sqlx::PgPool,
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    provider: &str,
    avatar_url: &str,
    user_id: uuid::Uuid,
    max_bytes: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<uuid::Uuid> {
    if avatar_url.is_empty() {
        return None;
    }
    // `get_avatar_download_headers` (`base.py:122-123`): empty for the
    // base adapter — no provider overrides it.
    let response = match client
        .get(avatar_url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "avatar download failed; falling back to provider URL");
            return None;
        }
    };
    let mut response = match response.error_for_status() {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "avatar download status failed; falling back to provider URL");
            return None;
        }
    };
    let headers = response.headers().clone();
    // `if content_length and int(content_length) > max_size: return None`
    // (`base.py:155-158`): a missing (or empty) header imposes no limit;
    // an unparseable one raises in Python, i.e. the same `None` outcome. A
    // present-but-undecodable value never parses either — requests would
    // hand Python a raw string that `int()` rejects.
    match headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
    {
        None if headers.contains_key(reqwest::header::CONTENT_LENGTH) => {
            tracing::warn!("avatar undecodable length; falling back to provider URL");
            return None;
        }
        None | Some("") => {}
        Some(raw) => match raw.parse::<i64>() {
            Ok(length)
                if pidash_storage::content_length_allowed(Some(length), max_bytes as i64) => {}
            _ => {
                tracing::warn!(
                    "avatar over size limit or bad length; falling back to provider URL"
                );
                return None;
            }
        },
    }
    // Raw header value, defaulting exactly like Python (`base.py:160`);
    // the exact-match allowlist (and its bug) lives in the shared helper.
    // A present-but-undecodable value misses the allowlist, like Python's
    // raw-string lookup miss — only a truly missing header defaults.
    let content_type = match headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => value.to_owned(),
        None if headers.contains_key(reqwest::header::CONTENT_TYPE) => {
            tracing::warn!("avatar undecodable content type; falling back to provider URL");
            return None;
        }
        None => "image/jpeg".to_owned(),
    };
    let extension = match pidash_storage::avatar_extension(Some(&content_type)) {
        Some(extension) => extension,
        None => {
            tracing::warn!("avatar content type not allowed; falling back to provider URL");
            return None;
        }
    };
    let mut content = Vec::new();
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(%error, "avatar stream failed; falling back to provider URL");
                return None;
            }
        };
        if !pidash_storage::chunk_fits(content.len() as u64, chunk.len() as u64, max_bytes) {
            tracing::warn!("avatar over size limit; falling back to provider URL");
            return None;
        }
        content.extend_from_slice(&chunk);
    }
    let file_size = content.len() as i64;
    // `f"{uuid4hex}-user-avatar.{extension}"` (`base.py:188`): the S3 key
    // and, verbatim, the `asset` column (`objects.create` stores the raw
    // string — `upload_to` never applies).
    let filename =
        pidash_storage::avatar_object_key(&uuid::Uuid::new_v4().simple().to_string(), extension);
    if pidash_storage::validate_storage(storage).is_err() {
        tracing::warn!("avatar storage misconfigured; falling back to provider URL");
        return None;
    }
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let Ok(signed) = pidash_storage::sign_put(
        storage,
        scheme,
        host,
        &filename,
        &content_type,
        &content,
        &amz_date,
        &date_stamp,
    ) else {
        tracing::warn!("avatar storage endpoint invalid; falling back to provider URL");
        return None;
    };
    let put = match client
        .put(signed.url)
        .header("Authorization", signed.authorization)
        .header("x-amz-date", signed.amz_date)
        .header("x-amz-content-sha256", signed.content_sha256)
        .header(reqwest::header::CONTENT_TYPE, content_type.clone())
        .body(content)
        .send()
        .await
    {
        Ok(put) => put,
        Err(error) => {
            tracing::warn!(%error, "avatar S3 PUT failed; falling back to provider URL");
            return None;
        }
    };
    if !put.status().is_success() {
        tracing::warn!(status = %put.status(), "avatar S3 PUT rejected; falling back to provider URL");
        return None;
    }
    // `get_object_metadata` (`storage.py:156-174`): HEAD the stored key; a
    // HEAD failure records `None` (NULL metadata) while the row is still
    // created.
    let metadata = head_storage_metadata(client, storage, scheme, host, &filename, &now).await;
    let asset_id = uuid::Uuid::new_v4();
    if insert_avatar_file_asset(
        pool,
        asset_id,
        &filename,
        provider,
        extension,
        &content_type,
        file_size,
        user_id,
        metadata,
        now,
    )
    .await
    .is_err()
    {
        tracing::warn!("avatar FileAsset insert failed; falling back to provider URL");
        return None;
    }
    Some(asset_id)
}

/// Shared `S3Storage.get_object_metadata` (`storage.py:156-174`) executed
/// over a shared-signed HEAD: the response headers projected through the
/// shared mapper (`LastModified` HTTP-date → ISO-8601, `Metadata` the
/// `x-amz-meta-*` map — `{}` for avatar uploads). `None` on any failure,
/// exactly Python's logged-and-`None` contract.
pub(crate) async fn head_storage_metadata(
    client: &reqwest::Client,
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_key: &str,
    now: &chrono::DateTime<chrono::Utc>,
) -> Option<serde_json::Value> {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let Ok(signed) =
        pidash_storage::sign_head(storage, scheme, host, object_key, &amz_date, &date_stamp)
    else {
        return None;
    };
    let head = client
        .head(signed.url)
        .header("Authorization", signed.authorization)
        .header("x-amz-date", signed.amz_date)
        .header("x-amz-content-sha256", signed.content_sha256)
        .send()
        .await
        .ok()?;
    if !head.status().is_success() {
        return None;
    }
    let headers = head.headers();
    let header_str = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let content_length = headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let last_modified = header_str("last-modified")
        .and_then(|value| chrono::DateTime::parse_from_rfc2822(&value).ok())
        .map(|dt| dt.to_rfc3339());
    Some(pidash_storage::head_to_storage_metadata(
        header_str("content-type").as_deref(),
        content_length,
        last_modified.as_deref(),
        header_str("etag").as_deref(),
        None,
    ))
}

/// Shared new `FileAsset` row (`base.py:197-206`):
/// `attributes={name, type, size}` (`{provider}-avatar.{extension}`),
/// `asset` the bare filename, `user` + `created_by` set, entity
/// `USER_AVATAR`, uploaded flag, S3 head metadata (`None` → NULL when the
/// HEAD failed, like boto3's `None`). Every other column rides its Django
/// default (the reviewed column list mirrors the D-17 google port of the
/// same table).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_avatar_file_asset(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    stored_key: &str,
    provider: &str,
    extension: &str,
    content_type: &str,
    file_size: i64,
    user_id: uuid::Uuid,
    storage_metadata: Option<serde_json::Value>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    let attributes =
        pidash_storage::avatar_attributes(provider, extension, content_type, file_size);
    sqlx::query(
        r#"INSERT INTO file_assets
           (id, created_at, updated_at, attributes, asset,
            created_by_id, updated_by_id, workspace_id,
            is_deleted, deleted_at, is_archived,
            comment_id, entity_type, external_id, external_source,
            is_uploaded, issue_id, page_id, project_id,
            size, storage_metadata, user_id, draft_issue_id, entity_identifier)
           VALUES ($1,$2,$2,$3,$4,$5,NULL,NULL,
                   FALSE,NULL,FALSE,
                   NULL,'USER_AVATAR',NULL,NULL,
                   TRUE,NULL,NULL,NULL,
                   $6,$7,$5,NULL,NULL)"#,
    )
    .bind(id)
    .bind(now)
    .bind(attributes)
    .bind(stored_key)
    .bind(user_id)
    .bind(file_size as f64)
    .bind(storage_metadata)
    .execute(pool)
    .await?;
    Ok(())
}

/// Shared `delete_old_avatar` (`adapter/base.py:236-254`): when the user
/// carries a live avatar asset, delete the S3 object, soft-delete the row
/// and clear both columns. `FileAsset.DoesNotExist` (no row, or a
/// soft-deleted one — the manager filters `deleted_at IS NULL`) passes
/// silently; any other failure is logged and the flow continues
/// (`except Exception: log_exception; return`) — so this returns `()`
/// always and never fails the pipeline.
///
/// Row-delete policy mirrors `delete_files` exactly: the S3 multi-delete
/// is idempotent and its `ClientError` collapses to `False`, after which
/// Python still deletes the row — so any *answered* HTTP response (even
/// an error status) soft-deletes the row + clears the FK. Only a
/// *transport* failure (Python's non-`ClientError` raise, e.g. connection
/// loss) or a misconfigured backend (Python fails lazily on first use)
/// keeps the old avatar intact. The row write is a soft delete, never a
/// hard `DELETE`: the user's FK still points at the row, so a hard
/// delete would die on the deferred FK in autocommit (PIDASHCONV-1321).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn delete_old_avatar(
    client: &reqwest::Client,
    pool: &sqlx::PgPool,
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    user_id: uuid::Uuid,
    avatar_asset_id: Option<uuid::Uuid>,
    now: chrono::DateTime<chrono::Utc>,
) {
    let Some(asset_id) = avatar_asset_id else {
        return;
    };
    let row: Option<(String,)> = match sqlx::query_as(
        "SELECT \"asset\" FROM \"file_assets\" WHERE \"id\" = $1 AND \"deleted_at\" IS NULL",
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(error) => {
            tracing::warn!(%error, "delete_old_avatar lookup failed");
            return;
        }
    };
    let Some((key,)) = row else {
        return;
    };
    if pidash_storage::validate_storage(storage).is_err() {
        tracing::warn!("delete_old_avatar storage misconfigured; keeping old avatar");
        return;
    }
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let Ok(signed) =
        pidash_storage::sign_delete(storage, scheme, host, &key, &amz_date, &date_stamp)
    else {
        tracing::warn!("delete_old_avatar storage endpoint invalid; keeping old avatar");
        return;
    };
    match client
        .delete(signed.url)
        .header("Authorization", signed.authorization)
        .header("x-amz-date", signed.amz_date)
        .header("x-amz-content-sha256", signed.content_sha256)
        .send()
        .await
    {
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "delete_old_avatar S3 delete failed; keeping old avatar");
            return;
        }
    }
    soft_delete_avatar_asset(pool, user_id, asset_id, now).await;
}
