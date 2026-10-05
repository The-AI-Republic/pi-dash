#![forbid(unsafe_code)]

//! Work-item task call sites (D-18, L8 tasks).
//!
//! Ports the `.delay()` publishers in `api/views/issue.py` (issue, link,
//! comment, relation and attachment endpoints), `_record_body_write` in
//! `api/views/page.py:156-170`, and the offline half of the attachment S3
//! upload/confirm flow (`api/views/issue.py:2235-2450`,
//! `settings/storage.py` `S3Storage`). Task *bodies* stay Python-owned
//! during coexistence and are never ported here.
//!
//! Fixture oracle: `rust-api/fixtures/v1_work_items/tasks/F18-10.tasks.json`
//! (PIDASHCONV-659). The tests below replay all 11 units: task names,
//! args, kwargs in call-site order, plus each unit's status and DB delta.
//! Fixture `messages` arrive in reverse publish order (LIFO broker drain —
//! e.g. create publishes issue_activity, model_activity, process_logs and
//! the fixture lists them back to front); the replay matches per task name
//! and asserts publish order from the Python source order.
//!
//! Wire contract (Porting guide Jobs plane): every publisher here returns
//! the Celery triple — task name, positional args, kwargs in call-site
//! order — and the handlers (in the `api` crate, which already depends on
//! `pidash-jobs`) wrap it with `CeleryTaskMessage::new(task, args, kwargs)`
//! plus `queue::enqueue_in` inside the request transaction. This crate
//! cannot name `pidash-jobs` (it depends on `pidash-services`), so the
//! triples are plain `String`/`Vec<Value>`/`Map<String, Value>` — the same
//! split as `app_project::tasks` and `v1_projects::tasks`.
//!
//! Reuse (never forked):
//!
//! * [`crate::v1_projects::tasks::model_activity_kwargs`] is the shared
//!   `model_activity` kernel — the D-18 sites use the identical kwarg
//!   order, so [`issue_model_activity_kwargs`] and
//!   [`comment_model_activity_kwargs`] delegate with `model_name` fixed.
//! * Task-name consts are pinned by value against the merged domains
//!   (see tests): `ISSUE_ACTIVITY_TASK` against `app_project::tasks`,
//!   `MODEL_ACTIVITY_TASK` against `v1_projects::tasks`.
//! * SigV4 *execution* (presigned POST/GET bytes) stays handler-side: the
//!   D-02 helpers in `crates/api/src/space/assets.rs` (`presigned_post`,
//!   `presigned_get_url`) already port `storage.py` + botocore, and the
//!   `api` crate already depends on `pidash-storage`. This module ports
//!   only the offline inputs: asset key, size clamp, MIME gate, POST
//!   fields, content-disposition, and the metadata-task gating predicate.
//! * `FileAsset.asset_url` (the presign response's `asset_url`) is a model
//!   property rendered handler-side (D-02 precedent:
//!   `space/assets.rs::asset_url_for`); the response key order is pinned
//!   here ([`PRESIGN_RESPONSE_KEY_ORDER`]) so both layers agree.
//!
//! Out of scope (observed in the fixture, owned elsewhere):
//!
//! * `process_logs` (ambient per-request emit) and
//!   `soft_delete_related_objects` (the `issue.delete()` signal cascade)
//!   are not D-18 call sites; the replay asserts their presence per unit
//!   but builds no publishers for them.
//! * Endpoint DB writes (the `db_delta` pins: row creates, the
//!   created_at/created_by stamp, soft-delete flags) belong to the queries
//!   (PIDASHCONV-669) and handlers (PIDASHCONV-673+) layers. Every function
//!   here is pure: no `sqlx`, no network.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-link-create-kwarg-order (`views/issue.py:1639-1648`): the only
//!   `issue_activity` site with `issue_id`/`project_id` before `actor_id`
//!   ([`LINK_CREATE_ACTIVITY_KWARG_ORDER`]); all others use
//!   [`ISSUE_ACTIVITY_KWARG_ORDER`].
//! * QUIRK-uuid-string-args: `model_activity.actor_id` (all 6 sites),
//!   `crawl_work_item_link_title` id on link *create* (`:1635`), and
//!   `page_id`/`user_id` (`page.py:340,476`) cross `.delay` as UUID
//!   objects (kombu renders `{"__type__": "uuid"}` on the wire). The
//!   builders take the string form (the D-33 Q6 convention): every
//!   consumer coerces (`QuerySet` lookups, `str(...)`, explicit
//!   `str | uuid.UUID` in `webhook_task.py:378-388`), so behavior is
//!   identical; only the informational repr headers differ.
//! * QUIRK-crawl-arg-types: link create passes a UUID object (`:1635`)
//!   while link patch passes serializer-data strings (`:1749`).
//! * QUIRK-patch-model-data (`views/issue.py:533,853`): the issue create
//!   and patch `model_activity` emits carry the *normalized* `data`
//!   (converted HTML), while both put branches pass raw `request.data`
//!   (`:693,751`).
//! * QUIRK-comment/link-create-actor (`:1926-1944`, `:1636-1647`): the
//!   `issue_activity` actor is the (possibly `request.data`-overridden)
//!   `created_by_id`, while the sibling `model_activity` on comment
//!   create uses `request.user.id`.
//! * QUIRK-attachment-size (`:2336`): `size_limit = min(size, LIMIT)` with
//!   no coercion — a non-numeric `size` raises `TypeError` (500), a
//!   negative one passes through verbatim, `True` stays `True`
//!   ([`AttachmentPostPlan::SizeTypeError`]).
//! * QUIRK-mime-dupes (`settings/common.py:652-739`): `text/markdown` and
//!   `application/x-compressed-tar-zip` each appear twice; membership is
//!   unaffected and the list is transcribed verbatim.
//! * QUIRK-attach-delete-double-save (`:2491`, `:2509`) and
//!   QUIRK-confirm-created-by-overwrite (`:2653`, every confirm while not
//!   uploaded resets `created_by`) are DB behaviors for the handlers
//!   layer; noted here so the flow ports keep the emit order around them.
//! * QUIRK-delete-emit-order: link delete emits *before*
//!   `issue_link.delete()` (`:1783-1789`), while issue (after `:898`),
//!   comment (after `:2111`) and attachment deletes emit after their
//!   write; the confirm emit precedes its `save()` (`:2631-2655`).
//!   Handler-side sequencing (transactional enqueue keeps each pair
//!   atomic), recorded here so the order survives the port.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde_json::{Map, Value};

/// Celery wire name for `issue_activity` (bare `@shared_task`,
/// `bgtasks/issue_activities_task.py:1503-1516`).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";
/// Celery wire name for `model_activity` (bare `@shared_task`,
/// `bgtasks/webhook_task.py:463-464`).
pub const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";
/// Celery wire name for `crawl_work_item_link_title` (bare `@shared_task`,
/// `bgtasks/work_item_link_task.py:262-263`).
pub const CRAWL_LINK_TITLE_TASK: &str =
    "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title";
/// Celery wire name for `page_transaction` (bare `@shared_task`,
/// `bgtasks/page_transaction_task.py:84-85`).
pub const PAGE_TRANSACTION_TASK: &str = "pi_dash.bgtasks.page_transaction_task.page_transaction";
/// Celery wire name for `track_page_version` (bare `@shared_task`,
/// `bgtasks/page_version_task.py:21-22`).
pub const TRACK_PAGE_VERSION_TASK: &str = "pi_dash.bgtasks.page_version_task.track_page_version";
/// Celery wire name for `get_asset_object_metadata` (bare `@shared_task`,
/// `bgtasks/storage_metadata_task.py:14-15`).
pub const GET_ASSET_OBJECT_METADATA_TASK: &str =
    "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata";

