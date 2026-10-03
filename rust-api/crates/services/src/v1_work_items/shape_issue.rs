#![forbid(unsafe_code)]

//! `IssueSerializer` core (D-18 serializers A, PIDASHCONV-660).
//!
//! Ports `apps/api/pi_dash/api/serializers/issue.py:54-108` (`_same_uuid`,
//! `normalize_description_input`) and `:109-496` (`IssueSerializer`: `__init__`
//! `:172-176`, `to_internal_value` `:177-180`, `validate` `:182-288`,
//! `create` `:290-371`, `update` `:373-427`, `get_url` `:429-434`,
//! `to_representation` `:436-494`, declared fields `:118-151`, `Meta`
//! `:153-159`).
//!
//! Fixture: F18-01
//! (`rust-api/fixtures/v1_work_items/serializers/F18-01.issue_serializer.golden.json`).
//! Every `#[test]` below replays it: golden in/out byte-identical,
//! including validation error strings and the description/markdown control
//! flow.
//!
//! This module is pure: every check that needs the database, the sanitizer,
//! the HTML parser or the markdown converter in Python takes the
//! already-resolved fact as an argument. The queries and handler layers
//! supply those facts and perform the writes; the error bodies, key orders
//! and check order here are the contract they must honor.
//!
//! Seams (caller-supplied, resolved outside this module):
//!
//! * Markdown → HTML conversion (`utils/markdown_converter.py`, the Tiptap
//!   renderer over markdown-it-py + tasklists plugin + `validate_html_content`).
//!   No Rust port exists and no issue owns it; [`normalize_description_input`]
//!   takes the converter as a closure and ports the precedence/popping/flag
//!   logic plus both error mappings exactly. Handler issue PIDASHCONV-673
//!   needs the real converter for byte-identical `description_markdown` writes.
//! * `nh3.clean` sanitization (`utils/content_validator.py:224-240`; `ammonia`
//!   4.x is locked for the handler layer, same bytes): the [`HtmlCheck`]
//!   verdict, following the D-30 page precedent
//!   (`app_pages::shape::substitute_description_html`).
//! * The lxml `fromstring`/`tostring` round-trip (`issue.py:224-226`): the
//!   round-tripped string, or failure. Failure/success is the only thing the
//!   serializer branches on.
//! * DB reads: assignee/label filter querysets (whose `-created_at` order is
//!   data-derived), state/parent/estimate/pod existence, the default
//!   `IssueType`, `has_active_run`, relation rows, expanded rows.
//!
//! Reused, not forked: [`crate::v1_projects::ser_collab`] `UserLite`
//! (assignees expansion — the same `api/serializers/user.py:13-38` unit D-19
//! ported) and [`crate::app_pages::shape::validate_binary_data`] (the same
//! `content_validator.py` function D-30 ported). `StateLite`/`CycleLite`/
//! `ModuleLite` are imported by `issue.py:44-47` but used only by sibling
//! scopes (PIDASHCONV-661/665), never by `IssueSerializer` core —
//! deliberately not imported here. Expanded labels arrive pre-rendered: the
//! `LabelSerializer` read shape is PIDASHCONV-661's scope.
//!
//! JSON rendering notes:
//!
//! * Every static error body is a byte-exact `&str` const in DRF key order;
//!   dynamic bodies use `format!` + [`escape_json_string`]. DRF's compact
//!   separators (`(',', ':')`) match `serde_json` compact output.
//! * `to_representation` builds a `serde_json::Map` in wire order; key order
//!   is insertion order (`preserve_order`, declared on this crate's
//!   `serde_json` dependency). Byte-exact order assertions in the tests guard it.
//! * DRF wraps `ValidationError("msg")` raised in `validate()` as
//!   `{"non_field_errors": ["msg"]}`, and `ValidationError({"f": "msg"})` as
//!   `{"f": ["msg"]}`. `ListField` child failures collect as an
//!   `{index: detail}` dict (`run_child_validation`, verified against the
//!   installed DRF `fields.py`), so a bad label renders
//!   `{"labels": {"1": ["Invalid pk ..."]}}`.
//! * `validate()` runs only when NO field error exists (DRF
//!   `Serializer.to_internal_value` raises before `run_validation` reaches
//!   `validate()`), and field errors combine in field order
//!   ([`FIELDS_IN_ORDER`]). Field validation runs over the `?fields=`-kept
//!   fields only (`self.fields` is filtered in `__init__`).
//!
//! Ported quirks (translate, don't redesign — all verified against the
//! Python/DRF sources or the fixture):
//!
//! * `type_id` (declared, `source="type"`) and the auto `type` field coexist:
//!   both render the same pk (`single_keys_in_order`), and on write both feed
//!   `validated_data["type"]` with the later auto field winning.
//! * `description_markdown` never reaches field validation (always pre-popped);
//!   an explicit `null` is silently dropped, a non-string 400s.
//! * A non-string legacy `description` is silently dropped (popped, never converted).
//! * `create()` has no `ignore_conflicts` (a conflicting bulk batch aborts the
//!   whole call into `IntegrityError → pass`) while `update()` passes
//!   `ignore_conflicts=True`.
//! * `?expand=url` removes `url`; `?expand=description_markdown` adds a `null`
//!   key; `?expand=<scalar>` nulls that scalar (Base `else` branch).
//! * Expanding a null-FK relation renders `{}` (DRF `SkipField` on every field).

use serde::Serialize;
use serde_json::{Map, Value};

use crate::app_pages::shape::validate_binary_data;
use crate::v1_projects::ser_collab::{user_lite_to_representation, UserLiteRow};

use super::{filter_fields, FieldSpec, FilterError};

/// Compare two UUID-ish values by canonical form
/// (`serializers/issue.py:54-67`).
///
/// `project_id` reaches the serializer as a raw `<str:project_id>` URL kwarg,
/// so uppercase/unhyphenated spellings must not falsely fail against a
/// canonical DB UUID. Falls back to plain string equality when either side is
/// not a parseable UUID. Callers stringify first (`None` as `"None"`, per
/// Python `str()`); `uuid::Uuid::parse_str` accepts the same spellings Python
/// `uuid.UUID` does (hyphenated, simple, braced, `urn:uuid:`).
pub fn same_uuid(left: &str, right: &str) -> bool {
    match (uuid::Uuid::parse_str(left), uuid::Uuid::parse_str(right)) {
        (Ok(a), Ok(b)) => a == b,
        _ => left == right,
    }
}

/// A `description_markdown` input value (`normalize_description_input`,
/// `serializers/issue.py:91-101`): absent and explicit-null both pop to
/// `None`; anything non-string and non-null 400s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownInput<'a> {
    /// Key absent: no pop, may still convert via the legacy key.
    Absent,
    /// Explicit `null`: popped, treated as `None` (silently dropped).
    Null,
    /// A string: converted (takes precedence over every other key).
    Text(&'a str),
    /// A non-string JSON value (number/bool/list/dict): 400.
    NonString,
}

/// A legacy `description` input value (`issue.py:95-97`): converted like
/// markdown only when `description_markdown` is absent/null AND
/// `description_html` is absent AND the value is a string; otherwise popped
/// and silently dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyInput<'a> {
    /// Key absent.
    Absent,
    /// Explicit `null`: popped, never converted.
    Null,
    /// A string: converted when (and only when) the markdown key is
    /// absent/null and `description_html` is absent.
    Text(&'a str),
    /// A non-string value: popped, silently dropped (never an error —
    /// only the markdown key raises `Must be a string.`).
    NonString,
}

/// What [`normalize_description_input`] does to the body: the popped-keys /
/// `description_html` effect of `issue.py:91-106`. The views apply this to
/// the request body before building the serializer; `to_internal_value` calls
/// it too for the other write paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizeAction {
    /// Neither markdown key present: `data` is returned unchanged (the SAME
    /// object, `F18-01 neither_key same_object`).
    Unchanged,
    /// A markdown key converted: both markdown keys are popped and
    /// `description_html` is set to the converted body (appended last —
    /// input key order is unobservable downstream since representation order
    /// is field order).
    SetHtml(String),
    /// A markdown key was present but nothing converted (both popped values
    /// `None`, or a non-string legacy with no markdown): the keys are popped
    /// and `description_html` is left untouched.
    DropKeys,
}

/// Outcome of [`normalize_description_input`]: the body effect plus the
/// `from_markdown` flag the views pass on as the
/// `description_from_markdown` context key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizeOutcome {
    pub action: NormalizeAction,
    pub from_markdown: bool,
}

/// Failures of [`normalize_description_input`], both rendered
/// `{"description_markdown": ["<message>"]}` with status 400.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NormalizeError {
    /// Non-string `description_markdown` (`issue.py:100-101`).
    #[error("Must be a string.")]
    NonStringMarkdown,
    /// The converter raised `ValueError` (`issue.py:102-105`); carries
    /// `str(exc)` verbatim (e.g. the 10MB cap message from
    /// `validate_html_content`).
    #[error("{0}")]
    ConvertFailed(String),
}

impl NormalizeError {
    /// Byte-exact 400 response body.
    pub fn body(&self) -> String {
        match self {
            NormalizeError::NonStringMarkdown => MARKDOWN_NON_STRING_BODY.to_string(),
            NormalizeError::ConvertFailed(message) => markdown_convert_failed_body(message),
        }
    }
}

/// Popped-value model for the precedence logic: `None` covers absent AND
/// explicit-null (`dict.pop(key, None)`), matching `issue.py:94-99`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Popped<'a> {
    None,
    Str(&'a str),
    Other,
}

impl<'a> From<MarkdownInput<'a>> for Popped<'a> {
    fn from(input: MarkdownInput<'a>) -> Self {
        match input {
            MarkdownInput::Absent | MarkdownInput::Null => Popped::None,
            MarkdownInput::Text(text) => Popped::Str(text),
            MarkdownInput::NonString => Popped::Other,
        }
    }
}

impl<'a> From<LegacyInput<'a>> for Popped<'a> {
    fn from(input: LegacyInput<'a>) -> Self {
        match input {
            LegacyInput::Absent | LegacyInput::Null => Popped::None,
            LegacyInput::Text(text) => Popped::Str(text),
            LegacyInput::NonString => Popped::Other,
        }
    }
}

/// Port of `normalize_description_input` (`serializers/issue.py:68-108`).
///
/// `convert` is the Tiptap `markdown_to_html` renderer (no Rust port exists
/// yet — see the module docs): `Ok(html)` is stored, `Err(message)` becomes
/// the 400 detail exactly like the Python `ValueError` (`issue.py:104-105`).
/// `html_present` is whether `description_html` is already in the body (it
/// blocks the legacy fallback, `issue.py:96`, and is otherwise untouched).
///
/// `QueryDict` bodies (`issue.py:93`, `data.dict()` takes the LAST value per
/// key) are a handler-layer concern: JSON callers pass the parsed values.
pub fn normalize_description_input(
    markdown: MarkdownInput<'_>,
    legacy: LegacyInput<'_>,
    html_present: bool,
    convert: &dyn Fn(&str) -> Result<String, String>,
) -> Result<NormalizeOutcome, NormalizeError> {
    if matches!(markdown, MarkdownInput::Absent) && matches!(legacy, LegacyInput::Absent) {
        return Ok(NormalizeOutcome {
            action: NormalizeAction::Unchanged,
            from_markdown: false,
        });
    }
    let mut effective = Popped::from(markdown);
    if matches!(effective, Popped::None) && !html_present {
        if let Popped::Str(text) = Popped::from(legacy) {
            effective = Popped::Str(text);
        }
    }
    match effective {
        Popped::None => Ok(NormalizeOutcome {
            action: NormalizeAction::DropKeys,
            from_markdown: false,
        }),
        Popped::Other => Err(NormalizeError::NonStringMarkdown),
        Popped::Str(text) => match convert(text) {
            Ok(html) => Ok(NormalizeOutcome {
                action: NormalizeAction::SetHtml(html),
                from_markdown: true,
            }),
            Err(message) => Err(NormalizeError::ConvertFailed(message)),
        },
    }
}

/// Port of the `to_internal_value` flag line (`issue.py:177-180`):
/// `from_markdown or context["description_from_markdown"]`. The views set the
/// context key when THEY normalized first, so markdown-converted HTML skips
/// the lxml/sanitizer branches in `validate()` on every write path.
pub fn description_from_markdown(normalized_here: bool, context_flag: bool) -> bool {
    normalized_here || context_flag
}

/// `IssueSerializer` fields in wire order (`serializers/issue.py:118-159`,
/// pinned by F18-01 `serializer_field_names`): Base-declared `id`, the five
/// declared fields, then the model fields in `_meta` order — including BOTH
/// `type_id` (declared, `source="type"`) and the auto `type` field. Field
/// errors combine in this order; `to_representation` renders the readable
/// subset (see [`READABLE_FIELDS_IN_ORDER`]) in this order.
pub const FIELDS_IN_ORDER: &[&str] = &[
    "id",
    "assignees",
    "labels",
    "type_id",
    "url",
    "description_markdown",
    "created_at",
    "updated_at",
    "deleted_at",
    "point",
    "name",
    "description_html",
    "description_binary",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "git_work_branch",
    "created_via",
    "agent_executor",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "parent",
    "state",
    "estimate_point",
    "type",
    "assigned_pod",
];

/// Write-only fields (`issue.py:118-151`): accepted on write, absent from the
/// base representation (assignees/labels are appended back by
/// `to_representation`; `description_markdown` never appears — except via the
/// `?expand=description_markdown` quirk, see [`render_issue`]).
pub const WRITE_ONLY_FIELDS: &[&str] = &["assignees", "labels", "description_markdown"];

/// Read-only fields: `Meta.read_only_fields` (`issue.py:155`) plus the
/// always-read extras — `url` (`SerializerMethodField`), `description_binary`
/// (DRF maps `BinaryField` to a read-only `ModelField`, so writes silently
/// drop it and the `validate()` binary branch is dead via this serializer)
/// and `created_at` (auto-added `DateTimeField`, `auto_now_add`). These never
/// appear in `validated_data`.
pub const READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "url",
    "description_binary",
    "workspace",
    "project",
    "updated_by",
    "updated_at",
    "created_at",
];

/// Readable fields in representation order: [`FIELDS_IN_ORDER`] minus
/// [`WRITE_ONLY_FIELDS`] (DRF `_readable_fields`). F18-01
/// `single_keys_in_order` is this list plus `assignees`, `labels`,
/// `relations_summary`, `has_open_blockers` appended by `to_representation`.
pub const READABLE_FIELDS_IN_ORDER: &[&str] = &[
    "id",
    "type_id",
    "url",
    "created_at",
    "updated_at",
    "deleted_at",
    "point",
    "name",
    "description_html",
    "description_binary",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "git_work_branch",
    "created_via",
    "agent_executor",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "parent",
    "state",
    "estimate_point",
    "type",
    "assigned_pod",
];

