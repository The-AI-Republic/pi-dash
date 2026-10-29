//! Comment operations: create and partial update.
//!
//! The serializer step of the v1 comment endpoints, moved from
//! `pidash_api::v1_work_items::handlers_social` (OVL-14, PIDASHCONV-1649):
//! `IssueCommentCreateSerializer` for create
//! (`app/serializers/issue.py`, driven by `CommentListCreateAPIEndpoint`,
//! `views/issue.py:1863-1935`) and the symmetric update
//! (`views/issue.py:2034-2089`).

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::domain::comments as comment_domain;
use crate::domain::WriteCtx;
use crate::read::{self, DecodedComment};
use crate::v1_work_items::python_number_str;
use crate::v1_work_items::shape_social::{
    render_comment_create, render_comment_value, validate_comment_create,
    CommentCreateRepresentationInput, CommentCreateRow, CommentWriteInput,
};
use crate::v1_work_items::tasks as work_tasks;

use super::common::{
    activate_timezone, db_error, map_read_error, now_utc, requested_data_text, requested_data_value,
};
use super::deps::Deps;
use super::error::{Error, Outcome, PAYLOAD_NOT_VALID_BODY, VALID_DETAIL_BODY};

/// Comment external-dedupe `exists()` (`:1895-1900`, `:2048-2053`): live +
/// project + slug + source + id. No pk exclusion — the patch caller
/// compares `external_id` in Python first (`:2047`, ported asymmetry 8).
/// A `None` source filters `IS NULL` (the patch's stored-source default
/// may be NULL — Django renders `None` as `IS NULL`).
async fn comment_external_exists(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
    source: Option<&str>,
    external_id: &str,
) -> Result<bool, Error> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "issue_comments" c
           WHERE c."deleted_at" IS NULL AND c."project_id" = $1
             AND c."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $2)
             AND c."external_source" IS NOT DISTINCT FROM $3 AND c."external_id" = $4)"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_one(pool)
    .await
    .map_err(|error| db_error(error, "comment-dedupe"))
}

/// The create-409 `"id"`: the duplicate row's id
/// (`:1902-1907`, `.first()` over the `-created_at` default ordering).
async fn comment_external_first_id(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
    source: &str,
    external_id: &str,
) -> Result<Option<Uuid>, Error> {
    sqlx::query_scalar(
        r#"SELECT c."id" FROM "issue_comments" c
           WHERE c."deleted_at" IS NULL AND c."project_id" = $1
             AND c."workspace_id" = (SELECT "id" FROM "workspaces" WHERE "slug" = $2)
             AND c."external_source" = $3 AND c."external_id" = $4
           ORDER BY c."created_at" DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "comment-dup-first"))
}

/// The comment-create activity `requested_data`
/// (`json.dumps(serializer.data)` at `:1928`): the 9-field create read
/// shape over the saved row.
fn render_comment_create_value(decoded: &DecodedComment) -> Result<Value, Error> {
    let labels: Vec<Option<&str>> = decoded.labels.iter().map(|item| item.as_deref()).collect();
    let row = CommentCreateRow {
        comment_json: &decoded.comment_json,
        comment_html: &decoded.comment_html,
        access: &decoded.access,
        external_source: decoded.external_source.as_deref(),
        external_id: decoded.external_id.as_deref(),
        labels: &labels,
        speaker_type: &decoded.speaker_type,
        speaker_label: &decoded.speaker_label,
        speaker_agent_run_id: decoded.speaker_agent_run_id.as_deref(),
    };
    let out = render_comment_create(&CommentCreateRepresentationInput {
        row: &row,
        fields: None,
        expand: &[],
        expansions: &[],
    })
    .map_err(|_| Error::ServerError)?;
    Ok(Value::Object(out))
}

/// Python truthiness for a `request.data.get()` value: missing/null are
/// falsy; `""`/`0`/`0.0`/`false`/`[]`/`{}` are falsy; everything else is
/// truthy.
fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().map(|n| n != 0.0).unwrap_or(true),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// The comment-PATCH dup-guard input (`:2045-2061`): `None` skips the
/// query (falsy id, `str()`-equal to stored, or a composite the filter
/// could never match); otherwise the `(source, id)` filter pair.
/// `data.get("external_source", stored)` — present-null queries `IS
/// NULL`; present-but-empty still filters; absent inherits the stored
/// value, NULL included; composite sources skip like composite ids. All
/// verified live.
fn patch_external_guard(
    map: &Map<String, Value>,
    stored_id: Option<&str>,
    stored_source: Option<String>,
) -> Option<(Option<String>, String)> {
    if !is_truthy(map.get("external_id")) {
        return None;
    }
    let stored = stored_id.unwrap_or_default();
    let incoming = match map.get("external_id").and_then(python_scalar_str) {
        Some(text) if text == stored => return None,
        Some(text) => text,
        None => return None,
    };
    let source: Option<String> = match map.get("external_source") {
        None => stored_source,
        Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        // Other scalars stringify; composites skip (their filter would
        // miss; the serializer 400s either way).
        Some(value) => Some(python_scalar_str(value)?),
    };
    Some((source, incoming))
}

/// The comment external-dedupe guard input: both values truthy (Python
/// `and`), rendered as filter strings via `get_prep_value` (`str()`).
/// Composites skip (their filter would miss; the serializer 400s).
fn dedupe_pair(map: &Map<String, Value>, source_default: Option<&str>) -> Option<(String, String)> {
    let id_raw = map.get("external_id");
    let source_raw = map.get("external_source");
    let id_truthy = is_truthy(id_raw);
    let source_truthy = match source_raw {
        Some(_) => is_truthy(source_raw),
        // Patch default: the stored source (NULL counts as missing here —
        // the `and` needs a truthy stored value too).
        None => source_default.map(|s| !s.is_empty()).unwrap_or(false),
    };
    if !(id_truthy && source_truthy) {
        return None;
    }
    let id = id_raw.and_then(python_scalar_str)?;
    let source = match source_raw {
        Some(value) => python_scalar_str(value)?,
        None => source_default.unwrap_or_default().to_owned(),
    };
    Some((source, id))
}