/// Kwarg order of the standard `issue_activity.delay` sites
/// (`views/issue.py:519,683,741,843,900,1750,1783,1926,2066,2112`).
/// Call-site order, which differs from the worker signature order
/// (`issue_activities_task.py:1504-1516` puts `current_instance` third).
pub const ISSUE_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "type",
    "requested_data",
    "actor_id",
    "issue_id",
    "project_id",
    "current_instance",
    "epoch",
];

/// Kwarg order of the `+ notification + origin` sites: relation create
/// (`:3096`), attachment delete (`:2493`) and attachment confirm (`:2631`).
pub const ISSUE_ACTIVITY_NOTIFY_KWARG_ORDER: &[&str] = &[
    "type",
    "requested_data",
    "actor_id",
    "issue_id",
    "project_id",
    "current_instance",
    "epoch",
    "notification",
    "origin",
];

/// Kwarg order of the link-create site only (`:1639-1648` —
/// QUIRK-link-create-kwarg-order).
pub const LINK_CREATE_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "type",
    "requested_data",
    "issue_id",
    "project_id",
    "actor_id",
    "current_instance",
    "epoch",
];

/// Kwarg order of all six `model_activity.delay` sites
/// (`:530,693,751,853,1937,2076`) — identical to the shared
/// [`crate::v1_projects::tasks::model_activity_kwargs`] kernel order.
pub const MODEL_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "model_name",
    "model_id",
    "requested_data",
    "current_instance",
    "actor_id",
    "slug",
    "origin",
];

/// Kwarg order of `page_transaction.delay`
/// (`api/views/page.py:159-163`).
pub const PAGE_TRANSACTION_KWARG_ORDER: &[&str] =
    &["new_description_html", "old_description_html", "page_id"];

/// Kwarg order of `track_page_version.delay`
/// (`api/views/page.py:164-169`).
pub const TRACK_PAGE_VERSION_KWARG_ORDER: &[&str] = &["page_id", "existing_instance", "user_id"];

/// `type` values passed to `issue_activity.delay`, one per endpoint action.
pub const ACTIVITY_ISSUE_CREATED: &str = "issue.activity.created";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_ISSUE_UPDATED: &str = "issue.activity.updated";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_ISSUE_DELETED: &str = "issue.activity.deleted";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_LINK_CREATED: &str = "link.activity.created";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_LINK_UPDATED: &str = "link.activity.updated";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_LINK_DELETED: &str = "link.activity.deleted";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_COMMENT_CREATED: &str = "comment.activity.created";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_COMMENT_UPDATED: &str = "comment.activity.updated";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_COMMENT_DELETED: &str = "comment.activity.deleted";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_RELATION_CREATED: &str = "issue_relation.activity.created";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_ATTACHMENT_CREATED: &str = "attachment.activity.created";
/// See [`ACTIVITY_ISSUE_CREATED`].
pub const ACTIVITY_ATTACHMENT_DELETED: &str = "attachment.activity.deleted";

/// `model_name` for the issue `model_activity` sites (`:530,693,751,853`).
pub const MODEL_ISSUE: &str = "issue";
/// `model_name` for the comment `model_activity` sites (`:1937,2076`).
pub const MODEL_ISSUE_COMMENT: &str = "issue_comment";

/// One standard `issue_activity.delay(...)` call: kwargs in
/// [`ISSUE_ACTIVITY_KWARG_ORDER`], `args` on the wire is `[]`.
///
/// `requested_data` and `current_instance` are the pre-rendered
/// `json.dumps(..., cls=DjangoJSONEncoder)` texts the call site holds
/// (`None` renders `null`, as on the create branches); `epoch` is
/// `int(timezone.now().timestamp())`, supplied by the caller; ids are the
/// string form (QUIRK-uuid-string-args).
#[allow(clippy::too_many_arguments)]
pub fn issue_activity_kwargs(
    activity_type: &str,
    requested_data: Option<&str>,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    current_instance: Option<&str>,
    epoch: i64,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(ISSUE_ACTIVITY_KWARG_ORDER.len());
    kwargs.insert("type".to_owned(), Value::String(activity_type.to_owned()));
    kwargs.insert(
        "requested_data".to_owned(),
        requested_data.map_or(Value::Null, |s| Value::String(s.to_owned())),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_owned()));
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_owned()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_owned()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        current_instance.map_or(Value::Null, |s| Value::String(s.to_owned())),
    );
    kwargs.insert("epoch".to_owned(), Value::Number(epoch.into()));
    kwargs
}

/// One `issue_activity.delay(..., notification=..., origin=...)` call:
/// kwargs in [`ISSUE_ACTIVITY_NOTIFY_KWARG_ORDER`], `args` on the wire is
/// `[]`. Relation create passes `notification=True`; both attachment
/// sites pass `requested_data=None` with `notification=True`.
#[allow(clippy::too_many_arguments)]
pub fn issue_activity_notify_kwargs(
    activity_type: &str,
    requested_data: Option<&str>,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    current_instance: Option<&str>,
    epoch: i64,
    notification: bool,
    origin: &str,
) -> Map<String, Value> {
    let mut kwargs = issue_activity_kwargs(
        activity_type,
        requested_data,
        actor_id,
        issue_id,
        project_id,
        current_instance,
        epoch,
    );
    kwargs.insert("notification".to_owned(), Value::Bool(notification));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    kwargs
}

/// The link-create `issue_activity.delay(...)` call (`:1639-1648`):
/// `requested_data` is the `json.dumps(serializer.data)` text,
/// `current_instance` is always `None`, and the kwargs follow
/// [`LINK_CREATE_ACTIVITY_KWARG_ORDER`] (QUIRK-link-create-kwarg-order).
pub fn link_create_activity_kwargs(
    requested_data: &str,
    issue_id: &str,
    project_id: &str,
    actor_id: &str,
    epoch: i64,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(LINK_CREATE_ACTIVITY_KWARG_ORDER.len());
    kwargs.insert(
        "type".to_owned(),
        Value::String(ACTIVITY_LINK_CREATED.to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.to_owned()),
    );
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_owned()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_owned()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_owned()));
    kwargs.insert("current_instance".to_owned(), Value::Null);
    kwargs.insert("epoch".to_owned(), Value::Number(epoch.into()));
    kwargs
}