/// Read-only blocker keys `to_representation` appends for single-item
/// payloads (`issue.py:161-163`, `RELATIONS_SUMMARY_KEYS`).
pub const RELATIONS_SUMMARY_KEYS: &[&str] = &["relations_summary", "has_open_blockers"];

/// Context key carrying the requesting user (`issue.py:165-170`,
/// `RELATIONS_VIEWER_CONTEXT`): when set on a single-item payload, the
/// `relations` block is appended.
pub const RELATIONS_VIEWER_CONTEXT: &str = "relations_viewer";

/// Context key the views set when they normalized the body first
/// (`issue.py:179,221`): OR-ed with the `to_internal_value` flag.
pub const DESCRIPTION_FROM_MARKDOWN_CONTEXT: &str = "description_from_markdown";

/// `BaseSerializer.to_representation` expansion map keys
/// (`api/serializers/base.py:91-106`), in source order. Only membership is
/// consulted: an `expand` name in the kept fields renders its mapped shape
/// when listed here, else the `<name>_id` passthrough rule applies (see
/// [`render_issue`]).
pub const BASE_EXPANSION_NAMES: &[&str] = &[
    "user",
    "workspace",
    "project",
    "default_assignee",
    "project_lead",
    "state",
    "created_by",
    "updated_by",
    "issue",
    "actor",
    "owned_by",
    "members",
    "parent",
    "estimate_point",
];

/// `validate()` start/target check (`serializers/issue.py:183-188`).
pub const START_EXCEEDS_TARGET_BODY: &str =
    r#"{"non_field_errors":["Start date cannot exceed target date"]}"#;
/// `validate()` state-project check (`:261-265`).
pub const STATE_WRONG_PROJECT_BODY: &str =
    r#"{"non_field_errors":["State is not valid please pass a valid state_id"]}"#;
/// `validate()` parent-scope check (`:267-276`).
pub const PARENT_WRONG_PROJECT_BODY: &str =
    r#"{"non_field_errors":["Parent is not valid issue_id please pass a valid issue_id"]}"#;
/// `validate()` estimate-point-scope check (`:278-286`).
pub const ESTIMATE_POINT_WRONG_PROJECT_BODY: &str =
    r#"{"non_field_errors":["Estimate point is not valid please pass a valid estimate_point_id"]}"#;
/// `validate()` lxml round-trip failure (`:222-229`). Raised as a bare
/// message, hence `non_field_errors` — including for the empty string
/// (`html.fromstring("")` raises `ParserError: Document is empty`).
pub const INVALID_HTML_BODY: &str = r#"{"non_field_errors":["Invalid HTML passed"]}"#;
/// `validate()` sanitizer rejection (`:232-235`).
pub const HTML_CONTENT_INVALID_BODY: &str = r#"{"error":["html content is not valid"]}"#;
/// `validate()` binary rejection (`:240-243`). Dead via this serializer
/// (`description_binary` is read-only so the branch never fires), ported
/// for direct-call parity.
pub const BINARY_INVALID_BODY: &str = r#"{"description_binary":["Invalid binary data"]}"#;
/// `validate()` pod-project check (`:199-203`).
pub const POD_WRONG_PROJECT_BODY: &str = r#"{"assigned_pod":["pod is in a different project"]}"#;
/// `validate()` pod-reassign guard (`:211-216`).
pub const POD_REASSIGN_ACTIVE_RUN_BODY: &str =
    r#"{"assigned_pod":["cannot reassign pod while the issue has an active run"]}"#;
/// `normalize_description_input` non-string markdown (`:100-101`).
pub const MARKDOWN_NON_STRING_BODY: &str = r#"{"description_markdown":["Must be a string."]}"#;

/// Escape a string for embedding in a JSON body (mirrors the D-30
/// `app_pages::shape` escaper, which is private to that module): `"`, `\`,
/// C0 controls; printable ASCII and all of non-ASCII pass through (DRF
/// `ensure_ascii=False`… precisely, DRF `JSONRenderer.ensure_ascii` defaults
/// `False`, so UTF-8 bytes pass through unescaped).
fn escape_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// `PrimaryKeyRelatedField` miss body for this serializer's relation fields
/// (`assignees`/`labels` children, `type_id`, `state`, `parent`,
/// `estimate_point`, `assigned_pod`): `{"<field>": ["Invalid pk \"<raw>\" -
/// object does not exist."]}`. `pk_value` is the RAW input echoed verbatim
/// (`relations.py:241,261`); JSON-escaped here.
pub fn pk_does_not_exist_body(field: &str, pk_value: &str) -> String {
    format!(
        "{{\"{}\":[\"Invalid pk \\\"{}\\\" - object does not exist.\"]}}",
        escape_json_string(field),
        escape_json_string(pk_value)
    )
}

/// `ListField` child-failure body for `assignees`/`labels`
/// (`fields.py:run_child_validation`): `{"<field>": {"<idx>": ["<msg>", ...],
/// ...}}` with indices in ascending enumeration order and messages per index
/// in order. Each message is one child detail already rendered to string
/// (e.g. the `Invalid pk ...` text).
pub fn list_index_errors_body(field: &str, failures: &[(usize, Vec<String>)]) -> String {
    let mut ordered: Vec<(usize, &Vec<String>)> =
        failures.iter().map(|(idx, msgs)| (*idx, msgs)).collect();
    ordered.sort_by_key(|(idx, _)| *idx);
    let mut out = String::from("{");
    out.push('"');
    out.push_str(&escape_json_string(field));
    out.push_str("\":{");
    for (pos, (idx, msgs)) in ordered.iter().enumerate() {
        if pos > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&idx.to_string());
        out.push_str("\":[");
        for (mpos, msg) in msgs.iter().enumerate() {
            if mpos > 0 {
                out.push(',');
            }
            out.push('"');
            out.push_str(&escape_json_string(msg));
            out.push('"');
        }
        out.push(']');
    }
    out.push_str("}}");
    out
}

/// `ListField` non-list body (`fields.py:not_a_list`):
/// `{"<field>": ["Expected a list of items but got type \"<pytype>\"."]}`.
/// `input_type` is the PYTHON type name (`type(data).__name__`): `dict`,
/// `str`, `int`, `float`, `bool`, `NoneType` for the JSON shapes.
pub fn not_a_list_body(field: &str, input_type: &str) -> String {
    format!(
        "{{\"{}\":[\"Expected a list of items but got type \\\"{}\\\".\"]}}",
        escape_json_string(field),
        escape_json_string(input_type)
    )
}

/// Converter-failure body (`issue.py:104-105`):
/// `{"description_markdown": ["<str(exc)>"]}`.
pub fn markdown_convert_failed_body(message: &str) -> String {
    format!(
        "{{\"description_markdown\":[\"{}\"]}}",
        escape_json_string(message)
    )
}

/// Combine several field errors into one 400 body, in the given order
/// (callers pass [`FIELDS_IN_ORDER`] order over the `?fields=`-kept fields —
/// DRF assigns `errors[field.field_name]` while iterating `self.fields`).
/// Each detail is the field's already-shaped value (list or index-dict).
pub fn field_errors_body(entries: &[(&str, Value)]) -> String {
    let mut out = String::from("{");
    for (pos, (field, detail)) in entries.iter().enumerate() {
        if pos > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&escape_json_string(field));
        out.push_str("\":");
        out.push_str(&serde_json::to_string(detail).expect("error detail is always serializable"));
    }
    out.push('}');
    out
}

/// Every failure `IssueSerializer.validate()` can produce
/// (`serializers/issue.py:182-288`), in source order. First failure wins;
/// all render status 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidateError {
    /// `start_date > target_date`, both present (`:183-188`).
    StartExceedsTarget,
    /// `assigned_pod` belongs to another project (`:199-203`).
    PodWrongProject,
    /// Pod value changed while `has_active_run` (`:211-216`).
    PodReassignActiveRun,
    /// lxml round-trip raised, or empty-string input (`:222-229`).
    InvalidHtml,
    /// `validate_html_content` rejected the round-tripped HTML (`:232-235`).
    HtmlContentInvalid,
    /// `validate_binary_data` rejected the blob (`:240-243`). Dead via this
    /// serializer (read-only field); ported for direct-call parity.
    BinaryInvalid,
    /// `state` is not a state of this project (`:261-265`).
    StateWrongProject,
    /// `parent` is not an issue of this workspace+project (`:267-276`).
    ParentWrongProject,
    /// `estimate_point` is not one of this workspace+project (`:278-286`).
    EstimatePointWrongProject,
}

impl ValidateError {
    /// HTTP status Django answers with: always 400 on this serializer.
    pub fn status(&self) -> u16 {
        400
    }

    /// Byte-exact response body.
    pub fn body(&self) -> &'static str {
        match self {
            ValidateError::StartExceedsTarget => START_EXCEEDS_TARGET_BODY,
            ValidateError::PodWrongProject => POD_WRONG_PROJECT_BODY,
            ValidateError::PodReassignActiveRun => POD_REASSIGN_ACTIVE_RUN_BODY,
            ValidateError::InvalidHtml => INVALID_HTML_BODY,
            ValidateError::HtmlContentInvalid => HTML_CONTENT_INVALID_BODY,
            ValidateError::BinaryInvalid => BINARY_INVALID_BODY,
            ValidateError::StateWrongProject => STATE_WRONG_PROJECT_BODY,
            ValidateError::ParentWrongProject => PARENT_WRONG_PROJECT_BODY,
            ValidateError::EstimatePointWrongProject => ESTIMATE_POINT_WRONG_PROJECT_BODY,
        }
    }
}

/// Caller-resolved `validate_html_content` verdict
/// (`utils/content_validator.py:211-243`): the nh3 cleaner runs at the
/// handler layer; this ports only the serializer's decision — reject, or
/// substitute the sanitized HTML (`issue.py:232-238`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtmlCheck<'a> {
    /// The `is_valid` element of the validator's 3-tuple.
    pub is_valid: bool,
    /// The `clean_html` element; written back when `Some` (`:237-238`).
    /// Python raises before substituting (`:234-235`), so on invalid
    /// input the substitution never happens.
    pub sanitized: Option<&'a str>,
}

/// Caller-resolved facts for the `description_html` branch
/// (`issue.py:218-238`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptionHtmlFacts<'a> {
    /// lxml `tostring(fromstring(value))`; `None` = raised (including the
    /// empty string, whose `fromstring` raises `ParserError`). Consulted only
    /// when a value is present and NOT `from_markdown`.
    pub roundtripped: Option<&'a str>,
    /// `validate_html_content` verdict on the round-tripped value; consulted
    /// only when the round-tripped value is truthy (non-empty).
    pub check: HtmlCheck<'a>,
}

/// Outcome of [`check_description_html`]: the effective `description_html`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlOutcome<'a> {
    /// Untouched (absent/`None` value, or `from_markdown`).
    Unchanged,
    /// The round-tripped value, further replaced by the sanitizer output
    /// when one was produced (`issue.py:224-226,237-238`).
    Replaced(&'a str),
}

/// Port of the `description_html` branch (`issue.py:218-238`).
pub fn check_description_html<'a>(
    facts: Option<&DescriptionHtmlFacts<'a>>,
    from_markdown: bool,
) -> Result<HtmlOutcome<'a>, ValidateError> {
    let Some(facts) = facts else {
        return Ok(HtmlOutcome::Unchanged);
    };
    if from_markdown {
        return Ok(HtmlOutcome::Unchanged);
    }
    let Some(roundtripped) = facts.roundtripped else {
        return Err(ValidateError::InvalidHtml);
    };
    let mut effective = roundtripped;
    if !effective.is_empty() {
        if !facts.check.is_valid {
            return Err(ValidateError::HtmlContentInvalid);
        }
        if let Some(clean) = facts.check.sanitized {
            effective = clean;
        }
    }
    Ok(HtmlOutcome::Replaced(effective))
}

/// Validated `assigned_pod` value: the pod's project (`runner/models.py`),
/// or `None` when clearing the pod (`issue.py:194`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PodFacts<'a> {
    pub project_id: &'a str,
}

/// Update-time instance facts for the pod branch (`issue.py:200-216`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstancePodFacts<'a> {
    /// `instance.project_id` (used when the context carries no `project_id`).
    pub project_id: &'a str,
    /// Current `instance.assigned_pod_id` (`None` = NULL/default-pod state).
    pub assigned_pod_id: Option<&'a str>,
}

/// `assigned_pod` input for [`validate_assigned_pod`] (`issue.py:193-216`):
/// `None` at the `ValidateInput` level means the key is absent (branch
/// skipped); `Some` carries the validated value plus scoping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignedPodInput<'a> {
    /// Validated pod (`None` = clearing).
    pub pod: Option<PodFacts<'a>>,
    /// `context["project_id"]` (raw URL kwarg, any UUID spelling).
    pub context_project_id: Option<&'a str>,
    /// Update-time instance (`None` on create — the reassign guard only
    /// fires on update, `:211`).
    pub instance: Option<InstancePodFacts<'a>>,
}

/// Port of the pod value-change gate (`issue.py:212-213`):
/// `str(new) != str(old)` with `None` stringifying as `"None"`. Both ids are
/// canonical UUIDs or `None` here, so plain `Option` comparison is exactly
/// the Python comparison. Handlers call this FIRST and resolve
/// `has_active_run` only when it returns true (the query runs only on a
/// change, `:210-211`).
pub fn pod_assignment_changed(new_pod_id: Option<&str>, current_pod_id: Option<&str>) -> bool {
    new_pod_id != current_pod_id
}

/// Port of the `assigned_pod` branch (`issue.py:193-216`).
/// `new_pod_id` is the validated pod's id (`None` when clearing);
/// `has_active_run` is the `AgentRun.objects.filter(work_item=instance,
/// status__in=NON_TERMINAL_STATUSES).exists()` fact — resolved lazily by the
/// caller (see [`pod_assignment_changed`]); the value is unread unless the
/// pod actually changed on an update.
pub fn validate_assigned_pod(
    input: &AssignedPodInput<'_>,
    new_pod_id: Option<&str>,
    has_active_run: bool,
) -> Result<(), ValidateError> {
    if let Some(pod) = input.pod {
        let mut project_id = input.context_project_id;
        if project_id.is_none() {
            if let Some(instance) = input.instance {
                project_id = Some(instance.project_id);
            }
        }
        if let Some(expected) = project_id {
            if !same_uuid(pod.project_id, expected) {
                return Err(ValidateError::PodWrongProject);
            }
        }
    }
    if let Some(instance) = input.instance {
        if pod_assignment_changed(new_pod_id, instance.assigned_pod_id) && has_active_run {
            return Err(ValidateError::PodReassignActiveRun);
        }
    }
    Ok(())
}