/// Pre-validate `comment_json` for HTML-form bodies
/// (`JSONField.get_value` arrives as a JSON string): parse in place, or
/// answer the field's `invalid` 400. JSON bodies pass through untouched.
fn form_comment_json(map: &mut Map<String, Value>, from_form: bool) -> Result<(), Error> {
    if !from_form {
        return Ok(());
    }
    if let Some(Value::String(text)) = map.get("comment_json").cloned() {
        match serde_json::from_str(&text) {
            Ok(value) => {
                map.insert("comment_json".to_owned(), value);
            }
            Err(_) => {
                return Err(Error::FieldErrors(
                    r#"{"comment_json":["Value must be valid JSON."]}"#.to_owned(),
                ));
            }
        }
    }
    // `get_value`: blank + not-required + no `allow_blank` → absent (both
    // are `ChoiceField`s with defaults and no `blank=True`).
    for key in ["access", "speaker_type"] {
        if map.get(key) == Some(&Value::String(String::new())) {
            map.shift_remove(key);
        }
    }
    // `get_value`: blank + `allow_null`, no `allow_blank` → None (the
    // `UUIDField` takes no `allow_blank` from `blank=True`).
    if map.get("speaker_agent_run_id") == Some(&Value::String(String::new())) {
        map.insert("speaker_agent_run_id".to_owned(), Value::Null);
    }
    Ok(())
}

/// The comment-create 409 body: `{"error": ..., "id": ...}` in view order.
fn comment_dup_body(id: &str) -> String {
    let mut map = Map::with_capacity(2);
    map.insert(
        "error".to_owned(),
        Value::String(COMMENT_DUP_MESSAGE.to_owned()),
    );
    map.insert("id".to_owned(), Value::String(id.to_owned()));
    serde_json::to_string(&map).expect("dup body")
}

/// Resolve the `created_at` override: the handler-side `parse_datetime`
/// cascade over the raw body value, returning the database instant plus
/// the echoed response text.
pub type CreatedAtResolver =
    dyn Fn(Option<&Value>) -> Result<(DateTime<Utc>, String), Error> + Send + Sync;