/// The issue `model_activity.delay(...)` calls (`:530,693,751,853`):
/// `model_name="issue"`, `requested_data` the normalized `data` on
/// create/patch but the raw `request.data` object on both put branches
/// (QUIRK-patch-model-data), `current_instance` the before-image text or
/// `None`. Delegates to the shared
/// [`crate::v1_projects::tasks::model_activity_kwargs`] kernel.
pub fn issue_model_activity_kwargs(
    model_id: &str,
    requested_data: Value,
    current_instance: Option<&str>,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> Map<String, Value> {
    crate::v1_projects::tasks::model_activity_kwargs(
        MODEL_ISSUE,
        model_id,
        requested_data,
        current_instance,
        actor_id,
        slug,
        origin,
    )
}

/// The comment `model_activity.delay(...)` calls (`:1937,2076`):
/// `model_name="issue_comment"`, `requested_data` the raw `request.data`
/// object. Delegates to the shared kernel (see
/// [`issue_model_activity_kwargs`]).
pub fn comment_model_activity_kwargs(
    model_id: &str,
    requested_data: Value,
    current_instance: Option<&str>,
    actor_id: &str,
    slug: &str,
    origin: &str,
) -> Map<String, Value> {
    crate::v1_projects::tasks::model_activity_kwargs(
        MODEL_ISSUE_COMMENT,
        model_id,
        requested_data,
        current_instance,
        actor_id,
        slug,
        origin,
    )
}

/// `crawl_work_item_link_title.delay(link_id, url)` (`:1635` on create,
/// `:1749` on patch): positional args, kwargs on the wire is `{}`. The id
/// is the string form (QUIRK-crawl-arg-types: a UUID object on create, a
/// serializer-data string on patch; the worker's `IssueLink.objects.get`
/// coerces both).
pub fn crawl_link_title_args(link_id: &str, url: &str) -> Vec<Value> {
    vec![
        Value::String(link_id.to_owned()),
        Value::String(url.to_owned()),
    ]
}

/// `get_asset_object_metadata.delay(str(asset_id))` (`:2507` on delete,
/// `:2649` on confirm): one positional arg, kwargs on the wire is `{}`.
/// Sent only when the row's `storage_metadata` is empty (see
/// [`storage_metadata_missing`]).
pub fn asset_metadata_args(asset_id: &str) -> Vec<Value> {
    vec![Value::String(asset_id.to_owned())]
}

/// `page_transaction.delay(...)` (`api/views/page.py:159-163`): kwargs in
/// [`PAGE_TRANSACTION_KWARG_ORDER`], `args` on the wire is `[]`.
/// `old_description_html` is `None` on page create (`:340`); both values
/// are the editor-serialised Tiptap HTML verbatim otherwise.
pub fn page_transaction_kwargs(
    new_description_html: &str,
    old_description_html: Option<&str>,
    page_id: &str,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(PAGE_TRANSACTION_KWARG_ORDER.len());
    kwargs.insert(
        "new_description_html".to_owned(),
        Value::String(new_description_html.to_owned()),
    );
    kwargs.insert(
        "old_description_html".to_owned(),
        old_description_html.map_or(Value::Null, |s| Value::String(s.to_owned())),
    );
    kwargs.insert("page_id".to_owned(), Value::String(page_id.to_owned()));
    kwargs
}

/// `track_page_version.delay(...)` (`api/views/page.py:164-169`): kwargs in
/// [`TRACK_PAGE_VERSION_KWARG_ORDER`], `args` on the wire is `[]`.
/// `existing_instance` is `None` on page create (`:340`) and the
/// [`page_version_existing_instance`] text on body updates (`:476`).
pub fn track_page_version_kwargs(
    page_id: &str,
    existing_instance: Option<&str>,
    user_id: &str,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(TRACK_PAGE_VERSION_KWARG_ORDER.len());
    kwargs.insert("page_id".to_owned(), Value::String(page_id.to_owned()));
    kwargs.insert(
        "existing_instance".to_owned(),
        existing_instance.map_or(Value::Null, |s| Value::String(s.to_owned())),
    );
    kwargs.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
    kwargs
}

/// `json.dumps({"description_html": old}, cls=DjangoJSONEncoder)`
/// (`api/views/page.py:166-168`): CPython default separators
/// (`{"description_html": "..."}`) with `ensure_ascii` escaping, verified
/// against CPython in the tests below.
pub fn page_version_existing_instance(old_description_html: &str) -> String {
    format!(
        "{{\"description_html\": {}}}",
        ensure_ascii(&serde_json::to_string(old_description_html).expect("str serializes"))
    )
}

/// `json.dumps(..., ensure_ascii=True)` over `serde_json` string output:
/// `serde_json` already escapes `"`, `\` and control chars and emits raw
/// UTF-8 otherwise, so only 0x7f (`\u007f`) and non-ASCII (lowercase
/// `\uXXXX`, surrogate pairs above U+FFFF) need rewriting. (Same kernel as
/// the private twins in `app_project::tasks` and `runner_enroll::tokens`,
/// kept local per the established pattern.)
fn ensure_ascii(compact_json: &str) -> String {
    let mut out = String::with_capacity(compact_json.len());
    for c in compact_json.chars() {
        if c.is_ascii() && c != '\u{7f}' {
            out.push(c);
        } else if c == '\u{7f}' {
            out.push_str("\\u007f");
        } else {
            let n = c as u32;
            if n < 0x1_0000 {
                out.push_str(&format!("\\u{n:04x}"));
            } else {
                let v = n - 0x1_0000;
                let (hi, lo) = (0xd800 + (v >> 10), 0xdc00 + (v & 0x3ff));
                out.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
            }
        }
    }
    out
}

/// `_record_body_write` (`api/views/page.py:156-170`) as its two publishes
/// in call order: `page_transaction` first, then `track_page_version`.
/// `old_description_html` is `None` on page create (`:340`); ids are the
/// string form (QUIRK-uuid-string-args).
pub fn record_body_write(
    page_id: &str,
    old_description_html: Option<&str>,
    new_description_html: &str,
    user_id: &str,
) -> [(&'static str, Map<String, Value>); 2] {
    let transaction = page_transaction_kwargs(new_description_html, old_description_html, page_id);
    let version = track_page_version_kwargs(
        page_id,
        old_description_html
            .map(page_version_existing_instance)
            .as_deref(),
        user_id,
    );
    [
        (PAGE_TRANSACTION_TASK, transaction),
        (TRACK_PAGE_VERSION_TASK, version),
    ]
}

// ---------------------------------------------------------------------------
// Attachment S3 upload/confirm flow (`views/issue.py:2312-2655`,
// `settings/storage.py` `S3Storage`)
// ---------------------------------------------------------------------------

/// Default `FILE_SIZE_LIMIT` (`settings/common.py:428`):
/// `int(get_config("FILE_SIZE_LIMIT", 5242880))`.
pub const FILE_SIZE_LIMIT_DEFAULT: i64 = 5_242_880;

/// `ATTACHMENT_MIME_TYPES` (`settings/common.py:652-739`), verbatim in
/// settings order — including the doubled `text/markdown` and
/// `application/x-compressed-tar-zip` (QUIRK-mime-dupes).
pub const ATTACHMENT_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/svg+xml",
    "image/webp",
    "image/tiff",
    "image/bmp",
    "application/pdf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "text/plain",
    "text/markdown",
    "application/rtf",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.oasis.opendocument.graphics",
    "application/vnd.visio",
    "image/x-portable-graymap",
    "image/x-portable-bitmap",
    "image/x-portable-pixmap",
    "application/vnd.oasis.opendocument.database",
    "audio/mpeg",
    "audio/wav",
    "audio/ogg",
    "audio/midi",
    "audio/x-midi",
    "audio/aac",
    "audio/flac",
    "audio/x-m4a",
    "video/mp4",
    "video/mpeg",
    "video/ogg",
    "video/webm",
    "video/quicktime",
    "video/x-msvideo",
    "video/x-ms-wmv",
    "application/zip",
    "application/x-rar",
    "application/x-rar-compressed",
    "application/x-tar",
    "application/gzip",
    "application/x-zip",
    "application/x-zip-compressed",
    "application/x-7z-compressed",
    "application/x-compressed",
    "application/x-compressed-tar",
    "application/x-compressed-tar-gz",
    "application/x-compressed-tar-bz2",
    "application/x-compressed-tar-zip",
    "application/x-compressed-tar-7z",
    "application/x-compressed-tar-rar",
    "application/x-compressed-tar-zip",
    "model/gltf-binary",
    "model/gltf+json",
    "application/octet-stream",
    "font/ttf",
    "font/otf",
    "font/woff",
    "font/woff2",
    "text/css",
    "text/javascript",
    "application/json",
    "text/xml",
    "text/csv",
    "application/xml",
    "application/x-sql",
    "application/x-gzip",
    "text/markdown",
];

/// Presign POST response key order (`views/issue.py:2406-2413`):
/// `upload_data`, `asset_id`, `attachment`, `asset_url`.
pub const PRESIGN_RESPONSE_KEY_ORDER: &[&str] =
    &["upload_data", "asset_id", "attachment", "asset_url"];

/// Python truthiness over a JSON value (`if not x` in the attachment
/// views): null/false/zero/empty all falsy, everything else truthy.
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `min(size, file_size_limit)` (`views/issue.py:2336`): the winner is
/// returned verbatim (Python `min` returns one of its operands, first on
/// ties). `None` means `min` raised `TypeError` — anything non-numeric.
/// (`serde_json` numbers are always finite, so the float comparison is
/// total; Python's NaN-wins-`min` edge has no JSON spelling.)
fn size_min_limit(size: &Value, file_size_limit: i64) -> Option<Value> {
    match size {
        Value::Bool(b) => {
            // `bool` is `int` in Python: `min(True, LIMIT)` is `True`.
            if (*b as i64) <= file_size_limit {
                Some(size.clone())
            } else {
                Some(Value::from(file_size_limit))
            }
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i <= file_size_limit {
                    Some(size.clone())
                } else {
                    Some(Value::from(file_size_limit))
                }
            } else if let Some(u) = n.as_u64() {
                if file_size_limit >= 0 && u <= file_size_limit as u64 {
                    Some(size.clone())
                } else {
                    Some(Value::from(file_size_limit))
                }
            } else if let Some(f) = n.as_f64() {
                if f <= file_size_limit as f64 {
                    Some(size.clone())
                } else {
                    Some(Value::from(file_size_limit))
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Outcome of the attachment presign POST validation
/// (`views/issue.py:2327-2344`), in check order.
#[derive(Debug, Clone, PartialEq)]
pub enum AttachmentPostPlan {
    /// `if not name or not size` (`:2332-2336`): 400
    /// [`invalid_request_body`].
    InvalidRequest,
    /// `size` is truthy but non-numeric, so `min(size, LIMIT)` (`:2336`)
    /// raises `TypeError` (QUIRK-attachment-size): the 500 path.
    SizeTypeError,
    /// `if not type or type not in settings.ATTACHMENT_MIME_TYPES`
    /// (`:2338-2343`): 400 [`invalid_file_type_body`].
    InvalidType,
    /// Validation passed: `size_limit` is the verbatim `min` winner,
    /// carried into the row (`attributes.size`, `size`) and the presign
    /// (`content-length-range`, `file_size`).
    Ready {
        /// The verbatim `min(size, file_size_limit)` winner.
        size_limit: Value,
    },
}

/// Validate an attachment presign POST (`views/issue.py:2327-2344`).
/// Inputs are the raw `request.data` values; `file_size_limit` is
/// `settings.FILE_SIZE_LIMIT`.
pub fn attachment_post_plan(
    name: &Value,
    size: &Value,
    mime: &Value,
    file_size_limit: i64,
) -> AttachmentPostPlan {
    if !py_truthy(name) || !py_truthy(size) {
        return AttachmentPostPlan::InvalidRequest;
    }
    let Some(size_limit) = size_min_limit(size, file_size_limit) else {
        return AttachmentPostPlan::SizeTypeError;
    };
    let allowed = mime
        .as_str()
        .is_some_and(|m| ATTACHMENT_MIME_TYPES.contains(&m));
    if !allowed {
        return AttachmentPostPlan::InvalidType;
    }
    AttachmentPostPlan::Ready { size_limit }
}

/// `{"error": "Invalid request.", "status": false}` 400
/// (`views/issue.py:2333-2335`).
pub fn invalid_request_body() -> Value {
    serde_json::json!({"error": "Invalid request.", "status": false})
}

/// `{"error": "Invalid file type.", "status": false}` 400
/// (`views/issue.py:2339-2342`).
pub fn invalid_file_type_body() -> Value {
    serde_json::json!({"error": "Invalid file type.", "status": false})
}

/// Object key for the new asset (`views/issue.py:2350`):
/// `f"{workspace.id}/{uuid4hex}-{name}"`. `id_hex` is the caller's
/// `uuid.uuid4().hex`.
pub fn attachment_asset_key(workspace_id: &str, id_hex: &str, name: &str) -> String {
    format!("{workspace_id}/{id_hex}-{name}")
}

/// `Fields` input to `generate_presigned_post` (`storage.py:71-89`) for a
/// concrete key: `{"Content-Type", "key"}` in insertion order. The
/// `${filename}` template branch (leading `starts-with` condition instead
/// of `key`) never triggers for [`attachment_asset_key`] keys; see
/// [`is_filename_template`].
pub fn upload_post_fields(file_type: &str, object_key: &str) -> Map<String, Value> {
    let mut fields = Map::with_capacity(2);
    fields.insert(
        "Content-Type".to_owned(),
        Value::String(file_type.to_owned()),
    );
    fields.insert("key".to_owned(), Value::String(object_key.to_owned()));
    fields
}

/// `object_name.startswith("${filename}")` (`storage.py:83`): selects the
/// `starts-with` condition branch of `generate_presigned_post`.
pub fn is_filename_template(object_key: &str) -> bool {
    object_key.starts_with("${filename}")
}

/// `urllib.parse.quote(filename)` with the default `safe='/'`
/// (`storage.py:7,111`): RFC 3986 over UTF-8 bytes, `/` surviving,
/// `%XX` uppercase.
fn url_quote(filename: &str) -> String {
    let mut out = String::with_capacity(filename.len());
    for b in filename.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0x0f) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

/// `_get_content_disposition` (`storage.py:102-114`): `None` filename takes
/// the caller's fresh `uuid4().hex` (`generated_hex`); an empty filename
/// yields the bare disposition; otherwise
/// `{disposition}; filename*=UTF-8''{quoted}`.
pub fn content_disposition(
    disposition: &str,
    filename: Option<&str>,
    generated_hex: &str,
) -> String {
    let filename = filename.unwrap_or(generated_hex);
    if filename.is_empty() {
        disposition.to_owned()
    } else {
        format!("{disposition}; filename*=UTF-8''{}", url_quote(filename))
    }
}

/// `if not issue_attachment.storage_metadata:` (`views/issue.py:2506`,
/// `:2648`): the row carries no HEAD result yet, so the confirm/delete
/// path must enqueue [`GET_ASSET_OBJECT_METADATA_TASK`]. `None` is the
/// NULL column (missing the same way an empty dict is).
pub fn storage_metadata_missing(storage_metadata: Option<&Value>) -> bool {
    storage_metadata.is_none_or(|meta| !py_truthy(meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Committed evidence replayed here without a database:
    /// `rust-api/fixtures/v1_work_items/tasks/F18-10.tasks.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/v1_work_items/tasks/F18-10.tasks.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn unit<'a>(fx: &'a Value, name: &str) -> &'a Value {
        fx.get("units")
            .and_then(|u| u.get(name))
            .unwrap_or_else(|| panic!("unit {name} present"))
    }

    fn tasks_of(unit: &Value) -> Vec<&str> {
        unit["messages"]
            .as_array()
            .expect("messages array")
            .iter()
            .map(|m| m["task"].as_str().expect("task str"))
            .collect()
    }

    fn message<'a>(unit: &'a Value, task: &str) -> &'a Value {
        unit["messages"]
            .as_array()
            .expect("messages array")
            .iter()
            .find(|m| m["task"].as_str() == Some(task))
            .unwrap_or_else(|| panic!("message {task} present"))
    }

    fn key_order(map: &Map<String, Value>) -> Vec<&str> {
        map.keys().map(String::as_str).collect()
    }

    /// Kombu UUID objects (`{"__type__": "uuid", ...}`) to dashed strings,
    /// so fixture kwargs compare directly against the string-form builders
    /// (QUIRK-uuid-string-args).
    fn normalize_uuids(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                if map.get("__type__").and_then(Value::as_str) == Some("uuid") {
                    let hex = map["__value__"]["hex"].as_str().expect("uuid hex");
                    let dashed = format!(
                        "{}-{}-{}-{}-{}",
                        &hex[0..8],
                        &hex[8..12],
                        &hex[12..16],
                        &hex[16..20],
                        &hex[20..32]
                    );
                    Value::String(dashed)
                } else {
                    Value::Object(
                        map.iter()
                            .map(|(k, v)| (k.clone(), normalize_uuids(v)))
                            .collect(),
                    )
                }
            }
            Value::Array(items) => Value::Array(items.iter().map(normalize_uuids).collect()),
            _ => value.clone(),
        }
    }

    #[test]
    fn task_names_are_bare_shared_task_paths() {
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            MODEL_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        assert_eq!(
            CRAWL_LINK_TITLE_TASK,
            "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title"
        );
        assert_eq!(
            PAGE_TRANSACTION_TASK,
            "pi_dash.bgtasks.page_transaction_task.page_transaction"
        );
        assert_eq!(
            TRACK_PAGE_VERSION_TASK,
            "pi_dash.bgtasks.page_version_task.track_page_version"
        );
        assert_eq!(
            GET_ASSET_OBJECT_METADATA_TASK,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
        // Cross-domain pins (same string, owned once per domain).
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            crate::app_project::tasks::ISSUE_ACTIVITY_TASK
        );
        assert_eq!(
            MODEL_ACTIVITY_TASK,
            crate::v1_projects::tasks::MODEL_ACTIVITY_TASK
        );
    }

    #[test]
    fn issue_create_replays() {
        let fx = fixture();
        let u = unit(&fx, "issue_create");
        assert_eq!(u["status"], 201);
        assert_eq!(u["db_delta"], json!({"issues": 1}));

        let activity = message(u, ISSUE_ACTIVITY_TASK);
        assert_eq!(activity["args"], json!([]));
        let kwargs = activity["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        // Exact `json.dumps(data)` text (`:519-527`).
        assert_eq!(
            kwargs["requested_data"],
            r#"{"name": "task probe issue", "priority": "low"}"#
        );
        let built = issue_activity_kwargs(
            ACTIVITY_ISSUE_CREATED,
            Some(r#"{"name": "task probe issue", "priority": "low"}"#),
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            "e49dfa13-c4e4-48a3-b15b-789d1b4ea015",
            "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            None,
            1790984094,
        );
        assert_eq!(key_order(&built), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(Value::Object(built), activity["kwargs"]);

        let model = message(u, MODEL_ACTIVITY_TASK);
        let mkwargs = model["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(mkwargs), MODEL_ACTIVITY_KWARG_ORDER);
        let mbuilt = issue_model_activity_kwargs(
            "e49dfa13-c4e4-48a3-b15b-789d1b4ea015",
            json!({"name": "task probe issue", "priority": "low"}),
            None,
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            "ws-conv659-2",
            "http://127.0.0.1:18359",
        );
        assert_eq!(key_order(&mbuilt), MODEL_ACTIVITY_KWARG_ORDER);
        assert_eq!(Value::Object(mbuilt), normalize_uuids(&model["kwargs"]));
    }

    #[test]
    fn issue_patch_replays() {
        let fx = fixture();
        let u = unit(&fx, "issue_patch");
        assert_eq!(u["status"], 200);
        assert_eq!(u["db_delta"], json!({}));

        let activity = message(u, ISSUE_ACTIVITY_TASK);
        let kwargs = activity["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(kwargs["type"], ACTIVITY_ISSUE_UPDATED);
        assert_eq!(kwargs["requested_data"], r#"{"priority": "high"}"#);
        // Before-image keeps the pre-patch value (`:810-843`).
        let before = kwargs["current_instance"].as_str().expect("before str");
        assert!(before.starts_with(r#"{"id": "e49dfa13-c4e4-48a3-b15b-789d1b4ea015""#));
        assert!(before.contains(r#""priority": "low""#));
        let built = issue_activity_kwargs(
            ACTIVITY_ISSUE_UPDATED,
            Some(r#"{"priority": "high"}"#),
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            "e49dfa13-c4e4-48a3-b15b-789d1b4ea015",
            "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            Some(before),
            1790984094,
        );
        assert_eq!(key_order(&built), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(Value::Object(built), activity["kwargs"]);

        // Patch `model_activity` carries the normalized `data`
        // (QUIRK-patch-model-data); here it equals the raw input.
        let model = message(u, MODEL_ACTIVITY_TASK);
        assert_eq!(
            model["kwargs"]["requested_data"],
            json!({"priority": "high"})
        );
        assert_eq!(
            model["kwargs"]["current_instance"],
            kwargs["current_instance"]
        );
    }

    #[test]
    fn link_create_replays_with_swapped_kwarg_order() {
        let fx = fixture();
        let u = unit(&fx, "link_create");
        assert_eq!(u["status"], 201);
        assert_eq!(u["db_delta"], json!({"issue_links": 1}));

        let activity = message(u, ISSUE_ACTIVITY_TASK);
        let kwargs = activity["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(kwargs), LINK_CREATE_ACTIVITY_KWARG_ORDER);
        let requested = r#"{"title": "t", "url": "https://example.com/task-probe", "issue_id": "e49dfa13-c4e4-48a3-b15b-789d1b4ea015"}"#;
        assert_eq!(kwargs["requested_data"], requested);
        let built = link_create_activity_kwargs(
            requested,
            "e49dfa13-c4e4-48a3-b15b-789d1b4ea015",
            "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            1790984094,
        );
        assert_eq!(key_order(&built), LINK_CREATE_ACTIVITY_KWARG_ORDER);
        assert_eq!(Value::Object(built), activity["kwargs"]);

        // Create passes a UUID object (`:1635`); the builder takes strings.
        let crawl = message(u, CRAWL_LINK_TITLE_TASK);
        assert_eq!(crawl["kwargs"], json!({}));
        let args = crawl_link_title_args(
            "68b605f3-4f7a-4a43-b228-596a71d9e432",
            "https://example.com/task-probe",
        );
        assert_eq!(Value::Array(args), normalize_uuids(&crawl["args"]));
    }

    #[test]
    fn comment_create_replays() {
        let fx = fixture();
        let u = unit(&fx, "comment_create");
        assert_eq!(u["status"], 201);
        assert_eq!(u["db_delta"], json!({"issue_comments": 1}));

        let activity = message(u, ISSUE_ACTIVITY_TASK);
        let kwargs = activity["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        // Exact `json.dumps(serializer.data)` text (`:1926-1934`).
        let requested = r#"{"comment_json": {}, "comment_html": "<p>task probe</p>", "access": "INTERNAL", "external_source": null, "external_id": null, "labels": [], "speaker_type": "human", "speaker_label": "", "speaker_agent_run_id": null}"#;
        assert_eq!(kwargs["requested_data"], requested);
        let built = issue_activity_kwargs(
            ACTIVITY_COMMENT_CREATED,
            Some(requested),
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            "e49dfa13-c4e4-48a3-b15b-789d1b4ea015",
            "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            None,
            1790984095,
        );
        assert_eq!(key_order(&built), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(Value::Object(built), activity["kwargs"]);

        let model = message(u, MODEL_ACTIVITY_TASK);
        let mkwargs = model["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(mkwargs), MODEL_ACTIVITY_KWARG_ORDER);
        assert_eq!(mkwargs["model_name"], MODEL_ISSUE_COMMENT);
        let mbuilt = comment_model_activity_kwargs(
            "d2301eb1-f1c5-4e57-9687-df11ec780040",
            json!({"comment_html": "<p>task probe</p>"}),
            None,
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            "ws-conv659-2",
            "http://127.0.0.1:18359",
        );
        assert_eq!(key_order(&mbuilt), MODEL_ACTIVITY_KWARG_ORDER);
        assert_eq!(Value::Object(mbuilt), normalize_uuids(&model["kwargs"]));
    }

    #[test]
    fn relation_create_replays_with_notification() {
        let fx = fixture();
        let u = unit(&fx, "relation_create");
        assert_eq!(u["status"], 201);
        assert_eq!(u["db_delta"], json!({"issue_relations": 1}));

        let activity = message(u, ISSUE_ACTIVITY_TASK);
        let kwargs = activity["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(kwargs), ISSUE_ACTIVITY_NOTIFY_KWARG_ORDER);
        let requested = r#"{"relation_type": "relates_to", "issues": ["25ace52e-c64d-4043-a700-911b1e42bffc"]}"#;
        assert_eq!(kwargs["requested_data"], requested);
        let built = issue_activity_notify_kwargs(
            ACTIVITY_RELATION_CREATED,
            Some(requested),
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            "e49dfa13-c4e4-48a3-b15b-789d1b4ea015",
            "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            None,
            1790984095,
            true,
            "http://127.0.0.1:18359",
        );
        assert_eq!(key_order(&built), ISSUE_ACTIVITY_NOTIFY_KWARG_ORDER);
        assert_eq!(Value::Object(built), activity["kwargs"]);
    }

    #[test]
    fn attachment_post_presign_pins_offline_inputs() {
        let fx = fixture();
        let u = unit(&fx, "attachment_post_presign");
        assert_eq!(u["status"], 200);
        // POST publishes no task — only the ambient `process_logs`.
        assert_eq!(tasks_of(u), ["pi_dash.bgtasks.logger_task.process_logs"]);

        // Response key set matches the Python insertion order const.
        let mut response_keys: Vec<&str> = u["response_keys"]
            .as_array()
            .expect("response keys")
            .iter()
            .map(|k| k.as_str().expect("key str"))
            .collect();
        response_keys.sort_unstable();
        let mut expected = PRESIGN_RESPONSE_KEY_ORDER.to_vec();
        expected.sort_unstable();
        assert_eq!(response_keys, expected);

        // The `Fields` input is the first two presigned response fields.
        let fields = upload_post_fields(
            "application/pdf",
            "92989e99-51c6-4725-8069-45784951694f/787811683bc64fbd817de3b1b73538c2-f.pdf",
        );
        assert_eq!(key_order(&fields), ["Content-Type", "key"]);
        let field_set: Vec<&str> = u["presign_fields_keys"]
            .as_array()
            .expect("presign fields")
            .iter()
            .map(|k| k.as_str().expect("key str"))
            .collect();
        for key in key_order(&fields) {
            assert!(field_set.contains(&key), "{key} pinned by fixture");
        }

        // Asset key shape `{workspace.id}/{uuid4hex}-{name}` (`:2350`),
        // against the full key the confirm unit's before-image carries.
        let confirm = unit(&fx, "attachment_confirm");
        let before = message(confirm, ISSUE_ACTIVITY_TASK)["kwargs"]["current_instance"]
            .as_str()
            .expect("confirm before-image");
        let asset: Value = serde_json::from_str(before).expect("before-image parses");
        assert_eq!(
            asset["asset"],
            attachment_asset_key(
                "92989e99-51c6-4725-8069-45784951694f",
                "787811683bc64fbd817de3b1b73538c2",
                "f.pdf"
            )
        );
        assert!(u["asset_key_pattern"]
            .as_str()
            .expect("pattern")
            .starts_with("92989e99-51c6-4725-8069-45784951694f/787811683bc64fbd817de3b"));

        // Happy-path validation: size 100 passes straight through.
        assert_eq!(
            attachment_post_plan(
                &json!("f.pdf"),
                &json!(100),
                &json!("application/pdf"),
                FILE_SIZE_LIMIT_DEFAULT,
            ),
            AttachmentPostPlan::Ready {
                size_limit: json!(100)
            }
        );
    }

    #[test]
    fn issue_delete_replays_with_soft_delete_signal() {
        let fx = fixture();
        let u = unit(&fx, "issue_delete");
        assert_eq!(u["status"], 204);
        // Soft delete keeps the row: no count delta, `deleted_at` stamped.
        assert_eq!(u["db_delta"], json!({}));
        assert!(u["row_after"]["deleted_at"].is_string());

        let activity = message(u, ISSUE_ACTIVITY_TASK);
        let kwargs = activity["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(kwargs["type"], ACTIVITY_ISSUE_DELETED);
        assert_eq!(
            kwargs["requested_data"],
            r#"{"issue_id": "e49dfa13-c4e4-48a3-b15b-789d1b4ea015"}"#
        );
        // Before-image keeps the pre-delete values (`:896-908`).
        let before = kwargs["current_instance"].as_str().expect("before str");
        assert!(before.contains(r#""priority": "high""#));
        assert!(before.contains(r#""deleted_at": null"#));

        // The `issue.delete()` signal cascade (not a D-18 call site):
        // `soft_delete_related_objects("db", "issue", uuid, using=None)`.
        let cascade = message(
            u,
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects",
        );
        assert_eq!(
            normalize_uuids(&cascade["args"]),
            json!(["db", "issue", "e49dfa13-c4e4-48a3-b15b-789d1b4ea015"])
        );
        assert_eq!(cascade["kwargs"], json!({"using": null}));
    }

    #[test]
    fn attachment_confirm_replays() {
        let fx = fixture();
        let u = unit(&fx, "attachment_confirm");
        assert_eq!(u["status"], 204);

        let activity = message(u, ISSUE_ACTIVITY_TASK);
        let kwargs = activity["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(kwargs), ISSUE_ACTIVITY_NOTIFY_KWARG_ORDER);
        assert_eq!(kwargs["type"], ACTIVITY_ATTACHMENT_CREATED);
        assert_eq!(kwargs["requested_data"], Value::Null);
        // Before-image is the pre-confirm serializer dump (`:2624-2631`):
        // `is_uploaded` still false, attributes pinned.
        let before = kwargs["current_instance"].as_str().expect("before str");
        assert!(before.contains(r#""is_uploaded": false"#));
        assert!(before.contains(r#""name": "f.pdf""#));
        let built = issue_activity_notify_kwargs(
            ACTIVITY_ATTACHMENT_CREATED,
            None,
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            "ad3f003c-17a4-490c-aafb-592a1a3b770d",
            "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            Some(before),
            1790984189,
            true,
            "http://127.0.0.1:18359",
        );
        assert_eq!(key_order(&built), ISSUE_ACTIVITY_NOTIFY_KWARG_ORDER);
        assert_eq!(Value::Object(built), activity["kwargs"]);

        // Empty `storage_metadata` (`{}` in the before-image) triggers the
        // metadata task (`:2648-2649`).
        assert!(storage_metadata_missing(Some(&json!({}))));
        let meta = message(u, GET_ASSET_OBJECT_METADATA_TASK);
        assert_eq!(meta["kwargs"], json!({}));
        assert_eq!(
            Value::Array(asset_metadata_args("a9b33b6b-ca20-48bf-91fa-67620548ce28")),
            meta["args"]
        );
    }

    #[test]
    fn attachment_delete_replays() {
        let fx = fixture();
        let u = unit(&fx, "attachment_delete");
        assert_eq!(u["status"], 204);

        let activity = message(u, ISSUE_ACTIVITY_TASK);
        let kwargs = activity["kwargs"].as_object().expect("kwargs map");
        assert_eq!(key_order(kwargs), ISSUE_ACTIVITY_NOTIFY_KWARG_ORDER);
        assert_eq!(kwargs["type"], ACTIVITY_ATTACHMENT_DELETED);
        assert_eq!(kwargs["requested_data"], Value::Null);
        assert_eq!(kwargs["current_instance"], Value::Null);
        let built = issue_activity_notify_kwargs(
            ACTIVITY_ATTACHMENT_DELETED,
            None,
            "79c81d76-5a93-4d3d-894d-5935576834b6",
            "ad3f003c-17a4-490c-aafb-592a1a3b770d",
            "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            None,
            1790984189,
            true,
            "http://127.0.0.1:18359",
        );
        assert_eq!(key_order(&built), ISSUE_ACTIVITY_NOTIFY_KWARG_ORDER);
        assert_eq!(Value::Object(built), activity["kwargs"]);

        let meta = message(u, GET_ASSET_OBJECT_METADATA_TASK);
        assert_eq!(
            Value::Array(asset_metadata_args("a9b33b6b-ca20-48bf-91fa-67620548ce28")),
            meta["args"]
        );
    }

    #[test]
    fn page_body_write_replays_in_publish_order() {
        let fx = fixture();
        let u = unit(&fx, "page_body_write_tasks");

        let [first, second] = record_body_write(
            "255f3646-9ace-4f46-9620-788071f7a05e",
            Some("<p>old</p>"),
            "<p>new</p>",
            "79c81d76-5a93-4d3d-894d-5935576834b6",
        );
        // Publish order is page_transaction then track_page_version
        // (`page.py:159-169`); the fixture lists them back to front.
        assert_eq!(first.0, PAGE_TRANSACTION_TASK);
        assert_eq!(second.0, TRACK_PAGE_VERSION_TASK);
        let tx_kwargs = message(u, PAGE_TRANSACTION_TASK)["kwargs"]
            .as_object()
            .expect("tx kwargs");
        assert_eq!(key_order(tx_kwargs), PAGE_TRANSACTION_KWARG_ORDER);
        assert_eq!(
            Value::Object(first.1.clone()),
            message(u, PAGE_TRANSACTION_TASK)["kwargs"]
        );
        assert_eq!(key_order(&first.1), PAGE_TRANSACTION_KWARG_ORDER);
        let version_kwargs = message(u, TRACK_PAGE_VERSION_TASK)["kwargs"]
            .as_object()
            .expect("version kwargs");
        assert_eq!(key_order(version_kwargs), TRACK_PAGE_VERSION_KWARG_ORDER);
        assert_eq!(
            Value::Object(second.1.clone()),
            message(u, TRACK_PAGE_VERSION_TASK)["kwargs"]
        );
        assert_eq!(key_order(&second.1), TRACK_PAGE_VERSION_KWARG_ORDER);
        // Exact `json.dumps({"description_html": old})` text.
        assert_eq!(
            version_kwargs["existing_instance"],
            r#"{"description_html": "<p>old</p>"}"#
        );
    }

    #[test]
    fn publish_order_matches_python_source_order() {
        // Fixture lists are reverse-publish (LIFO drain); reversing them
        // must yield the `.delay` order in the Python sources.
        let fx = fixture();
        let cases: &[(&str, &[&str])] = &[
            (
                "issue_create",
                &[
                    "pi_dash.bgtasks.issue_activities_task.issue_activity",
                    "pi_dash.bgtasks.webhook_task.model_activity",
                    "pi_dash.bgtasks.logger_task.process_logs",
                ],
            ),
            (
                "issue_patch",
                &[
                    "pi_dash.bgtasks.issue_activities_task.issue_activity",
                    "pi_dash.bgtasks.webhook_task.model_activity",
                    "pi_dash.bgtasks.logger_task.process_logs",
                ],
            ),
            (
                "link_create",
                &[
                    "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title",
                    "pi_dash.bgtasks.issue_activities_task.issue_activity",
                    "pi_dash.bgtasks.logger_task.process_logs",
                ],
            ),
            (
                "comment_create",
                &[
                    "pi_dash.bgtasks.issue_activities_task.issue_activity",
                    "pi_dash.bgtasks.webhook_task.model_activity",
                    "pi_dash.bgtasks.logger_task.process_logs",
                ],
            ),
            (
                "relation_create",
                &[
                    "pi_dash.bgtasks.issue_activities_task.issue_activity",
                    "pi_dash.bgtasks.logger_task.process_logs",
                ],
            ),
            (
                "attachment_post_presign",
                &["pi_dash.bgtasks.logger_task.process_logs"],
            ),
            (
                // The cascade publishes from the `issue.delete()` signal
                // (`:898`), before the `:900` activity emit.
                "issue_delete",
                &[
                    "pi_dash.bgtasks.deletion_task.soft_delete_related_objects",
                    "pi_dash.bgtasks.issue_activities_task.issue_activity",
                    "pi_dash.bgtasks.logger_task.process_logs",
                ],
            ),
            ("page_create", &["pi_dash.bgtasks.logger_task.process_logs"]),
            (
                "attachment_confirm",
                &[
                    "pi_dash.bgtasks.issue_activities_task.issue_activity",
                    "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata",
                    "pi_dash.bgtasks.logger_task.process_logs",
                ],
            ),
            (
                "attachment_delete",
                &[
                    "pi_dash.bgtasks.issue_activities_task.issue_activity",
                    "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata",
                    "pi_dash.bgtasks.logger_task.process_logs",
                ],
            ),
            (
                "page_body_write_tasks",
                &[
                    "pi_dash.bgtasks.page_transaction_task.page_transaction",
                    "pi_dash.bgtasks.page_version_task.track_page_version",
                ],
            ),
        ];
        for (unit_name, publish_order) in cases {
            let mut listed = tasks_of(unit(&fx, unit_name));
            listed.reverse();
            assert_eq!(listed, publish_order.to_vec(), "unit {unit_name}");
        }
    }

    #[test]
    fn page_create_passes_no_old_html() {
        // `_record_body_write(page.id, None, html, user)` (`page.py:340`).
        let [(tx_task, tx), (version_task, version)] =
            record_body_write("page-id", None, "<p>new</p>", "user-id");
        assert_eq!(tx_task, PAGE_TRANSACTION_TASK);
        assert_eq!(
            Value::Object(tx),
            json!({
                "new_description_html": "<p>new</p>",
                "old_description_html": null,
                "page_id": "page-id",
            })
        );
        assert_eq!(version_task, TRACK_PAGE_VERSION_TASK);
        assert_eq!(
            Value::Object(version),
            json!({
                "page_id": "page-id",
                "existing_instance": null,
                "user_id": "user-id",
            })
        );
    }

    #[test]
    fn uncovered_sites_keep_call_site_shapes() {
        // The fixture records the create triggers; these sibling sites use
        // the same builders with their own `type`/payload shapes
        // (synthetic ids, faithful shapes).
        let epoch = 1790984100;

        // Link patch (`:1749-1758`): string crawl args, standard order.
        assert_eq!(
            Value::Array(crawl_link_title_args("link-id", "https://example.com/x")),
            json!(["link-id", "https://example.com/x"])
        );
        let link_patch = issue_activity_kwargs(
            ACTIVITY_LINK_UPDATED,
            Some(r#"{"url": "https://example.com/x"}"#),
            "actor",
            "issue",
            "project",
            Some(r#"{"id": "link-id"}"#),
            epoch,
        );
        assert_eq!(key_order(&link_patch), ISSUE_ACTIVITY_KWARG_ORDER);

        // Link delete (`:1783-1788`): emit runs BEFORE `issue_link.delete()`
        // (the reverse of the comment/issue deletes).
        let link_delete = issue_activity_kwargs(
            ACTIVITY_LINK_DELETED,
            Some(r#"{"link_id": "link-id"}"#),
            "actor",
            "issue",
            "project",
            Some(r#"{"id": "link-id"}"#),
            epoch,
        );
        assert_eq!(link_delete["type"], ACTIVITY_LINK_DELETED);

        // Comment patch (`:2066-2082`): standard order + raw-data model emit.
        let comment_patch = issue_activity_kwargs(
            ACTIVITY_COMMENT_UPDATED,
            Some(r#"{"comment_html": "<p>edit</p>"}"#),
            "actor",
            "issue",
            "project",
            Some(r#"{"id": "comment-id"}"#),
            epoch,
        );
        assert_eq!(key_order(&comment_patch), ISSUE_ACTIVITY_KWARG_ORDER);
        let comment_model = comment_model_activity_kwargs(
            "comment-id",
            json!({"comment_html": "<p>edit</p>"}),
            Some(r#"{"id": "comment-id"}"#),
            "actor",
            "slug",
            "origin",
        );
        assert_eq!(key_order(&comment_model), MODEL_ACTIVITY_KWARG_ORDER);
        assert_eq!(comment_model["model_name"], MODEL_ISSUE_COMMENT);

        // Comment delete (`:2112-2119`): emit runs AFTER the delete.
        let comment_delete = issue_activity_kwargs(
            ACTIVITY_COMMENT_DELETED,
            Some(r#"{"comment_id": "comment-id"}"#),
            "actor",
            "issue",
            "project",
            Some(r#"{"id": "comment-id"}"#),
            epoch,
        );
        assert_eq!(comment_delete["type"], ACTIVITY_COMMENT_DELETED);

        // Issue put branches (`:683-693` update, `:741-751` create):
        // same shapes as patch/post through the same builders.
        let put_update = issue_activity_kwargs(
            ACTIVITY_ISSUE_UPDATED,
            Some(r#"{"priority": "high"}"#),
            "actor",
            "issue",
            "project",
            Some(r#"{"id": "issue"}"#),
            epoch,
        );
        assert_eq!(key_order(&put_update), ISSUE_ACTIVITY_KWARG_ORDER);
        let put_create_model = issue_model_activity_kwargs(
            "issue",
            json!({"name": "n"}),
            None,
            "actor",
            "slug",
            "origin",
        );
        assert_eq!(put_create_model["model_name"], MODEL_ISSUE);
        assert_eq!(put_create_model["current_instance"], Value::Null);
    }

    #[test]
    fn attachment_post_plan_follows_check_order() {
        let limit = FILE_SIZE_LIMIT_DEFAULT;
        // Missing/empty/zero inputs → 400 Invalid request (`:2332-2336`).
        for (name, size) in [
            (json!(null), json!(100)),
            (json!(""), json!(100)),
            (json!("f.pdf"), json!(null)),
            (json!("f.pdf"), json!(0)),
            (json!("f.pdf"), json!("")),
            (json!("f.pdf"), json!(false)),
        ] {
            assert_eq!(
                attachment_post_plan(&name, &size, &json!("application/pdf"), limit),
                AttachmentPostPlan::InvalidRequest,
                "name={name} size={size}"
            );
        }
        // Truthy but non-numeric size → TypeError 500 (`:2336`).
        for size in [json!("100"), json!([100]), json!({"n": 100})] {
            assert_eq!(
                attachment_post_plan(&json!("f.pdf"), &size, &json!("application/pdf"), limit),
                AttachmentPostPlan::SizeTypeError,
                "size={size}"
            );
        }
        // Missing/unlisted/non-string type → 400 Invalid file type.
        for mime in [
            json!(null),
            json!(false),
            json!(""),
            json!("text/html"),
            json!(5),
            json!(["application/pdf"]),
        ] {
            assert_eq!(
                attachment_post_plan(&json!("f.pdf"), &json!(100), &mime, limit),
                AttachmentPostPlan::InvalidType,
                "mime={mime}"
            );
        }
        // `min` winners pass through verbatim, negatives included.
        for (size, winner) in [
            (json!(100), json!(100)),
            (json!(10_000_000), json!(limit)),
            (json!(-5), json!(-5)),
            (json!(true), json!(true)),
            (json!(100.5), json!(100.5)),
        ] {
            assert_eq!(
                attachment_post_plan(&json!("f.pdf"), &size, &json!("application/pdf"), limit),
                AttachmentPostPlan::Ready { size_limit: winner },
                "size={size}"
            );
        }
        assert_eq!(
            invalid_request_body(),
            json!({"error": "Invalid request.", "status": false})
        );
        assert_eq!(
            invalid_file_type_body(),
            json!({"error": "Invalid file type.", "status": false})
        );
    }

    #[test]
    fn mime_allowlist_is_verbatim() {
        assert_eq!(ATTACHMENT_MIME_TYPES.len(), 73);
        assert!(ATTACHMENT_MIME_TYPES.contains(&"application/pdf"));
        assert!(!ATTACHMENT_MIME_TYPES.contains(&"text/html"));
        // QUIRK-mime-dupes: each appears exactly twice.
        for dupe in ["text/markdown", "application/x-compressed-tar-zip"] {
            assert_eq!(
                ATTACHMENT_MIME_TYPES.iter().filter(|m| **m == dupe).count(),
                2,
                "{dupe}"
            );
        }
    }

    #[test]
    fn content_disposition_matches_storage_py() {
        // Vectors verified against `urllib.parse.quote` (default safe='/').
        assert_eq!(
            content_disposition("attachment", Some("f.pdf"), "hex"),
            "attachment; filename*=UTF-8''f.pdf"
        );
        assert_eq!(
            content_disposition("attachment", Some("a b/c+d@é.pdf"), "hex"),
            "attachment; filename*=UTF-8''a%20b/c%2Bd%40%C3%A9.pdf"
        );
        assert_eq!(
            content_disposition("attachment", None, "0123456789abcdef0123456789abcdef"),
            "attachment; filename*=UTF-8''0123456789abcdef0123456789abcdef"
        );
        // Empty filename yields the bare disposition (`storage.py:110-114`).
        assert_eq!(content_disposition("inline", Some(""), "hex"), "inline");
    }

    #[test]
    fn filename_template_branch_never_triggers_for_asset_keys() {
        assert!(is_filename_template("${filename}"));
        assert!(is_filename_template("${filename}/rest"));
        assert!(!is_filename_template("ws-id/hex-f.pdf"));
        assert!(!is_filename_template(&attachment_asset_key(
            "ws-id",
            &"a".repeat(32),
            "f.pdf"
        )));
    }

    #[test]
    fn storage_metadata_gate_matches_views() {
        assert!(storage_metadata_missing(None));
        assert!(storage_metadata_missing(Some(&json!(null))));
        assert!(storage_metadata_missing(Some(&json!({}))));
        assert!(storage_metadata_missing(Some(&json!(""))));
        assert!(!storage_metadata_missing(Some(
            &json!({"ContentType": "application/pdf"})
        )));
    }

    #[test]
    fn existing_instance_rendering_matches_cpython_dumps() {
        // Expected values transcribed from CPython `json.dumps`.
        let cases = [
            ("<p>old</p>", r#"{"description_html": "<p>old</p>"}"#),
            ("a\"b\\c", r#"{"description_html": "a\"b\\c"}"#),
            (
                "line1\nline2\ttab",
                r#"{"description_html": "line1\nline2\ttab"}"#,
            ),
            (
                "café — naïve",
                r#"{"description_html": "caf\u00e9 \u2014 na\u00efve"}"#,
            ),
            (
                "a\u{2028}b\u{2029}c",
                r#"{"description_html": "a\u2028b\u2029c"}"#,
            ),
            (
                "𝄞music😀",
                r#"{"description_html": "\ud834\udd1emusic\ud83d\ude00"}"#,
            ),
            ("del\x7fchar", r#"{"description_html": "del\u007fchar"}"#),
            ("quote/slash", r#"{"description_html": "quote/slash"}"#),
        ];
        for (input, expected) in cases {
            assert_eq!(page_version_existing_instance(input), expected);
        }
    }
}
