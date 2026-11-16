//! D-09 cleanup retention + mongo flush DTOs.
//!
//! Port of the transform layer of
//! `apps/api/pi_dash/bgtasks/cleanup_task.py` (`:167-:265`): the five
//! `transform_*` functions plus the Python `str()` rendering they rely on.
//!
//! The transforms map one Django `.values()` row to one Mongo document.
//! Key order in every document matches the Python dict insertion order so
//! serialized output is byte-identical.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

/// A scalar Django value on its way through Python `str()`.
///
/// Production rows hold UUIDs and text here (all PKs/FKs are UUIDs); the
/// fixture goldens additionally exercise plain integers (`str(7) == "7"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScalarKey {
    Integer(i64),
    Uuid(Uuid),
    Text(String),
}

impl fmt::Display for ScalarKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScalarKey::Integer(n) => write!(f, "{n}"),
            ScalarKey::Uuid(id) => write!(f, "{id}"),
            ScalarKey::Text(s) => write!(f, "{s}"),
        }
    }
}

impl From<i64> for ScalarKey {
    fn from(v: i64) -> Self {
        ScalarKey::Integer(v)
    }
}

impl From<i32> for ScalarKey {
    fn from(v: i32) -> Self {
        ScalarKey::Integer(i64::from(v))
    }
}

impl From<i16> for ScalarKey {
    fn from(v: i16) -> Self {
        ScalarKey::Integer(i64::from(v))
    }
}

impl From<Uuid> for ScalarKey {
    fn from(v: Uuid) -> Self {
        ScalarKey::Uuid(v)
    }
}

impl From<String> for ScalarKey {
    fn from(v: String) -> Self {
        ScalarKey::Text(v)
    }
}

impl From<&str> for ScalarKey {
    fn from(v: &str) -> Self {
        ScalarKey::Text(v.to_owned())
    }
}

/// Render an `Option<ScalarKey>` the way Python `str()` does.
///
/// `str(None)` is `"None"`: several transforms wrap nullable columns in
/// `str()` (`entity_identifier`, `old_value`/`new_value`, the webhook text
/// fields), so a database NULL becomes the four-character string `"None"`
/// in the archived document. This ports that wart verbatim.
pub fn render_key(value: &Option<ScalarKey>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => "None".to_owned(),
    }
}

/// Render a `DateTime<Utc>` the way Python `str()` renders a Django
/// timezone-aware datetime: `2026-01-02 03:04:05+00:00`, with a six-digit
/// microsecond fraction when nonzero (`...05.123456+00:00`).
///
/// Django (`USE_TZ`) reads every timestamp as UTC, and sqlx decodes
/// `timestamptz` as `DateTime<Utc>`, so the offset is always `+00:00`.
/// Sub-microsecond remainders are truncated: Python datetimes only carry
/// microseconds.
pub fn format_py_datetime(dt: &DateTime<Utc>) -> String {
    use chrono::Timelike;
    let base = dt.format("%Y-%m-%d %H:%M:%S").to_string();
    let micros = dt.nanosecond() / 1_000;
    if micros == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{micros:06}+00:00")
    }
}

/// Render an optional datetime the way `str(x) if x.get() else None` does.
pub fn render_datetime(value: &Option<DateTime<Utc>>) -> Option<String> {
    value.as_ref().map(format_py_datetime)
}

/// A binary Django value (`BinaryField`) passing through to Mongo.
///
/// Production rows hold bytes (archived as BSON Binary); the committed
/// fixture golden for the issue-description version holds the text
/// `"Ymlu"`, which passes through as a BSON string exactly as Python's
/// passthrough would.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlobValue {
    Bytes(Vec<u8>),
    Text(String),
}

impl Serialize for BlobValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            BlobValue::Bytes(b) => serializer.collect_seq(b.iter()),
            BlobValue::Text(s) => serializer.serialize_str(s),
        }
    }
}

