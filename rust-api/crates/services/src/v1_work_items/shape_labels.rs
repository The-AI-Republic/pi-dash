#![forbid(unsafe_code)]

//! Issue/label lite + workpad shapes (D-18 serializers B, PIDASHCONV-661).
//!
//! Ports `apps/api/pi_dash/api/serializers/issue.py:497-579`
//! (`IssueLiteSerializer`, `IssueWorkpadSerializer`,
//! `LabelCreateUpdateSerializer`, `LabelSerializer`) and `:1061-1073`
//! (`LabelLiteSerializer`).
//!
//! Fixtures: F18-02 (`rust-api/fixtures/v1_work_items/serializers/
//! F18-02.label_link_relation.golden.json`, the four `:497-579` units) and the
//! F18-03 `LabelLiteSerializer` subset (`F18-03.comment_attachment_activity_
//! expand_search.golden.json`). Every `#[test]` below replays them: golden
//! in/out byte-identical, including validation error strings.
//!
//! This module is pure: the one check that needs the database in Python (the
//! `parent` pk lookup, which also performs the UUID-shape rejection inside
//! `QuerySet.get`) takes the already-resolved existence fact as an argument.
//! The handler layer (PIDASHCONV-679) calls [`parent_lookup_key`] for the
//! canonical pk to look up under `Label.objects` (the soft-deleting manager)
//! and passes the hit/miss in; the error bodies, key orders and check order
//! here are the contract it must honor.
//!
//! Reused, not forked: [`filter_fields`] (the shared `?fields=` kernel),
//! [`field_errors_body`] (combined 400 bodies) and [`BASE_EXPANSION_NAMES`]
//! (the `base.py:91-106` expansion map keys) from the sibling `shape_issue`
//! module. Single-field 400s go through [`field_errors_body`]
//! too rather than the single-field builders, so one error renders
//! byte-identically whether it stands alone or inside a combined body (the
//! `serde_json` string escaper matches `json.dumps` `ensure_ascii=False`
//! exactly, including the `\b`/`\f` short escapes).
//!
//! Write-path reachability (all verified against `api/views/issue.py`):
//!
//! * `LabelCreateUpdateSerializer` — label POST (full) and PATCH
//!   (`partial=True`, `:1362,1500`): [`validate_label_write`].
//! * `IssueWorkpadSerializer` — workpad PATCH (`partial=True`, `:3312`):
//!   [`validate_workpad_write`]. The view's explicit-`body` 400 (`:3287-3291`)
//!   and the row lock live in the handler (PIDASHCONV-676).
//! * `LabelSerializer`, `IssueLiteSerializer`, `LabelLiteSerializer` have no
//!   reachable write path — no view constructs them with `data=` (label
//!   create/patch go through `LabelCreateUpdateSerializer`; the lites render
//!   only) — so only their read shapes are ported.
//! * `create()`/`update()` on both write shapes are DRF's defaults (no
//!   overrides): `validated_data` is applied verbatim, plus the view-supplied
//!   `project_id` kwarg on label create. `Label.save()`'s `sort_order` bump
//!   (`db/models/label.py:46-54`), the 409 name/external-id checks and the
//!   `IntegrityError` mapping are handler/model scope (PIDASHCONV-679/667).
//!
//! Ported quirks (translate, don't redesign — all verified against the
//! Python/DRF sources, the pinned Django 4.2.30, or live probes):
//!
//! * `project_id` on `IssueLiteSerializer` is a DRF `ReadOnlyField`
//!   (`hasattr(model, name)` → `build_property_field`), and every lite field
//!   is read-only.
//! * `?expand=parent` on the label list feeds a `Label` to
//!   `IssueLiteSerializer` (`base.py:104` maps `parent` to the *issue* lite):
//!   `sequence_id` is missing on a label, so DRF `SkipField` drops the key
//!   (read-only fields are `required=False`, `fields.py:Field.__init__`) and
//!   the expansion renders `{"id", "project_id"}` — hence
//!   [`IssueLiteRow::sequence_id`] is `Option`.
//! * Expanding a null-FK relation renders `{}` (DRF `SkipField` on every
//!   field — the [`render_label`] `None` arm).
//! * `?expand=<scalar>` nulls that scalar and adds unknown kept names as
//!   `null` (Base `else` branch, `base.py:114-116`).
//! * `LabelCreateUpdateSerializer.Meta.read_only_fields` names only fields
//!   NOT in `fields`, so DRF ignores it and all seven fields are writable.
//! * Missing `sort_order` is omitted from `validated_data` (DRF maps no model
//!   `default=` to a field default — `get_field_kwargs` only clears
//!   `required`).
//! * `parent=""` validates as `None` (`RelatedField` forces `""` → `None`);
//!   only a JSON bool reaches `incorrect_type` — every other non-UUID input
//!   fails Django's UUID parse first and surfaces its curly-quote message.
//!
//! Documented approximations (all outside any golden; same class as the
//! merged `types::v1_assets` kernels):
//!
//! * `CharField` numeric coercion and the non-dict datatype word render
//!   through `serde_json` shortest-roundtrip: JSON floats spell exponents as
//!   `1e22` where Python spells `1e+22`, and integers beyond `u64` arrive as
//!   `f64` (Python keeps them exact). Strings, bools, `None` and in-range
//!   integers are exact.
//! * The Django UUID `invalid` message embeds `str(data)`: exact for strings,
//!   numbers and bools; JSON arrays/objects fall back to compact JSON where
//!   Python uses `repr` (`{"a":1}` vs `{'a': 1}`).
//! * `CharField` surrogate validation is skipped: `serde_json` rejects lone
//!   surrogates at parse, so the validator is unreachable on JSON input.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde_json::{Map, Value};

use super::shape_issue::{field_errors_body, BASE_EXPANSION_NAMES};
use super::{filter_fields, FieldSpec, FilterError};

/// `IssueLiteSerializer.Meta.fields` (`serializers/issue.py:507`).
pub const ISSUE_LITE_FIELDS: &[&str] = &["id", "sequence_id", "project_id"];

/// `IssueWorkpadSerializer` readable fields (`issue.py:522`): the declared
/// `body` (`source="workpad"`, `:518`) plus the read-only `updated_at`.
pub const WORKPAD_READ_FIELDS: &[&str] = &["body", "updated_at"];

/// `LabelCreateUpdateSerializer.Meta.fields` (`issue.py:536-544`), in field
/// (and therefore error-combination) order.
pub const LABEL_WRITE_FIELDS: &[&str] = &[
    "name",
    "color",
    "description",
    "external_source",
    "external_id",
    "parent",
    "sort_order",
];

/// `LabelSerializer` read order (`issue.py:565-577`, `fields="__all__"`):
/// declared `id`, then plain model fields in model order, then forward
/// relations in model order (DRF `get_default_field_names`), pinned by the
/// F18-02 `render_keys`.
pub const LABEL_READ_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "color",
    "sort_order",
    "external_source",
    "external_id",
    "created_by",
    "updated_by",
    "workspace",
    "project",
    "parent",
];

/// `LabelLiteSerializer.Meta.fields` (`issue.py:1071`).
pub const LABEL_LITE_FIELDS: &[&str] = &["id", "name", "color"];

/// Label text column limit (`db/models/label.py:19,21,23-24`,
/// `max_length=255`), enforced by DRF's `CharField` before anything else.
pub const MAX_LABEL_CHARS: usize = 255;

/// `FloatField` over-long-string guard (`fields.py`, shared with
/// `IntegerField`): code points, checked before parsing.
pub const MAX_FLOAT_STRING_CHARS: usize = 1000;

/// Missing required input (DRF `Field.validate_empty_values`, `required`).
pub const MSG_REQUIRED: &str = "This field is required.";
/// Explicit JSON null where `allow_null=False` (DRF `null`).
pub const MSG_NULL: &str = "This field may not be null.";
/// Empty or whitespace-only input where `allow_blank=False` (DRF `blank`).
pub const MSG_BLANK: &str = "This field may not be blank.";
/// Bool/array/object input to `CharField` (DRF `invalid`; numbers coerce via
/// `str()` instead).
pub const MSG_INVALID_STRING: &str = "Not a valid string.";
/// Null byte (Django `ProhibitNullCharactersValidator`, runs on every
/// `CharField` after `max_length`).
pub const MSG_NULL_CHARS: &str = "Null characters are not allowed.";
/// Non-numeric `sort_order` (DRF `FloatField.invalid`).
pub const MSG_INVALID_NUMBER: &str = "A valid number is required.";
/// Over-1000-code-point string to `FloatField` (DRF `max_string_length`).
pub const MSG_STRING_TOO_LARGE: &str = "String value too large.";
/// Bool `parent` (DRF `PrimaryKeyRelatedField.incorrect_type`, `bool` being
/// the only JSON type that reaches it).
pub const MSG_PARENT_INCORRECT_TYPE_BOOL: &str =
    "Incorrect type. Expected pk value, received bool.";