/// `POST .../comments/` (`CommentListCreateAPIEndpoint`,
/// `views/issue.py:1863-1935`): `IssueCommentCreateSerializer(data).save()`
/// plus the render.
///
/// The caller already ran auth, the gate and body negotiation; `write`
/// carries what it resolved. `resolve_created_at` is the handler-side
/// `created_at` override parser (its `parse_datetime` cascade stays in the
/// api crate); the operation calls it after the save, where the view
/// applies the override. `options` carries the per-caller deltas.
pub async fn create(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &CommentWrite<'_>,
    resolve_created_at: &CreatedAtResolver,
    options: CommentCreateOptions,
) -> Result<Outcome, Error> {
    let pool = &deps.pool;
    let slug = write.slug;
    let project_id = write.project_id;
    let issue_id = &write.issue_id;
    let actor_id = ctx.actor.user_id;
    let tz = activate_timezone(ctx.actor.timezone.as_deref())?;
    let body = write.body;
    let from_form = write.from_form;
    // The dup guard touches `request.data.get` first (`:1893`) — a
    // non-object body raises `AttributeError` into the 500.
    let Some(map) = body.as_object() else {
        return Err(Error::ServerError);
    };
    if let Some((source, external_id)) = dedupe_pair(map, None) {
        if comment_external_exists(pool, &project_id, slug, Some(&source), &external_id).await? {
            // The 409 `"id"` is the DUPLICATE's id (`:1902-1907`).
            let Some(first) =
                comment_external_first_id(pool, &project_id, slug, &source, &external_id).await?
            else {
                return Err(Error::ServerError);
            };
            return Err(Error::Conflict(comment_dup_body(&first.to_string())));
        }
    }
    let mut shape_map = map.clone();
    form_comment_json(&mut shape_map, from_form)?;
    let shape_value = Value::Object(shape_map);
    let validated = validate_comment_create(&CommentWriteInput {
        body: &shape_value,
        partial: false,
    })
    .map_err(|error| Error::FieldErrors(error.body().to_owned()))?;
    // `ProjectBaseModel.save` sets the workspace from the project.
    let comment_workspace: Option<Uuid> =
        sqlx::query_scalar(r#"SELECT "workspace_id" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "comment-post-workspace"))?
            .flatten();
    let Some(comment_workspace) = comment_workspace else {
        return Err(Error::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
    };
    let comment_id = Uuid::new_v4();
    let created_at = now_utc();
    let updated_at = now_utc();
    let comment_json = validated
        .comment_json
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    let comment_html = validated
        .comment_html
        .clone()
        .unwrap_or_else(|| "<p></p>".to_owned());
    let stripped = strip_comment_html(&comment_html);
    let labels = validated.labels.clone().unwrap_or_default();
    let access = validated
        .access
        .clone()
        .unwrap_or_else(|| "INTERNAL".to_owned());
    let speaker_type = validated
        .speaker_type
        .clone()
        .unwrap_or_else(|| "human".to_owned());
    let speaker_label = validated.speaker_label.clone().unwrap_or_default();
    // Canonical UUID text per the shape contract; the column is `uuid`.
    let speaker_run_id: Option<Uuid> = validated
        .speaker_agent_run_id
        .clone()
        .unwrap_or(None)
        .map(|text| text.parse().expect("canonical run uuid"));
    // `IssueComment.save` runs in one `transaction.atomic()`: the comment
    // INSERT, the `Description` INSERT, and the `description_id` UPDATE
    // (domain twins). Four `now()` reads keep their positions (comment
    // pair, description pair).
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| db_error(error, "comment-post-tx"))?;
    let empty_list: Vec<String> = Vec::new();
    let desc_stripped = description_stripped_for_create(&comment_html);
    let external_id = validated.external_id.clone().unwrap_or(None);
    let external_source = validated.external_source.clone().unwrap_or(None);
    let rows = comment_domain::CommentRows {
        id: comment_id,
        description_id: Uuid::new_v4(),
        created_at,
        updated_at,
        desc_created_at: now_utc(),
        desc_updated_at: now_utc(),
        workspace_id: comment_workspace,
        project_id,
        issue_id: *issue_id,
        actor_id: Some(actor_id),
        created_by: Some(actor_id),
        updated_by: None,
        deleted_at: None,
        html: &comment_html,
        json: &comment_json,
        stripped: &stripped,
        desc_stripped: desc_stripped.as_deref(),
        access: &access,
        speaker_type: &speaker_type,
        speaker_label: &speaker_label,
        speaker_agent_run_id: speaker_run_id,
        external_id: external_id.as_deref(),
        external_source: external_source.as_deref(),
        parent_id: None,
        attachments: &empty_list,
        labels: &labels,
        edited_at: None,
    };
    comment_domain::insert_comment_row(&mut *tx, &rows)
        .await
        .map_err(|error| {
            // No issue lookup precedes the create: a bogus `issue_id`
            // violates the FK, and `handle_exception` answers the
            // `IntegrityError` 400 (`api/views/base.py:142-147`). The
            // transaction rolls back on drop, like the failed ORM `save()`.
            if is_fk_violation(&error) {
                Error::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
            } else {
                db_error(error, "comment-post-insert")
            }
        })?;
    comment_domain::insert_description_row(&mut *tx, &rows)
        .await
        .map_err(|error| db_error(error, "comment-post-desc"))?;
    comment_domain::link_comment_description(&mut *tx, rows.id, rows.description_id)
        .await
        .map_err(|error| db_error(error, "comment-post-desc-link"))?;
    let description_id = rows.description_id;
    tx.commit().await.map_err(|error| {
        // Every FK here is `DEFERRABLE INITIALLY DEFERRED`, so the bogus
        // `issue_id` violation fires at COMMIT, not at the INSERT — and
        // `handle_exception` answers the `IntegrityError` 400
        // (`api/views/base.py:142-147`). Every other FK in this
        // transaction references a row verified above to exist.
        if is_fk_violation(&error) {
            Error::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
        } else {
            db_error(error, "comment-post-commit")
        }
    })?;
    // The overrides (`:1921-1924`): `created_at` (or `now()`), `created_by`
    // (or the actor), `actor_id` in memory only (BUG-3) — saved with
    // `update_fields=["created_at","created_by"]`.
    let (db_created_at, response_created_at) = resolve_created_at(map.get("created_at"))?;
    let created_by = override_user_id(map.get("created_by"), &actor_id)?;
    if let Err(error) =
        comment_domain::restamp_comment_created(pool, comment_id, db_created_at, created_by).await
    {
        if is_fk_violation(&error) {
            return Err(Error::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        return Err(db_error(error, "comment-post-override"));
    }
    // Response `actor` is the in-memory override-or-actor (BUG-3 — always
    // identical to the response `created_by`); the DB row keeps the
    // requester.
    let response_actor = created_by;
    let decoded = DecodedComment {
        id: comment_id.to_string(),
        is_member: None,
        created_at: response_created_at,
        updated_at: pidash_compat::datetime::render_datetime_in(&updated_at, &tz),
        deleted_at: None,
        comment_json: comment_json.clone(),
        comment_html: comment_html.clone(),
        attachments: Vec::new(),
        labels: labels.iter().map(|item| Some(item.clone())).collect(),
        access: access.clone(),
        external_source: validated.external_source.clone().unwrap_or(None),
        external_id: validated.external_id.clone().unwrap_or(None),
        speaker_type: speaker_type.clone(),
        speaker_label: speaker_label.clone(),
        speaker_agent_run_id: validated.speaker_agent_run_id.clone().unwrap_or(None),
        edited_at: None,
        created_by: created_by.map(|id| id.to_string()),
        updated_by: Some(actor_id.to_string()),
        project: project_id.to_string(),
        workspace: comment_workspace.to_string(),
        description: Some(description_id.to_string()),
        issue: issue_id.to_string(),
        actor: response_actor.map(|id| id.to_string()),
        parent: None,
    };
    // `requested_data` is the 9-field create shape over the saved row; the
    // activity actor is the post-override `created_by` (`"None"` wart).
    let requested = pidash_compat::py::django_dumps(&render_comment_create_value(&decoded)?);
    let actor_text = created_by
        .map(|id| id.to_string())
        .unwrap_or_else(|| "None".to_owned());
    let kwargs = work_tasks::issue_activity_kwargs(
        work_tasks::ACTIVITY_COMMENT_CREATED,
        Some(&requested),
        &actor_text,
        &issue_id.to_string(),
        &project_id.to_string(),
        None,
        Utc::now().timestamp(),
    );
    if options.emit_activity {
        deps.enqueue
            .enqueue_task(work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs)
            .await;
    }
    // The webhook carries the RAW body, the requester, and the app origin.
    // `origin` arrives pre-resolved from settings (`ok()` past the
    // save; the row + `issue_activity` persist on a misconfigured host,
    // so the caller passes `None` there instead of failing). Suppressed
    // callers (the MCP tools) skip the leg — they enqueue their own
    // shaped legs.
    if let Some(origin) = webhook_origin(options.emit_model_activity, write.origin.clone())? {
        let webhook = work_tasks::comment_model_activity_kwargs(
            &comment_id.to_string(),
            requested_data_value(body, from_form),
            None,
            &actor_id.to_string(),
            slug,
            &origin,
        );
        deps.enqueue
            .enqueue_task(work_tasks::MODEL_ACTIVITY_TASK, vec![], webhook)
            .await;
    }
    let web_base = write.web_base.clone();
    let url = read::comment_url(
        pool,
        &comment_workspace,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await
    .map_err(map_read_error)?;
    let body =
        render_comment_value(&decoded, url, None, &[], &[]).map_err(|_| Error::ServerError)?;
    Ok(Outcome::created(
        serde_json::to_string(&body).map_err(|_| Error::ServerError)?,
    ))
}

/// `strip_tags(self.comment_html) if self.comment_html != "" else ""`
/// (`db/models/issue.py:606` over `utils/html_processor.py`, MLStripper).
fn strip_comment_html(html: &str) -> String {
    if html.is_empty() {
        String::new()
    } else {
        pidash_db::app_pages::strip::ml_strip_tags(html)
    }
}

/// The `Description` row's stripped text on comment create:
/// `Description.save` RECOMPUTES it with Django's own `strip_tags`
/// (`db/models/description.py:22-29`), which keeps entities verbatim —
/// unlike the comment column's entity-decoding MLStripper. Empty HTML
/// stores NULL. (PATCH syncs the decoded form via queryset `update()`,
/// which runs no `save()` — that path is untouched. The PATCH update-mint
/// reuses this helper: `Description.objects.create` runs the same `save()`.)
fn description_stripped_for_create(comment_html: &str) -> Option<String> {
    if comment_html.is_empty() {
        None
    } else {
        Some(crate::space::sanitize::strip_tags(comment_html))
    }
}

/// `PATCH .../comments/<pk>/` (`views/issue.py:2034-2089`): the
/// partial-update twin of [`create`].
///
/// The caller already ran auth, the gate and body negotiation (see
/// [`create`]); `write` carries the pre-fetched `before` row so a missing
/// comment 404s before a malformed body 400s.
pub async fn update(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &CommentUpdate<'_>,
    options: CommentUpdateOptions,
) -> Result<Outcome, Error> {
    let pool = &deps.pool;
    let slug = write.slug;
    let project_id = write.project_id;
    let workspace_id = write.workspace_id;
    let issue_id = &write.issue_id;
    let pk = &write.pk;
    let before = write.before;
    let actor_id = ctx.actor.user_id;
    let tz = activate_timezone(ctx.actor.timezone.as_deref())?;
    let body = write.body;
    let from_form = write.from_form;
    let requested = requested_data_text(body, from_form);
    // The before-image renders the direct (un-annotated) row with its real
    // `get_url`: no `is_member` key.
    let before_decoded = read::decode_comment(before, &tz).map_err(map_read_error)?;
    let web_base = write.web_base.clone();
    let before_url = read::comment_url(
        pool,
        &workspace_id,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await
    .map_err(map_read_error)?;
    let current = pidash_compat::py::django_dumps(
        &render_comment_value(&before_decoded, before_url, None, &[], &[])
            .map_err(|_| Error::ServerError)?,
    );
    let Some(map) = body.as_object() else {
        // `json.dumps` accepts any body, but the guard's `.get` raises
        // `AttributeError` into the 500 (`:2046`).
        return Err(Error::ServerError);
    };
    // The dup guard (`:2045-2061`): truthy `external_id`, different from
    // the stored value (`str()`-compared), and an existing row under the
    // given-or-stored source. The 409 `"id"` is the CURRENT row's id.
    if let Some((source, incoming)) = patch_external_guard(
        map,
        before_decoded.external_id.as_deref(),
        before_decoded.external_source.clone(),
    ) {
        if comment_external_exists(pool, &project_id, slug, source.as_deref(), &incoming).await? {
            return Err(Error::Conflict(comment_dup_body(&pk.to_string())));
        }
    }
    let mut shape_map = map.clone();
    form_comment_json(&mut shape_map, from_form)?;
    let shape_value = Value::Object(shape_map);
    let validated = validate_comment_create(&CommentWriteInput {
        body: &shape_value,
        partial: true,
    })
    .map_err(|error| Error::FieldErrors(error.body().to_owned()))?;
    // Merge validated keys over the before-image (untouched keys keep
    // their values — equivalent to the ORM's field-wise `save()`).
    let before_json: Value = before
        .try_get("comment_json")
        .map_err(|_| Error::ServerError)?;
    let before_html: String = before
        .try_get("comment_html")
        .map_err(|_| Error::ServerError)?;
    let before_stripped: String = before
        .try_get("comment_stripped")
        .map_err(|_| Error::ServerError)?;
    let new_json = validated
        .comment_json
        .clone()
        .unwrap_or(before_json.clone());
    let new_html = validated
        .comment_html
        .clone()
        .unwrap_or(before_html.clone());
    // `IssueComment.save` recomputes `comment_stripped` on EVERY save
    // (`db/models/issue.py:606`), even when `comment_html` is untouched —
    // a stale stored value is repaired and then synced below.
    let new_stripped = strip_comment_html(&new_html);
    let new_access = validated
        .access
        .clone()
        .unwrap_or(before_decoded.access.clone());
    let new_source = validated
        .external_source
        .clone()
        .unwrap_or_else(|| before_decoded.external_source.clone());
    let new_external_id = validated
        .external_id
        .clone()
        .unwrap_or_else(|| before_decoded.external_id.clone());
    let new_labels = validated.labels.clone().unwrap_or_else(|| {
        before_decoded
            .labels
            .iter()
            .map(|item| item.clone().unwrap_or_default())
            .collect()
    });
    let new_speaker_type = validated
        .speaker_type
        .clone()
        .unwrap_or(before_decoded.speaker_type.clone());
    let new_speaker_label = validated
        .speaker_label
        .clone()
        .unwrap_or(before_decoded.speaker_label.clone());
    let new_run_id = validated
        .speaker_agent_run_id
        .clone()
        .unwrap_or_else(|| before_decoded.speaker_agent_run_id.clone());
    let now = now_utc();
    // `IssueComment.save` runs in one `transaction.atomic()`: the comment
    // UPDATE, the `Description` INSERT-or-sync, and the `description_id`
    // link write commit or roll back together.
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| db_error(error, "comment-patch-tx"))?;
    comment_domain::save_v1_comment_update(
        &mut *tx,
        &comment_domain::V1CommentUpdate {
            id: *pk,
            json: &new_json,
            html: &new_html,
            stripped: &new_stripped,
            access: &new_access,
            external_source: new_source.as_deref(),
            external_id: new_external_id.as_deref(),
            labels: &new_labels,
            speaker_type: &new_speaker_type,
            speaker_label: &new_speaker_label,
            speaker_agent_run_id: new_run_id
                .as_deref()
                .and_then(|text| text.parse::<Uuid>().ok()),
            updated_at: now,
            updated_by: actor_id,
        },
    )
    .await
    .map_err(|error| db_error(error, "comment-patch-update"))?;
    // The `Description` side of `IssueComment.save` (`db/models/issue.py:623-647`):
    // a NULL `description_id` (pre-0109 legacy rows) mints a fresh
    // `Description` over the saved triple; otherwise only changed tracked
    // fields sync, plus `updated_by`/`updated_at` — via queryset `update()`,
    // so no `save()` override re-strips. `_changes_on_save` compares
    // VALUES: a present-but-identical key syncs nothing.
    let html_changed = validated
        .comment_html
        .as_ref()
        .is_some_and(|html| *html != before_html);
    let json_changed = validated
        .comment_json
        .as_ref()
        .is_some_and(|json| *json != before_json);
    let stripped_changed = new_stripped != before_stripped;
    let stored_description_id =
        read::row_uuid_opt(before, "description_id").map_err(map_read_error)?;
    if let Some(description_id) = stored_description_id {
        if html_changed || json_changed || stripped_changed {
            comment_domain::sync_v1_description_subset(
                &mut *tx,
                &comment_domain::V1DescriptionSync {
                    id: description_id,
                    html: html_changed.then_some(new_html.as_str()),
                    stripped: stripped_changed.then_some(new_stripped.as_str()),
                    json: json_changed.then_some(&new_json),
                    updated_by: actor_id,
                    updated_at: now,
                },
            )
            .await
            .map_err(|error| db_error(error, "comment-patch-desc"))?;
        }
    } else {
        // Unlinked legacy row: `Description.objects.create` over
        // `description_defaults` (`:607-627`) runs `BaseModel.save`'s
        // creating arm (`db/models/base.py:36-39`), which overwrites the
        // passed defaults from crum — the acting user is `created_by`,
        // `updated_by` is NULL — then the
        // `update_fields=["description_id"]` link write.
        let desc_stripped = description_stripped_for_create(&new_html);
        let desc = comment_domain::DescriptionValues {
            id: Uuid::new_v4(),
            created_at: now_utc(),
            updated_at: now_utc(),
            created_by: Some(actor_id),
            updated_by: None,
            workspace_id,
            project_id,
            json: &new_json,
            html: &new_html,
            stripped: desc_stripped.as_deref(),
        };
        comment_domain::insert_description_values(&mut *tx, &desc)
            .await
            .map_err(|error| db_error(error, "comment-patch-desc-mint"))?;
        comment_domain::link_comment_description(&mut *tx, *pk, desc.id)
            .await
            .map_err(|error| db_error(error, "comment-patch-desc-link"))?;
    }
    tx.commit()
        .await
        .map_err(|error| db_error(error, "comment-patch-commit"))?;
    let kwargs = work_tasks::issue_activity_kwargs(
        work_tasks::ACTIVITY_COMMENT_UPDATED,
        Some(&requested),
        &actor_id.to_string(),
        &issue_id.to_string(),
        &project_id.to_string(),
        Some(&current),
        Utc::now().timestamp(),
    );
    if options.emit_activity {
        deps.enqueue
            .enqueue_task(work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs)
            .await;
    }
    // `origin` arrives pre-resolved from settings (`ok()` past the
    // save; the row + `issue_activity` persist on a misconfigured host,
    // so the caller passes `None` there instead of failing). Suppressed
    // callers (the MCP tools) skip the leg — they enqueue their own
    // shaped legs.
    if let Some(origin) = webhook_origin(options.emit_model_activity, write.origin.clone())? {
        let webhook = work_tasks::comment_model_activity_kwargs(
            &pk.to_string(),
            requested_data_value(body, from_form),
            Some(&current),
            &actor_id.to_string(),
            slug,
            &origin,
        );
        deps.enqueue
            .enqueue_task(work_tasks::MODEL_ACTIVITY_TASK, vec![], webhook)
            .await;
    }
    // The response refetches by id (`:2086`) and renders un-annotated.
    let Some(after) = read::fetch_comment_direct(pool, slug, &project_id, issue_id, pk)
        .await
        .map_err(map_read_error)?
    else {
        return Err(Error::ServerError);
    };
    let decoded = read::decode_comment(&after, &tz).map_err(map_read_error)?;
    let url = read::comment_url(
        pool,
        &workspace_id,
        &project_id,
        issue_id,
        web_base.as_deref(),
    )
    .await
    .map_err(map_read_error)?;
    let body =
        render_comment_value(&decoded, url, None, &[], &[]).map_err(|_| Error::ServerError)?;
    Ok(Outcome::ok(
        serde_json::to_string(&body).map_err(|_| Error::ServerError)?,
    ))
}

/// Comment external-duplicate 409 message (`views/issue.py:1910,2057`).
const COMMENT_DUP_MESSAGE: &str =
    "Work item comment with the same external id and external source already exists";

/// Python `str()` for a JSON scalar, for the external-dedupe guards:
/// `CharField.get_prep_value` stringifies filter values (ints stringify,
/// bools spell `True`/`False`), and the PATCH guard compares
/// `stored != str(incoming)` (`:2047`). Composites return `None`: their
/// `repr` can only match a stored repr-string, which no API write can
/// produce (the serializer 400s them first) — skipping the query is
/// equivalent to the filter miss. All verified live.
fn python_scalar_str(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(python_number_str(number)),
        Value::Bool(flag) => Some(if *flag { "True" } else { "False" }.to_owned()),
        _ => None,
    }
}

/// The `request.data.get("created_by", user.id)` override (the
/// handlers' `override_user_id`, returning [`Error`]): absent → the
/// actor, explicit null → NULL (clears), a UUID string → that user.
/// `UUIDField.to_python` coerces ints AND bools via `uuid.UUID(int=…)` —
/// in-range values proceed to the save (usually the FK-violation 400);
/// negative ints fail with `ValueError`. Floats and anything else fail
/// `get_prep_value` → the `ValidationError` 400 (Django wraps the
/// `UUID(hex=…)` `AttributeError`). All verified live.
fn override_user_id(raw: Option<&Value>, actor: &Uuid) -> Result<Option<Uuid>, Error> {
    match raw {
        None => Ok(Some(*actor)),
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => text
            .parse::<Uuid>()
            .map(Some)
            .map_err(|_| Error::FieldErrors(VALID_DETAIL_BODY.to_owned())),
        Some(Value::Bool(flag)) => Ok(Some(Uuid::from_u128(u128::from(*flag as u8)))),
        Some(Value::Number(number)) => {
            if let Some(int) = number.as_u64() {
                Ok(Some(Uuid::from_u128(u128::from(int))))
            } else {
                Err(Error::FieldErrors(VALID_DETAIL_BODY.to_owned()))
            }
        }
        Some(_) => Err(Error::FieldErrors(VALID_DETAIL_BODY.to_owned())),
    }
}

/// Whether a sqlx failure is a foreign-key violation (`IntegrityError` →
/// `{"error": "The payload is not valid"}`).
fn is_fk_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().as_deref() == Some("23503"),
        _ => false,
    }
}

/// The pre-resolved comment-create scope: auth, the gate and body
/// negotiation already ran; `body` is the negotiated payload with its
/// `from_form` flag.
pub struct CommentWrite<'a> {
    /// The workspace slug from the route.
    pub slug: &'a str,
    /// The gated project id (after identifier rewrite).
    pub project_id: Uuid,
    /// The commented issue id (no lookup precedes the create: a bogus id
    /// violates the FK at commit, answering the invalid-payload 400).
    pub issue_id: Uuid,
    /// The negotiated payload.
    pub body: &'a Value,
    /// Whether the payload arrived as an HTML form.
    pub from_form: bool,
    /// The settings-derived app origin, if configured (`None` 500s past
    /// the save, where the view resolves it).
    pub origin: Option<String>,
    /// The settings-derived web base for rendered URLs, if configured.
    pub web_base: Option<String>,
}

