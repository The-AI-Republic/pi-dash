#![forbid(unsafe_code)]

//! Relation shapes (D-18 serializers D, PIDASHCONV-663).
//!
//! Ports `apps/api/pi_dash/api/serializers/issue.py:729-906`:
//! `IssueRelationResponseSerializer` (`:729-769`),
//! `IssueRelationCreateSerializer` (`:770-807`, incl. `validate_issues`
//! `:801-807`), `IssueRelationRemoveSerializer` (`:808-820`),
//! `IssueRelationSerializer` (`:821-861`) and `RelatedIssueSerializer`
//! (`:862-906`).
//!
//! Fixture: the F18-02 relation subset (`rust-api/fixtures/v1_work_items/`
//! `serializers/F18-02.label_link_relation.golden.json`). Every `#[test]`
//! below replays it: golden in/out byte-identical, including validation
//! error strings and the two `Meta.fields` orders.
//!
//! This module is pure: no check here needs the database (relation types
//! are closed choices, UUIDs parse locally). The handler layer
//! (PIDASHCONV-676) owns the grouped aggregation, the `bulk_create`, the
//! refetch and the `issue_activity` task; the error bodies, key orders
//! and check order here are the contract it must honor.
//!
//! Reused, not forked: [`filter_fields`]/[`FieldSpec`]/[`FilterError`]
//! (the shared `?fields=` kernel) and [`field_errors_body`] +
//! [`BASE_EXPANSION_NAMES`] from the sibling `shape_issue` module, so
//! one error renders byte-identically alone or combined (the `serde_json`
//! escaper matches `json.dumps` `ensure_ascii=False` exactly, including
//! the `\b`/`\f` short escapes). The UUID child rule mirrors
//! `v1_cycles_modules::module_shapes::parse_issue_id` and the
//! `invalid_choice` input echo mirrors `runner_runs::shape::py_str` —
//! both private to their modules, and per-module copies are the codebase
//! precedent.
//!
//! Write-path reachability (all verified against `api/views/issue.py` +
//! `app/serializers/base.py`):
//!
//! * `IssueRelationCreateSerializer` — relation POST (`views/issue.py:3068`):
//!   [`validate_relation_create`]. Invalid → `Response(serializer.errors,
//!   400)`; valid → `bulk_create(batch_size=10, ignore_conflicts=True)`,
//!   refetch, then the Show/Related render with 201.
//! * `IssueRelationRemoveSerializer` — no reachable path: no view
//!   constructs it and `api/serializers/__init__.py` does not export it.
//!   [`validate_relation_remove`] is ported for direct-call parity and
//!   fixture replay.
//! * `IssueRelationResponseSerializer` — docs-only: the GET list
//!   (`views/issue.py:2964-3015`) returns a hand-built dict and never
//!   constructs this serializer (a plain `Serializer`, so no
//!   `fields=`/`expand=` either). [`render_relation_response`] ports its
//!   read shape.
//! * The two Shows render the POST refetch (`many=True`,
//!   `views/issue.py:3132-3137` — `RelatedIssueSerializer` when
//!   `is_reverse`, else `IssueRelationSerializer`) with no
//!   `fields=`/`expand=` kwargs, and the app-domain nested expansion
//!   likewise passes none — so [`render_issue_relation`] and
//!   [`render_related_issue`] take the `fields=`/`expand=` arguments for
//!   full `BaseSerializer` parity, but `expand` is unreached over HTTP.
//! * `duplicate`/`relates_to` grouping uses `list(set(...))`
//!   (`views/issue.py:3008-3009`) — set order is per-process random; the
//!   shape renders given lists in given order and the nondeterminism
//!   stays in the handler/queries layer.
//!
//! Ported quirks (translate, don't redesign — all verified against the
//! pinned DRF 3.15.2 sources or live probes):
//!
//! * `validate_issues`'s `"At least one issue ID is required."` is
//!   unreachable: `min_length=1` registers a post-child validator
//!   (`fields.py:ListField.__init__`) that fires first, so the field only
//!   passes non-empty and `if not value` never holds. Ported as
//!   [`VALIDATE_ISSUES_UNREACHABLE_MESSAGE`] (never returned), noted per
//!   the fixture.
//! * `IssueRelationSerializer.Meta.fields` lists `updated_at` BEFORE
//!   `updated_by` (`issue.py:851-860`); `RelatedIssueSerializer` lists
//!   `updated_by` before `updated_at` (`issue.py:893-905`). Both orders
//!   ported verbatim.
//! * `type_id`/`is_epic`/`state_id` vanish — not null — when
//!   `issue.type`/`issue.state` is `None` (DRF `SkipField` on the
//!   `AttributeError`, `fields.py:Field.get_attribute`; read-only fields
//!   are `required=False`). The probe row had `type=None`, hence the
//!   fixture's 11-key `RelatedIssueSerializer` render.
//! * `RelatedIssueSerializer.project_id` is a `PrimaryKeyRelatedField`
//!   over the `issue.project_id` attname: it renders through the pk-only
//!   optimization (`serializable_value` + `PKOnlyObject`, `relations.py`)
//!   — a plain UUID string on the wire (`project` is `NOT NULL`).
//! * `IssueRelationRemoveSerializer` is dead in Python (see above) yet
//!   ported whole per this issue's unit list.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde_json::{Map, Value};

use super::shape_issue::{field_errors_body, BASE_EXPANSION_NAMES};
use super::{filter_fields, FieldSpec, FilterError};

/// `IssueRelationResponseSerializer` declared fields
/// (`serializers/issue.py:738-768`), the GET grouping key order.
pub const RESPONSE_FIELDS: &[&str] = &[
    "blocking",
    "blocked_by",
    "duplicate",
    "relates_to",
    "start_after",
    "start_before",
    "finish_after",
    "finish_before",
];

/// `IssueRelationCreateSerializer.RELATION_TYPE_CHOICES` keys
/// (`issue.py:776-785`), in source order.
pub const RELATION_TYPE_CHOICES: &[&str] = &[
    "blocking",
    "blocked_by",
    "duplicate",
    "relates_to",
    "start_before",
    "start_after",
    "finish_before",
    "finish_after",
];

/// `IssueRelationCreateSerializer` fields in declared order (`:788-799`):
/// combined field errors follow it (`both_bad` probe).
pub const RELATION_CREATE_FIELDS: &[&str] = &["relation_type", "issues"];

/// `IssueRelationSerializer.Meta.fields` (`issue.py:851-860`) — note
/// `updated_at` BEFORE `updated_by`.
pub const RELATION_SHOW_FIELDS: &[&str] = &[
    "id",
    "project_id",
    "sequence_id",
    "relation_type",
    "name",
    "state_id",
    "priority",
    "created_by",
    "created_at",
    "updated_at",
    "updated_by",
];

/// `RelatedIssueSerializer.Meta.fields` (`issue.py:893-905`) — note
/// `updated_by` before `updated_at`, the reverse of the Show order.
pub const RELATED_SHOW_FIELDS: &[&str] = &[
    "id",
    "project_id",
    "sequence_id",
    "relation_type",
    "name",
    "type_id",
    "is_epic",
    "state_id",
    "priority",
    "created_by",
    "created_at",
    "updated_by",
    "updated_at",
];

/// Missing input where `required=True` (DRF `required`).
pub const MSG_REQUIRED: &str = "This field is required.";
/// Explicit JSON null where `allow_null=False` (DRF `null`).
pub const MSG_NULL: &str = "This field may not be null.";
/// `issues = []` (`ListField` `min_length=1`, DRF `min_length` — the
/// ungrammatical singular is DRF's own text, pinned by F18-02).
pub const MSG_MIN_ISSUES: &str = "Ensure this field has at least 1 elements.";
/// UUID child/item rejection (DRF `UUIDField` `invalid`).
pub const MSG_INVALID_UUID: &str = "Must be a valid UUID.";
/// Null request body (DRF serializer-level `null`).
pub const MSG_NO_DATA: &str = "No data provided";