impl From<Vec<u8>> for BlobValue {
    fn from(v: Vec<u8>) -> Self {
        BlobValue::Bytes(v)
    }
}

impl From<String> for BlobValue {
    fn from(v: String) -> Self {
        BlobValue::Text(v)
    }
}

impl From<&str> for BlobValue {
    fn from(v: &str) -> Self {
        BlobValue::Text(v.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Row inputs: one struct per `.values()` queryset, in source key order.
// ---------------------------------------------------------------------------

/// `get_api_logs_queryset` row (`cleanup_task.py:267`).
#[derive(Clone, Debug, PartialEq)]
pub struct ApiLogRow {
    pub id: ScalarKey,
    pub created_at: Option<DateTime<Utc>>,
    pub token_identifier: String,
    pub path: String,
    pub method: String,
    pub query_params: Option<Value>,
    pub headers: Option<Value>,
    pub body: Option<Value>,
    pub response_code: i32,
    pub response_body: Option<Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_by_id: Option<ScalarKey>,
}

/// `get_email_logs_queryset` row (`cleanup_task.py:294`).
#[derive(Clone, Debug, PartialEq)]
pub struct EmailLogRow {
    pub id: ScalarKey,
    pub created_at: Option<DateTime<Utc>>,
    pub receiver_id: Option<ScalarKey>,
    pub triggered_by_id: Option<ScalarKey>,
    pub entity_identifier: Option<ScalarKey>,
    pub entity_name: String,
    pub data: Option<Value>,
    pub processed_at: Option<DateTime<Utc>>,
    pub sent_at: Option<DateTime<Utc>>,
    pub entity: String,
    pub old_value: Option<ScalarKey>,
    pub new_value: Option<ScalarKey>,
    pub created_by_id: Option<ScalarKey>,
}

/// `get_page_versions_queryset` row (`cleanup_task.py:321`).
#[derive(Clone, Debug, PartialEq)]
pub struct PageVersionRow {
    pub id: ScalarKey,
    pub created_at: Option<DateTime<Utc>>,
    pub page_id: Option<ScalarKey>,
    pub workspace_id: Option<ScalarKey>,
    pub owned_by_id: Option<ScalarKey>,
    pub description_html: String,
    pub description_binary: Option<BlobValue>,
    pub description_stripped: Option<String>,
    pub description_json: Option<Value>,
    pub sub_pages_data: Option<Value>,
    pub created_by_id: Option<ScalarKey>,
    pub updated_by_id: Option<ScalarKey>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub last_saved_at: Option<DateTime<Utc>>,
}

/// `get_issue_description_versions_queryset` row (`cleanup_task.py:357`).
#[derive(Clone, Debug, PartialEq)]
pub struct IssueDescriptionVersionRow {
    pub id: ScalarKey,
    pub created_at: Option<DateTime<Utc>>,
    pub issue_id: Option<ScalarKey>,
    pub workspace_id: Option<ScalarKey>,
    pub project_id: Option<ScalarKey>,
    pub created_by_id: Option<ScalarKey>,
    pub updated_by_id: Option<ScalarKey>,
    pub owned_by_id: Option<ScalarKey>,
    pub last_saved_at: Option<DateTime<Utc>>,
    pub description_binary: Option<BlobValue>,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub description_json: Option<Value>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// `get_webhook_logs_queryset` row (`cleanup_task.py:393`).
#[derive(Clone, Debug, PartialEq)]
pub struct WebhookLogRow {
    pub id: ScalarKey,
    pub created_at: Option<DateTime<Utc>>,
    pub workspace_id: Option<ScalarKey>,
    pub webhook: Option<ScalarKey>,
    pub event_type: Option<ScalarKey>,
    pub request_method: Option<ScalarKey>,
    pub request_headers: Option<ScalarKey>,
    pub request_body: Option<ScalarKey>,
    pub response_status: Option<ScalarKey>,
    pub response_body: Option<ScalarKey>,
    pub response_headers: Option<ScalarKey>,
    pub retry_count: i16,
}

// ---------------------------------------------------------------------------
// Mongo documents: field order matches the Python dict insertion order.
// ---------------------------------------------------------------------------

/// `transform_api_log` output (`cleanup_task.py:167`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ApiLogDoc {
    pub id: String,
    pub created_at: Option<String>,
    pub token_identifier: String,
    pub path: String,
    pub method: String,
    pub query_params: Option<Value>,
    pub headers: Option<Value>,
    pub body: Option<Value>,
    pub response_code: i32,
    pub response_body: Option<Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_by_id: String,
}

/// `transform_email_log` output (`cleanup_task.py:186`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EmailLogDoc {
    pub id: String,
    pub created_at: Option<String>,
    pub receiver_id: String,
    pub triggered_by_id: String,
    pub entity_identifier: String,
    pub entity_name: String,
    pub data: Option<Value>,
    pub processed_at: Option<String>,
    pub sent_at: Option<String>,
    pub entity: String,
    pub old_value: String,
    pub new_value: String,
    pub created_by_id: String,
}

/// `transform_page_version` output (`cleanup_task.py:205`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageVersionDoc {
    pub id: String,
    pub created_at: Option<String>,
    pub page_id: String,
    pub workspace_id: String,
    pub owned_by_id: String,
    pub description_html: String,
    pub description_binary: Option<BlobValue>,
    pub description_stripped: Option<String>,
    pub description_json: Option<Value>,
    pub sub_pages_data: Option<Value>,
    pub created_by_id: String,
    pub updated_by_id: String,
    pub deleted_at: Option<String>,
    pub last_saved_at: Option<String>,
}

/// `transform_issue_description_version` output (`cleanup_task.py:225`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IssueDescriptionVersionDoc {
    pub id: String,
    pub created_at: Option<String>,
    pub issue_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub created_by_id: String,
    pub updated_by_id: String,
    pub owned_by_id: String,
    pub last_saved_at: Option<String>,
    pub description_binary: Option<BlobValue>,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub description_json: Option<Value>,
    pub deleted_at: Option<String>,
}