/// `max_length` failure for a label text field (DRF `CharField.max_length`,
/// checked on the stripped value before the null-characters validator).
fn max_length_message(max_chars: usize) -> String {
    format!("Ensure this field has no more than {max_chars} characters.")
}

/// The PYTHON type name of a parsed-JSON value (`type(data).__name__`), for
/// the non-object-body message. Objects take the dict path, so the `"dict"`
/// arm is unreachable.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Non-object write body (`Serializer.to_internal_value`, DRF
/// `serializers.py:340,484`): field validation never runs, so no DB fact is
/// consulted.
fn non_dict_body(json_type: &str) -> String {
    field_errors_body(&[(
        "non_field_errors",
        Value::Array(vec![Value::String(format!(
            "Invalid data. Expected a dictionary, but got {json_type}."
        ))]),
    )])
}

/// Render a JSON value the way Python `str(data)` would, for the Django UUID
/// `invalid` message. Strings verbatim; bools `True`/`False`; `None`;
/// numbers shortest-roundtrip (the merged float nuance); arrays/objects fall
/// back to compact JSON (documented approximation — see the module docs).
fn py_scalar_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// `parent` pk-miss message (DRF `PrimaryKeyRelatedField.does_not_exist`):
/// the RAW input echoed verbatim (`relations.py`, `pk_value=data`).
fn parent_does_not_exist_message(raw: &str) -> String {
    format!("Invalid pk \"{raw}\" - object does not exist.")
}

/// Format a 128-bit value as the canonical lowercase hyphenated UUID.
fn format_canonical_uuid(value: u128) -> String {
    let hex = format!("{value:032x}");
    // `hex` is 32 ASCII chars by construction; byte slicing is safe.
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// `int(core, 16)` acceptance for the 32-char UUID core (CPython semantics,
/// live-probed): surrounding Unicode whitespace, one leading `+`, one leading
/// `0x`/`0X`, and single `_` between hex digits are all accepted (`-` is
/// impossible — eaten as a dash before this step). Values always fit `u128`
/// (at most 32 hex digits by the length gate).
fn parse_hex_int(core: &str) -> Option<u128> {
    let trimmed = core.trim();
    let unsigned = trimmed.strip_prefix('+').unwrap_or(trimmed);
    let digits = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
        .unwrap_or(unsigned);
    if digits.is_empty() {
        return None;
    }
    let mut value: u128 = 0;
    // Reject a leading/trailing/doubled `_` (`int()` requires digits around
    // every underscore).
    let mut prev_underscore = true;
    let mut seen_digit = false;
    for c in digits.chars() {
        if c == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if c.is_ascii_hexdigit() {
            value = value
                .checked_mul(16)?
                .checked_add(c.to_digit(16)? as u128)?;
            prev_underscore = false;
            seen_digit = true;
        } else {
            return None;
        }
    }
    if prev_underscore || !seen_digit {
        return None;
    }
    Some(value)
}

/// Parse a `parent` string exactly like `uuid.UUID(hex=value)` (Django
/// `UUIDField.to_python`, 4.2.30; every branch live-probed against CPython):
/// replace-all `urn:`/`uuid:` (case-sensitive), strip `{}` ends, drop `-`,
/// require exactly 32 code points, then [`parse_hex_int`]. Returns the
/// canonical lowercase hyphenated form.
fn parse_uuid_hex(raw: &str) -> Option<String> {
    let no_scheme = raw.replace("urn:", "").replace("uuid:", "");
    let stripped = no_scheme.trim_matches(|c| c == '{' || c == '}');
    let compact: String = stripped.chars().filter(|c| *c != '-').collect();
    if compact.chars().count() != 32 {
        return None;
    }
    parse_hex_int(&compact).map(format_canonical_uuid)
}

/// Python `float(str)` acceptance for `sort_order` (DRF `FloatField` calls
/// `float(data)` directly): surrounding whitespace stripped, optional sign,
/// `inf`/`infinity`/`nan` in any case (the sign bit on `nan` is
/// unobservable downstream), single `_` between ASCII digits, otherwise the
/// `f64::from_str` core (which matches `float()` once underscores are
/// validated — same digits/dots/exponents/`inf`/`nan` vocabulary, no hex).
fn py_parse_float(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let unsigned = trimmed
        .strip_prefix('+')
        .or_else(|| trimmed.strip_prefix('-'))
        .unwrap_or(trimmed);
    match unsigned.to_ascii_lowercase().as_str() {
        "inf" | "infinity" => {
            return Some(if trimmed.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            });
        }
        "nan" => return Some(f64::NAN),
        _ => {}
    }
    if trimmed.contains('_') {
        // `float()` requires ASCII digits around every underscore (`1_e5`
        // and `1e_5` both fail — `e` is not a digit).
        let chars: Vec<char> = trimmed.chars().collect();
        for (pos, c) in chars.iter().enumerate() {
            if *c == '_' {
                let surrounded = pos > 0
                    && pos + 1 < chars.len()
                    && chars[pos - 1].is_ascii_digit()
                    && chars[pos + 1].is_ascii_digit();
                if !surrounded {
                    return None;
                }
            }
        }
        let cleaned: String = chars.iter().filter(|c| **c != '_').collect();
        cleaned.parse::<f64>().ok()
    } else {
        trimmed.parse::<f64>().ok()
    }
}

/// Outcome of [`validate_char`] for one text input.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CharOutcome {
    /// Key absent and not required (or `partial=True`): omitted from
    /// `validated_data` (DRF `SkipField` — no field here carries a default).
    Skip,
    /// Explicit JSON null on an `allow_null` field.
    Null,
    /// The stripped value.
    Text(String),
}

/// Every failure DRF `CharField` validation can produce, in check order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CharError {
    /// Key absent on a required field (create only — `partial` skips first).
    Required,
    /// Explicit JSON null where `allow_null=False`.
    Null,
    /// `""` or whitespace-only where `allow_blank=False`.
    Blank,
    /// Bool/array/object input.
    InvalidType,
    /// Stripped value over `max_length` (code points); carries the limit.
    MaxLength(usize),
    /// Null byte (runs after `max_length`).
    NullChars,
}

impl CharError {
    fn message(&self) -> String {
        match self {
            CharError::Required => MSG_REQUIRED.to_owned(),
            CharError::Null => MSG_NULL.to_owned(),
            CharError::Blank => MSG_BLANK.to_owned(),
            CharError::InvalidType => MSG_INVALID_STRING.to_owned(),
            CharError::MaxLength(max) => max_length_message(*max),
            CharError::NullChars => MSG_NULL_CHARS.to_owned(),
        }
    }
}

/// Port of DRF `CharField` validation (`fields.py:CharField` +
/// `Field.validate_empty_values`, DRF 3.15.2): absent → required/skip, null →
/// null/None, blank check on the raw value, bool/composite rejection,
/// numeric coercion via `str()`, whitespace trim, `max_length`, null-bytes.
/// `value=None` is the absent key (`Field.get_value` returns `empty`).
fn validate_char(
    value: Option<&Value>,
    required: bool,
    allow_null: bool,
    allow_blank: bool,
    max_chars: Option<usize>,
    partial: bool,
) -> Result<CharOutcome, CharError> {
    let Some(value) = value else {
        if partial || !required {
            return Ok(CharOutcome::Skip);
        }
        return Err(CharError::Required);
    };
    if value.is_null() {
        if allow_null {
            return Ok(CharOutcome::Null);
        }
        return Err(CharError::Null);
    }
    // `isinstance(data, bool) or not isinstance(data, (str, int, float))`
    // fails `invalid`; `serde_json` keeps bools distinct from numbers, so
    // the arms stay exact.
    let raw = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return Err(CharError::InvalidType),
    };
    // The blank check runs on the raw value: `data == '' or
    // str(data).strip() == ''` — whitespace-only fails unless blank is
    // allowed, in which case the field validates as `""`.
    if raw.is_empty() || raw.trim().is_empty() {
        if allow_blank {
            return Ok(CharOutcome::Text(String::new()));
        }
        return Err(CharError::Blank);
    }
    let stripped = raw.trim().to_owned();
    if let Some(max) = max_chars {
        if stripped.chars().count() > max {
            return Err(CharError::MaxLength(max));
        }
    }
    if stripped.contains('\0') {
        return Err(CharError::NullChars);
    }
    Ok(CharOutcome::Text(stripped))
}

/// Outcome of [`validate_sort_order`].
#[derive(Debug, Clone, PartialEq)]
enum FloatOutcome {
    /// Key absent (never required): omitted from `validated_data`.
    Skip,
    /// The parsed number (integers via `float(n)`, `bool` via
    /// `float(True/False)` — `FloatField` has no bool rejection).
    Number(f64),
}

/// Every failure `sort_order` validation can produce, in check order.
/// (`FloatField.overflow` is unreachable: JSON integers are `i64`/`u64` and
/// always convert finitely — Python's `OverflowError` needs > ~1.8e308.)
#[derive(Debug, Clone, PartialEq, Eq)]
enum FloatError {
    /// Explicit JSON null (`allow_null=False`).
    Null,
    /// Unparseable (or bool/array/object — `float()`-rejectable).
    Invalid,
    /// Over-1000-code-point string.
    StringTooLarge,
}