/// `validate_issues`'s custom message (`issue.py:801-807`). Unreachable in
/// Python — `min_length=1` fires first (see the module docs) — ported for
/// direct-call parity and never returned.
pub const VALIDATE_ISSUES_UNREACHABLE_MESSAGE: &str = "At least one issue ID is required.";

/// The GET grouping (`views/issue.py:3006-3015`): eight pre-ordered id
/// lists. UUIDs arrive already stringified; datetimes never appear here.
/// Plain `Serializer`, so no `fields=`/`expand=` (DRF would `TypeError`
/// on those kwargs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationResponseGroups<'a> {
    pub blocking: &'a [&'a str],
    pub blocked_by: &'a [&'a str],
    pub duplicate: &'a [&'a str],
    pub relates_to: &'a [&'a str],
    pub start_after: &'a [&'a str],
    pub start_before: &'a [&'a str],
    pub finish_after: &'a [&'a str],
    pub finish_before: &'a [&'a str],
}

/// Port of `IssueRelationResponseSerializer.to_representation`
/// (`issue.py:729-769`): the eight [`RESPONSE_FIELDS`] keys in declared
/// order, each a list of UUID strings (`ListField.to_representation`
/// maps `UUIDField` `hex_verbose`, i.e. `str(uuid)`).
pub fn render_relation_response(groups: &RelationResponseGroups<'_>) -> Map<String, Value> {
    let mut out = Map::with_capacity(RESPONSE_FIELDS.len());
    let lists: [(&str, &[&str]); 8] = [
        ("blocking", groups.blocking),
        ("blocked_by", groups.blocked_by),
        ("duplicate", groups.duplicate),
        ("relates_to", groups.relates_to),
        ("start_after", groups.start_after),
        ("start_before", groups.start_before),
        ("finish_after", groups.finish_after),
        ("finish_before", groups.finish_before),
    ];
    for (key, ids) in lists {
        out.insert(
            key.to_string(),
            Value::Array(ids.iter().map(|id| Value::String(id.to_string())).collect()),
        );
    }
    out
}

/// DRF type name for the `not_a_list` / `invalid`-dict messages,
/// mirroring Python's `type(data).__name__` for JSON-decoded values
/// (mirrors `v1_cycles_modules::module_shapes::json_type_name`,
/// `runner_runs::shape::json_datatype`).
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "str",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
        Value::Null => "NoneType",
    }
}

/// Python `str()` of a JSON-decoded value, for the `invalid_choice`
/// `input` slot: strings render bare, every other type renders its
/// `repr` (DRF formats the original object, `'"{}".format(input)`,
/// `fields.py:ChoiceField.to_internal_value`). Mirrors
/// `runner_runs::shape::py_str`.
fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => py_repr(value),
    }
}

/// Python `repr()` of a JSON-decoded value: `None` / `True` / `False`,
/// numbers in `str()` form, strings single-quoted (double-quoted when
/// they contain `'` but not `"`), containers with `", "` separators.
/// Mirrors `runner_runs::shape::py_repr`.
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else {
                py_float_repr(number.as_f64().expect("f64"))
            }
        }
        Value::String(text) => py_str_repr(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_str_repr(key), py_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a string: short escapes for whitespace, `\xXX` /
/// `\uXXXX` / `\UXXXXXXXX` for control characters, everything else
/// literal. Mirrors `runner_runs::shape::py_str_repr`, including its
/// known divergence: non-ASCII non-printables outside the `Cc` class
/// (format `Cf` / separator `Zl`/`Zp` characters such as the zero-width
/// space) pass through literally here while CPython escapes them. Only
/// reachable with such characters inside a `relation_type` value, which
/// no client sends; documented rather than tabled.
fn py_str_repr(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        if c == quote {
            out.push('\\');
            out.push(quote);
        } else {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c < ' ' || c == '\u{7f}' => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c if c.is_control() => {
                    let code = c as u32;
                    if code < 0x100 {
                        out.push_str(&format!("\\x{:02x}", code));
                    } else if code < 0x10000 {
                        out.push_str(&format!("\\u{:04x}", code));
                    } else {
                        out.push_str(&format!("\\U{:08x}", code));
                    }
                }
                c => out.push(c),
            }
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a float: shortest round-trip digits (taken from
/// serde's ryu rendering, which implements the same shortest spec as
/// CPython's `float_repr_style short`) laid out by Python's rules —
/// fixed notation for decimal exponents `-3..=16` with a mandatory `.0`
/// on integral values, else `d[.ddd]e±XX` with a signed, ≥2-digit
/// exponent. Mirrors `runner_runs::shape::py_float_repr`.
fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let negative = value.is_sign_negative();
    let abs = value.abs();
    if abs == 0.0 {
        return if negative { "-0.0" } else { "0.0" }.to_string();
    }
    // Shortest digits: `1.5`, `100.0`, `1e20`, `1.5e-05` shapes.
    let ryu = serde_json::Number::from_f64(abs)
        .expect("finite")
        .to_string();
    let (mantissa, exp): (&str, i32) = match ryu.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse().expect("ryu exponent")),
        None => (ryu.as_str(), 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len());
    let mut digits: Vec<char> = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    while digits.len() > 1 && digits[0] == '0' {
        digits.remove(0);
    }
    let after_point = mantissa.len() - point - usize::from(point < mantissa.len());
    // Value = digits × 10^(exp - after_point) = 0.digits × 10^dec_exp.
    let dec_exp = exp - after_point as i32 + digits.len() as i32;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-3..=16).contains(&dec_exp) {
        // Fixed notation.
        if dec_exp <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-dec_exp) as usize));
            out.extend(digits);
        } else if dec_exp as usize >= digits.len() {
            let pad = dec_exp as usize - digits.len();
            out.extend(digits);
            out.push_str(&"0".repeat(pad));
            out.push_str(".0");
        } else {
            let split = dec_exp as usize;
            out.extend(digits[..split].iter());
            out.push('.');
            out.extend(digits[split..].iter());
        }
    } else {
        // Exponential notation.
        out.push(digits[0]);
        if digits.len() > 1 {
            out.push('.');
            out.extend(digits[1..].iter());
        }
        let exp10 = dec_exp - 1;
        out.push('e');
        out.push(if exp10 < 0 { '-' } else { '+' });
        let mag = exp10.unsigned_abs().to_string();
        if mag.len() < 2 {
            out.push('0');
        }
        out.push_str(&mag);
    }
    out
}

/// Parses one UUID item the way DRF's `UUIDField` does (verified live):
/// UUID strings in any form Python's `uuid.UUID` accepts (hyphenated,
/// 32-hex, braced, `urn:uuid:`) normalise to lowercase hyphenated; JSON
/// integers coerce via `UUID(int=…)` (Python `bool` is an `int`, so
/// `true`/`false` coerce to 1/0); everything else — including floats
/// and nested containers — is invalid, while `null` items fail with the
/// child `null` message. Mirrors
/// `v1_cycles_modules::module_shapes::parse_issue_id`.
///
/// Known edge (shared with that mirror): JSON integers above `u64::MAX`
/// arrive as `f64` under `serde_json`'s default precision, so they fail
/// here while Python's unbounded `int` would accept values below
/// 2^128. Only reachable with absurd ids; the handler parses the body,
/// so no shape-local code can recover the digits.
fn parse_uuid_item(value: &Value) -> Result<String, &'static str> {
    match value {
        Value::String(text) => uuid::Uuid::parse_str(text)
            .map(|id| id.hyphenated().to_string())
            .map_err(|_| MSG_INVALID_UUID),
        Value::Number(number) => {
            if let Some(unsigned) = number.as_u64() {
                Ok(uuid::Uuid::from_u128(unsigned as u128)
                    .hyphenated()
                    .to_string())
            } else if let Some(signed) = number.as_i64() {
                if signed < 0 {
                    return Err(MSG_INVALID_UUID);
                }
                Ok(uuid::Uuid::from_u128(signed as u128)
                    .hyphenated()
                    .to_string())
            } else {
                Err(MSG_INVALID_UUID)
            }
        }
        Value::Bool(flag) => Ok(uuid::Uuid::from_u128(*flag as u128)
            .hyphenated()
            .to_string()),
        Value::Null => Err(MSG_NULL),
        Value::Array(_) | Value::Object(_) => Err(MSG_INVALID_UUID),
    }
}