/// Minimum `ProjectMember.role` for the `validate()` assignee filter
/// (`serializers/issue.py:250`, `role__gte=15`).
pub const ASSIGNEE_MIN_ROLE: i32 = 15;

/// `validate()` input (`serializers/issue.py:182-288`). `None` on an
/// `Option` field means the key is absent from `validated_data` (partial
/// writes skip absent keys via `data.get`); relation filters arrive
/// post-queryset (see below).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateInput<'a> {
    /// Validated `start_date`/`target_date` (`DateField`s, `YYYY-MM-DD`):
    /// lexicographic order is chronological order.
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    /// `assigned_pod` branch input (`None` = key absent, `:193`).
    pub assigned_pod: Option<AssignedPodInput<'a>>,
    /// Validated pod id for the change gate (`None` when clearing or when
    /// the pod key is absent — unread unless the branch runs).
    pub new_pod_id: Option<&'a str>,
    /// `has_active_run` fact (see [`validate_assigned_pod`]).
    pub has_active_run: bool,
    /// `description_html` branch facts (`None` = absent/`None`, `:223`).
    pub description_html: Option<DescriptionHtmlFacts<'a>>,
    /// Whether the HTML came from markdown (see
    /// [`description_from_markdown`]): skips both HTML branches (`:223,232`).
    pub from_markdown: bool,
    /// Validated `description_binary` bytes (`None` = absent/`None`, `:240`).
    /// Always `None` via this serializer (read-only field); the branch is
    /// ported for direct-call parity.
    pub description_binary: Option<&'a [u8]>,
    /// Post-filter assignees (`None` = key absent, `:246`): the handler runs
    /// `ProjectMember.objects.filter(project_id, is_active=True, role__gte=15,
    /// member_id__in=submitted).values_list("member_id", flat=True)` —
    /// `-created_at` DB order (`Meta.ordering`) — and passes the result
    /// through untouched.
    pub assignees: Option<Vec<&'a str>>,
    /// Post-filter labels (`None` = key absent, `:255`): the handler runs
    /// `Label.objects.filter(project_id, id__in=submitted).values_list("id",
    /// flat=True)` (`-created_at` order) and passes the result through.
    pub labels: Option<Vec<&'a str>>,
    /// `State.objects.filter(project_id, pk=state.id).exists()` (`:261-265`;
    /// `None` = key absent).
    pub state_exists_in_project: Option<bool>,
    /// `Issue.objects.filter(workspace_id, project_id,
    /// pk=parent.id).exists()` (`:267-276`; `None` = key absent).
    pub parent_exists_in_scope: Option<bool>,
    /// `EstimatePoint.objects.filter(workspace_id, project_id,
    /// pk=estimate_point.id).exists()` (`:278-286`; `None` = key absent).
    pub estimate_point_exists_in_scope: Option<bool>,
}

/// `validate()` output: the mutated keys. Every other validated key passes
/// through untouched (DRF `validated_data` passthrough, pinned by the
/// `ok_*` goldens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedIssue<'a> {
    /// Effective `description_html` (see [`HtmlOutcome`]).
    pub description_html: HtmlOutcome<'a>,
    /// Filtered assignees, passed through (`:247-252`).
    pub assignees: Option<Vec<&'a str>>,
    /// Filtered labels, passed through (`:256-258`).
    pub labels: Option<Vec<&'a str>>,
}

/// Port of `IssueSerializer.validate()` (`issue.py:182-288`): first failure
/// wins, in source order.
pub fn run_validate<'a>(input: &'a ValidateInput<'a>) -> Result<ValidatedIssue<'a>, ValidateError> {
    if let (Some(start), Some(target)) = (input.start_date, input.target_date) {
        if start > target {
            return Err(ValidateError::StartExceedsTarget);
        }
    }
    if let Some(pod) = &input.assigned_pod {
        validate_assigned_pod(pod, input.new_pod_id, input.has_active_run)?;
    }
    let description_html =
        check_description_html(input.description_html.as_ref(), input.from_markdown)?;
    if let Some(blob) = input.description_binary {
        if !blob.is_empty() && validate_binary_data(blob).is_err() {
            return Err(ValidateError::BinaryInvalid);
        }
    }
    if input.state_exists_in_project == Some(false) {
        return Err(ValidateError::StateWrongProject);
    }
    if input.parent_exists_in_scope == Some(false) {
        return Err(ValidateError::ParentWrongProject);
    }
    if input.estimate_point_exists_in_scope == Some(false) {
        return Err(ValidateError::EstimatePointWrongProject);
    }
    Ok(ValidatedIssue {
        description_html,
        assignees: input.assignees.clone(),
        labels: input.labels.clone(),
    })
}

/// `bulk_create` batch size for the relation rows (`issue.py:325,366,398,419`).
pub const RELATION_BULK_BATCH_SIZE: usize = 10;

/// `create()` passes no `ignore_conflicts` (`issue.py:313-326,354-367`): a
/// conflicting bulk batch aborts the whole call into `IntegrityError → pass`
/// (earlier batches stay, later batches are skipped). `update()` passes
/// `ignore_conflicts=True` ([`UPDATE_IGNORE_CONFLICTS`]) — a ported
/// asymmetry the handler layer must preserve per operation.
pub const CREATE_IGNORE_CONFLICTS: bool = false;
/// `update()` bulk flag (`issue.py:399,420`).
pub const UPDATE_IGNORE_CONFLICTS: bool = true;

/// One `IssueAssignee` bulk row (`issue.py:313-326,386-401`): `issue_assignees`
/// carries the issue's own `created_by`/`updated_by` audit pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssigneeBulkRow<'a> {
    pub assignee_id: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub created_by_id: Option<&'a str>,
    pub updated_by_id: Option<&'a str>,
}

/// One `IssueLabel` bulk row (`issue.py:354-367,407-422`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelBulkRow<'a> {
    pub label_id: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub created_by_id: Option<&'a str>,
    pub updated_by_id: Option<&'a str>,
}

/// The `create()` write plan (`issue.py:290-371`): `assignees`/`labels`/`type`
/// are popped from `validated_data` (`:291-292,298`); the issue row takes the
/// remaining scalars plus `project_id` from context (`:305`) and the resolved
/// type. `workspace_id` is NOT passed to `Issue.objects.create` (the model
/// resolves it from the project); it only scopes the relation rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePlan<'a> {
    /// `context["project_id"]` (`:294`).
    pub project_id: &'a str,
    /// `context["workspace_id"]` (`:295`), for the relation rows only.
    pub workspace_id: &'a str,
    /// Post-`validate()` assignees (`None` = key absent). `None` AND explicit
    /// `[]` both take the default-assignee branch (`:311`).
    pub assignee_ids: Option<Vec<&'a str>>,
    /// Post-`validate()` labels (`None` = key absent).
    pub label_ids: Option<Vec<&'a str>>,
    /// `context["default_assignee_id"]` (`:296`; may itself be `None`).
    pub default_assignee_id: Option<&'a str>,
    /// `ProjectMember.objects.filter(member_id=default, project_id,
    /// role__gte=15, is_active=True).exists()` (`:334-339`): the fallback
    /// fires only for a valid assignee.
    pub default_assignee_eligible: bool,
    /// Validated `type` (`None` = key absent OR explicit null — both run the
    /// default lookup, `:300`, since model instances are always truthy).
    /// Feeds from BOTH `type_id` and auto `type` inputs (same `source`;
    /// the later auto field wins when both are sent).
    pub explicit_type_id: Option<&'a str>,
    /// `IssueType.objects.filter(project_issue_types__project_id,
    /// is_default=True).first()` (`:302`; `None` when the project has no
    /// default type — the F18-01 seed has none).
    pub default_type_id: Option<&'a str>,
    /// Audit pair copied from the created issue (`:308-309`).
    pub created_by_id: Option<&'a str>,
    pub updated_by_id: Option<&'a str>,
}

/// The resolved `create()` writes: bulk rows, the default-assignee fallback
/// (`None` unless the caller passed no/non-empty assignees AND a valid
/// default exists), and the resolved type id (`None` lands as SQL NULL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWrites<'a> {
    pub assignee_rows: Vec<AssigneeBulkRow<'a>>,
    pub fallback_assignee: Option<AssigneeBulkRow<'a>>,
    pub label_rows: Vec<LabelBulkRow<'a>>,
    pub resolved_type_id: Option<&'a str>,
}

/// Port of `IssueSerializer.create()` (`issue.py:290-371`).
/// (`issue_type = issue_type`, `:303`, is a literal no-op — nothing to port.)
pub fn plan_create<'a>(plan: &CreatePlan<'a>) -> CreateWrites<'a> {
    let has_assignees = plan
        .assignee_ids
        .as_ref()
        .is_some_and(|ids| !ids.is_empty());
    let mut writes = CreateWrites {
        assignee_rows: Vec::new(),
        fallback_assignee: None,
        label_rows: Vec::new(),
        resolved_type_id: plan.explicit_type_id.or(plan.default_type_id),
    };
    if has_assignees {
        writes.assignee_rows = plan
            .assignee_ids
            .as_ref()
            .expect("create plan checked non-empty assignees")
            .iter()
            .map(|id| AssigneeBulkRow {
                assignee_id: id,
                project_id: plan.project_id,
                workspace_id: plan.workspace_id,
                created_by_id: plan.created_by_id,
                updated_by_id: plan.updated_by_id,
            })
            .collect();
    } else if let Some(default) = plan.default_assignee_id {
        if plan.default_assignee_eligible {
            writes.fallback_assignee = Some(AssigneeBulkRow {
                assignee_id: default,
                project_id: plan.project_id,
                workspace_id: plan.workspace_id,
                created_by_id: plan.created_by_id,
                updated_by_id: plan.updated_by_id,
            });
        }
    }
    if let Some(ids) = &plan.label_ids {
        if !ids.is_empty() {
            writes.label_rows = ids
                .iter()
                .map(|id| LabelBulkRow {
                    label_id: id,
                    project_id: plan.project_id,
                    workspace_id: plan.workspace_id,
                    created_by_id: plan.created_by_id,
                    updated_by_id: plan.updated_by_id,
                })
                .collect();
        }
    }
    writes
}

/// The `update()` write plan (`issue.py:373-427`): scoping/audit come from
/// the INSTANCE (`:378-381`), not the context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan<'a> {
    /// Post-`validate()` assignees (`None` = leave the relation alone).
    pub assignee_ids: Option<Vec<&'a str>>,
    /// Post-`validate()` labels (`None` = leave the relation alone).
    pub label_ids: Option<Vec<&'a str>>,
    /// `instance.project_id` / `instance.workspace_id` (`:378-379`).
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    /// `instance.created_by_id` / `instance.updated_by_id` (`:380-381`).
    pub created_by_id: Option<&'a str>,
    pub updated_by_id: Option<&'a str>,
}

/// The resolved `update()` writes: `None` means "leave the relation alone";
/// `Some` (even empty) means delete-all then recreate these rows, plus the
/// mandatory `updated_at` bump (`:426` — bumped even for relation-only
/// changes, and even when `validated_data` is otherwise empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateWrites<'a> {
    pub replace_assignees: Option<Vec<AssigneeBulkRow<'a>>>,
    pub replace_labels: Option<Vec<LabelBulkRow<'a>>>,
    pub bump_updated_at: bool,
}

/// Port of `IssueSerializer.update()` (`issue.py:373-427`). Remaining
/// `validated_data` scalars apply via `super().update()` + `save()` (which
/// recomputes `description_stripped`/`completed_at` — the models layer).
pub fn plan_update<'a>(plan: &UpdatePlan<'a>) -> UpdateWrites<'a> {
    UpdateWrites {
        replace_assignees: plan.assignee_ids.as_ref().map(|ids| {
            ids.iter()
                .map(|id| AssigneeBulkRow {
                    assignee_id: id,
                    project_id: plan.project_id,
                    workspace_id: plan.workspace_id,
                    created_by_id: plan.created_by_id,
                    updated_by_id: plan.updated_by_id,
                })
                .collect()
        }),
        replace_labels: plan.label_ids.as_ref().map(|ids| {
            ids.iter()
                .map(|id| LabelBulkRow {
                    label_id: id,
                    project_id: plan.project_id,
                    workspace_id: plan.workspace_id,
                    created_by_id: plan.created_by_id,
                    updated_by_id: plan.updated_by_id,
                })
                .collect()
        }),
        bump_updated_at: true,
    }
}

/// Port of `web_base_url` (`utils/host.py:70-84`): `WEB_URL` or `APP_BASE_URL`
/// (first TRUTHY — `""` falls through, `:79`), `None` when neither is
/// configured, else trailing slashes stripped (`rstrip("/")` strips ALL of
/// them). Deployment configuration, never the request host.
pub fn web_base_url(web_url: Option<&str>, app_base_url: Option<&str>) -> Option<String> {
    let base = web_url
        .filter(|s| !s.is_empty())
        .or_else(|| app_base_url.filter(|s| !s.is_empty()))?;
    Some(base.trim_end_matches('/').to_string())
}

/// Port of `issue_web_url` (`utils/host.py:87-97`, via `get_url`,
/// `issue.py:429-434`): `{base}/{slug}/browse/{identifier}-{sequence}`.
/// Returns `None` when the base is unconfigured OR any identifier part is
/// missing — empty slug/identifier count as missing (`not ...`), but
/// `sequence_id` uses an `is None` check, so sequence `0` still renders.
pub fn issue_url(
    base: Option<&str>,
    workspace_slug: Option<&str>,
    project_identifier: Option<&str>,
    sequence_id: Option<i64>,
) -> Option<String> {
    let base = base.filter(|s| !s.is_empty())?;
    let slug = workspace_slug.filter(|s| !s.is_empty())?;
    let identifier = project_identifier.filter(|s| !s.is_empty())?;
    let sequence = sequence_id?;
    Some(format!("{base}/{slug}/browse/{identifier}-{sequence}"))
}

/// Every relation type `grouped_relations` reports, in display order
/// (`orchestration/relations.py:RELATION_TYPES`). Every key is always present
/// in the `relations` block, even when empty.
pub const RELATION_TYPES: &[&str] = &[
    "blocked_by",
    "blocking",
    "relates_to",
    "duplicate",
    "start_before",
    "start_after",
    "finish_before",
    "finish_after",
    "implemented_by",
    "implements",
];

/// Per-direction cap on `relations_summary` lists
/// (`orchestration/blockers.py:SUMMARY_LIMIT`): open items sort first so they
/// are never the ones cut, and `has_open_blockers` stays computed over the
/// FULL set. The handler caps; this module positions what it is given.
pub const SUMMARY_LIMIT: usize = 100;