/// `transform_webhook_log` output (`cleanup_task.py:245`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WebhookLogDoc {
    pub id: String,
    pub created_at: Option<String>,
    pub workspace_id: String,
    pub webhook: String,
    pub event_type: String,
    pub request_method: String,
    pub request_headers: String,
    pub request_body: String,
    pub response_status: String,
    pub response_body: String,
    pub response_headers: String,
    pub retry_count: String,
}

// ---------------------------------------------------------------------------
// Transforms.
// ---------------------------------------------------------------------------

/// Port of `transform_api_log` (`cleanup_task.py:167-183`).
pub fn transform_api_log(record: &ApiLogRow) -> ApiLogDoc {
    ApiLogDoc {
        id: record.id.to_string(),
        created_at: render_datetime(&record.created_at),
        token_identifier: record.token_identifier.clone(),
        path: record.path.clone(),
        method: record.method.clone(),
        query_params: record.query_params.clone(),
        headers: record.headers.clone(),
        body: record.body.clone(),
        response_code: record.response_code,
        response_body: record.response_body.clone(),
        ip_address: record.ip_address.clone(),
        user_agent: record.user_agent.clone(),
        created_by_id: render_key(&record.created_by_id),
    }
}

/// Port of `transform_email_log` (`cleanup_task.py:186-202`).
pub fn transform_email_log(record: &EmailLogRow) -> EmailLogDoc {
    EmailLogDoc {
        id: record.id.to_string(),
        created_at: render_datetime(&record.created_at),
        receiver_id: render_key(&record.receiver_id),
        triggered_by_id: render_key(&record.triggered_by_id),
        entity_identifier: render_key(&record.entity_identifier),
        entity_name: record.entity_name.clone(),
        data: record.data.clone(),
        processed_at: render_datetime(&record.processed_at),
        sent_at: render_datetime(&record.sent_at),
        entity: record.entity.clone(),
        old_value: render_key(&record.old_value),
        new_value: render_key(&record.new_value),
        created_by_id: render_key(&record.created_by_id),
    }
}