/// Shape one single-message field failure for [`field_errors_body`].
fn field_entry(field: &'static str, message: String) -> (&'static str, Value) {
    (field, Value::Array(vec![Value::String(message)]))
}

/// Serializer-level `invalid` body for a non-object request body:
/// `{"non_field_errors": ["Invalid data. Expected a dictionary, but got
/// {type}."]}` (DRF `serializers.py`, verified live).
fn non_dict_body(body: &Value) -> String {
    field_errors_body(&[(
        "non_field_errors",
        Value::Array(vec![Value::String(format!(
            "Invalid data. Expected a dictionary, but got {}.",
            json_type_name(body)
        ))]),
    )])
}

/// Validated relation create (`validated_data` shape): the choice key
/// plus the normalised hyphenated issue ids, in input order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRelationCreate {
    pub relation_type: String,
    pub issues: Vec<String>,
}

/// Every failure [`validate_relation_create`] can produce. Both arms
/// carry the byte-exact 400 body (the view answers 400, no DB fact is
/// consulted, so there is no caller-contract arm).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationCreateError {
    /// Null body (`No data provided`) or non-object body.
    #[error("{0}")]
    NotADict(String),
    /// Combined field errors in [`RELATION_CREATE_FIELDS`] order.
    #[error("{0}")]
    Fields(String),
}

impl RelationCreateError {
    /// The byte-exact 400 response body.
    pub fn body(&self) -> &str {
        match self {
            RelationCreateError::NotADict(body) | RelationCreateError::Fields(body) => body,
        }
    }
}

/// Validates `relation_type` (`ChoiceField`, `issue.py:788-792`):
/// absent → `required`, null → `null`, an exact choice key passes
/// through, anything else → `invalid_choice` with the input rendered by
/// Python `str()` (`'"{}".format(input)`, verified live for `5`, `True`,
/// `['blocking']`, `1.5`). DRF looks `str(data)` up in the choice map;
/// no non-string JSON value stringifies to one of the eight alpha keys,
/// so exact-match-then-fail is exactly equivalent.
fn validate_relation_type(raw: Option<&Value>) -> Result<String, Value> {
    let Some(value) = raw else {
        return Err(Value::Array(vec![Value::String(MSG_REQUIRED.to_string())]));
    };
    if value.is_null() {
        return Err(Value::Array(vec![Value::String(MSG_NULL.to_string())]));
    }
    if let Some(choice) = value.as_str() {
        if RELATION_TYPE_CHOICES.contains(&choice) {
            return Ok(choice.to_string());
        }
    }
    Err(Value::Array(vec![Value::String(format!(
        "\"{}\" is not a valid choice.",
        py_str(value)
    ))]))
}

/// Validates `issues` (`ListField(child=UUIDField(), min_length=1)`,
/// `issue.py:793-799`): absent → `required`, null → `null`, non-list →
/// `not_a_list` with the Python type name, empty → `min_length`
/// (a post-child validator in DRF, observably the empty-list error),
/// else per-item UUID rules with failures collected into the
/// index-keyed object (`{"0": [...]}`, ascending, gaps kept — verified
/// live). On success the normalised hyphenated ids, in input order.
fn validate_issues_value(raw: Option<&Value>) -> Result<Vec<String>, Value> {
    let Some(value) = raw else {
        return Err(Value::Array(vec![Value::String(MSG_REQUIRED.to_string())]));
    };
    if value.is_null() {
        return Err(Value::Array(vec![Value::String(MSG_NULL.to_string())]));
    }
    let Value::Array(items) = value else {
        return Err(Value::Array(vec![Value::String(format!(
            "Expected a list of items but got type \"{}\".",
            json_type_name(value)
        ))]));
    };
    if items.is_empty() {
        return Err(Value::Array(vec![Value::String(
            MSG_MIN_ISSUES.to_string(),
        )]));
    }
    let mut ids = Vec::with_capacity(items.len());
    let mut failures = Map::new();
    for (index, item) in items.iter().enumerate() {
        match parse_uuid_item(item) {
            Ok(id) => ids.push(id),
            Err(message) => {
                failures.insert(
                    index.to_string(),
                    Value::Array(vec![Value::String(message.to_string())]),
                );
            }
        }
    }
    if failures.is_empty() {
        Ok(ids)
    } else {
        Err(Value::Object(failures))
    }
}

/// Port of `IssueRelationCreateSerializer` field validation
/// (`serializers/issue.py:770-807` over plain-DRF `Serializer`).
///
/// The body must be an object (null → `No data provided`, anything else
/// non-object → `invalid` with the JSON type name). Both fields run even
/// after failures and errors combine in [`RELATION_CREATE_FIELDS`]
/// order; unknown input keys are silently ignored. `validate()` is not
/// overridden, and `validate_issues` (`:801-807`) provably never raises
/// (see [`VALIDATE_ISSUES_UNREACHABLE_MESSAGE`]), so field success is
/// overall success. The POST view (`views/issue.py:3068`) constructs
/// without `partial`, so absent keys always fail.
pub fn validate_relation_create(
    body: &Value,
) -> Result<ValidatedRelationCreate, RelationCreateError> {
    if body.is_null() {
        return Err(RelationCreateError::NotADict(field_errors_body(&[(
            "non_field_errors",
            Value::Array(vec![Value::String(MSG_NO_DATA.to_string())]),
        )])));
    }
    let Some(obj) = body.as_object() else {
        return Err(RelationCreateError::NotADict(non_dict_body(body)));
    };
    let mut errors: Vec<(&'static str, Value)> = Vec::new();

    let relation_type = match validate_relation_type(obj.get("relation_type")) {
        Ok(choice) => Some(choice),
        Err(detail) => {
            errors.push(("relation_type", detail));
            None
        }
    };
    let issues = match validate_issues_value(obj.get("issues")) {
        Ok(ids) => Some(ids),
        Err(detail) => {
            errors.push(("issues", detail));
            None
        }
    };

    if !errors.is_empty() {
        return Err(RelationCreateError::Fields(field_errors_body(&errors)));
    }
    Ok(ValidatedRelationCreate {
        relation_type: relation_type.expect("no errors means both fields parsed"),
        issues: issues.expect("no errors means both fields parsed"),
    })
}

/// Validated relation remove (`validated_data` shape): the normalised
/// hyphenated id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRelationRemove {
    pub related_issue: String,
}

/// Every failure [`validate_relation_remove`] can produce. Both arms
/// carry the byte-exact 400 body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationRemoveError {
    /// Null body (`No data provided`) or non-object body.
    #[error("{0}")]
    NotADict(String),
    /// The `related_issue` failure.
    #[error("{0}")]
    Fields(String),
}

impl RelationRemoveError {
    /// The byte-exact 400 response body.
    pub fn body(&self) -> &str {
        match self {
            RelationRemoveError::NotADict(body) | RelationRemoveError::Fields(body) => body,
        }
    }
}