/// Per-caller options for [`create`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentCreateOptions {
    /// Emit the `issue_activity` created leg. The v1 view passes `true`;
    /// callers that enqueue their own shaped activity (the MCP tools)
    /// pass `false`.
    pub emit_activity: bool,
    /// Emit the webhook (`model_activity`) leg. The v1 view passes
    /// `true`; origin-less callers that enqueue their own shaped legs
    /// (the MCP tools) pass `false`, which skips the leg entirely — no
    /// origin is required.
    pub emit_model_activity: bool,
}

/// The pre-resolved comment-update scope: `before` is the live row the
/// handler fetched before parsing the body, so a missing comment 404s
/// before a malformed body 400s.
pub struct CommentUpdate<'a> {
    /// The workspace slug from the route.
    pub slug: &'a str,
    /// The gated project id (after identifier rewrite).
    pub project_id: Uuid,
    /// The gated workspace id.
    pub workspace_id: Uuid,
    /// The commented issue id.
    pub issue_id: Uuid,
    /// The patched comment id.
    pub pk: Uuid,
    /// The pre-fetched live row (the handler 404s before parsing).
    pub before: &'a PgRow,
    /// The negotiated payload.
    pub body: &'a Value,
    /// Whether the payload arrived as an HTML form.
    pub from_form: bool,
    /// The settings-derived app origin, if configured (see
    /// [`CommentWrite::origin`]).
    pub origin: Option<String>,
    /// The settings-derived web base for rendered URLs, if configured.
    pub web_base: Option<String>,
}