/// Port of `transform_page_version` (`cleanup_task.py:205-222`).
pub fn transform_page_version(record: &PageVersionRow) -> PageVersionDoc {
    PageVersionDoc {
        id: record.id.to_string(),
        created_at: render_datetime(&record.created_at),
        page_id: render_key(&record.page_id),
        workspace_id: render_key(&record.workspace_id),
        owned_by_id: render_key(&record.owned_by_id),
        description_html: record.description_html.clone(),
        description_binary: record.description_binary.clone(),
        description_stripped: record.description_stripped.clone(),
        description_json: record.description_json.clone(),
        sub_pages_data: record.sub_pages_data.clone(),
        created_by_id: render_key(&record.created_by_id),
        updated_by_id: render_key(&record.updated_by_id),
        deleted_at: render_datetime(&record.deleted_at),
        last_saved_at: render_datetime(&record.last_saved_at),
    }
}

/// Port of `transform_issue_description_version` (`cleanup_task.py:225-242`).
pub fn transform_issue_description_version(
    record: &IssueDescriptionVersionRow,
) -> IssueDescriptionVersionDoc {
    IssueDescriptionVersionDoc {
        id: record.id.to_string(),
        created_at: render_datetime(&record.created_at),
        issue_id: render_key(&record.issue_id),
        workspace_id: render_key(&record.workspace_id),
        project_id: render_key(&record.project_id),
        created_by_id: render_key(&record.created_by_id),
        updated_by_id: render_key(&record.updated_by_id),
        owned_by_id: render_key(&record.owned_by_id),
        last_saved_at: render_datetime(&record.last_saved_at),
        description_binary: record.description_binary.clone(),
        description_html: record.description_html.clone(),
        description_stripped: record.description_stripped.clone(),
        description_json: record.description_json.clone(),
        deleted_at: render_datetime(&record.deleted_at),
    }
}

/// Port of `transform_webhook_log` (`cleanup_task.py:245-263`).
///
/// Every value column is `str()`-wrapped in Python, including the nullable
/// text columns (`None` becomes `"None"`) and the integer columns
/// (`retry_count`, `response_status` content, `webhook` id content).
pub fn transform_webhook_log(record: &WebhookLogRow) -> WebhookLogDoc {
    WebhookLogDoc {
        id: record.id.to_string(),
        created_at: render_datetime(&record.created_at),
        workspace_id: render_key(&record.workspace_id),
        webhook: render_key(&record.webhook),
        event_type: render_key(&record.event_type),
        request_method: render_key(&record.request_method),
        request_headers: render_key(&record.request_headers),
        request_body: render_key(&record.request_body),
        response_status: render_key(&record.response_status),
        response_body: render_key(&record.response_body),
        response_headers: render_key(&record.response_headers),
        retry_count: record.retry_count.to_string(),
    }
}

/// Serialize a document exactly as it is compared against the fixture
/// goldens: `serde_json` preserves struct field declaration order, which
/// matches the Python dict insertion order field for field.
pub fn doc_json<T: Serialize>(doc: &T) -> String {
    serde_json::to_string(doc).expect("cleanup docs are JSON-serializable")
}