/// Port of `IssueRelationRemoveSerializer` field validation
/// (`serializers/issue.py:808-820` over plain-DRF `Serializer`):
/// `related_issue` is a required `UUIDField` with the same item rules
/// as the create `issues` children (int/bool coerce, null → `null`,
/// everything else unparseable → `invalid`). Single-field serializer,
/// so `Fields` carries exactly one entry; unknown input keys are
/// silently ignored. No view constructs this serializer — ported for
/// direct-call parity and fixture replay.
pub fn validate_relation_remove(
    body: &Value,
) -> Result<ValidatedRelationRemove, RelationRemoveError> {
    if body.is_null() {
        return Err(RelationRemoveError::NotADict(field_errors_body(&[(
            "non_field_errors",
            Value::Array(vec![Value::String(MSG_NO_DATA.to_string())]),
        )])));
    }
    let Some(obj) = body.as_object() else {
        return Err(RelationRemoveError::NotADict(non_dict_body(body)));
    };
    let raw = obj.get("related_issue");
    if raw.is_none() {
        return Err(RelationRemoveError::Fields(field_errors_body(&[
            field_entry("related_issue", MSG_REQUIRED.to_string()),
        ])));
    }
    match parse_uuid_item(raw.expect("checked Some")) {
        Ok(related_issue) => Ok(ValidatedRelationRemove { related_issue }),
        Err(message) => Err(RelationRemoveError::Fields(field_errors_body(&[
            field_entry("related_issue", message.to_string()),
        ]))),
    }
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::String(text.to_string()),
        None => Value::Null,
    }
}

/// Failure modes of [`render_issue_relation`] and
/// [`render_related_issue`]: the caller-contract arm is 500-class for
/// the handler to map (mirroring `shape_issue::RenderError` and the
/// sibling `LabelRenderError`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationShowError {
    /// Nested `fields=` dict (`TypeError` parity, see [`filter_fields`]).
    #[error("fields filter failed: {0}")]
    Fields(#[from] FilterError),
    /// A map-hit `expand` name with no caller value (Python always renders
    /// the related object, or `{}` when the FK is null).
    #[error("expand '{0}' needs its rendered value (None renders {{}})")]
    MissingExpansion(String),
}

/// One `IssueRelation` row for `IssueRelationSerializer.to_representation`
/// (`issue.py:821-861`): the relation's own columns plus the
/// `related_issue` traversal facts. Datetimes cross this boundary
/// already DRF-formatted (mirroring `shape_issue`, where `created_at` is
/// a `&str` passthrough — the queries layer formats).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRelationRow<'a> {
    /// `related_issue.id` (hyphenated UUID string).
    pub id: &'a str,
    /// `related_issue.project_id` (`NOT NULL` — always present).
    pub project_id: &'a str,
    pub sequence_id: i64,
    /// The relation row's own `relation_type`.
    pub relation_type: &'a str,
    /// `related_issue.name`.
    pub name: &'a str,
    /// `related_issue.state.id`: `None` (state unset) OMITS the key
    /// (DRF `SkipField`, verified live) — it never renders null.
    pub state_id: Option<&'a str>,
    /// `related_issue.priority`.
    pub priority: &'a str,
    pub created_by: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub updated_by: Option<&'a str>,
}

/// `IssueRelationSerializer.to_representation()` input (`issue.py:821-861`
/// over the `BaseSerializer` passes, `api/serializers/base.py:72-117`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueRelationShowInput<'a> {
    /// The relation row.
    pub row: &'a IssueRelationRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// Rendered values for map-hit `expand` names among this serializer's
    /// fields (`created_by`, `updated_by`): `Some(value)` renders the
    /// object, `None` renders `{}` (null FK). Looked up only for names in
    /// [`BASE_EXPANSION_NAMES`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `IssueRelationSerializer.to_representation()`
/// (`issue.py:821-861`) over the `BaseSerializer` passes
/// (`base.py:19-30,72-117`).
///
/// Keys render in [`RELATION_SHOW_FIELDS`] order (`Meta.fields` order —
/// note `updated_at` before `updated_by`), gated by `fields=`; a `None`
/// `state_id` drops the key (`SkipField` parity, verified live) while
/// direct-column `None`s (`created_by`, `updated_by`) render null.
///
/// Base-expansion rules, in `expand` order, for names in the kept fields
/// (mirroring the sibling `render_label`):
///
/// * map hit ([`BASE_EXPANSION_NAMES`]) → the caller value, or `{}` for a
///   null FK; a missing caller value is
///   [`RelationShowError::MissingExpansion`];
/// * anything else → `null` (verbatim from `base.py:114-116` — no
///   non-map field of this serializer has a `<name>_id` attribute on the
///   `IssueRelation` instance, so the passthrough always yields `None`).
pub fn render_issue_relation(
    input: &IssueRelationShowInput<'_>,
) -> Result<Map<String, Value>, RelationShowError> {
    let kept = filter_fields(RELATION_SHOW_FIELDS, input.fields)?;
    let kept_contains = |name: &str| kept.iter().any(|kept| kept == name);

    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "project_id" => Value::String(row.project_id.to_string()),
            "sequence_id" => Value::Number(row.sequence_id.into()),
            "relation_type" => Value::String(row.relation_type.to_string()),
            "name" => Value::String(row.name.to_string()),
            // `related_issue.state.id` traversal: a `None` state raises
            // `AttributeError` inside DRF and the read-only field
            // `SkipField`s — the key vanishes (verified live).
            "state_id" => match row.state_id {
                Some(state_id) => Value::String(state_id.to_string()),
                None => continue,
            },
            "priority" => Value::String(row.priority.to_string()),
            "created_by" => opt_str(row.created_by),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "updated_by" => opt_str(row.updated_by),
            // `filter_fields` only ever yields `RELATION_SHOW_FIELDS`
            // names, and every one is matched above.
            _ => unreachable!("render_issue_relation matched every kept readable field"),
        };
        out.insert(name.clone(), value);
    }

    apply_base_expansion(&mut out, &kept_contains, input.expand, input.expansions)?;

    Ok(out)
}

/// The `issue.type` traversal fact for [`RelatedIssueRow`]: present
/// together or not at all (a `None` type drops BOTH `type_id` and
/// `is_epic` via `SkipField`, verified live — one `Option` enforces the
/// coupling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedIssueType<'a> {
    /// `issue.type.id` (hyphenated UUID string).
    pub id: &'a str,
    /// `issue.type.is_epic`.
    pub is_epic: bool,
}

/// One `IssueRelation` row for `RelatedIssueSerializer.to_representation`
/// (`issue.py:862-906`): the relation's own columns plus the `issue`
/// traversal facts. Datetimes cross this boundary already DRF-formatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedIssueRow<'a> {
    /// `issue.id` (hyphenated UUID string).
    pub id: &'a str,
    /// `issue.project_id` (`NOT NULL` — always present; renders via the
    /// pk-only optimization as a plain UUID string on the wire).
    pub project_id: &'a str,
    pub sequence_id: i64,
    /// The relation row's own `relation_type`.
    pub relation_type: &'a str,
    /// `issue.name`.
    pub name: &'a str,
    /// `issue.type` traversal: `None` OMITS both `type_id` and `is_epic`
    /// (`SkipField`, verified live — the fixture row has `type=None`).
    pub issue_type: Option<RelatedIssueType<'a>>,
    /// `issue.state.id`: `None` OMITS the key (`SkipField`, verified
    /// live) — it never renders null.
    pub state_id: Option<&'a str>,
    /// `issue.priority`.
    pub priority: &'a str,
    pub created_by: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_by: Option<&'a str>,
    pub updated_at: &'a str,
}

/// `RelatedIssueSerializer.to_representation()` input (`issue.py:862-906`
/// over the `BaseSerializer` passes, `api/serializers/base.py:72-117`).
#[derive(Debug, Clone, PartialEq)]
pub struct RelatedIssueShowInput<'a> {
    /// The relation row.
    pub row: &'a RelatedIssueRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// Rendered values for map-hit `expand` names among this serializer's
    /// fields (`created_by`, `updated_by`): `Some(value)` renders the
    /// object, `None` renders `{}` (null FK). Looked up only for names in
    /// [`BASE_EXPANSION_NAMES`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `RelatedIssueSerializer.to_representation()`