/// Per-caller options for [`update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentUpdateOptions {
    /// Emit the `issue_activity` update leg (see
    /// [`CommentCreateOptions`]).
    pub emit_activity: bool,
    /// Emit the webhook (`model_activity`) leg (see
    /// [`CommentCreateOptions`]).
    pub emit_model_activity: bool,
}

/// Resolve the `model_activity` webhook origin for the caller's options:
/// suppressed callers (`emit_model_activity: false`, the MCP tools) skip
/// the leg entirely — no origin is required — while emitting callers
/// 500 past the save when the origin is unconfigured, exactly the
/// view's `base_host` raise.
fn webhook_origin(
    emit_model_activity: bool,
    origin: Option<String>,
) -> Result<Option<String>, Error> {
    if !emit_model_activity {
        return Ok(None);
    }
    Ok(Some(origin.ok_or(Error::ServerError)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch_map(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn comment_dup_body_bytes() {
        assert_eq!(
            comment_dup_body("6c6c6c6c-0000-4000-8000-000000000000"),
            r#"{"error":"Work item comment with the same external id and external source already exists","id":"6c6c6c6c-0000-4000-8000-000000000000"}"#,
        );
    }

    #[test]
    fn created_by_override_arms() {
        let actor = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("uuid");
        assert_eq!(override_user_id(None, &actor).expect("ok"), Some(actor));
        assert_eq!(
            override_user_id(Some(&Value::Null), &actor).expect("ok"),
            None
        );
        let other = "22222222-2222-4222-8222-222222222222";
        assert_eq!(
            override_user_id(Some(&Value::String(other.to_owned())), &actor).expect("ok"),
            Some(Uuid::parse_str(other).expect("uuid"))
        );
        assert_eq!(
            override_user_id(Some(&Value::String("garbage".to_owned())), &actor)
                .expect_err("errors"),
            Error::FieldErrors(VALID_DETAIL_BODY.to_owned())
        );
        // Ints coerce via `UUID(int=…)` and proceed to the save (live
        // Django FK-violates into the payload 400); corrected from the
        // valid-detail pin by live probe in review.
        assert_eq!(
            override_user_id(Some(&Value::from(7)), &actor).expect("int"),
            Some(Uuid::from_u128(7))
        );
    }

    #[test]
    fn truthiness_matrix() {
        assert!(!is_truthy(None));
        assert!(!is_truthy(Some(&Value::Null)));
        assert!(!is_truthy(Some(&Value::Bool(false))));
        assert!(!is_truthy(Some(&Value::String(String::new()))));
        assert!(!is_truthy(Some(&Value::from(0))));
        assert!(!is_truthy(Some(&Value::Array(vec![]))));
        assert!(is_truthy(Some(&Value::Bool(true))));
        assert!(is_truthy(Some(&Value::String("x".to_owned()))));
        assert!(is_truthy(Some(&Value::from(7))));
    }

    #[test]
    fn dedupe_pair_arms() {
        // Both truthy strings → the pair.
        let map = patch_map(&[
            ("external_source", Value::String("gh".to_owned())),
            ("external_id", Value::String("1".to_owned())),
        ]);
        assert_eq!(
            dedupe_pair(&map, None),
            Some(("gh".to_owned(), "1".to_owned()))
        );
        // Either falsy → no check.
        let map = patch_map(&[("external_id", Value::String(String::new()))]);
        assert_eq!(dedupe_pair(&map, None), None);
        let map = patch_map(&[("external_id", Value::String("1".to_owned()))]);
        assert_eq!(dedupe_pair(&map, None), None);
        // Patch default: the stored source fills an absent key.
        let map = patch_map(&[("external_id", Value::String("1".to_owned()))]);
        assert_eq!(
            dedupe_pair(&map, Some("gh")),
            Some(("gh".to_owned(), "1".to_owned()))
        );
        // Scalars stringify via `get_prep_value` (`str()`); the serializer
        // then coerces numbers or 400s bools. Corrected from the 500 pin
        // by live probe in review.
        let map = patch_map(&[
            ("external_source", Value::String("gh".to_owned())),
            ("external_id", Value::from(7)),
        ]);
        assert_eq!(
            dedupe_pair(&map, None),
            Some(("gh".to_owned(), "7".to_owned()))
        );
        let map = patch_map(&[
            ("external_source", Value::from(true)),
            ("external_id", Value::String("1".to_owned())),
        ]);
        assert_eq!(
            dedupe_pair(&map, None),
            Some(("True".to_owned(), "1".to_owned()))
        );
        // Composites skip (their filter would miss; the serializer 400s).
        let map = patch_map(&[
            ("external_source", Value::String("gh".to_owned())),
            ("external_id", Value::Array(vec![Value::from(1)])),
        ]);
        assert_eq!(dedupe_pair(&map, None), None);
    }

    #[test]
    fn form_comment_json_arms() {
        let mut map = patch_map(&[
            ("comment_json", Value::String("{\"a\":1}".to_owned())),
            ("access", Value::String(String::new())),
            ("speaker_type", Value::String(String::new())),
            ("comment_html", Value::String(String::new())),
        ]);
        form_comment_json(&mut map, true).expect("ok");
        assert_eq!(map.get("comment_json"), Some(&serde_json::json!({"a": 1})));
        assert!(map.get("access").is_none());
        assert!(map.get("speaker_type").is_none());
        assert_eq!(map.get("comment_html"), Some(&Value::String(String::new())));
        let mut bad = patch_map(&[("comment_json", Value::String("{oops".to_owned()))]);
        assert_eq!(
            form_comment_json(&mut bad, true).expect_err("errors"),
            Error::FieldErrors("{\"comment_json\":[\"Value must be valid JSON.\"]}".to_owned())
        );
        // JSON bodies pass through untouched.
        let mut json = patch_map(&[("access", Value::String(String::new()))]);
        form_comment_json(&mut json, false).expect("ok");
        assert_eq!(json.get("access"), Some(&Value::String(String::new())));
    }

    #[test]
    fn strip_comment_html_arms() {
        assert_eq!(strip_comment_html(""), "");
        assert_eq!(strip_comment_html("<p>hi</p>"), "hi");
    }

    #[test]
    fn override_user_id_int_bool_arms() {
        // `UUIDField.to_python` coerces ints AND bools via `UUID(int=…)`
        // (verified live); the save then FK-violates into the 400.
        let actor = Uuid::nil();
        assert_eq!(
            override_user_id(Some(&Value::from(7)), &actor).expect("int"),
            Some(Uuid::from_u128(7))
        );
        assert_eq!(
            override_user_id(Some(&Value::from(true)), &actor).expect("bool"),
            Some(Uuid::from_u128(1))
        );
        assert_eq!(
            override_user_id(Some(&Value::from(false)), &actor).expect("bool"),
            Some(Uuid::from_u128(0))
        );
        // Negative ints (`ValueError`) and floats (`ValidationError`, the
        // `UUID(hex=…)` `AttributeError` wrapped) → valid-detail 400.
        for raw in [Value::from(-7), Value::from(7.5), Value::from(7.0)] {
            assert!(
                matches!(
                    override_user_id(Some(&raw), &actor),
                    Err(Error::FieldErrors(body)) if body == VALID_DETAIL_BODY
                ),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn patch_external_guard_arms() {
        let stored_id = Some("extA");
        let stored_source = Some("srcA".to_owned());
        // Falsy id skips.
        let map = patch_map(&[("external_id", Value::String(String::new()))]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            None
        );
        // `str()`-equal skips — including int 5 vs stored "5".
        let map = patch_map(&[("external_id", Value::String("extA".to_owned()))]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            None
        );
        let map = patch_map(&[("external_id", Value::from(5))]);
        assert_eq!(
            patch_external_guard(&map, Some("5"), stored_source.clone()),
            None
        );
        let map = patch_map(&[("external_id", Value::from(true))]);
        assert_eq!(
            patch_external_guard(&map, Some("True"), stored_source.clone()),
            None
        );
        // Changed id + absent source inherits the stored source.
        let map = patch_map(&[("external_id", Value::String("extB".to_owned()))]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            Some((Some("srcA".to_owned()), "extB".to_owned()))
        );
        // Explicit-null source queries IS NULL; empty still filters.
        let map = patch_map(&[
            ("external_id", Value::String("extB".to_owned())),
            ("external_source", Value::Null),
        ]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            Some((None, "extB".to_owned()))
        );
        let map = patch_map(&[
            ("external_id", Value::String("extB".to_owned())),
            ("external_source", Value::String(String::new())),
        ]);
        assert_eq!(
            patch_external_guard(&map, stored_id, stored_source.clone()),
            Some((Some(String::new()), "extB".to_owned()))
        );
        // Differing scalars filter via `get_prep_value` (`str()`); the
        // serializer then coerces numbers or 400s bools (verified live).
        let map = patch_map(&[("external_id", Value::from(6))]);
        assert_eq!(
            patch_external_guard(&map, stored_id, None),
            Some((None, "6".to_owned()))
        );
        let map = patch_map(&[
            ("external_id", Value::String("extB".to_owned())),
            ("external_source", Value::from(7)),
        ]);
        assert_eq!(
            patch_external_guard(&map, stored_id, None),
            Some((Some("7".to_owned()), "extB".to_owned()))
        );
        // Composites skip (their filter would miss; the serializer 400s).
        for raw in [
            Value::Array(vec![Value::from(1)]),
            Value::Object(Map::new()),
        ] {
            let map = patch_map(&[("external_id", raw.clone())]);
            assert_eq!(patch_external_guard(&map, stored_id, None), None, "{raw:?}");
            let map = patch_map(&[
                ("external_id", Value::String("extB".to_owned())),
                ("external_source", raw.clone()),
            ]);
            assert_eq!(patch_external_guard(&map, stored_id, None), None, "{raw:?}");
        }
    }

    #[test]
    fn fk_violation_arms() {
        #[derive(Debug)]
        struct FakeDbError {
            code: &'static str,
        }
        impl std::fmt::Display for FakeDbError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "fake {}", self.code)
            }
        }
        impl std::error::Error for FakeDbError {}
        impl sqlx::error::DatabaseError for FakeDbError {
            fn message(&self) -> &str {
                "fake"
            }
            fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
                Some(self.code.into())
            }
            fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
                self
            }
            fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
                self
            }
            fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
                self
            }
            fn kind(&self) -> sqlx::error::ErrorKind {
                sqlx::error::ErrorKind::ForeignKeyViolation
            }
        }
        let fk = sqlx::Error::Database(Box::new(FakeDbError { code: "23503" }));
        assert!(is_fk_violation(&fk));
        let other = sqlx::Error::Database(Box::new(FakeDbError { code: "23505" }));
        assert!(!is_fk_violation(&other));
        assert!(!is_fk_violation(&sqlx::Error::RowNotFound));
    }

    #[test]
    fn form_comment_json_blank_speaker_clears() {
        // `get_value`: blank + `allow_null`, no `allow_blank` → None.
        let mut map = patch_map(&[("speaker_agent_run_id", Value::String(String::new()))]);
        form_comment_json(&mut map, true).expect("ok");
        assert_eq!(map.get("speaker_agent_run_id"), Some(&Value::Null));
        let mut map = patch_map(&[("speaker_agent_run_id", Value::String(String::new()))]);
        form_comment_json(&mut map, false).expect("ok");
        assert_eq!(
            map.get("speaker_agent_run_id"),
            Some(&Value::String(String::new()))
        );
    }

    #[test]
    fn description_stripped_for_create_keeps_entities() {
        // `Description.save` recomputes with Django's `strip_tags`
        // (verbatim entities), unlike the comment column (verified live).
        assert_eq!(description_stripped_for_create(""), None);
        assert_eq!(
            description_stripped_for_create("<p>R&amp;D</p>").as_deref(),
            Some("R&amp;D")
        );
        assert_eq!(
            description_stripped_for_create("<p>hi</p>").as_deref(),
            Some("hi")
        );
    }

    #[test]
    fn python_scalar_str_float_spelling() {
        // Moved with `python_scalar_str`: `str(1e100)` is `1e+100`
        // (PIDASHCONV-758).
        let float: Value = serde_json::from_str("1e100").expect("parses");
        assert_eq!(python_scalar_str(&float).as_deref(), Some("1e+100"));
        let sci: Value = serde_json::from_str("1.5e3").expect("parses");
        assert_eq!(python_scalar_str(&sci).as_deref(), Some("1500.0"));
    }

    #[test]
    fn webhook_origin_suppression_arms() {
        // Suppressed callers (the MCP tools) skip the webhook leg with no
        // origin required; emitting callers (v1) require one — the `None`
        // 500 lands past the save, with the row + the gated
        // `issue_activity` already persisted, exactly the view's
        // `base_host` raise.
        assert_eq!(webhook_origin(false, None).expect("ok"), None);
        assert_eq!(
            webhook_origin(false, Some("https://app.example.test".to_owned())).expect("ok"),
            None
        );
        assert_eq!(
            webhook_origin(true, Some("https://app.example.test".to_owned())).expect("ok"),
            Some("https://app.example.test".to_owned())
        );
        assert_eq!(webhook_origin(true, None), Err(Error::ServerError));
    }

    // --- Live-DB legacy-mint stamp test (PIDASHCONV-1658) ---

    use crate::domain::activity::TaskEnqueueHook;
    use crate::domain::transition::IssueTransitionHook;
    use crate::domain::{WriteActor, WriteCtx};
    use crate::orchestration::entries::{FireOutcome, FireRequest};
    use crate::read::test_support::{pool, seed_world};
    use std::sync::{Arc, Mutex};

    type EnqueueCall = (String, Vec<Value>, Map<String, Value>);

    struct RecordingEnqueue {
        calls: Mutex<Vec<EnqueueCall>>,
    }

    impl TaskEnqueueHook for RecordingEnqueue {
        fn enqueue_task(
            &self,
            task: &str,
            args: Vec<Value>,
            kwargs: Map<String, Value>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            self.calls
                .lock()
                .expect("calls lock")
                .push((task.to_owned(), args, kwargs));
            Box::pin(async move {})
        }
    }

    struct NoopTransition;

    impl IssueTransitionHook for NoopTransition {
        fn enqueue_completion_comment(
            &self,
            _issue_id: Uuid,
            _prev_state_id: Option<Uuid>,
            _current_state_id: Option<Uuid>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            Box::pin(async move {})
        }

        fn fire(
            &self,
            _actor_id: Uuid,
            _request: FireRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<FireOutcome, ()>> + Send + '_>>
        {
            Box::pin(async move { Ok(FireOutcome::NoTransition) })
        }
    }

    #[tokio::test]
    async fn live_update_mints_legacy_description_with_actor_stamps() {
        let Some(pool) = pool().await else {
            return;
        };
        let world = seed_world(&pool).await;
        // A second user plays the updater; the seeded user stays the
        // comment's stored creator, so the two stamps diverge.
        let updater = Uuid::new_v4();
        let username = format!("mint-{}", updater.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar,\
             date_joined, created_at, updated_at, last_location, created_location,\
             is_superuser, is_managed, is_password_expired, is_active, is_staff,\
             is_email_verified, is_password_autoset, token, user_timezone,\
             last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,\
             is_bot, display_name, is_email_valid, is_password_reset_required)\
             VALUES ($1,'!',$2,$3,'Mint','User','',\
             now(), now(), now(), '','', false,false,false,true,false,false,false,'','UTC',\
             '','','email','',false,'Mint User',false,false)",
        )
        .bind(updater)
        .bind(&username)
        .bind(format!("{username}@example.test"))
        .execute(&pool)
        .await
        .expect("seed updater");
        // The seeded comment is unlinked (`description_id` NULL, the
        // pre-0109 shape); stamp it as created by the seeded user.
        sqlx::query("UPDATE issue_comments SET created_by_id = $1 WHERE id = $2")
            .bind(world.user_id)
            .bind(world.comment_id)
            .execute(&pool)
            .await
            .expect("stamp comment creator");
        let before = read::fetch_comment_direct(
            &pool,
            &world.workspace_slug,
            &world.project_id,
            &world.issue_id,
            &world.comment_id,
        )
        .await
        .expect("fetch before")
        .expect("comment row");
        let body = serde_json::json!({"comment_html": "<p>minted</p>"});
        let deps = Deps {
            pool: pool.clone(),
            orm: sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone()),
            transition: Arc::new(NoopTransition),
            enqueue: Arc::new(RecordingEnqueue {
                calls: Mutex::new(Vec::new()),
            }),
        };
        let ctx = WriteCtx::new(WriteActor {
            user_id: updater,
            timezone: None,
        });
        let write = CommentUpdate {
            slug: &world.workspace_slug,
            project_id: world.project_id,
            workspace_id: world.workspace_id,
            issue_id: world.issue_id,
            pk: world.comment_id,
            before: &before,
            body: &body,
            from_form: false,
            origin: None,
            web_base: None,
        };
        let outcome = update(
            &deps,
            &ctx,
            &write,
            CommentUpdateOptions {
                emit_activity: false,
                emit_model_activity: false,
            },
        )
        .await
        .expect("update succeeds");
        assert_eq!(outcome.status, 200);
        // `Description.objects.create` runs `BaseModel.save`'s creating
        // arm (`db/models/base.py:36-39`), which overwrites the passed
        // defaults from crum: `created_by` is the acting user and
        // `updated_by` is NULL — not the comment's stored creator.
        let minted: (Uuid, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            r#"SELECT d."id", d."created_by_id", d."updated_by_id" FROM "descriptions" d
               JOIN "issue_comments" c ON c."description_id" = d."id" WHERE c."id" = $1"#,
        )
        .bind(world.comment_id)
        .fetch_one(&pool)
        .await
        .expect("minted description");
        assert_ne!(world.user_id, updater);
        assert_eq!(minted.1, Some(updater));
        assert_eq!(minted.2, None);
    }
}