impl FloatError {
    fn message(&self) -> String {
        match self {
            FloatError::Null => MSG_NULL.to_owned(),
            FloatError::Invalid => MSG_INVALID_NUMBER.to_owned(),
            FloatError::StringTooLarge => MSG_STRING_TOO_LARGE.to_owned(),
        }
    }
}

/// Port of DRF `FloatField` validation for `sort_order` (`fields.py`).
fn validate_sort_order(value: Option<&Value>) -> Result<FloatOutcome, FloatError> {
    let Some(value) = value else {
        return Ok(FloatOutcome::Skip);
    };
    if value.is_null() {
        return Err(FloatError::Null);
    }
    match value {
        Value::String(text) => {
            if text.chars().count() > MAX_FLOAT_STRING_CHARS {
                return Err(FloatError::StringTooLarge);
            }
            match py_parse_float(text) {
                Some(number) => Ok(FloatOutcome::Number(number)),
                None => Err(FloatError::Invalid),
            }
        }
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                // `float(int)` — always finite for `i64`.
                Ok(FloatOutcome::Number(int as f64))
            } else if let Some(int) = number.as_u64() {
                Ok(FloatOutcome::Number(int as f64))
            } else if let Some(float) = number.as_f64() {
                Ok(FloatOutcome::Number(float))
            } else {
                Err(FloatError::Invalid)
            }
        }
        Value::Bool(true) => Ok(FloatOutcome::Number(1.0)),
        Value::Bool(false) => Ok(FloatOutcome::Number(0.0)),
        Value::Array(_) | Value::Object(_) | Value::Null => Err(FloatError::Invalid),
    }
}

/// What the `parent` input needs after the pure checks: mirrors
/// `RelatedField.run_validation` (`""` → `None`) +
/// `PrimaryKeyRelatedField.to_internal_value` (`relations.py`) over Django's
/// `UUIDField.to_python`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParentNeed {
    /// Key absent (never required; `partial` also skips): no lookup.
    Skip,
    /// Explicit null or `""`: validates as `None`, no lookup.
    Null,
    /// Parses as a UUID: the handler looks up `canonical` and reports the
    /// hit/miss. `raw` is the pre-parse rendering for the miss message.
    Query { canonical: String, raw: String },
}

/// Pure `parent` failures (before the existence lookup).
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParentError {
    /// JSON bool (DRF raises `TypeError` before the queryset call).
    IncorrectType,
    /// Fails `uuid.UUID(...)`: carries the `str(data)` rendering for the
    /// Django curly-quote message.
    BadUuid(String),
}

impl ParentError {
    fn message(&self) -> String {
        match self {
            ParentError::IncorrectType => MSG_PARENT_INCORRECT_TYPE_BOOL.to_owned(),
            // Django `UUIDField` invalid (`django/db/models/fields/
            // __init__.py`, 4.2.30): literal curly quotes, `str(data)`
            // value, single-message list via `get_error_detail`
            // (live-probed). The caller embeds this via
            // `field_errors_body` for exact escaping.
            ParentError::BadUuid(raw) => format!("\u{201c}{raw}\u{201d} is not a valid UUID."),
        }
    }
}

/// Classify one `parent` input (`None` = absent key).
fn classify_parent(value: Option<&Value>) -> Result<ParentNeed, ParentError> {
    let Some(value) = value else {
        return Ok(ParentNeed::Skip);
    };
    match value {
        Value::Null => Ok(ParentNeed::Null),
        // `RelatedField.run_validation` forces exactly `""` to `None`
        // (whitespace-only does NOT take this path — it fails UUID parse).
        Value::String(text) if text.is_empty() => Ok(ParentNeed::Null),
        Value::String(text) => match parse_uuid_hex(text) {
            Some(canonical) => Ok(ParentNeed::Query {
                canonical,
                raw: text.clone(),
            }),
            None => Err(ParentError::BadUuid(py_scalar_str(value))),
        },
        // `isinstance(data, bool)` raises `TypeError` before `queryset.get`
        // (even though `uuid.UUID(int=True)` would parse).
        Value::Bool(_) => Err(ParentError::IncorrectType),
        Value::Number(number) => {
            // `uuid.UUID(int=n)` accepts `0 <= n < 2**128`; floats take the
            // `hex=` path and fail (`AttributeError`, caught by Django).
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    Err(ParentError::BadUuid(py_scalar_str(value)))
                } else {
                    Ok(ParentNeed::Query {
                        canonical: format_canonical_uuid(int as u128),
                        raw: int.to_string(),
                    })
                }
            } else if let Some(int) = number.as_u64() {
                Ok(ParentNeed::Query {
                    canonical: format_canonical_uuid(int as u128),
                    raw: int.to_string(),
                })
            } else {
                Err(ParentError::BadUuid(py_scalar_str(value)))
            }
        }
        // `uuid.UUID(hex={...})` raises `AttributeError`, caught by Django
        // alongside `ValueError` (`to_python`) — compact-JSON rendering is
        // the documented approximation here.
        Value::Array(_) | Value::Object(_) => Err(ParentError::BadUuid(py_scalar_str(value))),
    }
}

/// Pure pre-pass for the handler: the canonical `Label.objects` pk to look up
/// for this write body, or `None` for no lookup. Reads only the `parent` key
/// of an object body — non-object bodies fail before field validation runs,
/// so Python never queries for them either.
pub fn parent_lookup_key(body: &Value) -> Option<String> {
    let obj = body.as_object()?;
    match classify_parent(obj.get("parent")) {
        Ok(ParentNeed::Query { canonical, .. }) => Some(canonical),
        _ => None,
    }
}

/// `LabelCreateUpdateSerializer(data, partial=...)` input
/// (`api/views/issue.py:1362,1500`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelWriteInput<'a> {
    /// The request body as parsed JSON.
    pub body: &'a Value,
    /// PATCH (`partial=True`) vs POST: absent keys are always skipped either
    /// way here except `name`, which is required on create.
    pub partial: bool,
    /// `Label.objects` (the soft-deleting manager) hit for
    /// [`parent_lookup_key`]; consulted only when the parent arm parsed a
    /// UUID (`None` there is a caller-contract violation).
    pub parent_exists: Option<bool>,
}

/// Validated label write (`validated_data` shape): `None` = key omitted
/// (absent input); the double-`Option` fields distinguish explicit null.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedLabel {
    pub name: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub external_source: Option<Option<String>>,
    pub external_id: Option<Option<String>>,
    /// Canonical UUID string when set.
    pub parent: Option<Option<String>>,
    pub sort_order: Option<f64>,
}

/// Every failure [`validate_label_write`] can produce.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelWriteError {
    /// Non-object body; carries the byte-exact 400 body.
    #[error("{0}")]
    NotADict(String),
    /// Combined field errors in [`LABEL_WRITE_FIELDS`] order; carries the
    /// byte-exact 400 body.
    #[error("{0}")]
    Fields(String),
    /// Caller-contract violation: `parent_exists` is `None` while the parent
    /// arm parsed a UUID (the handler must pass `Some` whenever
    /// [`parent_lookup_key`] returns `Some`). No wire body — the handler
    /// maps it to a 500.
    #[error(
        "parent_exists is None while the parent arm needs it \
         (handler must pass Some when parent_lookup_key returns Some)"
    )]
    MissingParentFact,
}

impl LabelWriteError {
    /// The 400 response body (`None` for the caller-contract arm).
    pub fn body(&self) -> Option<&str> {
        match self {
            LabelWriteError::NotADict(body) | LabelWriteError::Fields(body) => Some(body),
            LabelWriteError::MissingParentFact => None,
        }
    }
}

/// Shape one field failure for [`field_errors_body`].
fn field_entry(field: &'static str, message: String) -> (&'static str, Value) {
    (field, Value::Array(vec![Value::String(message)]))
}