/// (`issue.py:862-906`) over the `BaseSerializer` passes
/// (`base.py:19-30,72-117`).
///
/// Keys render in [`RELATED_SHOW_FIELDS`] order (`Meta.fields` order —
/// note `updated_by` before `updated_at`, the reverse of the Show
/// order), gated by `fields=`; a `None` `issue_type` drops `type_id` AND
/// `is_epic`, and a `None` `state_id` drops `state_id` (`SkipField`
/// parity, verified live), while direct-column `None`s render null.
///
/// Base-expansion rules are the Show's: map hit → caller value or `{}`
/// for a null FK ([`RelationShowError::MissingExpansion`] when the
/// caller value is absent), anything else → `null` (no non-map field of
/// this serializer has a `<name>_id` attribute on the `IssueRelation`
/// instance).
pub fn render_related_issue(
    input: &RelatedIssueShowInput<'_>,
) -> Result<Map<String, Value>, RelationShowError> {
    let kept = filter_fields(RELATED_SHOW_FIELDS, input.fields)?;
    let kept_contains = |name: &str| kept.iter().any(|kept| kept == name);

    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "project_id" => Value::String(row.project_id.to_string()),
            "sequence_id" => Value::Number(row.sequence_id.into()),
            "relation_type" => Value::String(row.relation_type.to_string()),
            "name" => Value::String(row.name.to_string()),
            // `issue.type.id` traversal: a `None` type `SkipField`s both
            // this key and `is_epic` (verified live).
            "type_id" => match row.issue_type.as_ref() {
                Some(issue_type) => Value::String(issue_type.id.to_string()),
                None => continue,
            },
            "is_epic" => match row.issue_type.as_ref() {
                Some(issue_type) => Value::Bool(issue_type.is_epic),
                None => continue,
            },
            // `issue.state.id` traversal: a `None` state `SkipField`s
            // the key (verified live).
            "state_id" => match row.state_id {
                Some(state_id) => Value::String(state_id.to_string()),
                None => continue,
            },
            "priority" => Value::String(row.priority.to_string()),
            "created_by" => opt_str(row.created_by),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_by" => opt_str(row.updated_by),
            "updated_at" => Value::String(row.updated_at.to_string()),
            // `filter_fields` only ever yields `RELATED_SHOW_FIELDS`
            // names, and every one is matched above.
            _ => unreachable!("render_related_issue matched every kept readable field"),
        };
        out.insert(name.clone(), value);
    }

    apply_base_expansion(&mut out, &kept_contains, input.expand, input.expansions)?;

    Ok(out)
}