/// Per-type cap on `grouped_relations` lists
/// (`orchestration/relations.py:GROUP_LIMIT`). The handler caps.
pub const GROUP_LIMIT: usize = 100;

/// One `relations_summary` item (`orchestration/blockers.py:_summary_item`):
/// `{identifier, state, state_group}` — no titles or bodies. Serializable in
/// wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SummaryItem<'a> {
    pub identifier: String,
    pub state: Option<&'a str>,
    pub state_group: Option<&'a str>,
}

/// One `grouped_relations` item (`orchestration/relations.py:_item`):
/// `{id, identifier, name, state, state_group}`. Serializable in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelationItem<'a> {
    pub id: &'a str,
    pub identifier: String,
    pub name: &'a str,
    pub state: Option<&'a str>,
    pub state_group: Option<&'a str>,
}

/// Caller-resolved blocker summary (`relations_summary(instance)` at
/// `issue.py:479`): both capped lists plus the uncapped `has_open_blockers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerSummary<'a> {
    pub blocked_by: Vec<SummaryItem<'a>>,
    pub blocking: Vec<SummaryItem<'a>>,
    pub has_open_blockers: bool,
}

/// Caller-resolved grouped relations (`grouped_relations(instance,
/// member_project_issues(viewer, slug))` at `issue.py:492`), one list per
/// [`RELATION_TYPES`] entry in order. `..Default::default()` fills the rest
/// with empty lists; rendering always emits all ten keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupedRelations<'a> {
    pub blocked_by: Vec<RelationItem<'a>>,
    pub blocking: Vec<RelationItem<'a>>,
    pub relates_to: Vec<RelationItem<'a>>,
    pub duplicate: Vec<RelationItem<'a>>,
    pub start_before: Vec<RelationItem<'a>>,
    pub start_after: Vec<RelationItem<'a>>,
    pub finish_before: Vec<RelationItem<'a>>,
    pub finish_after: Vec<RelationItem<'a>>,
    pub implemented_by: Vec<RelationItem<'a>>,
    pub implements: Vec<RelationItem<'a>>,
}

impl<'a> GroupedRelations<'a> {
    /// The ten lists in [`RELATION_TYPES`] order.
    fn ordered(&self) -> [(&str, &Vec<RelationItem<'a>>); 10] {
        [
            ("blocked_by", &self.blocked_by),
            ("blocking", &self.blocking),
            ("relates_to", &self.relates_to),
            ("duplicate", &self.duplicate),
            ("start_before", &self.start_before),
            ("start_after", &self.start_after),
            ("finish_before", &self.finish_before),
            ("finish_after", &self.finish_after),
            ("implemented_by", &self.implemented_by),
            ("implements", &self.implements),
        ]
    }
}

/// A database row for `IssueSerializer.to_representation`
/// (`db/models/issue.py`, `issues` table — column types/nullability per
/// F18-05). UUID and FK primary keys are canonical strings
/// (`PrimaryKeyRelatedField`, read-only); datetimes/dates cross this boundary
/// already rendered as DRF strings (the handler owns timezone conversion and
/// DRF ISO-8601, following the `app_issues` precedent) — rendering here is a
/// byte-exact passthrough. Floats serialize via `serde_json` (shortest
/// round-trip, matching DRF for all finite magnitudes in this domain).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueRow<'a> {
    pub id: &'a str,
    /// Rendered under BOTH `type_id` and `type` (the declared/auto double
    /// field — same source, same value).
    pub type_id: Option<&'a str>,
    /// `get_url()` output (`None` = unconfigured base or missing part).
    pub url: Option<String>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub point: Option<i64>,
    pub name: &'a str,
    pub description_html: &'a str,
    /// Raw `bytea` value (`None` = SQL NULL → `null`). Non-null renders via
    /// DRF `JSONEncoder` bytes rule (`obj.decode()`, UTF-8 strict); undecodable
    /// bytes 500 in Django and fail here as
    /// [`RenderError::BinaryNotUtf8`] for the handler to map.
    pub description_binary: Option<&'a [u8]>,
    pub priority: &'a str,
    pub complexity_score: i64,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i64,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub is_draft: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub git_work_branch: &'a str,
    pub created_via: Option<&'a str>,
    pub agent_executor: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub parent: Option<&'a str>,
    pub state: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub assigned_pod: Option<&'a str>,
}

/// Failure modes of [`render_issue`]: caller-contract violations and the two
/// 500-class render failures. None of these are 400 wire bodies — handlers
/// map them (the binary/NaN arms reproduce Django 500s; the `Missing*` arms
/// are unreachable when the handler fetches what the branch needs).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// Nested `fields=` dict (`TypeError` parity, see [`filter_fields`]).
    #[error("fields filter failed: {0}")]
    Fields(#[from] FilterError),
    /// Non-UTF-8 `description_binary`: Django's `obj.decode()` raises
    /// `UnicodeDecodeError` → 500.
    #[error("description_binary is not valid UTF-8 (Django 500s here)")]
    BinaryNotUtf8,
    /// Non-finite `sort_order`: `serde_json` cannot render NaN/Infinity
    /// (Postgres `float8` admits them; Django emits the literal tokens).
    #[error("sort_order is not finite (Django emits the NaN/Infinity literal)")]
    NonFiniteFloat,
    /// Single-item payload passed the blocker gate but no summary was
    /// supplied (Python always queries here).
    #[error("single-item payload needs the blocker summary")]
    MissingBlockers,
    /// A map-hit `expand` name with no caller value (Python always renders
    /// the related object, or `{}` when the FK is null).
    #[error("expand '{0}' needs its rendered value (None renders {{}})")]
    MissingExpansion(String),
}

/// `to_representation()` input (`issue.py:436-494` + the Base passes).
#[derive(Debug, Clone, PartialEq)]
pub struct RepresentationInput<'a> {
    /// The issue row.
    pub row: &'a IssueRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    /// Plain names only reach here in practice (comma-split query strings).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// `many=True` (under a `ListSerializer`): skips the blocker keys and
    /// `relations`, but Base expansion still applies.
    pub is_list: bool,
    /// `IssueAssignee` ids in queryset (`-created_at`) order.
    pub assignee_ids: &'a [&'a str],
    /// User rows for `expand=assignees`, in `User.objects.filter(pk__in=...)`
    /// order (`-created_at`); rendered via the reused D-19 `UserLite`.
    pub assignee_rows: &'a [UserLiteRow<'a>],
    /// `IssueLabel` ids in queryset (`-created_at`) order.
    pub label_ids: &'a [&'a str],
    /// Caller-rendered expanded labels (the `LabelSerializer` read shape is
    /// PIDASHCONV-661's scope), in `Label.objects.filter(pk__in=...)` order.
    pub expanded_labels: &'a [Value],
    /// Blocker summary (required exactly when the payload is single and the
    /// fields gate passes — i.e. whenever Python would query).
    pub blockers: Option<&'a BlockerSummary<'a>>,
    /// Grouped relations: `Some` = viewer present (the
    /// `relations_viewer` context key is set AND the payload is single AND the
    /// fields gate passes); `None` omits the block.
    pub relations: Option<&'a GroupedRelations<'a>>,
    /// Rendered values for map-hit `expand` names (`state`, `project`,
    /// `workspace`, `created_by`, `updated_by`, `parent`, `estimate_point`
    /// among this serializer's fields): `Some(value)` renders the object,
    /// `None` renders `{}` (null FK — DRF `SkipField` on every field).
    /// Looked up only for names in [`BASE_EXPANSION_NAMES`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Render one `relations_summary` direction value.
fn summary_list(items: &[SummaryItem<'_>]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| serde_json::to_value(item).expect("summary item is always serializable"))
            .collect(),
    )
}

/// Render the `relations` block: all ten [`RELATION_TYPES`] keys always.
fn relations_block(groups: &GroupedRelations<'_>) -> Map<String, Value> {
    let mut out = Map::with_capacity(RELATION_TYPES.len());
    for (name, items) in groups.ordered() {
        let rendered: Vec<Value> = items
            .iter()
            .map(|item| serde_json::to_value(item).expect("relation item is always serializable"))
            .collect();
        out.insert(name.to_string(), Value::Array(rendered));
    }
    out
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::String(text.to_string()),
        None => Value::Null,
    }
}

fn opt_i64(value: Option<i64>) -> Value {
    match value {
        Some(number) => Value::Number(number.into()),
        None => Value::Null,
    }
}