/// Port of `LabelCreateUpdateSerializer` field validation
/// (`serializers/issue.py:526-554` over DRF `ModelSerializer`, no `validate()`
/// override): every writable field runs even after failures, errors combine
/// in field order, unknown input keys are silently ignored, and there are no
/// uniqueness validators (DRF ignores `Meta.constraints`).
pub fn validate_label_write(
    input: &LabelWriteInput<'_>,
) -> Result<ValidatedLabel, LabelWriteError> {
    let Some(obj) = input.body.as_object() else {
        return Err(LabelWriteError::NotADict(non_dict_body(json_type_name(
            input.body,
        ))));
    };
    let mut errors: Vec<(&'static str, Value)> = Vec::new();

    // `name = CharField(max_length=255)`: required, no null, no blank.
    let name = match validate_char(
        obj.get("name"),
        true,
        false,
        false,
        Some(MAX_LABEL_CHARS),
        input.partial,
    ) {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("name", error.message()));
            None
        }
    };
    // `color = CharField(max_length=255, blank=True)`: optional, blank ok.
    let color = match validate_char(
        obj.get("color"),
        false,
        false,
        true,
        Some(MAX_LABEL_CHARS),
        input.partial,
    ) {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("color", error.message()));
            None
        }
    };
    // `description = TextField(blank=True)`: optional, blank ok, no length cap.
    let description = match validate_char(
        obj.get("description"),
        false,
        false,
        true,
        None,
        input.partial,
    ) {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("description", error.message()));
            None
        }
    };
    // `external_source/external_id`: optional, null ok, blank ok, 255 cap.
    let external_source = match validate_char(
        obj.get("external_source"),
        false,
        true,
        true,
        Some(MAX_LABEL_CHARS),
        input.partial,
    ) {
        Ok(CharOutcome::Text(value)) => Some(Some(value)),
        Ok(CharOutcome::Null) => Some(None),
        Ok(CharOutcome::Skip) => None,
        Err(error) => {
            errors.push(field_entry("external_source", error.message()));
            None
        }
    };
    let external_id = match validate_char(
        obj.get("external_id"),
        false,
        true,
        true,
        Some(MAX_LABEL_CHARS),
        input.partial,
    ) {
        Ok(CharOutcome::Text(value)) => Some(Some(value)),
        Ok(CharOutcome::Null) => Some(None),
        Ok(CharOutcome::Skip) => None,
        Err(error) => {
            errors.push(field_entry("external_id", error.message()));
            None
        }
    };
    // `parent`: self-FK, optional, null ok; existence via the caller fact.
    let mut parent: Option<Option<String>> = None;
    match classify_parent(obj.get("parent")) {
        Ok(ParentNeed::Skip) => {}
        Ok(ParentNeed::Null) => parent = Some(None),
        Ok(ParentNeed::Query { canonical, raw }) => match input.parent_exists {
            Some(true) => parent = Some(Some(canonical)),
            Some(false) => {
                errors.push(field_entry("parent", parent_does_not_exist_message(&raw)));
            }
            None => return Err(LabelWriteError::MissingParentFact),
        },
        Err(error) => {
            errors.push(field_entry("parent", error.message()));
        }
    }
    // `sort_order = FloatField(default=65535)`: optional (no DRF default —
    // missing keys are omitted, never 65535), no null.
    let mut sort_order: Option<f64> = None;
    match validate_sort_order(obj.get("sort_order")) {
        Ok(FloatOutcome::Skip) => {}
        Ok(FloatOutcome::Number(value)) => sort_order = Some(value),
        Err(error) => {
            errors.push(field_entry("sort_order", error.message()));
        }
    }

    if !errors.is_empty() {
        return Err(LabelWriteError::Fields(field_errors_body(&errors)));
    }
    Ok(ValidatedLabel {
        name,
        color,
        description,
        external_source,
        external_id,
        parent,
        sort_order,
    })
}

/// `IssueWorkpadSerializer(instance, data, partial=True)` input
/// (`api/views/issue.py:3312`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkpadWriteInput<'a> {
    /// The request body as parsed JSON.
    pub body: &'a Value,
    /// Always true on the endpoint; kept explicit for direct-call parity.
    pub partial: bool,
}

/// Validated workpad write: `None` = `body` absent (omitted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedWorkpad {
    pub workpad: Option<String>,
}

/// Every failure [`validate_workpad_write`] can produce; both carry the
/// byte-exact 400 body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkpadWriteError {
    #[error("{0}")]
    NotADict(String),
    #[error("{0}")]
    Fields(String),
}

impl WorkpadWriteError {
    /// The 400 response body.
    pub fn body(&self) -> &str {
        match self {
            WorkpadWriteError::NotADict(body) | WorkpadWriteError::Fields(body) => body,
        }
    }
}

/// Port of `IssueWorkpadSerializer` field validation
/// (`serializers/issue.py:511-523`): the single declared
/// `body = CharField(source="workpad", allow_blank=True, required=False)`
/// (no `max_length` — declared fields ignore the model `TextField`); the
/// validated key is `workpad` (`source=`); `updated_at` is read-only and
/// every other input key is silently ignored.
pub fn validate_workpad_write(
    input: &WorkpadWriteInput<'_>,
) -> Result<ValidatedWorkpad, WorkpadWriteError> {
    let Some(obj) = input.body.as_object() else {
        return Err(WorkpadWriteError::NotADict(non_dict_body(json_type_name(
            input.body,
        ))));
    };
    match validate_char(obj.get("body"), false, false, true, None, input.partial) {
        Ok(CharOutcome::Text(value)) => Ok(ValidatedWorkpad {
            workpad: Some(value),
        }),
        Ok(_) => Ok(ValidatedWorkpad { workpad: None }),
        Err(error) => Err(WorkpadWriteError::Fields(field_errors_body(&[
            field_entry("body", error.message()),
        ]))),
    }
}

/// `IssueLiteSerializer` source row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueLiteRow<'a> {
    pub id: &'a str,
    /// `None` = attribute missing → the key is dropped (DRF `SkipField`):
    /// the `?expand=parent` label case, where a `Label` has no
    /// `sequence_id`. Real issues always carry `Some`.
    pub sequence_id: Option<i64>,
    pub project_id: &'a str,
}

/// Port of `IssueLiteSerializer` reads (`serializers/issue.py:497-508`).
/// No `fields=`/`expand=` parameters: the only construction site is the Base
/// `parent` expansion (`base.py:104,112`), which passes neither.
pub fn render_issue_lite(row: &IssueLiteRow<'_>) -> Map<String, Value> {
    let mut out = Map::with_capacity(3);
    out.insert("id".to_string(), Value::String(row.id.to_string()));
    if let Some(sequence_id) = row.sequence_id {
        out.insert("sequence_id".to_string(), Value::Number(sequence_id.into()));
    }
    out.insert(
        "project_id".to_string(),
        Value::String(row.project_id.to_string()),
    );
    out
}

/// `IssueWorkpadSerializer` source row (`workpad` is `NOT NULL`, so `body`
/// is always present).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkpadRow<'a> {
    pub body: &'a str,
    /// Pre-formatted DRF `iso-8601` (`...Z`) rendering of `updated_at`.
    pub updated_at: &'a str,
}

/// Port of `IssueWorkpadSerializer` reads (`issue.py:511-523`): `{body,
/// updated_at}` in field order. The endpoint constructs it without
/// `fields=`/`expand=` (`views/issue.py:3279`).
pub fn render_workpad(row: &WorkpadRow<'_>) -> Map<String, Value> {
    let mut out = Map::with_capacity(2);
    out.insert("body".to_string(), Value::String(row.body.to_string()));
    out.insert(
        "updated_at".to_string(),
        Value::String(row.updated_at.to_string()),
    );
    out
}

/// `LabelLiteSerializer` source row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub color: &'a str,
}

/// Port of `LabelLiteSerializer` reads (`issue.py:1061-1072`): `{id, name,
/// color}` in field order. Consumed with `many=True` and no `fields=`/
/// `expand=` (`IssueExpandSerializer.get_labels`, `:1094`).
pub fn render_label_lite(row: &LabelLiteRow<'_>) -> Map<String, Value> {
    let mut out = Map::with_capacity(3);
    out.insert("id".to_string(), Value::String(row.id.to_string()));
    out.insert("name".to_string(), Value::String(row.name.to_string()));
    out.insert("color".to_string(), Value::String(row.color.to_string()));
    out
}

/// `LabelSerializer` source row: datetimes arrive pre-formatted (DRF
/// `iso-8601`, `...Z` for UTC), pks as canonical strings.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    /// `NOT NULL` (`blank=True`, no `null`) — always present.
    pub description: &'a str,
    /// `NOT NULL` — always present.
    pub color: &'a str,
    pub sort_order: f64,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    /// `NOT NULL` — always present.
    pub workspace: &'a str,
    /// Nullable FK (`WorkspaceBaseModel.project`, `null=True` — the
    /// project-null labels of the uniqueness constraint).
    pub project: Option<&'a str>,
    pub parent: Option<&'a str>,
}

/// Failure modes of [`render_label`]: the caller-contract arms are 500-class
/// for the handler to map (mirroring `shape_issue::RenderError`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelRenderError {
    /// Nested `fields=` dict (`TypeError` parity, see [`filter_fields`]).
    #[error("fields filter failed: {0}")]
    Fields(#[from] FilterError),
    /// Non-finite `sort_order`: `serde_json` cannot render NaN/Infinity
    /// (Postgres `float8` admits them; Django emits the literal tokens).
    #[error("sort_order is not finite (Django emits the NaN/Infinity literal)")]
    NonFiniteFloat,
    /// A map-hit `expand` name with no caller value (Python always renders
    /// the related object, or `{}` when the FK is null).
    #[error("expand '{0}' needs its rendered value (None renders {{}})")]
    MissingExpansion(String),
}

/// `LabelSerializer.to_representation()` input (`issue.py:557-577` over the
/// `BaseSerializer` passes, `api/serializers/base.py:72-117`).
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRepresentationInput<'a> {
    /// The label row.
    pub row: &'a LabelRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// Rendered values for map-hit `expand` names among this serializer's
    /// fields (`created_by`, `updated_by`, `workspace`, `project`, `parent`):
    /// `Some(value)` renders the object, `None` renders `{}` (null FK).
    /// Looked up only for names in [`BASE_EXPANSION_NAMES`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::String(text.to_string()),
        None => Value::Null,
    }
}