/// Base expansion (`base.py:76-116`), shared by both Show renders: in
/// `expand` order, names outside the kept fields are skipped; map hits
/// render the caller value (`{}` for a null FK,
/// [`RelationShowError::MissingExpansion`] when the caller passed none);
/// anything else renders `null`.
fn apply_base_expansion(
    out: &mut Map<String, Value>,
    kept_contains: &dyn Fn(&str) -> bool,
    expand: &[&str],
    expansions: &[(&str, Option<Value>)],
) -> Result<(), RelationShowError> {
    for name in expand {
        if !kept_contains(name) {
            continue;
        }
        if BASE_EXPANSION_NAMES.contains(name) {
            let found = expansions.iter().find(|(key, _)| key == name);
            match found {
                Some((_, Some(value))) => {
                    out.insert(name.to_string(), value.clone());
                }
                Some((_, None)) => {
                    out.insert(name.to_string(), Value::Object(Map::new()));
                }
                None => return Err(RelationShowError::MissingExpansion(name.to_string())),
            }
        } else {
            out.insert(name.to_string(), Value::Null);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const F18_02: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/serializers/F18-02.label_link_relation.golden.json"
    );

    const UID: &str = "25ace52e-c64d-4043-a700-911b1e42bffc";

    fn fixture(path: &str) -> Value {
        let raw = std::fs::read_to_string(path).expect("golden fixture exists");
        serde_json::from_str(&raw).expect("golden fixture is valid JSON")
    }

    fn unit<'a>(fx: &'a Value, name: &str) -> &'a Value {
        fx.pointer(&format!("/units/{name}"))
            .unwrap_or_else(|| panic!("golden lacks units.{name}"))
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
    /// through `serde_json` exactly like the wire.
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

    fn out_keys(out: &Map<String, Value>) -> Vec<&str> {
        out.keys().map(String::as_str).collect()
    }

    /// The fixture records read-shape `None`s as the string `"None"`.
    fn opt<'a>(render: &'a Value, key: &str) -> Option<&'a str> {
        match render[key].as_str().expect("render values are strings") {
            "None" => None,
            text => Some(text),
        }
    }

    fn includes(names: &[&str]) -> Vec<FieldSpec> {
        names
            .iter()
            .map(|name| FieldSpec::Include(name.to_string()))
            .collect()
    }

    // ---- F18-02 response replays ------------------------------------------

    #[test]
    fn response_keys_and_render_match_f18_02() {
        let fx = fixture(F18_02);
        let response = unit(&fx, "IssueRelationResponseSerializer");
        let render = &response["render"];
        let empty: [&str; 0] = [];
        let groups = RelationResponseGroups {
            blocking: &empty,
            blocked_by: &empty,
            duplicate: &empty,
            relates_to: &empty,
            start_after: &empty,
            start_before: &empty,
            finish_after: &empty,
            finish_before: &empty,
        };
        let out = render_relation_response(&groups);
        assert_eq!(RESPONSE_FIELDS.len(), 8);
        assert_eq!(out_keys(&out), RESPONSE_FIELDS);
        assert_eq!(Value::Object(out), render.clone());
    }

    #[test]
    fn response_non_empty_ids_render_in_order() {
        let one = [UID];
        let empty: [&str; 0] = [];
        let groups = RelationResponseGroups {
            blocking: &one,
            blocked_by: &empty,
            duplicate: &empty,
            relates_to: &empty,
            start_after: &empty,
            start_before: &empty,
            finish_after: &empty,
            finish_before: &empty,
        };
        let out = render_relation_response(&groups);
        assert_eq!(
            serde_json::to_string(&out).expect("serializes"),
            r#"{"blocking":["25ace52e-c64d-4043-a700-911b1e42bffc"],"blocked_by":[],"duplicate":[],"relates_to":[],"start_after":[],"start_before":[],"finish_after":[],"finish_before":[]}"#
        );
    }

    // ---- F18-02 create replays --------------------------------------------

    #[test]
    fn create_ok_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueRelationCreateSerializer");
        let body = serde_json::json!({"relation_type": "blocking", "issues": [UID]});
        let validated = validate_relation_create(&body).expect("ok is valid");
        let golden = &create["ok"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            format!("'{}'", validated.relation_type),
            golden["validated"]["relation_type"]
                .as_str()
                .expect("golden carries relation_type")
        );
        let repr = format!(
            "[{}]",
            validated
                .issues
                .iter()
                .map(|id| format!("UUID('{id}')"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        assert_eq!(
            repr,
            golden["validated"]["issues"]
                .as_str()
                .expect("golden carries issues")
        );
    }

    #[test]
    fn create_bad_type_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueRelationCreateSerializer");
        let body = serde_json::json!({"relation_type": "nope", "issues": [UID]});
        let err = validate_relation_create(&body).expect_err("bad type fails");
        let golden = &create["bad_type"];
        assert!(!golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            golden["errors"]["relation_type"][0]["code"].as_str(),
            Some("invalid_choice")
        );
        assert_eq!(
            err.body(),
            expected_field_body(&golden["errors"], "relation_type")
        );
        assert_eq!(
            err.body(),
            r#"{"relation_type":["\"nope\" is not a valid choice."]}"#
        );
    }

    #[test]
    fn create_empty_issues_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueRelationCreateSerializer");
        let body = serde_json::json!({"relation_type": "blocking", "issues": []});
        let err = validate_relation_create(&body).expect_err("empty issues fail");
        let golden = &create["empty_issues"];
        assert_eq!(
            golden["errors"]["issues"][0]["code"].as_str(),
            Some("min_length")
        );
        assert_eq!(err.body(), expected_field_body(&golden["errors"], "issues"));
        assert_eq!(
            err.body(),
            r#"{"issues":["Ensure this field has at least 1 elements."]}"#
        );
    }

    #[test]
    fn create_missing_issues_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueRelationCreateSerializer");
        let body = serde_json::json!({"relation_type": "blocking"});
        let err = validate_relation_create(&body).expect_err("missing issues fail");
        let golden = &create["missing_issues"];
        assert_eq!(err.body(), expected_field_body(&golden["errors"], "issues"));
        assert_eq!(err.body(), r#"{"issues":["This field is required."]}"#);
    }

    #[test]
    fn create_bad_uuid_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueRelationCreateSerializer");
        let body = serde_json::json!({"relation_type": "blocking", "issues": ["nope"]});
        let err = validate_relation_create(&body).expect_err("bad uuid fails");
        // The golden records `str(serializer.errors)` — the dict repr —
        // while the wire carries the JSON body; the per-index message is
        // identical in both (verified live).
        let golden = &create["bad_uuid"];
        let recorded = golden["errors"]["issues"][0]["message"]
            .as_str()
            .expect("golden carries the message");
        assert!(
            recorded.contains("Must be a valid UUID."),
            "golden pins the child message: {recorded}"
        );
        assert_eq!(err.body(), r#"{"issues":{"0":["Must be a valid UUID."]}}"#);
    }

    #[test]
    fn create_validate_issues_branch_is_dead() {
        // `min_length=1` fires for `[]`, so the custom message never
        // surfaces; the const pins the Python text for direct-call parity.
        let body = serde_json::json!({"relation_type": "blocking", "issues": []});
        let err = validate_relation_create(&body).expect_err("empty fails");
        assert_eq!(
            err.body(),
            r#"{"issues":["Ensure this field has at least 1 elements."]}"#
        );
        assert_eq!(
            VALIDATE_ISSUES_UNREACHABLE_MESSAGE,
            "At least one issue ID is required."
        );
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueRelationCreateSerializer");
        assert!(
            create["validate_issues_note"]
                .as_str()
                .expect("golden carries the note")
                .contains(VALIDATE_ISSUES_UNREACHABLE_MESSAGE),
            "golden documents the dead branch"
        );
    }

    // ---- create live-DRF vectors ------------------------------------------

    fn create_body(body: Value) -> Result<ValidatedRelationCreate, RelationCreateError> {
        validate_relation_create(&body)
    }

    #[test]
    fn create_field_order_and_unknown_keys() {
        assert_eq!(RELATION_CREATE_FIELDS, &["relation_type", "issues"]);
        assert_eq!(RELATION_TYPE_CHOICES.len(), 8);
        // Both fields run; errors combine in declared order (live probe).
        let err = create_body(serde_json::json!({"relation_type": "nope", "issues": []}))
            .expect_err("both bad");
        assert_eq!(
            err.body(),
            r#"{"relation_type":["\"nope\" is not a valid choice."],"issues":["Ensure this field has at least 1 elements."]}"#
        );
        // Unknown keys are silently ignored.
        let ok = create_body(
            serde_json::json!({"relation_type": "blocking", "issues": [UID], "nope": 1}),
        )
        .expect("unknown keys ignored");
        assert_eq!(ok.relation_type, "blocking");
        assert_eq!(ok.issues, vec![UID.to_string()]);
    }

    #[test]
    fn create_body_shapes_match_live_drf() {
        // Null body.
        let err = create_body(Value::Null).expect_err("null body fails");
        assert_eq!(err.body(), r#"{"non_field_errors":["No data provided"]}"#);
        // Non-object bodies name the JSON type.
        for (body, datatype) in [
            (serde_json::json!([1]), "list"),
            (serde_json::json!("x"), "str"),
            (serde_json::json!(5), "int"),
            (serde_json::json!(1.5), "float"),
            (serde_json::json!(true), "bool"),
        ] {
            let err = create_body(body).expect_err("non-dict fails");
            assert_eq!(
                err.body(),
                format!(
                    r#"{{"non_field_errors":["Invalid data. Expected a dictionary, but got {datatype}."]}}"#
                )
            );
        }
        // Missing choice.
        let err =
            create_body(serde_json::json!({"issues": [UID]})).expect_err("missing choice fails");
        assert_eq!(
            err.body(),
            r#"{"relation_type":["This field is required."]}"#
        );
        // Null choice.
        let err = create_body(serde_json::json!({"relation_type": null, "issues": [UID]}))
            .expect_err("null choice fails");
        assert_eq!(
            err.body(),
            r#"{"relation_type":["This field may not be null."]}"#
        );
    }

    #[test]
    fn create_choice_echo_matches_python_str() {
        // Verified live: DRF echoes `str(input)` inside the quotes.
        for (input, echoed) in [
            (serde_json::json!("nope"), "nope"),
            (serde_json::json!(""), ""),
            (serde_json::json!(5), "5"),
            (serde_json::json!(true), "True"),
            (serde_json::json!(false), "False"),
            (serde_json::json!(["blocking"]), "['blocking']"),
            (serde_json::json!({"a": 1}), "{'a': 1}"),
            (serde_json::json!(1.5), "1.5"),
            (serde_json::json!(1.0), "1.0"),
        ] {
            let err = create_body(serde_json::json!({"relation_type": input, "issues": [UID]}))
                .expect_err("non-choice fails");
            let expected = format!(
                "{{\"relation_type\":[\"\\\"{}\\\" is not a valid choice.\"]}}",
                echoed.replace('\\', "\\\\").replace('"', "\\\"")
            );
            assert_eq!(err.body(), expected, "input {input}");
        }
        // Every declared choice passes.
        for choice in RELATION_TYPE_CHOICES {
            let ok = create_body(serde_json::json!({"relation_type": choice, "issues": [UID]}))
                .expect("declared choice passes");
            assert_eq!(ok.relation_type, choice.to_string());
        }
    }

    #[test]
    fn create_float_choice_echo_follows_python_repr() {
        // CPython `repr` spot checks through the invalid_choice echo.
        for (input, echoed) in [
            (serde_json::json!(1e20), "1e+20"),
            (serde_json::json!(1e-5), "1e-05"),
            (serde_json::json!(0.0001), "0.0001"),
            (serde_json::json!(-0.0), "-0.0"),
            (serde_json::json!(100.0), "100.0"),
        ] {
            let err = create_body(serde_json::json!({"relation_type": input, "issues": [UID]}))
                .expect_err("float is not a choice");
            assert_eq!(
                err.body(),
                format!("{{\"relation_type\":[\"\\\"{echoed}\\\" is not a valid choice.\"]}}"),
                "input {input}"
            );
        }
    }

    #[test]
    fn create_issues_edge_cases_match_live_drf() {
        // Null, non-list shapes.
        let err = create_body(serde_json::json!({"relation_type": "blocking", "issues": null}))
            .expect_err("null fails");
        assert_eq!(err.body(), r#"{"issues":["This field may not be null."]}"#);
        for (issues, typename) in [
            (serde_json::json!("x"), "str"),
            (serde_json::json!(5), "int"),
            (serde_json::json!(1.5), "float"),
            (serde_json::json!(true), "bool"),
            (serde_json::json!({"0": UID}), "dict"),
        ] {
            let err =
                create_body(serde_json::json!({"relation_type": "blocking", "issues": issues}))
                    .expect_err("non-list fails");
            assert_eq!(
                err.body(),
                format!(
                    "{{\"issues\":[\"Expected a list of items but got type \\\"{typename}\\\".\"]}}"
                )
            );
        }
        // Int/bool items coerce via `UUID(int=…)`; spellings normalise.
        let ok = create_body(serde_json::json!({"relation_type": "blocking", "issues": [123]}))
            .expect("int coerces");
        assert_eq!(ok.issues, vec!["00000000-0000-0000-0000-00000000007b"]);
        let ok = create_body(serde_json::json!({"relation_type": "blocking", "issues": [true]}))
            .expect("bool coerces");
        assert_eq!(ok.issues, vec!["00000000-0000-0000-0000-000000000001"]);
        let ok = create_body(
            serde_json::json!({"relation_type": "blocking", "issues": [UID.to_uppercase()]}),
        )
        .expect("uppercase normalises");
        assert_eq!(ok.issues, vec![UID.to_string()]);
        let ok = create_body(
            serde_json::json!({"relation_type": "blocking", "issues": [format!("{{{UID}}}")]}),
        )
        .expect("braced normalises");
        assert_eq!(ok.issues, vec![UID.to_string()]);
        let ok = create_body(
            serde_json::json!({"relation_type": "blocking", "issues": [UID.replace('-', "")]}),
        )
        .expect("32-hex normalises");
        assert_eq!(ok.issues, vec![UID.to_string()]);
        // Bad items collect per index with gaps kept; null items fail null.
        let err = create_body(
            serde_json::json!({"relation_type": "blocking", "issues": ["a", UID, "b"]}),
        )
        .expect_err("bad items fail");
        assert_eq!(
            err.body(),
            r#"{"issues":{"0":["Must be a valid UUID."],"2":["Must be a valid UUID."]}}"#
        );
        let err = create_body(serde_json::json!({"relation_type": "blocking", "issues": [null]}))
            .expect_err("null item fails");
        assert_eq!(
            err.body(),
            r#"{"issues":{"0":["This field may not be null."]}}"#
        );
        // Floats (even integral), negatives and containers are invalid.
        for bad in [
            serde_json::json!(1.5),
            serde_json::json!(123.0),
            serde_json::json!(-1),
            serde_json::json!(["x"]),
            serde_json::json!({"a": 1}),
        ] {
            let err =
                create_body(serde_json::json!({"relation_type": "blocking", "issues": [bad]}))
                    .expect_err("item fails");
            assert_eq!(
                err.body(),
                r#"{"issues":{"0":["Must be a valid UUID."]}}"#,
                "item {bad}"
            );
        }
    }

    // ---- F18-02 remove replays --------------------------------------------

    #[test]
    fn remove_ok_matches_f18_02() {
        let fx = fixture(F18_02);
        let remove = unit(&fx, "IssueRelationRemoveSerializer");
        let body = serde_json::json!({"related_issue": UID});
        let validated = validate_relation_remove(&body).expect("ok is valid");
        let golden = &remove["ok"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            format!("UUID('{}')", validated.related_issue),
            golden["validated"]["related_issue"]
                .as_str()
                .expect("golden carries related_issue")
        );
    }

    #[test]
    fn remove_missing_matches_f18_02() {
        let fx = fixture(F18_02);
        let remove = unit(&fx, "IssueRelationRemoveSerializer");
        let err = validate_relation_remove(&serde_json::json!({})).expect_err("missing fails");
        let golden = &remove["missing"];
        assert_eq!(
            err.body(),
            expected_field_body(&golden["errors"], "related_issue")
        );
        assert_eq!(
            err.body(),
            r#"{"related_issue":["This field is required."]}"#
        );
    }

    #[test]
    fn remove_bad_uuid_matches_f18_02() {
        let fx = fixture(F18_02);
        let remove = unit(&fx, "IssueRelationRemoveSerializer");
        let body = serde_json::json!({"related_issue": "nope"});
        let err = validate_relation_remove(&body).expect_err("bad uuid fails");
        let golden = &remove["bad_uuid"];
        assert_eq!(
            golden["errors"]["related_issue"][0]["code"].as_str(),
            Some("invalid")
        );
        assert_eq!(
            err.body(),
            expected_field_body(&golden["errors"], "related_issue")
        );
        assert_eq!(err.body(), r#"{"related_issue":["Must be a valid UUID."]}"#);
    }

    #[test]
    fn remove_edge_cases_match_live_drf() {
        // Null body / non-dict body.
        let err = validate_relation_remove(&Value::Null).expect_err("null body fails");
        assert_eq!(err.body(), r#"{"non_field_errors":["No data provided"]}"#);
        let err = validate_relation_remove(&serde_json::json!([1])).expect_err("non-dict fails");
        assert_eq!(
            err.body(),
            r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}"#
        );
        // Null, int, bool items share the create child rules.
        let err = validate_relation_remove(&serde_json::json!({"related_issue": null}))
            .expect_err("null fails");
        assert_eq!(
            err.body(),
            r#"{"related_issue":["This field may not be null."]}"#
        );
        let ok = validate_relation_remove(&serde_json::json!({"related_issue": 7}))
            .expect("int coerces");
        assert_eq!(ok.related_issue, "00000000-0000-0000-0000-000000000007");
        let ok = validate_relation_remove(&serde_json::json!({"related_issue": false}))
            .expect("bool coerces");
        assert_eq!(ok.related_issue, "00000000-0000-0000-0000-000000000000");
        // Unknown keys ignored.
        let ok = validate_relation_remove(&serde_json::json!({"related_issue": UID, "x": 1}))
            .expect("unknown keys ignored");
        assert_eq!(ok.related_issue, UID);
    }

    // ---- F18-02 show replays --------------------------------------------

    #[test]
    fn relation_show_keys_and_render_match_f18_02() {
        let fx = fixture(F18_02);
        let show = unit(&fx, "IssueRelationSerializer");
        assert_eq!(str_list(&show["render_keys"]), RELATION_SHOW_FIELDS);
        let render = &show["render"];
        let row = IssueRelationRow {
            id: render["id"].as_str().expect("id"),
            project_id: render["project_id"].as_str().expect("project_id"),
            sequence_id: render["sequence_id"]
                .as_str()
                .expect("sequence_id repr")
                .parse::<i64>()
                .expect("sequence_id parses"),
            relation_type: render["relation_type"].as_str().expect("relation_type"),
            name: render["name"].as_str().expect("name"),
            state_id: opt(render, "state_id"),
            priority: render["priority"].as_str().expect("priority"),
            created_by: opt(render, "created_by"),
            created_at: render["created_at"].as_str().expect("created_at"),
            updated_at: render["updated_at"].as_str().expect("updated_at"),
            updated_by: opt(render, "updated_by"),
        };
        assert!(row.state_id.is_some(), "golden row has state set");
        let out = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out_keys(&out), RELATION_SHOW_FIELDS);
        assert_eq!(out["sequence_id"], serde_json::json!(2));
        assert_eq!(out["created_by"], Value::Null);
        assert_eq!(out["updated_by"], Value::Null);
        assert_eq!(
            serde_json::to_string(&out).expect("serializes"),
            r#"{"id":"25ace52e-c64d-4043-a700-911b1e42bffc","project_id":"d715be3d-234f-46ef-89a3-97f0c7c04b7e","sequence_id":2,"relation_type":"blocking","name":"Second issue","state_id":"97b22834-b823-4109-a527-b39aa310ceae","priority":"none","created_by":null,"created_at":"2026-10-02T23:23:04.659651Z","updated_at":"2026-10-02T23:23:04.659651Z","updated_by":null}"#
        );
    }

    #[test]
    fn relation_show_state_none_omits_state_id() {
        // Live probe: `related_issue.state` None -> `state_id` vanishes.
        let row = IssueRelationRow {
            id: UID,
            project_id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            sequence_id: 2,
            relation_type: "blocking",
            name: "Second issue",
            state_id: None,
            priority: "none",
            created_by: None,
            created_at: "2026-10-02T23:23:04.659651Z",
            updated_at: "2026-10-02T23:23:04.659651Z",
            updated_by: None,
        };
        let out = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        let expected: Vec<&str> = RELATION_SHOW_FIELDS
            .iter()
            .filter(|key| **key != "state_id")
            .copied()
            .collect();
        assert_eq!(out_keys(&out), expected);
        assert!(!out.contains_key("state_id"));
    }

    #[test]
    fn related_show_keys_and_render_match_f18_02() {
        let fx = fixture(F18_02);
        let related = unit(&fx, "RelatedIssueSerializer");
        // The golden row has `type=None`: `type_id` + `is_epic` SkipField
        // away, leaving 11 keys (verified live).
        let expected_keys: Vec<&str> = RELATED_SHOW_FIELDS
            .iter()
            .filter(|key| **key != "type_id" && **key != "is_epic")
            .copied()
            .collect();
        assert_eq!(str_list(&related["render_keys"]), expected_keys);
        let render = &related["render"];
        let row = RelatedIssueRow {
            id: render["id"].as_str().expect("id"),
            project_id: render["project_id"].as_str().expect("project_id"),
            sequence_id: render["sequence_id"]
                .as_str()
                .expect("sequence_id repr")
                .parse::<i64>()
                .expect("sequence_id parses"),
            relation_type: render["relation_type"].as_str().expect("relation_type"),
            name: render["name"].as_str().expect("name"),
            issue_type: None,
            state_id: opt(render, "state_id"),
            priority: render["priority"].as_str().expect("priority"),
            created_by: opt(render, "created_by"),
            created_at: render["created_at"].as_str().expect("created_at"),
            updated_by: opt(render, "updated_by"),
            updated_at: render["updated_at"].as_str().expect("updated_at"),
        };
        assert!(row.state_id.is_some(), "golden row has state set");
        let out = render_related_issue(&RelatedIssueShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out_keys(&out), expected_keys);
        assert_eq!(out["sequence_id"], serde_json::json!(1));
        assert_eq!(
            serde_json::to_string(&out).expect("serializes"),
            r#"{"id":"a7509d00-345f-47fb-bee3-6bcf7d3339e2","project_id":"d715be3d-234f-46ef-89a3-97f0c7c04b7e","sequence_id":1,"relation_type":"blocking","name":"First issue","state_id":"97b22834-b823-4109-a527-b39aa310ceae","priority":"high","created_by":null,"created_at":"2026-10-02T23:23:04.659651Z","updated_by":null,"updated_at":"2026-10-02T23:23:04.659651Z"}"#
        );
    }

    #[test]
    fn related_show_type_and_state_arms() {
        // Type + state set: all 13 keys, `is_epic` a JSON bool (live probe).
        let row = RelatedIssueRow {
            id: UID,
            project_id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            sequence_id: 1,
            relation_type: "blocking",
            name: "First issue",
            issue_type: Some(RelatedIssueType {
                id: "00000000-0000-0000-0000-000000000003",
                is_epic: true,
            }),
            state_id: Some("00000000-0000-0000-0000-000000000002"),
            priority: "high",
            created_by: None,
            created_at: "2026-10-02T23:23:04.659651Z",
            updated_by: None,
            updated_at: "2026-10-02T23:23:04.659651Z",
        };
        let out = render_related_issue(&RelatedIssueShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out_keys(&out), RELATED_SHOW_FIELDS);
        assert_eq!(
            out["type_id"],
            serde_json::json!("00000000-0000-0000-0000-000000000003")
        );
        assert_eq!(out["is_epic"], Value::Bool(true));
        // State unset: `state_id` vanishes alongside the type keys.
        let row_none = RelatedIssueRow {
            issue_type: None,
            state_id: None,
            ..row
        };
        let out_none = render_related_issue(&RelatedIssueShowInput {
            row: &row_none,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(
            out_keys(&out_none),
            vec![
                "id",
                "project_id",
                "sequence_id",
                "relation_type",
                "name",
                "priority",
                "created_by",
                "created_at",
                "updated_by",
                "updated_at"
            ]
        );
    }

    // ---- show fields=/expand= parity --------------------------------------

    fn show_row() -> IssueRelationRow<'static> {
        IssueRelationRow {
            id: UID,
            project_id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            sequence_id: 2,
            relation_type: "blocking",
            name: "Second issue",
            state_id: Some("97b22834-b823-4109-a527-b39aa310ceae"),
            priority: "none",
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            created_at: "2026-10-02T23:23:04.659651Z",
            updated_at: "2026-10-02T23:23:04.659651Z",
            updated_by: None,
        }
    }

    fn related_row() -> RelatedIssueRow<'static> {
        RelatedIssueRow {
            id: UID,
            project_id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            sequence_id: 1,
            relation_type: "blocking",
            name: "First issue",
            issue_type: Some(RelatedIssueType {
                id: "00000000-0000-0000-0000-000000000003",
                is_epic: false,
            }),
            state_id: Some("97b22834-b823-4109-a527-b39aa310ceae"),
            priority: "high",
            created_by: None,
            created_at: "2026-10-02T23:23:04.659651Z",
            updated_by: None,
            updated_at: "2026-10-02T23:23:04.659651Z",
        }
    }

    #[test]
    fn show_fields_subsets_gate_keys_in_wire_order() {
        let row = show_row();
        // Reversed request order still yields wire order; unknowns ignored.
        let specs = includes(&["name", "id", "nope"]);
        let out = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out_keys(&out), vec!["id", "name"]);
        // A kept-but-unset traversal still SkipFields.
        let row_no_state = IssueRelationRow {
            state_id: None,
            ..row
        };
        let specs = includes(&["id", "state_id"]);
        let out = render_issue_relation(&IssueRelationShowInput {
            row: &row_no_state,
            fields: Some(&specs),
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out_keys(&out), vec!["id"]);
        // Nested entries raise before filtering.
        let specs = vec![FieldSpec::Nested("id".to_string(), vec![])];
        assert!(matches!(
            render_issue_relation(&IssueRelationShowInput {
                row: &show_row(),
                fields: Some(&specs),
                expand: &[],
                expansions: &[],
            }),
            Err(RelationShowError::Fields(_))
        ));
        // Related show honors the same kernel.
        let specs = includes(&["is_epic", "type_id"]);
        let out = render_related_issue(&RelatedIssueShowInput {
            row: &related_row(),
            fields: Some(&specs),
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out_keys(&out), vec!["type_id", "is_epic"]);
    }

    #[test]
    fn show_expand_quirks_match_base() {
        let row = show_row();
        // Map hit renders the caller value in place (position kept).
        let user = serde_json::json!({"id": "11111111-1111-1111-1111-111111111111"});
        let out = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: None,
            expand: &["created_by"],
            expansions: &[("created_by", Some(user.clone()))],
        })
        .expect("renders");
        assert_eq!(out["created_by"], user);
        assert_eq!(out_keys(&out)[7], "created_by");
        // Null FK renders `{}`.
        let out = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: None,
            expand: &["updated_by"],
            expansions: &[("updated_by", None)],
        })
        .expect("renders");
        assert_eq!(out["updated_by"], Value::Object(Map::new()));
        // Scalar expand nulls the scalar; unknown names are skipped.
        let out = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: None,
            expand: &["name", "nope"],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out["name"], Value::Null);
        assert!(!out.contains_key("nope"));
        // Expand of a filtered-out field is skipped.
        let specs = includes(&["id"]);
        let out = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: Some(&specs),
            expand: &["created_by"],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out_keys(&out), vec!["id"]);
        // Missing caller value for a kept map hit errors.
        let err = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: None,
            expand: &["created_by"],
            expansions: &[],
        })
        .expect_err("missing expansion fails");
        assert_eq!(
            err,
            RelationShowError::MissingExpansion("created_by".to_string())
        );
        // Related show shares the expansion helper.
        let out = render_related_issue(&RelatedIssueShowInput {
            row: &related_row(),
            fields: None,
            expand: &["created_by", "priority"],
            expansions: &[("created_by", None)],
        })
        .expect("renders");
        assert_eq!(out["created_by"], Value::Object(Map::new()));
        assert_eq!(out["priority"], Value::Null);
    }
}