/// Port of `IssueSerializer.to_representation()` (`issue.py:436-494`) over the
/// `BaseSerializer` passes (`api/serializers/base.py:72-117`,
/// `fields=` filtering at `:19-30`).
///
/// Key order is wire order: readable kept fields, then (when kept)
/// `assignees`/`labels` appended, then the gated `relations_summary` /
/// `has_open_blockers` / `relations` keys.
///
/// Base-expansion rules, in `expand` order, for names in the kept fields
/// (write-only names count as kept here):
///
/// * map hit ([`BASE_EXPANSION_NAMES`]) → the caller value, or `{}` for a
///   null FK; a missing caller value is [`RenderError::MissingExpansion`];
/// * `type` → the `type_id` value (no-op overwrite — the only kept name
///   whose `<name>_id` attribute exists);
/// * anything else → `null` — including ADDING `description_markdown: null`
///   and NULLING scalars (`name`, ...) or `url` (which the later url-pop
///   then removes). All verbatim from `base.py:114-116`.
///
/// `assignees`/`labels` overwrite whatever the Base pass wrote (their
/// `else`-branch `null`) with live id lists, or the expanded shapes when
/// expanded (`UserLite` via the reused D-19 port; labels via the
/// caller-rendered values owned by PIDASHCONV-661).
pub fn render_issue(input: &RepresentationInput<'_>) -> Result<Map<String, Value>, RenderError> {
    let kept = filter_fields(FIELDS_IN_ORDER, input.fields)?;
    let requested: Vec<&str> = match input.fields {
        // `_requested_fields` (`issue.py:172-176`): plain string names only,
        // including non-field names like `relations_summary`.
        Some(specs) => specs
            .iter()
            .filter_map(|spec| match spec {
                FieldSpec::Include(name) => Some(name.as_str()),
                FieldSpec::Nested(_, _) => None,
            })
            .collect(),
        None => Vec::new(),
    };
    let kept_contains = |name: &str| kept.iter().any(|kept| kept == name);

    let row = input.row;
    let mut out = Map::with_capacity(kept.len() + 4);
    for name in &kept {
        if WRITE_ONLY_FIELDS.contains(&name.as_str()) {
            continue;
        }
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "type_id" | "type" => opt_str(row.type_id),
            "url" => match &row.url {
                Some(url) => Value::String(url.clone()),
                None => Value::Null,
            },
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "deleted_at" => opt_str(row.deleted_at),
            "point" => opt_i64(row.point),
            "name" => Value::String(row.name.to_string()),
            "description_html" => Value::String(row.description_html.to_string()),
            "description_binary" => match row.description_binary {
                None => Value::Null,
                Some(bytes) => match std::str::from_utf8(bytes) {
                    Ok(text) => Value::String(text.to_string()),
                    Err(_) => return Err(RenderError::BinaryNotUtf8),
                },
            },
            "priority" => Value::String(row.priority.to_string()),
            "complexity_score" => Value::Number(row.complexity_score.into()),
            "start_date" => opt_str(row.start_date),
            "target_date" => opt_str(row.target_date),
            "sequence_id" => Value::Number(row.sequence_id.into()),
            "sort_order" => match serde_json::Number::from_f64(row.sort_order) {
                Some(number) => Value::Number(number),
                None => return Err(RenderError::NonFiniteFloat),
            },
            "completed_at" => opt_str(row.completed_at),
            "archived_at" => opt_str(row.archived_at),
            "is_draft" => Value::Bool(row.is_draft),
            "external_source" => opt_str(row.external_source),
            "external_id" => opt_str(row.external_id),
            "git_work_branch" => Value::String(row.git_work_branch.to_string()),
            "created_via" => opt_str(row.created_via),
            "agent_executor" => opt_str(row.agent_executor),
            "created_by" => opt_str(row.created_by),
            "updated_by" => opt_str(row.updated_by),
            "project" => Value::String(row.project.to_string()),
            "workspace" => Value::String(row.workspace.to_string()),
            "parent" => opt_str(row.parent),
            "state" => opt_str(row.state),
            "estimate_point" => opt_str(row.estimate_point),
            "assigned_pod" => opt_str(row.assigned_pod),
            // `filter_fields` only ever yields `FIELDS_IN_ORDER` names, and
            // every non-write-only one is matched above.
            _ => unreachable!("render_issue matched every kept readable field"),
        };
        out.insert(name.clone(), value);
    }

    // Base expansion (`base.py:76-116`), inside `super().to_representation`.
    for name in input.expand {
        if !kept_contains(name) {
            continue;
        }
        if BASE_EXPANSION_NAMES.contains(name) {
            let found = input.expansions.iter().find(|(key, _)| key == name);
            match found {
                Some((_, Some(value))) => {
                    out.insert(name.to_string(), value.clone());
                }
                Some((_, None)) => {
                    out.insert(name.to_string(), Value::Object(Map::new()));
                }
                None => return Err(RenderError::MissingExpansion(name.to_string())),
            }
        } else if *name == "type" {
            out.insert(name.to_string(), opt_str(row.type_id));
        } else {
            out.insert(name.to_string(), Value::Null);
        }
    }

    // Omit `url` when null (`issue.py:438-441`) — after expansion, so
    // `?expand=url` removes the key while `?expand=description_markdown`'s
    // null survives.
    if out.get("url").is_none_or(Value::is_null) {
        out.remove("url");
    }

    if kept_contains("assignees") {
        if input.expand.contains(&"assignees") {
            let rendered: Vec<Value> = input
                .assignee_rows
                .iter()
                .map(|row| {
                    serde_json::to_value(user_lite_to_representation(row))
                        .expect("UserLite view is always serializable")
                })
                .collect();
            out.insert("assignees".to_string(), Value::Array(rendered));
        } else {
            let ids: Vec<Value> = input
                .assignee_ids
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect();
            out.insert("assignees".to_string(), Value::Array(ids));
        }
    }
    if kept_contains("labels") {
        if input.expand.contains(&"labels") {
            out.insert(
                "labels".to_string(),
                Value::Array(input.expanded_labels.to_vec()),
            );
        } else {
            let ids: Vec<Value> = input
                .label_ids
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect();
            out.insert("labels".to_string(), Value::Array(ids));
        }
    }

    // Blocker summary: single-item payloads only, `?fields=`-gated
    // (`issue.py:470-481`). `requested` empty (no `fields=`) shows both keys.
    if !input.is_list
        && (requested.is_empty()
            || requested
                .iter()
                .any(|name| RELATIONS_SUMMARY_KEYS.contains(name)))
    {
        let Some(blockers) = input.blockers else {
            return Err(RenderError::MissingBlockers);
        };
        for key in RELATIONS_SUMMARY_KEYS {
            if requested.is_empty() || requested.contains(key) {
                let value = match *key {
                    "relations_summary" => {
                        let mut summary = Map::with_capacity(2);
                        summary
                            .insert("blocked_by".to_string(), summary_list(&blockers.blocked_by));
                        summary.insert("blocking".to_string(), summary_list(&blockers.blocking));
                        Value::Object(summary)
                    }
                    _ => Value::Bool(blockers.has_open_blockers),
                };
                out.insert(key.to_string(), value);
            }
        }
    }

    // Viewer relations: single-item payloads with a viewer, `?fields=`-gated
    // (`issue.py:483-492`). `Some` on the input already folds the three
    // Python conditions (viewer set, single, gate); the gate re-check below
    // mirrors `:487` for callers that pass relations unconditionally.
    if let Some(groups) = input.relations {
        if !input.is_list && (requested.is_empty() || requested.contains(&"relations")) {
            out.insert(
                "relations".to_string(),
                Value::Object(relations_block(groups)),
            );
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const F18_01: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/serializers/F18-01.issue_serializer.golden.json"
    );

    fn fixture() -> Value {
        let raw = std::fs::read_to_string(F18_01).expect("F18-01 golden exists");
        serde_json::from_str(&raw).expect("F18-01 golden is valid JSON")
    }

    fn goldens<'a>(fx: &'a Value, unit: &str) -> &'a Value {
        fx.pointer(&format!("/units/{unit}/goldens"))
            .unwrap_or_else(|| panic!("F18-01 lacks units.{unit}.goldens"))
    }

    fn case<'a>(fx: &'a Value, unit: &str, case: &str) -> &'a Value {
        goldens(fx, unit)
            .get(case)
            .unwrap_or_else(|| panic!("F18-01 lacks {unit}.{case}"))
    }

    fn str_list(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("golden carries a string list")
            .iter()
            .map(|item| item.as_str().expect("entries are strings"))
            .collect()
    }

    /// Expected wire body for a golden `errors` object shaped
    /// `{field: [{message, code}]}` (single message per field), rendered
    /// through `serde_json` exactly like the wire (quotes escaped).
    fn expected_field_body(errors: &Value, field: &str) -> String {
        let message = errors[field][0]["message"]
            .as_str()
            .expect("golden error carries a message");
        let mut body = Map::with_capacity(1);
        body.insert(
            field.to_string(),
            Value::Array(vec![Value::String(message.to_string())]),
        );
        serde_json::to_string(&body).expect("error body serializes")
    }

    /// Quiet `validate()` input: every key absent, no markdown.
    fn quiet_validate() -> ValidateInput<'static> {
        ValidateInput {
            start_date: None,
            target_date: None,
            assigned_pod: None,
            new_pod_id: None,
            has_active_run: false,
            description_html: None,
            from_markdown: false,
            description_binary: None,
            assignees: None,
            labels: None,
            state_exists_in_project: None,
            parent_exists_in_scope: None,
            estimate_point_exists_in_scope: None,
        }
    }

    const POD_A: &str = "11111111-1111-1111-1111-111111111111";
    const POD_B: &str = "22222222-2222-2222-2222-222222222222";
    const PROJECT: &str = "d715be3d-234f-46ef-89a3-97f0c7c04b7e";

    fn pod_input(
        pod_project: Option<&'static str>,
        instance: Option<InstancePodFacts<'static>>,
    ) -> AssignedPodInput<'static> {
        AssignedPodInput {
            pod: pod_project.map(|project_id| PodFacts { project_id }),
            context_project_id: Some(PROJECT),
            instance,
        }
    }

    #[test]
    fn same_uuid_matches_all_fixture_goldens() {
        let fx = fixture();
        let goldens = goldens(&fx, "same_uuid");
        let uuid = "79c81d76-5a93-4d3d-894d-5935576834b6";
        assert_eq!(
            same_uuid(uuid, "79C81D76-5A93-4D3D-894D-5935576834B6"),
            goldens["canonical_vs_upper"].as_bool().unwrap()
        );
        assert_eq!(
            same_uuid(uuid, "79c81d765a934d3d894d5935576834b6"),
            goldens["canonical_vs_unhyphenated"].as_bool().unwrap()
        );
        assert_eq!(
            same_uuid(uuid, "00000000-0000-0000-0000-000000000000"),
            goldens["canonical_vs_other"].as_bool().unwrap()
        );
        assert_eq!(
            same_uuid("not-a-uuid", "not-a-uuid"),
            goldens["non_uuid_equal"].as_bool().unwrap()
        );
        assert_eq!(
            same_uuid("not-a-uuid", "also-not-a-uuid"),
            goldens["non_uuid_unequal"].as_bool().unwrap()
        );
        assert_eq!(
            same_uuid(uuid, "not-a-uuid"),
            goldens["uuid_vs_non_uuid"].as_bool().unwrap()
        );
        // `None` stringifies as `"None"` before comparison (`str(None)`).
        assert_eq!(
            same_uuid("None", "None"),
            goldens["none_vs_none"].as_bool().unwrap()
        );
    }

    #[test]
    fn field_names_match_fixture_in_order() {
        let fx = fixture();
        let expected = str_list(&fx["units"]["serializer_field_names"]["fields_in_order"]);
        assert_eq!(FIELDS_IN_ORDER.len(), 36);
        assert_eq!(FIELDS_IN_ORDER, expected.as_slice());
        // Both spellings of the type FK are present (declared + auto).
        assert!(FIELDS_IN_ORDER.contains(&"type_id"));
        assert!(FIELDS_IN_ORDER.contains(&"type"));
    }

    #[test]
    fn readable_fields_are_field_order_minus_write_only() {
        let derived: Vec<&str> = FIELDS_IN_ORDER
            .iter()
            .filter(|name| !WRITE_ONLY_FIELDS.contains(name))
            .copied()
            .collect();
        assert_eq!(READABLE_FIELDS_IN_ORDER.len(), 33);
        assert_eq!(READABLE_FIELDS_IN_ORDER, derived.as_slice());
        assert_eq!(
            WRITE_ONLY_FIELDS,
            ["assignees", "labels", "description_markdown"]
        );
        // The binary branch is dead via this serializer (read-only field).
        assert!(READ_ONLY_FIELDS.contains(&"description_binary"));
        assert!(READ_ONLY_FIELDS.contains(&"id"));
        assert!(READ_ONLY_FIELDS.contains(&"workspace"));
        assert!(READ_ONLY_FIELDS.contains(&"project"));
        assert!(READ_ONLY_FIELDS.contains(&"updated_by"));
        assert!(READ_ONLY_FIELDS.contains(&"updated_at"));
    }

    #[test]
    fn normalize_neither_key_returns_same_object() {
        let fx = fixture();
        let golden = case(&fx, "normalize_description_input", "neither_key");
        let convert = |_: &str| -> Result<String, String> { panic!("must not convert") };
        let outcome = normalize_description_input(
            MarkdownInput::Absent,
            LegacyInput::Absent,
            false,
            &convert,
        )
        .expect("neither key never fails");
        assert_eq!(outcome.action, NormalizeAction::Unchanged);
        assert_eq!(
            outcome.from_markdown,
            golden["from_markdown"].as_bool().unwrap()
        );
        assert!(golden["same_object"].as_bool().unwrap());
        assert!(!outcome.from_markdown);
    }

    #[test]
    fn normalize_markdown_and_legacy_convert() {
        let fx = fixture();
        // `markdown`: input order is unpinned (out_keys may list either
        // order — representation order is field order regardless), so the
        // replay pins the SET plus the converted bytes and the flag.
        let golden = case(&fx, "normalize_description_input", "markdown");
        let html = golden["description_html"].as_str().unwrap();
        let convert = |_: &str| -> Result<String, String> { Ok(html.to_string()) };
        let outcome = normalize_description_input(
            MarkdownInput::Text("# Hi\n- a\n- [ ] t"),
            LegacyInput::Absent,
            false,
            &convert,
        )
        .expect("markdown converts");
        assert_eq!(
            outcome,
            NormalizeOutcome {
                action: NormalizeAction::SetHtml(html.to_string()),
                from_markdown: true,
            }
        );
        assert_eq!(
            outcome.from_markdown,
            golden["from_markdown"].as_bool().unwrap()
        );
        let mut out_keys = str_list(&golden["out_keys"]);
        out_keys.sort_unstable();
        assert_eq!(out_keys, ["description_html", "name"]);

        // `legacy_description`: converted like markdown when no html key.
        let golden = case(&fx, "normalize_description_input", "legacy_description");
        let html = golden["description_html"].as_str().unwrap();
        let convert = |_: &str| -> Result<String, String> { Ok(html.to_string()) };
        let outcome = normalize_description_input(
            MarkdownInput::Absent,
            LegacyInput::Text("legacy *body*"),
            false,
            &convert,
        )
        .expect("legacy converts");
        assert_eq!(
            outcome.from_markdown,
            golden["from_markdown"].as_bool().unwrap()
        );
        assert!(matches!(outcome.action, NormalizeAction::SetHtml(ref set) if set == html));
    }

    #[test]
    fn normalize_html_present_and_absent_branches() {
        let fx = fixture();
        let convert = |_: &str| -> Result<String, String> { panic!("must not convert") };
        // `legacy_plus_html`: legacy popped, stored html kept, no flag.
        let golden = case(&fx, "normalize_description_input", "legacy_plus_html");
        let outcome = normalize_description_input(
            MarkdownInput::Absent,
            LegacyInput::Text("dropped"),
            true,
            &convert,
        )
        .expect("legacy with html drops keys");
        assert_eq!(outcome.action, NormalizeAction::DropKeys);
        assert_eq!(
            outcome.from_markdown,
            golden["from_markdown"].as_bool().unwrap()
        );
        assert_eq!(
            golden["out"]["description_html"].as_str().unwrap(),
            "<p>kept</p>"
        );
        // `html_only`: no markdown key at all → untouched.
        let golden = case(&fx, "normalize_description_input", "html_only");
        let outcome =
            normalize_description_input(MarkdownInput::Absent, LegacyInput::Absent, true, &convert)
                .expect("html alone is untouched");
        assert_eq!(outcome.action, NormalizeAction::Unchanged);
        assert_eq!(
            outcome.from_markdown,
            golden["from_markdown"].as_bool().unwrap()
        );
        // Explicit-null markdown is silently dropped (popped to None).
        let outcome =
            normalize_description_input(MarkdownInput::Null, LegacyInput::Absent, false, &convert)
                .expect("null markdown drops");
        assert_eq!(outcome.action, NormalizeAction::DropKeys);
        assert!(!outcome.from_markdown);
        // Non-string legacy is silently dropped (never an error).
        let outcome = normalize_description_input(
            MarkdownInput::Absent,
            LegacyInput::NonString,
            false,
            &convert,
        )
        .expect("non-string legacy drops");
        assert_eq!(outcome.action, NormalizeAction::DropKeys);
    }

    #[test]
    fn normalize_errors_match_fixture_bodies() {
        let fx = fixture();
        let convert = |_: &str| -> Result<String, String> { panic!("must not convert") };
        // `non_string_markdown`.
        let golden = case(&fx, "normalize_description_input", "non_string_markdown");
        assert!(golden["raised"].as_bool().unwrap());
        let err = normalize_description_input(
            MarkdownInput::NonString,
            LegacyInput::Absent,
            false,
            &convert,
        )
        .expect_err("non-string markdown 400s");
        assert_eq!(err, NormalizeError::NonStringMarkdown);
        assert_eq!(
            err.body(),
            expected_field_body(&golden["errors"], "description_markdown")
        );
        assert_eq!(err.body(), MARKDOWN_NON_STRING_BODY);
        // `oversize_markdown`: the converter's ValueError message verbatim.
        let golden = case(&fx, "normalize_description_input", "oversize_markdown");
        let message = golden["errors"]["description_markdown"][0]["message"]
            .as_str()
            .unwrap();
        assert_eq!(message, "HTML content exceeds maximum size limit (10MB)");
        let failing = |_: &str| -> Result<String, String> { Err(message.to_string()) };
        let err = normalize_description_input(
            MarkdownInput::Text("big"),
            LegacyInput::Absent,
            false,
            &failing,
        )
        .expect_err("converter failure 400s");
        assert_eq!(err, NormalizeError::ConvertFailed(message.to_string()));
        assert_eq!(
            err.body(),
            expected_field_body(&golden["errors"], "description_markdown")
        );
    }

    #[test]
    fn to_internal_value_flag_ors_context() {
        let fx = fixture();
        let golden = goldens(&fx, "to_internal_value");
        // The golden converted markdown (`flag: true`): description_html set,
        // markdown key gone.
        assert!(golden["validated_has_description_html"].as_bool().unwrap());
        assert!(!golden["validated_has_description_markdown"]
            .as_bool()
            .unwrap());
        assert_eq!(
            golden["description_html"].as_str().unwrap(),
            "<p><strong>bold</strong></p>"
        );
        assert!(golden["flag"].as_bool().unwrap());
        assert!(description_from_markdown(true, false));
        assert!(description_from_markdown(false, true));
        assert!(description_from_markdown(true, true));
        assert!(!description_from_markdown(false, false));
    }

    #[test]
    fn validate_ok_cases_pass_through() {
        let fx = fixture();
        // `ok_minimal`: absent keys skip every branch (DRF `SkipField` —
        // missing keys stay out of `validated_data`, never `None`), which
        // is this kernel's `None` — identical downstream (`pop(..., None)`,
        // `data.get(...)`, `is not None`).
        let golden = case(&fx, "validate", "ok_minimal");
        assert!(golden["valid"].as_bool().unwrap());
        let validated_keys = str_list(&golden["validated_keys"]);
        assert_eq!(validated_keys, ["assignees", "labels", "name", "priority"]);
        let minimal = quiet_validate();
        let out = run_validate(&minimal).expect("minimal is valid");
        assert_eq!(out.description_html, HtmlOutcome::Unchanged);
        assert_eq!(out.assignees, None);
        assert_eq!(out.labels, None);
        // `ok_full`: dates in order, html round-trips through the caller
        // facts (lxml + nh3 values stable here), filters pass values through.
        let golden = case(&fx, "validate", "ok_full");
        assert!(golden["valid"].as_bool().unwrap());
        let validated = &golden["validated"];
        let facts = DescriptionHtmlFacts {
            roundtripped: Some(validated["description_html"].as_str().unwrap()),
            check: HtmlCheck {
                is_valid: true,
                sanitized: Some(validated["description_html"].as_str().unwrap()),
            },
        };
        let input = ValidateInput {
            start_date: Some(validated["start_date"].as_str().unwrap()),
            target_date: Some(validated["target_date"].as_str().unwrap()),
            description_html: Some(facts),
            assignees: Some(vec!["79c81d76-5a93-4d3d-894d-5935576834b6"]),
            labels: Some(vec!["c833c492-fa3d-49f5-85a4-0ef810642731"]),
            state_exists_in_project: Some(true),
            ..quiet_validate()
        };
        let out = run_validate(&input).expect("full is valid");
        assert_eq!(
            out.description_html,
            HtmlOutcome::Replaced(validated["description_html"].as_str().unwrap())
        );
        assert_eq!(out.assignees, input.assignees);
        assert_eq!(out.labels, input.labels);
    }

    #[test]
    fn validate_error_bodies_match_fixture() {
        let fx = fixture();
        let check = |case_name: &str, field: &str, err: ValidateError, input: ValidateInput<'_>| {
            let golden = case(&fx, "validate", case_name);
            assert!(!golden["valid"].as_bool().unwrap());
            assert_eq!(run_validate(&input), Err(err));
            assert_eq!(err.status(), 400);
            assert_eq!(err.body(), expected_field_body(&golden["errors"], field));
        };
        // `start_after_target`.
        check(
            "start_after_target",
            "non_field_errors",
            ValidateError::StartExceedsTarget,
            ValidateInput {
                start_date: Some("2026-02-01"),
                target_date: Some("2026-01-01"),
                ..quiet_validate()
            },
        );
        // `state_wrong_project`.
        check(
            "state_wrong_project",
            "non_field_errors",
            ValidateError::StateWrongProject,
            ValidateInput {
                state_exists_in_project: Some(false),
                ..quiet_validate()
            },
        );
        // `parent_other_project` (and the misnamed `parent_wrong_project`
        // golden, which the fixture notes is really a same-project probe).
        check(
            "parent_other_project",
            "non_field_errors",
            ValidateError::ParentWrongProject,
            ValidateInput {
                parent_exists_in_scope: Some(false),
                ..quiet_validate()
            },
        );
        let same_project = ValidateInput {
            parent_exists_in_scope: Some(true),
            ..quiet_validate()
        };
        let ok = run_validate(&same_project);
        assert!(
            ok.is_ok(),
            "same-project parent passes (both parent goldens)"
        );
        // `estimate_point_other_project_exists` (scoped check; the sibling
        // `estimate_point_other_project` golden is the field-level miss).
        check(
            "estimate_point_other_project_exists",
            "non_field_errors",
            ValidateError::EstimatePointWrongProject,
            ValidateInput {
                estimate_point_exists_in_scope: Some(false),
                ..quiet_validate()
            },
        );
        assert!(run_validate(&ValidateInput {
            estimate_point_exists_in_scope: Some(true),
            ..quiet_validate()
        })
        .is_ok());
        // `html_empty_string`: empty input raises in lxml (`roundtripped: None`).
        check(
            "html_empty_string",
            "non_field_errors",
            ValidateError::InvalidHtml,
            ValidateInput {
                description_html: Some(DescriptionHtmlFacts {
                    roundtripped: None,
                    check: HtmlCheck {
                        is_valid: true,
                        sanitized: None,
                    },
                }),
                ..quiet_validate()
            },
        );
        // `html_over_10mb_surviving_roundtrip`.
        check(
            "html_over_10mb_surviving_roundtrip",
            "error",
            ValidateError::HtmlContentInvalid,
            ValidateInput {
                description_html: Some(DescriptionHtmlFacts {
                    roundtripped: Some("<p>x</p>"),
                    check: HtmlCheck {
                        is_valid: false,
                        sanitized: None,
                    },
                }),
                ..quiet_validate()
            },
        );
        // First failure wins in source order: dates beat everything later.
        let input = ValidateInput {
            start_date: Some("2026-02-01"),
            target_date: Some("2026-01-01"),
            state_exists_in_project: Some(false),
            ..quiet_validate()
        };
        assert_eq!(run_validate(&input), Err(ValidateError::StartExceedsTarget));
    }

    #[test]
    fn validate_html_roundtrip_branches_match_fixture() {
        let fx = fixture();
        // Each golden's validated value is the lxml round-trip output (a
        // caller fact); the kernel substitutes it verbatim.
        for case_name in ["invalid_html", "invalid_html_garbage", "html_too_big"] {
            let golden = case(&fx, "validate", case_name);
            assert!(golden["valid"].as_bool().unwrap());
            let html = golden["validated"]["description_html"].as_str().unwrap();
            let input = ValidateInput {
                description_html: Some(DescriptionHtmlFacts {
                    roundtripped: Some(html),
                    check: HtmlCheck {
                        is_valid: true,
                        sanitized: None,
                    },
                }),
                ..quiet_validate()
            };
            let out = run_validate(&input).expect("round-trip branch passes");
            assert_eq!(
                out.description_html,
                HtmlOutcome::Replaced(html),
                "{case_name}"
            );
        }
        assert_eq!(
            case(&fx, "validate", "invalid_html")["validated"]["description_html"]
                .as_str()
                .unwrap(),
            "<p>unclosed</p>"
        );
        assert_eq!(
            case(&fx, "validate", "invalid_html_garbage")["validated"]["description_html"]
                .as_str()
                .unwrap(),
            "<span>&lt;&lt;&gt;&gt;</span>"
        );
        // `html_single_giant_text_node`: the 10MB text node round-trips to
        // `<p></p>` (libxml2 huge-text behaviour, recorded live).
        let golden = case(&fx, "validate", "html_single_giant_text_node");
        assert_eq!(golden["input_len"].as_u64().unwrap(), 10485767);
        let input = ValidateInput {
            description_html: Some(DescriptionHtmlFacts {
                roundtripped: Some(golden["validated"].as_str().unwrap()),
                check: HtmlCheck {
                    is_valid: true,
                    sanitized: None,
                },
            }),
            ..quiet_validate()
        };
        let out = run_validate(&input).expect("giant node collapses");
        assert_eq!(out.description_html, HtmlOutcome::Replaced("<p></p>"));
        // `from_markdown` skips both HTML branches even for failing facts.
        let input = ValidateInput {
            description_html: Some(DescriptionHtmlFacts {
                roundtripped: None,
                check: HtmlCheck {
                    is_valid: false,
                    sanitized: None,
                },
            }),
            from_markdown: true,
            ..quiet_validate()
        };
        let out = run_validate(&input).expect("markdown skips html branches");
        assert_eq!(out.description_html, HtmlOutcome::Unchanged);
    }

    #[test]
    fn validate_binary_branch_is_dead_but_ported() {
        let fx = fixture();
        let golden = case(&fx, "validate", "description_binary_is_read_only");
        // Both probes (base64 + garbage) validate with the key dropped.
        assert!(golden["probes"]["base64_input"]["valid"].as_bool().unwrap());
        assert!(golden["probes"]["garbage_input"]["valid"]
            .as_bool()
            .unwrap());
        // Absent → skipped.
        assert!(run_validate(&quiet_validate()).is_ok());
        // Present-but-invalid still 400s through the reused D-30 validator
        // (direct-call parity for the dead branch).
        let input = ValidateInput {
            description_binary: Some(b"xx"),
            ..quiet_validate()
        };
        assert_eq!(run_validate(&input), Err(ValidateError::BinaryInvalid));
        assert_eq!(ValidateError::BinaryInvalid.body(), BINARY_INVALID_BODY);
        // Present-and-valid passes (4+ bytes, no suspicious patterns).
        let input = ValidateInput {
            description_binary: Some(b"a-valid-pdf-body"),
            ..quiet_validate()
        };
        assert!(run_validate(&input).is_ok());
        // `bad_binary` golden: dropped input validates clean.
        assert!(case(&fx, "validate", "bad_binary")["valid"]
            .as_bool()
            .unwrap());
    }

    #[test]
    fn validate_filter_keys_pass_post_queryset_values_through() {
        let fx = fixture();
        // `assignee_filtering`: the handler's ProjectMember queryset
        // (project + active + role>=15) decided; the kernel echoes.
        let golden = case(&fx, "validate", "assignee_filtering");
        assert!(golden["valid"].as_bool().unwrap());
        assert_eq!(ASSIGNEE_MIN_ROLE, 15);
        let input = ValidateInput {
            assignees: Some(vec!["79c81d76-5a93-4d3d-894d-5935576834b6"]),
            ..quiet_validate()
        };
        let out = run_validate(&input).expect("filtered assignees pass");
        assert_eq!(out.assignees, input.assignees);
        // `label_cross_project_filter`: cross-project labels are silently
        // dropped by the handler's Label queryset (no error).
        let golden = case(&fx, "validate", "label_cross_project_filter");
        assert!(golden["valid"].as_bool().unwrap());
        let input = ValidateInput {
            labels: Some(vec!["c833c492-fa3d-49f5-85a4-0ef810642731"]),
            ..quiet_validate()
        };
        let out = run_validate(&input).expect("filtered labels pass");
        assert_eq!(out.labels, input.labels);
    }

    /// Extract the raw pk echoed inside an `Invalid pk "..." - object does
    /// not exist.` message.
    fn pk_from_does_not_exist(message: &str) -> &str {
        message
            .strip_prefix("Invalid pk \"")
            .and_then(|rest| rest.strip_suffix("\" - object does not exist."))
            .expect("message is a does_not_exist echo")
    }

    #[test]
    fn field_level_pk_miss_bodies_match_fixture() {
        let fx = fixture();
        // `estimate_point_other_project`: field-level miss (unscoped
        // queryset), distinct from the scoped non-field error.
        let golden = case(&fx, "validate", "estimate_point_other_project");
        assert!(!golden["valid"].as_bool().unwrap());
        let message = golden["errors"]["estimate_point"][0]["message"]
            .as_str()
            .unwrap();
        assert_eq!(
            golden["errors"]["estimate_point"][0]["code"]
                .as_str()
                .unwrap(),
            "does_not_exist"
        );
        let body = pk_does_not_exist_body("estimate_point", pk_from_does_not_exist(message));
        assert_eq!(
            body,
            expected_field_body(&golden["errors"], "estimate_point")
        );
        // `pod_nonexistent` (validate_assigned_pod unit).
        let golden = case(&fx, "validate_assigned_pod", "pod_nonexistent");
        let message = golden["errors"]["assigned_pod"][0]["message"]
            .as_str()
            .unwrap();
        let body = pk_does_not_exist_body("assigned_pod", pk_from_does_not_exist(message));
        assert_eq!(body, expected_field_body(&golden["errors"], "assigned_pod"));
        assert!(message.contains("00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn labels_index_dict_body_matches_fixture() {
        let fx = fixture();
        let golden = case(&fx, "validate", "label_filtering");
        assert!(!golden["valid"].as_bool().unwrap());
        // The probe recorded the raw `{index: [ErrorDetail]}` dict repr with
        // a null code (dicts carry no code); the wire renders the index dict.
        let recorded = golden["errors"]["labels"][0]["message"].as_str().unwrap();
        assert!(recorded.starts_with("{1: [ErrorDetail("), "{recorded}");
        assert!(recorded.contains("does_not_exist"));
        assert!(golden["errors"]["labels"][0]["code"].is_null());
        let pk = "69709953-4e54-43bc-a056-fe31a4817e85";
        assert!(recorded.contains(pk));
        let body = list_index_errors_body(
            "labels",
            &[(
                1,
                vec![format!("Invalid pk \"{pk}\" - object does not exist.")],
            )],
        );
        assert_eq!(
            body,
            format!(
                "{{\"labels\":{{\"1\":[\"Invalid pk \\\"{pk}\\\" - object does not exist.\"]}}}}"
            )
        );
        // Indices render ascending (enumeration order).
        let body = list_index_errors_body(
            "labels",
            &[
                (3, vec!["c".to_string()]),
                (1, vec!["a".to_string(), "b".to_string()]),
            ],
        );
        assert_eq!(body, "{\"labels\":{\"1\":[\"a\",\"b\"],\"3\":[\"c\"]}}");
    }

    #[test]
    fn combined_field_errors_follow_field_order() {
        // `not_a_list` shape (DRF `fields.py`, same ListField machinery).
        assert_eq!(
            not_a_list_body("assignees", "dict"),
            "{\"assignees\":[\"Expected a list of items but got type \\\"dict\\\".\"]}"
        );
        // Combined bodies order by field position (assignees < labels).
        let body = field_errors_body(&[
            (
                "assignees",
                serde_json::json!(["Expected a list of items but got type \"str\"."]),
            ),
            (
                "labels",
                serde_json::json!({"1": ["Invalid pk \"x\" - object does not exist."]}),
            ),
        ]);
        assert_eq!(
            body,
            "{\"assignees\":[\"Expected a list of items but got type \\\"str\\\".\"],\"labels\":{\"1\":[\"Invalid pk \\\"x\\\" - object does not exist.\"]}}"
        );
        let pos = |name: &str| FIELDS_IN_ORDER.iter().position(|f| *f == name).unwrap();
        assert!(pos("assignees") < pos("labels"));
        assert!(pos("labels") < pos("estimate_point"));
        assert!(pos("estimate_point") < pos("assigned_pod"));
    }

    #[test]
    fn pod_branch_matches_fixture_and_stays_lazy() {
        let fx = fixture();
        let goldens = goldens(&fx, "validate_assigned_pod");
        // `pod_ok`: same-project pod passes (create probe — no instance, so
        // the reassign guard cannot fire).
        assert!(goldens["pod_ok"]["valid"].as_bool().unwrap());
        let input = pod_input(Some(PROJECT), None);
        assert_eq!(validate_assigned_pod(&input, Some(POD_A), true), Ok(()));
        // Assigning from NULL on update with no active run passes too.
        let instance = InstancePodFacts {
            project_id: PROJECT,
            assigned_pod_id: None,
        };
        let input = pod_input(Some(PROJECT), Some(instance));
        assert_eq!(validate_assigned_pod(&input, Some(POD_A), false), Ok(()));
        // ...while the same assignment WITH an active run gates (the gate is
        // the value change itself, `:207-209`).
        assert_eq!(
            validate_assigned_pod(&input, Some(POD_A), true),
            Err(ValidateError::PodReassignActiveRun)
        );
        // `pod_wrong_project`: the project check fires before any run query
        // (the `has_active_run` value is unread on this path).
        let golden = &goldens["pod_wrong_project"];
        assert!(!golden["valid"].as_bool().unwrap());
        let other = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let input = pod_input(Some(other), Some(instance));
        assert_eq!(
            validate_assigned_pod(&input, Some(POD_A), true),
            Err(ValidateError::PodWrongProject)
        );
        assert_eq!(
            ValidateError::PodWrongProject.body(),
            expected_field_body(&golden["errors"], "assigned_pod")
        );
        // Non-canonical context spellings still match (same_uuid gate).
        let upper = "D715BE3D-234F-46EF-89A3-97F0C7C04B7E";
        let input = AssignedPodInput {
            pod: Some(PodFacts {
                project_id: PROJECT,
            }),
            context_project_id: Some(upper),
            instance: None,
        };
        assert_eq!(validate_assigned_pod(&input, Some(POD_A), false), Ok(()));
        // `has_active_run_queued`: the fact itself (caller-resolved query).
        assert!(goldens["has_active_run_queued"].as_bool().unwrap());
        // `pod_same_value_active_run`: unchanged value never consults the run
        // flag — even `true` passes (query must not run).
        let golden = &goldens["pod_same_value_active_run"];
        assert!(golden["valid"].as_bool().unwrap());
        assert!(!pod_assignment_changed(Some(POD_A), Some(POD_A)));
        assert!(!pod_assignment_changed(None, None));
        let instance = InstancePodFacts {
            project_id: PROJECT,
            assigned_pod_id: Some(POD_A),
        };
        let input = pod_input(Some(PROJECT), Some(instance));
        assert_eq!(validate_assigned_pod(&input, Some(POD_A), true), Ok(()));
        // `pod_reassign_active_run`: any real change (incl. from NULL) gates.
        let golden = &goldens["pod_reassign_active_run"];
        assert!(!golden["valid"].as_bool().unwrap());
        assert!(pod_assignment_changed(Some(POD_B), Some(POD_A)));
        assert!(pod_assignment_changed(Some(POD_A), None));
        assert!(pod_assignment_changed(None, Some(POD_A)));
        let input = pod_input(Some(PROJECT), Some(instance));
        assert_eq!(
            validate_assigned_pod(&input, Some(POD_B), true),
            Err(ValidateError::PodReassignActiveRun)
        );
        assert_eq!(
            ValidateError::PodReassignActiveRun.body(),
            expected_field_body(&golden["errors"], "assigned_pod")
        );
        // `pod_reassign_after_done`: changed value, no active run → passes.
        let golden = &goldens["pod_reassign_after_done"];
        assert!(golden["valid"].as_bool().unwrap());
        assert_eq!(validate_assigned_pod(&input, Some(POD_B), false), Ok(()));
        // Create path (no instance): the reassign guard never fires.
        let input = pod_input(Some(PROJECT), None);
        assert_eq!(validate_assigned_pod(&input, Some(POD_B), true), Ok(()));
        // Clearing the pod skips the project check but still gates reassign.
        let clearing = AssignedPodInput {
            pod: None,
            context_project_id: Some(PROJECT),
            instance: Some(instance),
        };
        assert_eq!(
            validate_assigned_pod(&clearing, None, true),
            Err(ValidateError::PodReassignActiveRun)
        );
        assert_eq!(validate_assigned_pod(&clearing, None, false), Ok(()));
        // run_validate wires the pod branch in order (dates first).
        let input = ValidateInput {
            start_date: Some("2026-02-01"),
            target_date: Some("2026-01-01"),
            assigned_pod: Some(pod_input(Some(other), Some(instance))),
            new_pod_id: Some(POD_A),
            has_active_run: false,
            ..quiet_validate()
        };
        assert_eq!(run_validate(&input), Err(ValidateError::StartExceedsTarget));
        let input = ValidateInput {
            assigned_pod: Some(pod_input(Some(other), Some(instance))),
            new_pod_id: Some(POD_A),
            has_active_run: false,
            ..quiet_validate()
        };
        assert_eq!(run_validate(&input), Err(ValidateError::PodWrongProject));
    }

    const WORKSPACE: &str = "92989e99-51c6-4725-8069-45784951694f";
    const USER: &str = "79c81d76-5a93-4d3d-894d-5935576834b6";
    const LABEL_A: &str = "c833c492-fa3d-49f5-85a4-0ef810642731";
    const LABEL_B: &str = "b4d98934-9159-4ac9-9e58-8a5b7b152ec3";

    fn create_plan() -> CreatePlan<'static> {
        CreatePlan {
            project_id: PROJECT,
            workspace_id: WORKSPACE,
            assignee_ids: None,
            label_ids: None,
            default_assignee_id: None,
            default_assignee_eligible: false,
            explicit_type_id: None,
            default_type_id: None,
            created_by_id: None,
            updated_by_id: None,
        }
    }

    #[test]
    fn create_plans_match_fixture() {
        let fx = fixture();
        assert_eq!(RELATION_BULK_BATCH_SIZE, 10);
        const {
            assert!(!CREATE_IGNORE_CONFLICTS);
            assert!(UPDATE_IGNORE_CONFLICTS);
        }
        // `create_explicit`: 1 assignee + 2 labels; type resolves None (the
        // seed project has no default IssueType).
        let golden = case(&fx, "create", "create_explicit");
        assert_eq!(golden["before"]["issues"].as_u64().unwrap(), 5);
        assert_eq!(golden["after"]["issues"].as_u64().unwrap(), 6);
        assert_eq!(golden["after"]["assignees"].as_u64().unwrap(), 1);
        assert_eq!(golden["after"]["labels"].as_u64().unwrap(), 2);
        let row = &golden["issue_row"];
        assert_eq!(row["project_id"].as_str().unwrap(), PROJECT);
        assert_eq!(row["workspace_id"].as_str().unwrap(), WORKSPACE);
        assert_eq!(row["type_id"].as_str().unwrap(), "None");
        let plan = CreatePlan {
            assignee_ids: Some(vec![USER]),
            label_ids: Some(vec![LABEL_A, LABEL_B]),
            ..create_plan()
        };
        let writes = plan_create(&plan);
        assert_eq!(writes.assignee_rows.len(), 1);
        assert_eq!(writes.assignee_rows[0].assignee_id, USER);
        assert_eq!(writes.assignee_rows[0].project_id, PROJECT);
        assert_eq!(writes.assignee_rows[0].workspace_id, WORKSPACE);
        assert_eq!(writes.fallback_assignee, None);
        assert_eq!(writes.label_rows.len(), 2);
        assert_eq!(writes.label_rows[0].label_id, LABEL_A);
        assert_eq!(writes.label_rows[1].label_id, LABEL_B);
        assert_eq!(writes.resolved_type_id, None);
        // `create_default_assignee`: no assignees → valid default fires.
        let golden = case(&fx, "create", "create_default_assignee");
        let plan = CreatePlan {
            default_assignee_id: Some(USER),
            default_assignee_eligible: true,
            ..create_plan()
        };
        let writes = plan_create(&plan);
        assert!(writes.assignee_rows.is_empty());
        let fallback = writes.fallback_assignee.expect("fallback fires");
        assert_eq!(fallback.assignee_id, USER);
        assert_eq!(
            str_list(&golden["assignee_rows"]),
            vec![fallback.assignee_id]
        );
        // Explicit `[]` takes the fallback branch too (`:311`).
        let plan = CreatePlan {
            assignee_ids: Some(vec![]),
            default_assignee_id: Some(USER),
            default_assignee_eligible: true,
            ..create_plan()
        };
        assert!(plan_create(&plan).fallback_assignee.is_some());
        // `create_invalid_default_assignee`: invalid default → no rows.
        let golden = case(&fx, "create", "create_invalid_default_assignee");
        assert!(golden["assignee_rows"].as_array().unwrap().is_empty());
        let plan = CreatePlan {
            default_assignee_id: Some(USER),
            default_assignee_eligible: false,
            ..create_plan()
        };
        let writes = plan_create(&plan);
        assert!(writes.assignee_rows.is_empty());
        assert_eq!(writes.fallback_assignee, None);
        // Explicit type wins over the default lookup.
        let plan = CreatePlan {
            explicit_type_id: Some("type-1"),
            default_type_id: Some("type-2"),
            ..create_plan()
        };
        assert_eq!(plan_create(&plan).resolved_type_id, Some("type-1"));
        let plan = CreatePlan {
            default_type_id: Some("type-2"),
            ..create_plan()
        };
        assert_eq!(plan_create(&plan).resolved_type_id, Some("type-2"));
    }

    #[test]
    fn update_plans_match_fixture() {
        let fx = fixture();
        // `update_replace`: `assignees=[]` clears, labels replaced, bump.
        let golden = case(&fx, "update", "update_replace");
        assert!(golden["assignee_rows"].as_array().unwrap().is_empty());
        assert_eq!(str_list(&golden["label_rows"]), vec![LABEL_B]);
        assert!(golden["updated_at_bumped"].as_bool().unwrap());
        let plan = UpdatePlan {
            assignee_ids: Some(vec![]),
            label_ids: Some(vec![LABEL_B]),
            project_id: PROJECT,
            workspace_id: WORKSPACE,
            created_by_id: None,
            updated_by_id: None,
        };
        let writes = plan_update(&plan);
        assert_eq!(writes.replace_assignees, Some(vec![]));
        assert_eq!(writes.replace_labels.as_ref().unwrap().len(), 1);
        assert_eq!(writes.replace_labels.as_ref().unwrap()[0].label_id, LABEL_B);
        assert!(writes.bump_updated_at);
        // `update_scalars_only`: omitted keys leave relations untouched.
        let golden = case(&fx, "update", "update_scalars_only");
        assert_eq!(str_list(&golden["label_rows"]), vec![LABEL_B]);
        let plan = UpdatePlan {
            assignee_ids: None,
            label_ids: None,
            project_id: PROJECT,
            workspace_id: WORKSPACE,
            created_by_id: None,
            updated_by_id: None,
        };
        let writes = plan_update(&plan);
        assert_eq!(writes.replace_assignees, None);
        assert_eq!(writes.replace_labels, None);
        assert!(writes.bump_updated_at);
    }

    #[test]
    fn web_and_issue_urls_match_fixture() {
        let fx = fixture();
        let goldens = goldens(&fx, "get_url");
        // `configured`.
        let url = issue_url(
            web_base_url(Some("http://127.0.0.1:18359"), None).as_deref(),
            Some("ws-conv659-2"),
            Some("CT00003"),
            Some(3),
        );
        assert_eq!(url.as_deref(), goldens["configured"].as_str());
        // `unconfigured`.
        assert!(goldens["unconfigured"].is_null());
        assert_eq!(web_base_url(None, None), None);
        assert_eq!(issue_url(None, Some("ws"), Some("P"), Some(1)), None);
        // Fallback + rstrip rules (`host.py:79-84`).
        assert_eq!(
            web_base_url(Some(""), Some("https://app.example///")).as_deref(),
            Some("https://app.example")
        );
        assert_eq!(
            web_base_url(Some("https://web.example/"), Some("https://app.example")).as_deref(),
            Some("https://web.example")
        );
        // Empty parts are missing; sequence 0 still renders (`is None`).
        assert_eq!(issue_url(Some("b"), Some(""), Some("P"), Some(1)), None);
        assert_eq!(issue_url(Some("b"), Some("w"), Some(""), Some(1)), None);
        assert_eq!(issue_url(Some("b"), Some("w"), Some("P"), None), None);
        assert_eq!(
            issue_url(Some("b"), Some("w"), Some("P"), Some(0)).as_deref(),
            Some("b/w/browse/P-0")
        );
    }

    fn full_row() -> IssueRow<'static> {
        IssueRow {
            id: "a7509d00-345f-47fb-bee3-6bcf7d3339e2",
            type_id: Some("type-1"),
            url: Some("http://127.0.0.1:18359/ws-conv659-2/browse/CT00003-3".to_string()),
            created_at: "2026-10-02T23:17:12.023611Z",
            updated_at: "2026-10-02T23:17:12.023611Z",
            deleted_at: None,
            point: None,
            name: "Renamed again",
            description_html: "<h1>Title</h1><p>body <em>text</em></p>",
            description_binary: None,
            priority: "low",
            complexity_score: 0,
            start_date: None,
            target_date: None,
            sequence_id: 3,
            sort_order: 95535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            git_work_branch: "",
            created_via: None,
            agent_executor: None,
            created_by: None,
            updated_by: None,
            project: PROJECT,
            workspace: WORKSPACE,
            parent: None,
            state: Some("97b22834-b823-4109-a527-b39aa310ceae"),
            estimate_point: None,
            assigned_pod: None,
        }
    }

    fn includes(names: &[&str]) -> Vec<FieldSpec> {
        names
            .iter()
            .map(|name| FieldSpec::Include(name.to_string()))
            .collect()
    }

    fn empty_blockers() -> BlockerSummary<'static> {
        BlockerSummary {
            blocked_by: vec![],
            blocking: vec![],
            has_open_blockers: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_with(
        row: &IssueRow<'static>,
        fields: Option<&[FieldSpec]>,
        expand: &[&str],
        is_list: bool,
        assignee_ids: &[&str],
        label_ids: &[&str],
        expanded_labels: &[Value],
        blockers: Option<&BlockerSummary<'static>>,
        relations: Option<&GroupedRelations<'static>>,
    ) -> Map<String, Value> {
        let no_rows: &[UserLiteRow] = &[];
        let no_expansions: &[(&str, Option<Value>)] = &[];
        render_issue(&RepresentationInput {
            row,
            fields,
            expand,
            is_list,
            assignee_ids,
            assignee_rows: no_rows,
            label_ids,
            expanded_labels,
            blockers,
            relations,
            expansions: no_expansions,
        })
        .expect("render succeeds")
    }

    #[test]
    fn render_single_keys_in_order_match_fixture() {
        let fx = fixture();
        let goldens = goldens(&fx, "to_representation");
        let row = full_row();
        let blockers = empty_blockers();
        let out = render_with(
            &row,
            None,
            &[],
            false,
            &[],
            &[LABEL_B],
            &[],
            Some(&blockers),
            None,
        );
        let keys: Vec<&str> = out.keys().map(String::as_str).collect();
        assert_eq!(keys, str_list(&goldens["single_keys_in_order"]));
        assert_eq!(keys.len(), 37);
        // `scalars` spot values.
        let scalars = &goldens["scalars"];
        assert_eq!(
            out["name"].as_str().unwrap(),
            scalars["name"].as_str().unwrap()
        );
        assert_eq!(
            out["priority"].as_str().unwrap(),
            scalars["priority"].as_str().unwrap()
        );
        assert_eq!(
            out["description_html"].as_str().unwrap(),
            scalars["description_html"].as_str().unwrap()
        );
        assert_eq!(
            out["url"].as_str().unwrap(),
            scalars["url"].as_str().unwrap()
        );
        assert_eq!(out["assignees"], scalars["assignees"]);
        assert_eq!(out["labels"], scalars["labels"]);
        // `type` double-renders `type_id` (same value, two keys).
        assert_eq!(out["type_id"], out["type"]);
        // Floats render shortest-round-trip (`95535.0`, not `95535`).
        let bytes = serde_json::to_string(&Value::Object(out)).unwrap();
        assert!(bytes.contains("\"sort_order\":95535.0"), "{bytes}");
    }

    #[test]
    fn render_blocker_keys_single_vs_list() {
        let fx = fixture();
        let goldens = goldens(&fx, "to_representation");
        let row = full_row();
        let blockers = empty_blockers();
        // `blocker_keys`: empty lists + false flag.
        let out = render_with(&row, None, &[], false, &[], &[], &[], Some(&blockers), None);
        assert_eq!(
            out["relations_summary"],
            goldens["blocker_keys"]["relations_summary"]
        );
        assert_eq!(
            out["has_open_blockers"],
            goldens["blocker_keys"]["has_open_blockers"]
        );
        // `has_relations_without_viewer`: no relations block without a viewer.
        assert!(!goldens["has_relations_without_viewer"].as_bool().unwrap());
        assert!(!out.contains_key("relations"));
        // `list_blocker_keys_present`: lists carry neither key.
        assert!(goldens["list_blocker_keys_present"]
            .as_array()
            .unwrap()
            .is_empty());
        let out = render_with(&row, None, &[], true, &[], &[], &[], None, None);
        assert!(!out.contains_key("relations_summary"));
        assert!(!out.contains_key("has_open_blockers"));
        // `list_vs_single_key_diff`: exactly the two blocker keys differ.
        assert_eq!(
            str_list(&goldens["list_vs_single_key_diff"]),
            ["has_open_blockers", "relations_summary"]
        );
        // Missing blockers on a gated single payload is a caller error.
        let no_rows: &[UserLiteRow] = &[];
        let no_expansions: &[(&str, Option<Value>)] = &[];
        let err = render_issue(&RepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            is_list: false,
            assignee_ids: &[],
            assignee_rows: no_rows,
            label_ids: &[],
            expanded_labels: &[],
            blockers: None,
            relations: None,
            expansions: no_expansions,
        });
        assert_eq!(err, Err(RenderError::MissingBlockers));
    }

    #[test]
    fn render_expand_labels_passes_caller_shape_through() {
        let fx = fixture();
        let goldens = goldens(&fx, "to_representation");
        let row = full_row();
        let blockers = empty_blockers();
        // The expanded label object is PIDASHCONV-661's shape; this module
        // positions it byte-identically under `labels`.
        let label = goldens["expand_assignees_labels"]["labels"][0].clone();
        let out = render_with(
            &row,
            None,
            &["labels"],
            false,
            &[],
            &[LABEL_B],
            std::slice::from_ref(&label),
            Some(&blockers),
            None,
        );
        assert_eq!(out["labels"], goldens["expand_assignees_labels"]["labels"]);
        assert_eq!(
            out["assignees"],
            goldens["expand_assignees_labels"]["assignees"]
        );
        assert_eq!(label["id"].as_str().unwrap(), LABEL_B);
        assert_eq!(label["sort_order"].as_f64().unwrap(), 65535.0);
    }

    #[test]
    fn render_assignees_expand_uses_reused_user_lite() {
        let row = full_row();
        let blockers = empty_blockers();
        let user_rows = [UserLiteRow {
            id: USER,
            first_name: "Ada",
            last_name: "L",
            email: Some("ada@example.test"),
            avatar: "",
            avatar_url: None,
            display_name: "Ada L",
        }];
        let no_expansions: &[(&str, Option<Value>)] = &[];
        let out = render_issue(&RepresentationInput {
            row: &row,
            fields: None,
            expand: &["assignees"],
            is_list: false,
            assignee_ids: &[USER],
            assignee_rows: &user_rows,
            label_ids: &[],
            expanded_labels: &[],
            blockers: Some(&blockers),
            relations: None,
            expansions: no_expansions,
        })
        .expect("assignee expand renders");
        // The D-19 UserLite wire shape, in order.
        let keys: Vec<&str> = out["assignees"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "first_name",
                "last_name",
                "email",
                "avatar",
                "avatar_url",
                "display_name"
            ]
        );
        assert_eq!(out["assignees"][0]["id"].as_str().unwrap(), USER);
    }

    #[test]
    fn render_fields_subsets_gate_computed_keys() {
        let fx = fixture();
        let goldens = goldens(&fx, "to_representation");
        let row = full_row();
        let blockers = empty_blockers();
        // `fields_subset_keys`: blocker keys gated out.
        let fields = includes(&["id", "name"]);
        let out = render_with(&row, Some(&fields), &[], false, &[], &[], &[], None, None);
        let keys: Vec<&str> = out.keys().map(String::as_str).collect();
        assert_eq!(keys, str_list(&goldens["fields_subset_keys"]));
        // `fields_with_blocker_key`: only the requested blocker key appended.
        let fields = includes(&["id", "name", "relations_summary"]);
        let out = render_with(
            &row,
            Some(&fields),
            &[],
            false,
            &[],
            &[],
            &[],
            Some(&blockers),
            None,
        );
        let keys: Vec<&str> = out.keys().map(String::as_str).collect();
        assert_eq!(keys, str_list(&goldens["fields_with_blocker_key"]));
        assert!(!out.contains_key("has_open_blockers"));
        // Unknown-only fields keep nothing (never raises).
        let fields = includes(&["nope"]);
        let out = render_with(&row, Some(&fields), &[], false, &[], &[], &[], None, None);
        assert!(out.is_empty());
        // Nested `fields=` dicts raise (TypeError parity).
        let nested = vec![FieldSpec::Nested("state".to_string(), vec![])];
        let no_rows: &[UserLiteRow] = &[];
        let no_expansions: &[(&str, Option<Value>)] = &[];
        let err = render_issue(&RepresentationInput {
            row: &row,
            fields: Some(&nested),
            expand: &[],
            is_list: false,
            assignee_ids: &[],
            assignee_rows: no_rows,
            label_ids: &[],
            expanded_labels: &[],
            blockers: None,
            relations: None,
            expansions: no_expansions,
        });
        assert!(matches!(err, Err(RenderError::Fields(_))));
    }

    #[test]
    fn render_relations_viewer_block_matches_fixture() {
        let fx = fixture();
        let goldens = goldens(&fx, "to_representation");
        let row = full_row();
        let blockers = empty_blockers();
        // `relations_with_viewer`: all ten keys, always, in RELATION_TYPES order.
        assert_eq!(RELATION_TYPES.len(), 10);
        let groups = GroupedRelations::default();
        let out = render_with(
            &row,
            None,
            &[],
            false,
            &[],
            &[],
            &[],
            Some(&blockers),
            Some(&groups),
        );
        let relations = out["relations"].as_object().unwrap();
        let keys: Vec<&str> = relations.keys().map(String::as_str).collect();
        assert_eq!(keys, RELATION_TYPES);
        assert_eq!(out["relations"], goldens["relations_with_viewer"]);
        // `relations_tail_keys`: append order after the id lists.
        let keys: Vec<&str> = out.keys().map(String::as_str).collect();
        let tail = &keys[keys.len() - 4..];
        assert_eq!(tail, str_list(&goldens["relations_tail_keys"]));
        // `?fields=` gates `relations` too.
        let fields = includes(&["id", "relations"]);
        let out = render_with(
            &row,
            Some(&fields),
            &[],
            false,
            &[],
            &[],
            &[],
            None,
            Some(&groups),
        );
        assert!(out.contains_key("relations"));
        assert!(!out.contains_key("relations_summary"));
        let fields = includes(&["id"]);
        let out = render_with(
            &row,
            Some(&fields),
            &[],
            false,
            &[],
            &[],
            &[],
            None,
            Some(&groups),
        );
        assert!(!out.contains_key("relations"));
        // Lists never carry `relations`, even with a viewer.
        let out = render_with(&row, None, &[], true, &[], &[], &[], None, Some(&groups));
        assert!(!out.contains_key("relations"));
    }

    #[test]
    fn render_url_omitted_when_unconfigured() {
        let fx = fixture();
        assert!(
            goldens(&fx, "to_representation")["url_omitted_when_unconfigured"]
                .as_bool()
                .unwrap()
        );
        let mut row = full_row();
        row.url = None;
        let blockers = empty_blockers();
        let out = render_with(&row, None, &[], false, &[], &[], &[], Some(&blockers), None);
        assert!(!out.contains_key("url"));
    }

    #[test]
    fn render_base_expand_quirks_match_python() {
        let row = full_row();
        let blockers = empty_blockers();
        // `?expand=name` nulls the scalar (else branch, `base.py:116`).
        let out = render_with(
            &row,
            None,
            &["name"],
            false,
            &[],
            &[],
            &[],
            Some(&blockers),
            None,
        );
        assert_eq!(out["name"], Value::Null);
        // `?expand=url` removes the key (nulled, then popped).
        let out = render_with(
            &row,
            None,
            &["url"],
            false,
            &[],
            &[],
            &[],
            Some(&blockers),
            None,
        );
        assert!(!out.contains_key("url"));
        // `?expand=description_markdown` ADDS a null key.
        let out = render_with(
            &row,
            None,
            &["description_markdown"],
            false,
            &[],
            &[],
            &[],
            Some(&blockers),
            None,
        );
        assert_eq!(out["description_markdown"], Value::Null);
        // `?expand=type` is a no-op overwrite (the `<name>_id` exists).
        let out = render_with(
            &row,
            None,
            &["type"],
            false,
            &[],
            &[],
            &[],
            Some(&blockers),
            None,
        );
        assert_eq!(out["type"], out["type_id"]);
        // Map hits render the caller value; null FKs render `{}`.
        let state_value = serde_json::json!({"id": "s", "name": "Todo"});
        let no_rows: &[UserLiteRow] = &[];
        let expansions = [("state", Some(state_value.clone())), ("parent", None)];
        let out = render_issue(&RepresentationInput {
            row: &row,
            fields: None,
            expand: &["state", "parent"],
            is_list: false,
            assignee_ids: &[],
            assignee_rows: no_rows,
            label_ids: &[],
            expanded_labels: &[],
            blockers: Some(&blockers),
            relations: None,
            expansions: &expansions,
        })
        .expect("map hits render");
        assert_eq!(out["state"], state_value);
        assert_eq!(out["parent"], Value::Object(Map::new()));
        // A map hit with no caller value is a caller error (Python always
        // renders here).
        let err = render_issue(&RepresentationInput {
            row: &row,
            fields: None,
            expand: &["state"],
            is_list: false,
            assignee_ids: &[],
            assignee_rows: no_rows,
            label_ids: &[],
            expanded_labels: &[],
            blockers: Some(&blockers),
            relations: None,
            expansions: &[],
        });
        assert_eq!(err, Err(RenderError::MissingExpansion("state".to_string())));
        // Expands outside the kept fields are skipped entirely.
        let fields = includes(&["id"]);
        let out = render_with(
            &row,
            Some(&fields),
            &["state", "name"],
            false,
            &[],
            &[],
            &[],
            None,
            None,
        );
        assert_eq!(out.keys().len(), 1);
    }

    #[test]
    fn render_binary_and_float_edges() {
        let mut row = full_row();
        let blockers = empty_blockers();
        // Null binary → null.
        let out = render_with(&row, None, &[], false, &[], &[], &[], Some(&blockers), None);
        assert_eq!(out["description_binary"], Value::Null);
        // UTF-8 bytes decode like DRF `JSONEncoder` (`obj.decode()`).
        row.description_binary = Some("plain-text".as_bytes());
        let out = render_with(&row, None, &[], false, &[], &[], &[], Some(&blockers), None);
        assert_eq!(out["description_binary"].as_str().unwrap(), "plain-text");
        // Undecodable bytes fail for the handler to map to the Django 500.
        row.description_binary = Some(&[0xff, 0xfe]);
        let no_rows: &[UserLiteRow] = &[];
        let no_expansions: &[(&str, Option<Value>)] = &[];
        let err = render_issue(&RepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            is_list: false,
            assignee_ids: &[],
            assignee_rows: no_rows,
            label_ids: &[],
            expanded_labels: &[],
            blockers: Some(&blockers),
            relations: None,
            expansions: no_expansions,
        });
        assert_eq!(err, Err(RenderError::BinaryNotUtf8));
        // Non-finite floats fail loudly (Postgres admits them; JSON cannot).
        row.description_binary = None;
        row.sort_order = f64::NAN;
        let err = render_issue(&RepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            is_list: false,
            assignee_ids: &[],
            assignee_rows: no_rows,
            label_ids: &[],
            expanded_labels: &[],
            blockers: Some(&blockers),
            relations: None,
            expansions: no_expansions,
        });
        assert_eq!(err, Err(RenderError::NonFiniteFloat));
    }

    #[test]
    fn relation_type_order_and_limits_match_python() {
        assert_eq!(
            RELATION_TYPES,
            [
                "blocked_by",
                "blocking",
                "relates_to",
                "duplicate",
                "start_before",
                "start_after",
                "finish_before",
                "finish_after",
                "implemented_by",
                "implements",
            ]
        );
        assert_eq!(SUMMARY_LIMIT, 100);
        assert_eq!(GROUP_LIMIT, 100);
        assert_eq!(
            RELATIONS_SUMMARY_KEYS,
            ["relations_summary", "has_open_blockers"]
        );
        assert_eq!(RELATIONS_VIEWER_CONTEXT, "relations_viewer");
        assert_eq!(
            DESCRIPTION_FROM_MARKDOWN_CONTEXT,
            "description_from_markdown"
        );
        // Summary/relation items serialize in wire order.
        let summary = SummaryItem {
            identifier: "CT00003-3".to_string(),
            state: Some("Todo"),
            state_group: Some("unstarted"),
        };
        assert_eq!(
            serde_json::to_string(&summary).unwrap(),
            "{\"identifier\":\"CT00003-3\",\"state\":\"Todo\",\"state_group\":\"unstarted\"}"
        );
        let item = RelationItem {
            id: "a7509d00-345f-47fb-bee3-6bcf7d3339e2",
            identifier: "CT00003-3".to_string(),
            name: "Renamed again",
            state: None,
            state_group: None,
        };
        assert_eq!(
            serde_json::to_string(&item).unwrap(),
            "{\"id\":\"a7509d00-345f-47fb-bee3-6bcf7d3339e2\",\"identifier\":\"CT00003-3\",\"name\":\"Renamed again\",\"state\":null,\"state_group\":null}"
        );
    }
}