/// Port of `LabelSerializer.to_representation()` (`issue.py:557-577`) over
/// the `BaseSerializer` passes (`base.py:19-30,72-117`).
///
/// Base-expansion rules, in `expand` order, for names in the kept fields:
///
/// * map hit ([`BASE_EXPANSION_NAMES`]) → the caller value, or `{}` for a
///   null FK; a missing caller value is
///   [`LabelRenderError::MissingExpansion`];
/// * anything else → `null` (verbatim from `base.py:114-116` — no non-map
///   label field has a `<name>_id` attribute, so the passthrough always
///   yields `None`).
pub fn render_label(
    input: &LabelRepresentationInput<'_>,
) -> Result<Map<String, Value>, LabelRenderError> {
    let kept = filter_fields(LABEL_READ_FIELDS, input.fields)?;
    let kept_contains = |name: &str| kept.iter().any(|kept| kept == name);

    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "deleted_at" => opt_str(row.deleted_at),
            "name" => Value::String(row.name.to_string()),
            "description" => Value::String(row.description.to_string()),
            "color" => Value::String(row.color.to_string()),
            "sort_order" => match serde_json::Number::from_f64(row.sort_order) {
                Some(number) => Value::Number(number),
                None => return Err(LabelRenderError::NonFiniteFloat),
            },
            "external_source" => opt_str(row.external_source),
            "external_id" => opt_str(row.external_id),
            "created_by" => opt_str(row.created_by),
            "updated_by" => opt_str(row.updated_by),
            "workspace" => Value::String(row.workspace.to_string()),
            "project" => opt_str(row.project),
            "parent" => opt_str(row.parent),
            // `filter_fields` only ever yields `LABEL_READ_FIELDS` names,
            // and every one is matched above.
            _ => unreachable!("render_label matched every kept readable field"),
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
                None => return Err(LabelRenderError::MissingExpansion(name.to_string())),
            }
        } else {
            out.insert(name.to_string(), Value::Null);
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const F18_02: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/serializers/F18-02.label_link_relation.golden.json"
    );
    const F18_03: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/serializers/F18-03.comment_attachment_activity_expand_search.golden.json"
    );

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

    /// Fixture validated strings are Python reprs; every recorded value here
    /// is quote-free, so `repr(s) == 's'`.
    fn py_repr(text: &str) -> String {
        format!("'{text}'")
    }

    fn out_keys(out: &Map<String, Value>) -> Vec<&str> {
        out.keys().map(String::as_str).collect()
    }

    const PARENT_UUID: &str = "c833c492-fa3d-49f5-85a4-0ef810642731";

    fn label_input(body: &Value) -> LabelWriteInput<'_> {
        LabelWriteInput {
            body,
            partial: false,
            parent_exists: None,
        }
    }

    fn label_row_fixture() -> LabelRow<'static> {
        LabelRow {
            id: "c833c492-fa3d-49f5-85a4-0ef810642731",
            created_at: "2026-10-02T23:17:12.019714Z",
            updated_at: "2026-10-02T23:17:12.019714Z",
            deleted_at: None,
            name: "bug",
            description: "",
            color: "",
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            created_by: None,
            updated_by: None,
            workspace: "92989e99-51c6-4725-8069-45784951694f",
            project: Some("d715be3d-234f-46ef-89a3-97f0c7c04b7e"),
            parent: None,
        }
    }

    // ---- F18-02 replays ---------------------------------------------------

    #[test]
    fn issue_lite_keys_and_render_match_f18_02() {
        let fx = fixture(F18_02);
        let lite = unit(&fx, "IssueLiteSerializer");
        assert_eq!(str_list(&lite["render_keys"]), ISSUE_LITE_FIELDS);
        let render = &lite["render"];
        let row = IssueLiteRow {
            id: render["id"].as_str().expect("id is a string"),
            sequence_id: Some(
                render["sequence_id"]
                    .as_i64()
                    .expect("sequence_id is an int"),
            ),
            project_id: render["project_id"]
                .as_str()
                .expect("project_id is a string"),
        };
        let out = render_issue_lite(&row);
        assert_eq!(out_keys(&out), ISSUE_LITE_FIELDS);
        assert_eq!(Value::Object(out), render.clone());
    }

    #[test]
    fn workpad_render_matches_f18_02() {
        let fx = fixture(F18_02);
        let workpad = unit(&fx, "IssueWorkpadSerializer");
        assert_eq!(str_list(&workpad["render_keys"]), WORKPAD_READ_FIELDS);
        let render = &workpad["render"];
        let row = WorkpadRow {
            body: render["body"].as_str().expect("body is a string"),
            updated_at: render["updated_at"]
                .as_str()
                .expect("updated_at is a string"),
        };
        let out = render_workpad(&row);
        assert_eq!(out_keys(&out), WORKPAD_READ_FIELDS);
        assert_eq!(Value::Object(out), render.clone());
    }

    #[test]
    fn workpad_write_cases_match_f18_02() {
        let fx = fixture(F18_02);
        let workpad = unit(&fx, "IssueWorkpadSerializer");
        // patch_ok: `{"body": "new *workpad*"}` validates to `workpad`.
        let body = serde_json::json!({"body": "new *workpad*"});
        let validated = validate_workpad_write(&WorkpadWriteInput {
            body: &body,
            partial: true,
        })
        .expect("patch_ok is valid");
        let golden = &workpad["patch_ok"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            py_repr(&validated.workpad.expect("workpad is set")),
            golden["validated"]["workpad"]
                .as_str()
                .expect("golden carries workpad")
        );
        // patch_blank: `allow_blank` accepts `""`.
        let body = serde_json::json!({"body": ""});
        let validated = validate_workpad_write(&WorkpadWriteInput {
            body: &body,
            partial: true,
        })
        .expect("patch_blank is valid");
        let golden = &workpad["patch_blank"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            py_repr(&validated.workpad.expect("workpad is set")),
            golden["validated"]["workpad"]
                .as_str()
                .expect("golden carries workpad")
        );
        // patch_empty: absent `body` is omitted (`required=False`).
        let body = serde_json::json!({});
        let validated = validate_workpad_write(&WorkpadWriteInput {
            body: &body,
            partial: true,
        })
        .expect("patch_empty is valid");
        let golden = &workpad["patch_empty"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert!(validated.workpad.is_none());
        assert!(golden["validated"]
            .as_object()
            .expect("golden carries validated")
            .is_empty());
    }

    #[test]
    fn label_write_fields_match_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "LabelCreateUpdateSerializer");
        assert_eq!(str_list(&create["fields"]), LABEL_WRITE_FIELDS);
    }

    #[test]
    fn label_create_ok_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "LabelCreateUpdateSerializer");
        let body = serde_json::json!({"name": "ux", "color": "#ff0000", "description": "d"});
        let validated = validate_label_write(&label_input(&body)).expect("create_ok is valid");
        let golden = &create["create_ok"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        let validated_golden = golden["validated"]
            .as_object()
            .expect("golden carries validated");
        assert_eq!(validated_golden.len(), 3);
        assert_eq!(
            py_repr(&validated.name.expect("name is set")),
            validated_golden["name"]
                .as_str()
                .expect("golden carries name")
        );
        assert_eq!(
            py_repr(&validated.color.expect("color is set")),
            validated_golden["color"]
                .as_str()
                .expect("golden carries color")
        );
        assert_eq!(
            py_repr(&validated.description.expect("description is set")),
            validated_golden["description"]
                .as_str()
                .expect("golden carries description")
        );
        // Absent optionals are omitted (never defaulted).
        assert!(validated.external_source.is_none());
        assert!(validated.external_id.is_none());
        assert!(validated.parent.is_none());
        assert!(validated.sort_order.is_none());
    }

    #[test]
    fn label_missing_name_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "LabelCreateUpdateSerializer");
        let body = serde_json::json!({});
        let error = validate_label_write(&label_input(&body)).expect_err("name is required");
        let golden = &create["missing_name"];
        assert!(!golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            error.body().expect("wire body"),
            expected_field_body(&golden["errors"], "name")
        );
    }

    #[test]
    fn label_blank_name_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "LabelCreateUpdateSerializer");
        let body = serde_json::json!({"name": ""});
        let error = validate_label_write(&label_input(&body)).expect_err("blank name fails");
        let golden = &create["blank_name"];
        assert!(!golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            error.body().expect("wire body"),
            expected_field_body(&golden["errors"], "name")
        );
    }

    #[test]
    fn label_render_matches_f18_02() {
        let fx = fixture(F18_02);
        let label = unit(&fx, "LabelSerializer");
        assert_eq!(str_list(&label["render_keys"]), LABEL_READ_FIELDS);
        let render = &label["render"];
        // The fixture records `None` and floats as Python reprs.
        let opt = |key: &str| match render[key].as_str().expect("render values are strings") {
            "None" => None,
            text => Some(text),
        };
        assert_eq!(render["sort_order"].as_str(), Some("65535.0"));
        let row = LabelRow {
            id: render["id"].as_str().expect("id"),
            created_at: render["created_at"].as_str().expect("created_at"),
            updated_at: render["updated_at"].as_str().expect("updated_at"),
            deleted_at: opt("deleted_at"),
            name: render["name"].as_str().expect("name"),
            description: render["description"].as_str().expect("description"),
            color: render["color"].as_str().expect("color"),
            sort_order: render["sort_order"]
                .as_str()
                .expect("sort_order repr")
                .parse::<f64>()
                .expect("sort_order parses"),
            external_source: opt("external_source"),
            external_id: opt("external_id"),
            created_by: opt("created_by"),
            updated_by: opt("updated_by"),
            workspace: render["workspace"].as_str().expect("workspace"),
            project: opt("project"),
            parent: opt("parent"),
        };
        let out = render_label(&LabelRepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out_keys(&out), LABEL_READ_FIELDS);
        assert_eq!(out["id"], render["id"]);
        assert_eq!(out["created_at"], render["created_at"]);
        assert_eq!(out["updated_at"], render["updated_at"]);
        assert_eq!(out["deleted_at"], Value::Null);
        assert_eq!(out["name"], render["name"]);
        assert_eq!(out["description"], render["description"]);
        assert_eq!(out["color"], render["color"]);
        assert_eq!(out["sort_order"], serde_json::json!(65535.0f64));
        assert_eq!(out["sort_order"].to_string(), "65535.0");
        for key in [
            "external_source",
            "external_id",
            "created_by",
            "updated_by",
            "parent",
        ] {
            assert_eq!(
                render[key].as_str(),
                Some("None"),
                "{key} is None in the golden"
            );
            assert_eq!(out[key], Value::Null, "{key} renders null");
        }
        assert_eq!(out["workspace"], render["workspace"]);
        assert_eq!(out["project"], render["project"]);
    }

    // ---- F18-03 LabelLite replay ------------------------------------------

    #[test]
    fn label_lite_keys_and_render_match_f18_03() {
        let fx = fixture(F18_03);
        let lite = unit(&fx, "LabelLiteSerializer");
        assert_eq!(str_list(&lite["render_keys"]), LABEL_LITE_FIELDS);
        let render = &lite["render"];
        let row = LabelLiteRow {
            id: render["id"].as_str().expect("id is a string"),
            name: render["name"].as_str().expect("name is a string"),
            color: render["color"].as_str().expect("color is a string"),
        };
        let out = render_label_lite(&row);
        assert_eq!(out_keys(&out), LABEL_LITE_FIELDS);
        assert_eq!(Value::Object(out), render.clone());
    }

    // ---- DRF-source-derived write edges -----------------------------------

    #[test]
    fn workpad_type_matrix() {
        let check = |body: &Value| {
            validate_workpad_write(&WorkpadWriteInput {
                body,
                partial: true,
            })
        };
        // Numerics coerce via `str()`; bools and composites fail `invalid`.
        assert_eq!(
            check(&serde_json::json!({"body": 12}))
                .expect("int coerces")
                .workpad,
            Some("12".to_string())
        );
        assert_eq!(
            check(&serde_json::json!({"body": 1.5}))
                .expect("float coerces")
                .workpad,
            Some("1.5".to_string())
        );
        assert_eq!(
            check(&serde_json::json!({"body": null}))
                .expect_err("null fails")
                .body(),
            "{\"body\":[\"This field may not be null.\"]}"
        );
        for raw in ["true", "[1]", "{\"a\":1}"] {
            let body: Value =
                serde_json::from_str(&format!("{{\"body\":{raw}}}")).expect("test body parses");
            assert_eq!(
                check(&body).expect_err("composite fails").body(),
                "{\"body\":[\"Not a valid string.\"]}",
                "{raw} fails invalid"
            );
        }
    }

    #[test]
    fn workpad_trims_and_allows_blank() {
        let check = |body: &Value| {
            validate_workpad_write(&WorkpadWriteInput {
                body,
                partial: true,
            })
            .expect("valid")
            .workpad
        };
        assert_eq!(
            check(&serde_json::json!({"body": "  x  "})),
            Some("x".to_string())
        );
        // Whitespace-only validates as `""` (`allow_blank=True`).
        assert_eq!(
            check(&serde_json::json!({"body": "   "})),
            Some(String::new())
        );
        assert_eq!(
            check(&serde_json::json!({"body": "\t\n "})),
            Some(String::new())
        );
    }

    #[test]
    fn workpad_null_char_rejected() {
        let body = serde_json::json!({"body": "a\0b"});
        assert_eq!(
            validate_workpad_write(&WorkpadWriteInput {
                body: &body,
                partial: true
            })
            .expect_err("null byte fails")
            .body(),
            "{\"body\":[\"Null characters are not allowed.\"]}"
        );
    }

    #[test]
    fn workpad_ignores_unknown_keys() {
        let body = serde_json::json!({"workpad": "x", "updated_at": "y", "body": "b"});
        let validated = validate_workpad_write(&WorkpadWriteInput {
            body: &body,
            partial: true,
        })
        .expect("unknown keys are ignored");
        assert_eq!(validated.workpad, Some("b".to_string()));
    }

    #[test]
    fn non_dict_bodies_fail_before_field_validation() {
        for (raw, datatype) in [
            ("[1]", "list"),
            ("null", "NoneType"),
            ("\"s\"", "str"),
            ("1", "int"),
            ("1.5", "float"),
            ("true", "bool"),
        ] {
            let body: Value = serde_json::from_str(raw).expect("test body parses");
            let expected = format!(
                "{{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got {datatype}.\"]}}"
            );
            assert_eq!(
                validate_label_write(&label_input(&body))
                    .expect_err("non-dict label body fails")
                    .body(),
                Some(expected.as_str()),
                "{raw} on the label write"
            );
            // No lookup for non-object bodies (field validation never runs).
            assert_eq!(parent_lookup_key(&body), None);
        }
        let body: Value = serde_json::from_str("[1]").expect("test body parses");
        assert_eq!(
            validate_workpad_write(&WorkpadWriteInput {
                body: &body,
                partial: true
            })
            .expect_err("non-dict workpad body fails")
            .body(),
            "{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got list.\"]}"
        );
    }

    #[test]
    fn label_name_matrix() {
        let create = |body: &Value| validate_label_write(&label_input(body));
        // Missing on PATCH skips (no error); missing on create requires.
        let body = serde_json::json!({"color": "c"});
        let validated = validate_label_write(&LabelWriteInput {
            body: &body,
            partial: true,
            parent_exists: None,
        })
        .expect("partial skips missing name");
        assert!(validated.name.is_none());
        assert_eq!(
            create(&serde_json::json!({"body": null}))
                .expect_err("null body-adjacent key still misses name")
                .body(),
            Some("{\"name\":[\"This field is required.\"]}")
        );
        assert_eq!(
            create(&serde_json::json!({"name": null}))
                .expect_err("null fails")
                .body(),
            Some("{\"name\":[\"This field may not be null.\"]}")
        );
        assert_eq!(
            create(&serde_json::json!({"name": 5}))
                .expect("int coerces")
                .name,
            Some("5".to_string())
        );
        assert_eq!(
            create(&serde_json::json!({"name": true}))
                .expect_err("bool fails")
                .body(),
            Some("{\"name\":[\"Not a valid string.\"]}")
        );
        // Whitespace-only fails `blank` (the trim check runs on the raw value).
        assert_eq!(
            create(&serde_json::json!({"name": "   "}))
                .expect_err("spaces fail")
                .body(),
            Some("{\"name\":[\"This field may not be blank.\"]}")
        );
        assert_eq!(
            create(&serde_json::json!({"name": "  ux  "}))
                .expect("trims")
                .name,
            Some("ux".to_string())
        );
        // Length cap counts stripped code points; `max_length` wins over the
        // null-characters validator (append order in `CharField.__init__`).
        assert!(create(&serde_json::json!({"name": "x".repeat(255)}))
            .expect("255 chars pass")
            .name
            .is_some());
        assert_eq!(
            create(&serde_json::json!({"name": "x".repeat(256)}))
                .expect_err("256 chars fail")
                .body(),
            Some("{\"name\":[\"Ensure this field has no more than 255 characters.\"]}")
        );
        assert_eq!(
            create(&serde_json::json!({"name": format!("{}\0", "x".repeat(256))}))
                .expect_err("long + null byte fails max_length first")
                .body(),
            Some("{\"name\":[\"Ensure this field has no more than 255 characters.\"]}")
        );
        assert_eq!(
            create(&serde_json::json!({"name": "ok\0"}))
                .expect_err("null byte fails")
                .body(),
            Some("{\"name\":[\"Null characters are not allowed.\"]}")
        );
    }

    #[test]
    fn label_nullable_blankables() {
        let create = |body: &Value| validate_label_write(&label_input(body));
        // `color`: blank ok, null rejected.
        assert_eq!(
            create(&serde_json::json!({"name": "n", "color": ""}))
                .expect("blank color passes")
                .color,
            Some(String::new())
        );
        assert_eq!(
            create(&serde_json::json!({"name": "n", "color": null}))
                .expect_err("null color fails")
                .body(),
            Some("{\"color\":[\"This field may not be null.\"]}")
        );
        // `description` (`TextField`): no length cap at all.
        assert!(
            create(&serde_json::json!({"name": "n", "description": "x".repeat(10_000)}))
                .expect("long description passes")
                .description
                .is_some()
        );
        // `external_*`: null ok (validates `None`), blank ok, omitted ok.
        let validated = create(&serde_json::json!({
            "name": "n",
            "external_source": null,
            "external_id": "",
        }))
        .expect("externals pass");
        assert_eq!(validated.external_source, Some(None));
        assert_eq!(validated.external_id, Some(Some(String::new())));
        let validated = create(&serde_json::json!({"name": "n"})).expect("omitted pass");
        assert!(validated.external_source.is_none());
        assert!(validated.external_id.is_none());
    }

    #[test]
    fn label_parent_matrix() {
        let create = |body: &Value, parent_exists: Option<bool>| {
            validate_label_write(&LabelWriteInput {
                body,
                partial: false,
                parent_exists,
            })
        };
        let body_for = |raw: &str| -> Value {
            serde_json::from_str(&format!("{{\"name\":\"n\",\"parent\":{raw}}}"))
                .expect("test body parses")
        };
        // Absent / null / `""` need no lookup.
        for raw in [
            "{}",
            "{\"name\":\"n\",\"parent\":null}",
            "{\"name\":\"n\",\"parent\":\"\"}",
        ] {
            let body: Value = serde_json::from_str(raw).expect("test body parses");
            assert_eq!(parent_lookup_key(&body), None, "{raw} needs no lookup");
        }
        assert!(create(&serde_json::json!({"name": "n"}), None)
            .expect("absent parent passes")
            .parent
            .is_none());
        assert_eq!(
            create(&serde_json::json!({"name": "n", "parent": null}), None)
                .expect("null parent passes")
                .parent,
            Some(None)
        );
        assert_eq!(
            create(&serde_json::json!({"name": "n", "parent": ""}), None)
                .expect("empty parent passes as None")
                .parent,
            Some(None)
        );
        // Whitespace-only is NOT forced to `None` — it fails UUID parse.
        assert_eq!(
            create(&body_for("\"   \""), None)
                .expect_err("spaces fail")
                .body(),
            Some("{\"parent\":[\"\u{201c}   \u{201d} is not a valid UUID.\"]}")
        );
        // Bool reaches `incorrect_type` (the only JSON type that does).
        assert_eq!(
            create(&body_for("true"), None)
                .expect_err("bool fails")
                .body(),
            Some("{\"parent\":[\"Incorrect type. Expected pk value, received bool.\"]}")
        );
        // Non-UUID inputs surface Django's curly-quote message (probe vectors).
        for (raw, rendered) in [
            ("\"xyz\"", "xyz"),
            ("1.5", "1.5"),
            ("[1]", "[1]"),
            ("-1", "-1"),
        ] {
            let expected =
                format!("{{\"parent\":[\"\u{201c}{rendered}\u{201d} is not a valid UUID.\"]}}");
            assert_eq!(
                create(&body_for(raw), None)
                    .expect_err("bad uuid fails")
                    .body(),
                Some(expected.as_str()),
                "{raw} fails UUID parse"
            );
            assert_eq!(parent_lookup_key(&body_for(raw)), None);
        }
        // Objects: compact-JSON rendering (documented approximation — Python
        // `repr` would spell `{'a': 1}`); the inner quotes escape on the wire.
        assert_eq!(
            create(&body_for("{\"a\":1}"), None)
                .expect_err("object fails")
                .body(),
            Some("{\"parent\":[\"\u{201c}{\\\"a\\\":1}\u{201d} is not a valid UUID.\"]}")
        );
        // Valid UUIDs query: hit validates (canonical), miss echoes the RAW
        // input (uppercase preserved).
        let mixed = "D715BE3D-234F-46EF-89A3-97F0C7C04B7E";
        let body = body_for(&format!("\"{mixed}\""));
        assert_eq!(
            parent_lookup_key(&body),
            Some("d715be3d-234f-46ef-89a3-97f0c7c04b7e".to_string())
        );
        let expected_miss =
            format!("{{\"parent\":[\"Invalid pk \\\"{mixed}\\\" - object does not exist.\"]}}");
        assert_eq!(
            create(&body, Some(false)).expect_err("miss fails").body(),
            Some(expected_miss.as_str())
        );
        assert_eq!(
            create(&body, Some(true)).expect("hit passes").parent,
            Some(Some("d715be3d-234f-46ef-89a3-97f0c7c04b7e".to_string()))
        );
        // Small ints are valid UUID ints (queried, realistically missed).
        let body = body_for("5");
        assert_eq!(
            parent_lookup_key(&body),
            Some("00000000-0000-0000-0000-000000000005".to_string())
        );
        assert_eq!(
            create(&body, Some(false))
                .expect_err("int miss fails")
                .body(),
            Some("{\"parent\":[\"Invalid pk \\\"5\\\" - object does not exist.\"]}")
        );
        // Missing caller fact is loud (handler 500, never a wrong verdict).
        assert_eq!(
            create(&body_for(&format!("\"{PARENT_UUID}\"")), None)
                .expect_err("missing fact errors"),
            LabelWriteError::MissingParentFact
        );
        assert_eq!(LabelWriteError::MissingParentFact.body(), None);
    }

    #[test]
    fn label_uuid_spellings_match_cpython() {
        // Every vector below was live-probed against `uuid.UUID(hex=...)`.
        let canonical = |raw: &str| {
            let body: Value =
                serde_json::from_str(&format!("{{\"name\":\"n\",\"parent\":\"{raw}\"}}"))
                    .expect("test body parses");
            parent_lookup_key(&body)
        };
        assert_eq!(
            canonical("A8098C1A-F86E-11DA-BD1A-00112444BE1E"),
            Some("a8098c1a-f86e-11da-bd1a-00112444be1e".to_string())
        );
        assert_eq!(
            canonical("a8098c1af86e11dabd1a00112444be1e"),
            Some("a8098c1a-f86e-11da-bd1a-00112444be1e".to_string())
        );
        assert_eq!(
            canonical("{a8098c1a-f86e-11da-bd1a-00112444be1e}"),
            Some("a8098c1a-f86e-11da-bd1a-00112444be1e".to_string())
        );
        assert_eq!(
            canonical("{{a8098c1a-f86e-11da-bd1a-00112444be1e}}"),
            Some("a8098c1a-f86e-11da-bd1a-00112444be1e".to_string())
        );
        assert_eq!(
            canonical("urn:uuid:a8098c1a-f86e-11da-bd1a-00112444be1e"),
            Some("a8098c1a-f86e-11da-bd1a-00112444be1e".to_string())
        );
        // `int(s, 16)` leniency: padded with a space, `0x`, or `_` — all
        // zero-padded on the left by `uuid.UUID`.
        assert_eq!(
            canonical(" a8098c1af86e11dabd1a00112444be1"),
            Some("0a8098c1-af86-e11d-abd1-a00112444be1".to_string())
        );
        assert_eq!(
            canonical("0xa8098c1af86e11dabd1a00112444be"),
            Some("00a8098c-1af8-6e11-dabd-1a00112444be".to_string())
        );
        assert_eq!(
            canonical("a809_8c1af86e11dabd1a00112444be1"),
            Some("0a8098c1-af86-e11d-abd1-a00112444be1".to_string())
        );
        // Trailing padding breaks the 32-char gate; 33 hex is too long.
        assert_eq!(canonical("a8098c1af86e11dabd1a00112444be1e "), None);
        assert_eq!(canonical("a8098c1af86e11dabd1a00112444be1e0"), None);
    }

    #[test]
    fn label_sort_order_matrix() {
        let create = |body: &Value| validate_label_write(&label_input(body));
        let body_for = |raw: &str| -> Value {
            serde_json::from_str(&format!("{{\"name\":\"n\",\"sort_order\":{raw}}}"))
                .expect("test body parses")
        };
        assert_eq!(
            create(&body_for("\"12.5\""))
                .expect("numeric string passes")
                .sort_order,
            Some(12.5)
        );
        assert_eq!(
            create(&body_for("12")).expect("int passes").sort_order,
            Some(12.0)
        );
        // `FloatField` coerces bools (`float(True)`) instead of rejecting.
        assert_eq!(
            create(&body_for("true")).expect("bool passes").sort_order,
            Some(1.0)
        );
        assert_eq!(
            create(&body_for("false")).expect("bool passes").sort_order,
            Some(0.0)
        );
        for raw in ["\"abc\"", "\"\"", "[1]", "{\"a\":1}"] {
            assert_eq!(
                create(&body_for(raw))
                    .expect_err("non-numeric fails")
                    .body(),
                Some("{\"sort_order\":[\"A valid number is required.\"]}"),
                "{raw} fails invalid"
            );
        }
        assert_eq!(
            create(&body_for("null")).expect_err("null fails").body(),
            Some("{\"sort_order\":[\"This field may not be null.\"]}")
        );
        // 1000-code-point boundary: over fails before parsing, at passes.
        assert_eq!(
            create(&body_for(&format!("\"{}\"", "0".repeat(1001))))
                .expect_err("1001 chars fail")
                .body(),
            Some("{\"sort_order\":[\"String value too large.\"]}")
        );
        assert_eq!(
            create(&body_for(&format!("\"{}\"", "0".repeat(1000))))
                .expect("1000 chars pass")
                .sort_order,
            Some(0.0)
        );
        // `float()` vocabulary: underscores, padding, infinities, nan.
        assert_eq!(
            create(&body_for("\"1_0\""))
                .expect("underscores pass")
                .sort_order,
            Some(10.0)
        );
        assert_eq!(
            create(&body_for("\" 12.5 \""))
                .expect("padding passes")
                .sort_order,
            Some(12.5)
        );
        assert_eq!(
            create(&body_for("\"inf\"")).expect("inf passes").sort_order,
            Some(f64::INFINITY)
        );
        assert_eq!(
            create(&body_for("\"-INFINITY\""))
                .expect("-inf passes")
                .sort_order,
            Some(f64::NEG_INFINITY)
        );
        assert!(create(&body_for("\"nan\""))
            .expect("nan passes")
            .sort_order
            .expect("nan set")
            .is_nan());
        for raw in ["\"1__0\"", "\"1_\"", "\"_1\"", "\"1_e5\"", "\"1e_5\""] {
            assert!(
                create(&body_for(raw))
                    .expect_err("bad underscores fail")
                    .body()
                    == Some("{\"sort_order\":[\"A valid number is required.\"]}"),
                "{raw} fails invalid"
            );
        }
        // Absent is omitted (never the model default 65535).
        assert!(create(&serde_json::json!({"name": "n"}))
            .expect("absent passes")
            .sort_order
            .is_none());
    }

    #[test]
    fn label_errors_combine_in_field_order() {
        let body = serde_json::json!({
            "name": "",
            "color": "x".repeat(256),
            "description": true,
            "external_id": ["x"],
            "parent": "xyz",
            "sort_order": "abc",
        });
        assert_eq!(
            validate_label_write(&label_input(&body))
                .expect_err("combined errors")
                .body(),
            Some(
                "{\"name\":[\"This field may not be blank.\"],\
                 \"color\":[\"Ensure this field has no more than 255 characters.\"],\
                 \"description\":[\"Not a valid string.\"],\
                 \"external_id\":[\"Not a valid string.\"],\
                 \"parent\":[\"\u{201c}xyz\u{201d} is not a valid UUID.\"],\
                 \"sort_order\":[\"A valid number is required.\"]}"
            )
        );
    }

    #[test]
    fn label_partial_patch_and_full_create() {
        // PATCH: only the supplied key validates.
        let body = serde_json::json!({"color": "c"});
        let validated = validate_label_write(&LabelWriteInput {
            body: &body,
            partial: true,
            parent_exists: None,
        })
        .expect("partial passes");
        assert!(validated.name.is_none());
        assert_eq!(validated.color, Some("c".to_string()));
        // Full create: all seven keys validate together.
        let body = serde_json::json!({
            "name": "ux",
            "color": "#ff0000",
            "description": "d",
            "external_source": "s",
            "external_id": null,
            "parent": PARENT_UUID,
            "sort_order": "1.5",
        });
        let validated = validate_label_write(&LabelWriteInput {
            body: &body,
            partial: false,
            parent_exists: Some(true),
        })
        .expect("full create passes");
        assert_eq!(validated.name, Some("ux".to_string()));
        assert_eq!(validated.color, Some("#ff0000".to_string()));
        assert_eq!(validated.description, Some("d".to_string()));
        assert_eq!(validated.external_source, Some(Some("s".to_string())));
        assert_eq!(validated.external_id, Some(None));
        assert_eq!(validated.parent, Some(Some(PARENT_UUID.to_string())));
        assert_eq!(validated.sort_order, Some(1.5));
    }

    // ---- Read-shape edges ---------------------------------------------------

    #[test]
    fn issue_lite_sequence_none_drops_key() {
        // The `?expand=parent` label case: a `Label` has no `sequence_id`,
        // so DRF `SkipField` drops the key.
        let row = IssueLiteRow {
            id: "c833c492-fa3d-49f5-85a4-0ef810642731",
            sequence_id: None,
            project_id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
        };
        let out = render_issue_lite(&row);
        assert_eq!(out_keys(&out), ["id", "project_id"]);
        assert_eq!(
            out["id"],
            serde_json::json!("c833c492-fa3d-49f5-85a4-0ef810642731")
        );
        assert_eq!(
            out["project_id"],
            serde_json::json!("d715be3d-234f-46ef-89a3-97f0c7c04b7e")
        );
    }

    #[test]
    fn label_fields_filter_and_expand() {
        let row = label_row_fixture();
        let plain = |fields: Option<&[FieldSpec]>, expand: &[&str]| {
            render_label(&LabelRepresentationInput {
                row: &row,
                fields,
                expand,
                expansions: &[],
            })
            .expect("renders")
        };
        // No filter: all fifteen keys in wire order.
        assert_eq!(out_keys(&plain(None, &[])), LABEL_READ_FIELDS);
        // Subset keeps wire order and ignores unknowns.
        let specs = ["name", "id", "nope"]
            .iter()
            .map(|name| FieldSpec::Include(name.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(out_keys(&plain(Some(&specs), &[])), ["id", "name"]);
        // Nested `fields=` raises (shared-kernel parity).
        let nested = vec![FieldSpec::Nested("name".to_string(), vec![])];
        assert!(matches!(
            render_label(&LabelRepresentationInput {
                row: &row,
                fields: Some(&nested),
                expand: &[],
                expansions: &[],
            }),
            Err(LabelRenderError::Fields(_))
        ));
        // Unknown expands are skipped; scalar expands null; filtered-out
        // expands never consult the caller map (no `MissingExpansion`).
        let out = plain(None, &["nope"]);
        assert!(!out.contains_key("nope"));
        let out = plain(None, &["name"]);
        assert_eq!(out["name"], Value::Null);
        let out = plain(Some(&specs), &["project"]);
        assert!(!out.contains_key("project"));
        // Map hits render the caller value, or `{}` for a null FK.
        let expanded: Value = serde_json::json!({"id": "p", "name": "proj"});
        let out = render_label(&LabelRepresentationInput {
            row: &row,
            fields: None,
            expand: &["project"],
            expansions: &[("project", Some(expanded.clone()))],
        })
        .expect("renders");
        assert_eq!(out["project"], expanded);
        let out = render_label(&LabelRepresentationInput {
            row: &row,
            fields: None,
            expand: &["parent"],
            expansions: &[("parent", None)],
        })
        .expect("renders");
        assert_eq!(out["parent"], Value::Object(Map::new()));
        // A map hit with no caller value is loud.
        assert_eq!(
            render_label(&LabelRepresentationInput {
                row: &row,
                fields: None,
                expand: &["project"],
                expansions: &[],
            }),
            Err(LabelRenderError::MissingExpansion("project".to_string()))
        );
    }

    #[test]
    fn label_render_null_project_and_deleted() {
        let row = LabelRow {
            project: None,
            deleted_at: Some("2026-10-03T00:00:00Z"),
            ..label_row_fixture()
        };
        let out = render_label(&LabelRepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("renders");
        assert_eq!(out["project"], Value::Null);
        assert_eq!(out["deleted_at"], serde_json::json!("2026-10-03T00:00:00Z"));
    }

    #[test]
    fn label_sort_order_nonfinite_errors() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let row = LabelRow {
                sort_order: value,
                ..label_row_fixture()
            };
            assert_eq!(
                render_label(&LabelRepresentationInput {
                    row: &row,
                    fields: None,
                    expand: &[],
                    expansions: &[],
                }),
                Err(LabelRenderError::NonFiniteFloat)
            );
        }
    }

    #[test]
    fn parent_lookup_key_contract() {
        // UUID inputs (any accepted spelling) yield the canonical key.
        let body = serde_json::json!({"parent": "D715BE3D-234F-46EF-89A3-97F0C7C04B7E"});
        assert_eq!(
            parent_lookup_key(&body),
            Some("d715be3d-234f-46ef-89a3-97f0c7c04b7e".to_string())
        );
        // Everything else yields no lookup.
        for raw in [
            "{}",
            "{\"parent\":null}",
            "{\"parent\":\"\"}",
            "{\"parent\":\"xyz\"}",
            "{\"parent\":true}",
            "[1]",
            "null",
        ] {
            let body: Value = serde_json::from_str(raw).expect("test body parses");
            assert_eq!(parent_lookup_key(&body), None, "{raw} needs no lookup");
        }
    }
}