/// The same document as an ordered JSON map (e.g. for BSON conversion).
pub fn doc_map<T: Serialize>(doc: &T) -> Map<String, Value> {
    match serde_json::to_value(doc).expect("cleanup docs are JSON-serializable") {
        Value::Object(map) => map,
        _ => unreachable!("cleanup docs serialize to JSON objects"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Committed translation evidence this port replays:
    /// `rust-api/fixtures/tasks_cleanup/cleanup.json`.
    static FIXTURE: &str = include_str!("../../../../fixtures/tasks_cleanup/cleanup.json");

    fn golden(name: &str) -> String {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let doc = &fixture["transforms"][name];
        assert!(doc.is_object(), "missing golden {name}");
        serde_json::to_string(doc).expect("golden serializes")
    }

    fn dt(s: &str) -> Option<DateTime<Utc>> {
        Some(s.parse::<DateTime<Utc>>().expect("fixture datetime parses"))
    }

    #[test]
    fn api_log_full_replays_byte_identical() {
        // Synthetic generator input: ints for ids, real datetime strings,
        // JSON values for the text passthroughs.
        let row = ApiLogRow {
            id: ScalarKey::Integer(7),
            created_at: dt("2026-01-02T03:04:05Z"),
            token_identifier: "tok_abc".to_owned(),
            path: "/api/issues".to_owned(),
            method: "POST".to_owned(),
            query_params: Some(serde_json::json!({"page": "2"})),
            headers: Some(serde_json::json!({"X-A": "b"})),
            body: Some(serde_json::json!({"k": 1})),
            response_code: 201,
            response_body: Some(serde_json::json!({"id": "x"})),
            ip_address: Some("10.0.0.1".to_owned()),
            user_agent: Some("ua/1".to_owned()),
            created_by_id: Some(ScalarKey::Integer(42)),
        };
        assert_eq!(doc_json(&transform_api_log(&row)), golden("api_log_full"));
    }

    #[test]
    fn api_log_missing_created_at_is_none() {
        let row = ApiLogRow {
            id: ScalarKey::Integer(8),
            created_at: None,
            token_identifier: "t".to_owned(),
            path: "/p".to_owned(),
            method: "GET".to_owned(),
            query_params: None,
            headers: None,
            body: None,
            response_code: 200,
            response_body: None,
            ip_address: Some("127.0.0.1".to_owned()),
            user_agent: Some("u".to_owned()),
            created_by_id: Some(ScalarKey::Integer(1)),
        };
        assert_eq!(
            doc_json(&transform_api_log(&row)),
            golden("api_log_missing_created_at_is_none")
        );
    }

    #[test]
    fn email_log_processed_at_none_replays_byte_identical() {
        let row = EmailLogRow {
            id: ScalarKey::Integer(11),
            created_at: dt("2026-02-03T04:05:06Z"),
            receiver_id: Some(ScalarKey::Integer(5)),
            triggered_by_id: Some(ScalarKey::Integer(6)),
            entity_identifier: Some(ScalarKey::Text("E-1".to_owned())),
            entity_name: "issue".to_owned(),
            data: Some(serde_json::json!({"a": 1})),
            processed_at: None,
            sent_at: dt("2026-02-03T05:00:00Z"),
            entity: "issue".to_owned(),
            old_value: Some(ScalarKey::Text("a".to_owned())),
            new_value: Some(ScalarKey::Text("b".to_owned())),
            created_by_id: Some(ScalarKey::Integer(6)),
        };
        assert_eq!(
            doc_json(&transform_email_log(&row)),
            golden("email_log_processed_at_none")
        );
    }

    #[test]
    fn page_version_nones_replays_byte_identical() {
        let row = PageVersionRow {
            id: ScalarKey::Integer(21),
            created_at: dt("2026-03-01T00:00:00Z"),
            page_id: Some(ScalarKey::Text(
                "aaaaaaaa-1111-2222-3333-444444444444".to_owned(),
            )),
            workspace_id: Some(ScalarKey::Text(
                "bbbbbbbb-1111-2222-3333-444444444444".to_owned(),
            )),
            owned_by_id: Some(ScalarKey::Integer(9)),
            description_html: "<p>hi</p>".to_owned(),
            description_binary: None,
            description_stripped: Some("hi".to_owned()),
            description_json: Some(serde_json::json!({"t": "doc"})),
            sub_pages_data: Some(serde_json::json!({"x": []})),
            created_by_id: Some(ScalarKey::Integer(9)),
            updated_by_id: Some(ScalarKey::Integer(10)),
            deleted_at: None,
            last_saved_at: None,
        };
        assert_eq!(
            doc_json(&transform_page_version(&row)),
            golden("page_version_nones")
        );
    }

    #[test]
    fn issue_description_version_replays_byte_identical() {
        // Note: the generator fed the text "Ymlu" (not bytes) for
        // description_binary; the Text variant replays it exactly.
        let row = IssueDescriptionVersionRow {
            id: ScalarKey::Integer(31),
            created_at: None,
            issue_id: Some(ScalarKey::Integer(100)),
            workspace_id: Some(ScalarKey::Text(
                "cccccccc-1111-2222-3333-444444444444".to_owned(),
            )),
            project_id: Some(ScalarKey::Text(
                "dddddddd-1111-2222-3333-444444444444".to_owned(),
            )),
            created_by_id: Some(ScalarKey::Integer(3)),
            updated_by_id: Some(ScalarKey::Integer(4)),
            owned_by_id: Some(ScalarKey::Integer(4)),
            last_saved_at: dt("2026-04-01T12:00:00Z"),
            description_binary: Some(BlobValue::Text("Ymlu".to_owned())),
            description_html: "<p>v</p>".to_owned(),
            description_stripped: Some("v".to_owned()),
            description_json: Some(serde_json::json!({})),
            deleted_at: dt("2026-04-02T00:00:00Z"),
        };
        assert_eq!(
            doc_json(&transform_issue_description_version(&row)),
            golden("issue_description_version")
        );
    }

    #[test]
    fn webhook_log_str_coercions_replays_byte_identical() {
        // The generator fed ints and dicts; in production these columns
        // are text/ints whose str() renders identically. The dict reprs
        // ("{'H': 'v'}") replay as opaque text.
        let row = WebhookLogRow {
            id: ScalarKey::Integer(55),
            created_at: dt("2026-05-01T00:00:00Z"),
            workspace_id: Some(ScalarKey::Text(
                "eeeeeeee-1111-2222-3333-444444444444".to_owned(),
            )),
            webhook: Some(ScalarKey::Integer(77)),
            event_type: Some(ScalarKey::Text("ISSUE_CREATED".to_owned())),
            request_method: Some(ScalarKey::Text("POST".to_owned())),
            request_headers: Some(ScalarKey::Text("{'H': 'v'}".to_owned())),
            request_body: Some(ScalarKey::Text("{'n': 1}".to_owned())),
            response_status: Some(ScalarKey::Integer(200)),
            response_body: Some(ScalarKey::Text("ok".to_owned())),
            response_headers: Some(ScalarKey::Text("{'R': 'h'}".to_owned())),
            retry_count: 0,
        };
        assert_eq!(
            doc_json(&transform_webhook_log(&row)),
            golden("webhook_log_str_coercions")
        );
    }

    #[test]
    fn uuid_and_none_render_like_python_str() {
        let id = Uuid::parse_str("aaaaaaaa-1111-2222-3333-444444444444").unwrap();
        assert_eq!(
            ScalarKey::Uuid(id).to_string(),
            "aaaaaaaa-1111-2222-3333-444444444444"
        );
        assert_eq!(render_key(&None), "None");
        assert_eq!(render_key(&Some(ScalarKey::Integer(0))), "0");
    }

    #[test]
    fn datetime_renders_like_python_str() {
        let whole = "2026-01-02T03:04:05Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(format_py_datetime(&whole), "2026-01-02 03:04:05+00:00");
        let frac = "2026-01-02T03:04:05.123456789Z"
            .parse::<DateTime<Utc>>()
            .unwrap();
        assert_eq!(
            format_py_datetime(&frac),
            "2026-01-02 03:04:05.123456+00:00"
        );
        assert_eq!(render_datetime(&None), None);
    }
}
