#![forbid(unsafe_code)]

//! Comment/attachment/activity shapes (D-18 serializers E, PIDASHCONV-664).
//!
//! Ports `apps/api/pi_dash/api/serializers/issue.py:907-927`
//! (`IssueAttachmentSerializer`), `:928-964`
//! (`IssueCommentCreateSerializer`), `:965-1019` (`IssueCommentSerializer`,
//! incl. `get_url` `:994-1000`, `to_representation` `:1001-1007`, `validate`
//! `:1008-1019`), `:1020-1032` (`IssueActivitySerializer`) and `:1119-1138`
//! (`IssueAttachmentUploadSerializer`).
//!
//! Fixture: the F18-03 comment/attachment/activity subset
//! (`rust-api/fixtures/v1_work_items/serializers/
//! F18-03.comment_attachment_activity_expand_search.golden.json`). Every
//! `#[test]` below replays it: golden in/out byte-identical, including
//! validation error strings.
//!
//! This module is pure: the HTML-parser verdict in Python takes the
//! already-resolved fact as an argument (the [`CommentHtmlInput::roundtripped`]
//! seam, same pattern as the merged `shape_issue` lxml seam). The handler
//! layer (PIDASHCONV-674/675) supplies that fact and performs the writes; the
//! error bodies, key orders and check order here are the contract it must
//! honor.
//!
//! Reused, not forked: [`filter_fields`] (the shared `?fields=` kernel),
//! [`field_errors_body`] (combined 400 bodies — the `labels` list detail and
//! index-dict are built as `Value`s so they combine in field order; the
//! single-body `list_index_errors_body`/`not_a_list_body` spellings are
//! asserted byte-identical in the tests), [`INVALID_HTML_BODY`] (the comment
//! `validate()` body — byte-identical to the issue one),
//! [`BASE_EXPANSION_NAMES`] (the `base.py:91-106` expansion map keys) and
//! `super::shape_issue::{issue_url, web_base_url}` (comment `get_url`,
//! exercised in the tests) from the sibling `shape_issue` module.
//!
//! Write-path reachability (all verified against `api/views/issue.py`):
//!
//! * `IssueCommentCreateSerializer` — comment POST (full, `:1916`) and PATCH
//!   (`partial=True`, `:2063`): [`validate_comment_create`]. Its read shape
//!   matters too: POST `:1930` embeds `serializer.data` in the activity
//!   payload — [`render_comment_create`].
//! * `IssueAttachmentUploadSerializer` — docs-only (`OpenApiRequest`, `:2249`);
//!   the upload POST (`:2297+`) reads `request.data` directly with custom
//!   checks. Still ported ([`validate_upload_write`]) — the fixture pins it.
//! * `IssueCommentSerializer`, `IssueAttachmentSerializer`,
//!   `IssueActivitySerializer` have no reachable write path in api-v1 — no
//!   view constructs them with `data=` — so only their read shapes plus the
//!   comment `get_url`/`to_representation`/`validate` are ported. (The `app/`
//!   family constructs the full comment/attachment serializers with `data=`
//!   — `app/views/issue/comment.py:90,122`, `app/views/issue/attachment.py:38`
//!   — but those views belong to another domain's scope.)
//! * `create()`/`update()` on both write shapes are DRF's defaults (no
//!   overrides): `validated_data` is applied verbatim, plus the view-supplied
//!   `project_id`/`issue_id`/`actor` kwargs on comment create and the
//!   view's `created_at`/`created_by`/`actor_id` overrides (`:1919-1924`).
//!   The 409 external-id checks and `IssueComment.save()` description
//!   handling are handler/model scope (PIDASHCONV-674).
//!
//! Ported quirks (translate, don't redesign — all verified against the
//! Python/DRF sources, the pinned Django 4.2.30, or live probes against the
//! installed DRF 3.16.1 — pinned 3.15.2; the classes used here are unchanged
//! between them):
//!
//! * `is_member` is a `BooleanField(read_only=True)` fed by a queryset
//!   annotation (`views/issue.py:1817,1973`). `read_only` forces
//!   `required=False`, so on un-annotated rows `getattr` raises
//!   `AttributeError` and DRF `SkipField` drops the key (`fields.py`
//!   `Field.get_attribute`): list/detail GETs carry it (2nd key), create/update
//!   responses do not. Hence [`CommentRow::is_member`] is `Option`.
//! * `IssueCommentCreateSerializer.Meta.read_only_fields` names only fields
//!   NOT in `Meta.fields`, so DRF ignores it and all nine fields are
//!   writable (same class as the 661 label quirk).
//! * `?expand=url` removes `url` (the else branch nulls it, then the url-pop
//!   drops it); `?expand=<scalar>` nulls that scalar; FK fields outside the
//!   expansion map (`comment.description`, attachment
//!   `comment`/`page`/`draft_issue`) re-emit their id through the
//!   `getattr(instance, f"{expand}_id", None)` passthrough (`base.py:114-116`,
//!   an observable no-op).
//! * `?expand=parent` on a comment feeds the parent *comment* to the *issue*
//!   lite (`base.py:104` maps `parent` there): expansions arrive pre-rendered
//!   from the caller, same as every other map hit.
//! * `asset` renders as the stored key: `S3Storage.url()` returns the bare
//!   name (`settings/storage.py:20-21`), and no call site passes request
//!   context (which would absolutize it).
//! * DRF `UUIDField` accepts JSON bools (`isinstance(True, int)` → `UUID(int=1)`).
//! * DRF `IntegerField` accepts `"12.0"`/`123.0` (the `re_decimal` strip),
//!   underscores between digits, `+`/`-`, and surrounding int-whitespace;
//!   `None` fails `null`, over-1000-code-point strings fail
//!   `max_string_length`.
//! * A JSON-null request body fails `{"non_field_errors": ["No data provided"]}`
//!   (`serializers.py` errors-property edge case), not the invalid-dict message.
//!
//! Documented approximations (all outside any golden; same class as the merged
//! siblings):
//!
//! * Integers past `i64` fail `invalid` (Python keeps them exact); `size`
//!   values past it are unreachable in practice (the view clamps to the 5 MB
//!   limit downstream).
//! * `int()` inputs with non-ASCII decimal digits (e.g. fullwidth `１２`, which
//!   CPython accepts) fail `invalid`; ASCII digits, signs, underscores and
//!   int-whitespace are exact.
//! * `CharField` numeric coercion and the `invalid_choice` input rendering
//!   use Python `str()` spelling (`super::python_number_str`); integers
//!   beyond `u64` keep their digits in the unified workspace build and
//!   demote to `f64` only when `serde_json` lacks `arbitrary_precision`
//!   (standalone services builds). Strings, bools and `None` are exact.
//! * `invalid_choice` messages for exotic composite inputs escape only
//!   `Cc` controls for the non-ASCII range (Python also escapes `Zl`/`Zp` and
//!   other non-printables); ASCII and printable text are exact.
//! * `CharField` surrogate validation is skipped: `serde_json` rejects lone
//!   surrogates at parse, so the validator is unreachable on JSON input.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde_json::{Map, Value};

use super::shape_issue::{field_errors_body, BASE_EXPANSION_NAMES, INVALID_HTML_BODY};
use super::{filter_fields, python_number_str, FieldSpec, FilterError};

/// `IssueAttachmentSerializer` read order (`issue.py:907-927`,
/// `fields="__all__"`): declared `id`, then plain model fields in model
/// order, then forward relations in model order (DRF
/// `get_default_field_names`), pinned by the F18-03 `render_keys`.
pub const ATTACHMENT_READ_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "attributes",
    "asset",
    "entity_type",
    "entity_identifier",
    "is_deleted",
    "is_archived",
    "external_id",
    "external_source",
    "size",
    "is_uploaded",
    "storage_metadata",
    "created_by",
    "updated_by",
    "user",
    "workspace",
    "draft_issue",
    "project",
    "issue",
    "comment",
    "page",
];

/// `IssueCommentCreateSerializer.Meta.fields` (`issue.py:938-948`), in field
/// (and therefore error-combination and `validated_data`) order.
pub const COMMENT_CREATE_FIELDS: &[&str] = &[
    "comment_json",
    "comment_html",
    "access",
    "external_source",
    "external_id",
    "labels",
    "speaker_type",
    "speaker_label",
    "speaker_agent_run_id",
];

/// `IssueCommentSerializer` read order (`issue.py:965-993`): declared `id`,
/// `is_member`, `url`, then plain model fields minus the two `exclude`d
/// (`comment_stripped`, `comment_json`), then forward relations — pinned by
/// the F18-03 `render_keys` plus the `is_member` slot (absent there: the probe
/// row carried no annotation, so DRF `SkipField` dropped it).
pub const COMMENT_READ_FIELDS: &[&str] = &[
    "id",
    "is_member",
    "url",
    "created_at",
    "updated_at",
    "deleted_at",
    "comment_html",
    "attachments",
    "labels",
    "access",
    "external_source",
    "external_id",
    "speaker_type",
    "speaker_label",
    "speaker_agent_run_id",
    "edited_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "description",
    "issue",
    "actor",
    "parent",
];

/// `IssueActivitySerializer` read order (`issue.py:1020-1031`,
/// `exclude=["created_by", "updated_by"]`): declared `id`, plain model
/// fields, forward relations minus the two excluded — pinned by the F18-03
/// `render_keys`.
pub const ACTIVITY_READ_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "verb",
    "field",
    "old_value",
    "new_value",
    "comment",
    "attachments",
    "old_identifier",
    "new_identifier",
    "epoch",
    "project",
    "workspace",
    "issue",
    "issue_comment",
    "actor",
];

/// `IssueAttachmentUploadSerializer` fields (`issue.py:1127-1136`), in
/// declaration (and therefore error-combination) order.
pub const UPLOAD_FIELDS: &[&str] = &["name", "type", "size", "external_id", "external_source"];

/// `IssueComment.access` choices (`db/models/issue.py:575-579`).
pub const ACCESS_CHOICES: &[&str] = &["INTERNAL", "EXTERNAL"];

/// `IssueComment.speaker_type` choices (`SpeakerType`, `:551-555`).
pub const SPEAKER_TYPE_CHOICES: &[&str] = &["human", "agent", "system", "integration"];

/// `external_source`/`external_id` column limit (`db/models/issue.py:580-581`,
/// `max_length=255`).
pub const MAX_EXTERNAL_CHARS: usize = 255;

/// `speaker_label` column limit (`:589`, `max_length=128`).
pub const MAX_SPEAKER_LABEL_CHARS: usize = 128;

/// `labels` item limit (the `ArrayField` base `CharField(max_length=32)`,
/// `:566`).
pub const MAX_LABEL_ITEM_CHARS: usize = 32;

/// `IntegerField` over-long-string guard (`fields.py`, shared with
/// `FloatField`): code points, checked before parsing.
pub const MAX_INTEGER_STRING_CHARS: usize = 1000;

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
/// Non-integer `size` (DRF `IntegerField.invalid`).
pub const MSG_INVALID_INTEGER: &str = "A valid integer is required.";
/// Over-1000-code-point string to `IntegerField` (DRF `max_string_length`).
pub const MSG_STRING_TOO_LARGE: &str = "String value too large.";
/// Unparseable `speaker_agent_run_id` (DRF `UUIDField.invalid` — no
/// interpolation, byte-identical for every input).
pub const MSG_INVALID_UUID: &str = "Must be a valid UUID.";
/// JSON-null request body (DRF `serializers.py` errors-property edge case).
pub const MSG_NO_DATA: &str = "No data provided";

/// `max_length` failure for a text field (DRF `CharField.max_length`,
/// checked on the stripped value before the null-characters validator).
fn max_length_message(max_chars: usize) -> String {
    format!("Ensure this field has no more than {max_chars} characters.")
}

/// Bad `access`/`speaker_type` value (DRF `ChoiceField.invalid_choice`):
/// `"{input}" is not a valid choice.` with `input` the Python `str()` of the
/// raw input (see [`py_str`]).
fn invalid_choice_message(py_input: &str) -> String {
    format!("\"{py_input}\" is not a valid choice.")
}

/// Python `str.strip()` membership (live-probed): Rust `is_whitespace` plus
/// `\x1c`-`\x1f` (which Python strips but `White_Space` omits). The `re`
/// `\s` class used by `re_decimal` matches the same set.
fn py_is_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// Python `str.strip()` (no-arg): strip [`py_is_space`] from both ends.
fn py_strip(text: &str) -> &str {
    text.trim_matches(py_is_space)
}

/// Render a parsed-JSON value the way Python `str(data)` would, for the
/// `invalid_choice` message. Strings verbatim; bools `True`/`False`; `None`;
/// numbers via [`super::python_number_str`]; arrays/objects recurse with
/// Python `repr` separators (`, `, `: `) and [`py_repr_str`] quoting.
fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => python_number_str(number),
        Value::Null => "None".to_owned(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(obj) => {
            let inner: Vec<String> = obj
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_str(key), py_str(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a parsed-JSON value (the container-item rule: strings
/// quoted, everything else like [`py_str`]).
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(text) => py_repr_str(text),
        other => py_str(other),
    }
}

/// Python `repr()` of a string: single quotes unless the value contains `'`
/// but not `"` (then double quotes); backslash and the active quote escaped;
/// `\n`/`\r`/`\t` short escapes; other `Cc` controls as `\xhh`; printable
/// text (including non-ASCII) verbatim. `Zl`/`Zp`/format-char escaping is the
/// documented approximation.
fn py_repr_str(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let mut out = String::with_capacity(text.len() + 2);
    out.push(if use_double { '"' } else { '\'' });
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' if !use_double => out.push_str("\\'"),
            '"' if use_double => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let code = c as u32;
                if code <= 0xff {
                    out.push_str(&format!("\\x{code:02x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(if use_double { '"' } else { '\'' });
    out
}

/// The PYTHON type name of a parsed-JSON value (`type(data).__name__`), for
/// the non-object-body and non-list messages.
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
/// `serializers.py:342,481`): field validation never runs. A JSON null takes
/// the `No data provided` edge (`serializers.py:582`).
fn non_dict_body(body: &Value) -> String {
    let message = if body.is_null() {
        MSG_NO_DATA.to_owned()
    } else {
        format!(
            "Invalid data. Expected a dictionary, but got {}.",
            json_type_name(body)
        )
    };
    field_errors_body(&[(
        "non_field_errors",
        Value::Array(vec![Value::String(message)]),
    )])
}

/// Outcome of [`validate_char`] for one text input.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CharOutcome {
    /// Key absent and not required: omitted from
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
/// `Field.validate_empty_values`): absent → required/skip (`partial` is moot —
/// the one required field, upload `name`, has no partial path), null →
/// null/None, blank check on the raw value, bool/composite rejection,
/// numeric coercion via `str()`, Python-whitespace trim, `max_length`,
/// null-bytes. `value=None` is the absent key (`Field.get_value` returns
/// `empty`).
fn validate_char(
    value: Option<&Value>,
    required: bool,
    allow_null: bool,
    allow_blank: bool,
    max_chars: Option<usize>,
) -> Result<CharOutcome, CharError> {
    let Some(value) = value else {
        if !required {
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
        Value::Number(number) => python_number_str(number),
        _ => return Err(CharError::InvalidType),
    };
    // The blank check runs on the raw value: `data == '' or
    // str(data).strip() == ''` — whitespace-only fails unless blank is
    // allowed, in which case the field validates as `""`.
    if raw.is_empty() || py_strip(&raw).is_empty() {
        if allow_blank {
            return Ok(CharOutcome::Text(String::new()));
        }
        return Err(CharError::Blank);
    }
    let stripped = py_strip(&raw).to_owned();
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

/// Outcome of [`validate_choice`] for one input.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ChoiceOutcome {
    /// Key absent and not required: omitted from
    /// `validated_data`.
    Skip,
    /// The matched choice value.
    Choice(String),
}

/// Every failure DRF `ChoiceField` validation can produce, in check order.
/// Both fields here use `allow_blank=False`, so `""` falls through to
/// `invalid_choice` (`'"" is not a valid choice.'`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum ChoiceError {
    /// Explicit JSON null (neither choice field allows null).
    Null,
    /// Value outside the choice set; carries the Python `str()` of the input.
    InvalidChoice(String),
}

impl ChoiceError {
    fn message(&self) -> String {
        match self {
            ChoiceError::Null => MSG_NULL.to_owned(),
            ChoiceError::InvalidChoice(py_input) => invalid_choice_message(py_input),
        }
    }
}

/// Port of DRF `ChoiceField` validation (`fields.py:ChoiceField` +
/// `Field.validate_empty_values`): absent → skip (neither choice field is
/// required, so `partial` is moot), null → null failure, else the
/// `str(input)` lookup in `choice_strings_to_values` (keys and values
/// coincide for both fields here). `value=None` is the absent key.
fn validate_choice(value: Option<&Value>, choices: &[&str]) -> Result<ChoiceOutcome, ChoiceError> {
    let Some(value) = value else {
        return Ok(ChoiceOutcome::Skip);
    };
    if value.is_null() {
        return Err(ChoiceError::Null);
    }
    // `choice_strings_to_values[str(data)]`: numbers/bools stringify, then
    // miss (no numeric choice exists); composites stringify to their `str()`.
    let key = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => python_number_str(number),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Array(_) | Value::Object(_) => py_str(value),
        Value::Null => unreachable!("null returns above"),
    };
    if choices.contains(&key.as_str()) {
        return Ok(ChoiceOutcome::Choice(key));
    }
    Err(ChoiceError::InvalidChoice(key))
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
/// live-probed): surrounding int-whitespace, one leading `+`, one leading
/// `0x`/`0X`, and single `_` between hex digits are all accepted (`-` is
/// impossible — eaten as a dash before this step). Values always fit `u128`
/// (at most 32 hex digits by the length gate).
fn parse_hex_int(core: &str) -> Option<u128> {
    // `int()` strips the Rust-`trim()` set (probed over `\x1c`-`\x1f`,
    // `\x85`, `\xa0`, `\u180e`, `\u2000`-`\u2003`, `\u2009`, `\u3000`,
    // `\ufeff` — all agree).
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

/// Parse a UUID string exactly like `uuid.UUID(hex=value)` (CPython `uuid.py`,
/// every branch live-probed): replace-all `urn:`/`uuid:` (case-sensitive),
/// strip `{}` ends, drop `-`, require exactly 32 code points, then
/// [`parse_hex_int`]. Returns the canonical lowercase hyphenated form.
fn parse_uuid_hex(raw: &str) -> Option<String> {
    let no_scheme = raw.replace("urn:", "").replace("uuid:", "");
    let stripped = no_scheme.trim_matches(|c| c == '{' || c == '}');
    let compact: String = stripped.chars().filter(|c| *c != '-').collect();
    if compact.chars().count() != 32 {
        return None;
    }
    parse_hex_int(&compact).map(format_canonical_uuid)
}

/// Outcome of [`validate_uuid`] for one input.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UuidOutcome {
    /// Key absent and not required: omitted from
    /// `validated_data`.
    Skip,
    /// Explicit JSON null on an `allow_null` field.
    Null,
    /// The canonical lowercase hyphenated UUID.
    Uuid(String),
}

/// Every failure DRF `UUIDField` validation can produce, in check order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UuidError {
    /// Explicit JSON null where `allow_null=False`.
    Null,
    /// Unparseable input (no interpolation — one message for all).
    Invalid,
}

impl UuidError {
    fn message(&self) -> String {
        match self {
            UuidError::Null => MSG_NULL.to_owned(),
            UuidError::Invalid => MSG_INVALID_UUID.to_owned(),
        }
    }
}

/// Port of DRF `UUIDField` validation (`fields.py:UUIDField` +
/// `Field.validate_empty_values`): absent → skip (`speaker_agent_run_id` is
/// optional, so `partial` is moot), null → null/None, then
/// `uuid.UUID(int=data)` for JSON ints — bools included
/// (`isinstance(True, int)`), `uuid.UUID(hex=data)` for strings, `invalid`
/// for floats and composites. `value=None` is the absent key.
fn validate_uuid(value: Option<&Value>, allow_null: bool) -> Result<UuidOutcome, UuidError> {
    let Some(value) = value else {
        return Ok(UuidOutcome::Skip);
    };
    if value.is_null() {
        if allow_null {
            return Ok(UuidOutcome::Null);
        }
        return Err(UuidError::Null);
    }
    match value {
        // `uuid.UUID(int=data)`: 0..2^128-1; negatives and past-the-top fail.
        // JSON ints past `u64` arrive as `f64` (documented approximation).
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int >= 0 {
                    return Ok(UuidOutcome::Uuid(format_canonical_uuid(int as u128)));
                }
            } else if let Some(int) = number.as_u64() {
                return Ok(UuidOutcome::Uuid(format_canonical_uuid(int as u128)));
            }
            Err(UuidError::Invalid)
        }
        // The bool quirk: `isinstance(True, int)` takes the int arm.
        Value::Bool(bit) => Ok(UuidOutcome::Uuid(format_canonical_uuid(u128::from(
            u8::from(*bit),
        )))),
        Value::String(text) => parse_uuid_hex(text)
            .map(UuidOutcome::Uuid)
            .ok_or(UuidError::Invalid),
        Value::Array(_) | Value::Object(_) => Err(UuidError::Invalid),
        // Unreachable (null returns above); `Null` keeps the arm honest.
        Value::Null => Err(UuidError::Null),
    }
}

/// Outcome of [`validate_json`] for one input.
#[derive(Debug, Clone, PartialEq)]
enum JsonOutcome {
    /// Key absent and not required: omitted from
    /// `validated_data`.
    Skip,
    /// The value, passed through verbatim.
    Value(Value),
}

/// The one failure DRF `JSONField` validation can produce here — null.
/// `to_internal_value` round-trips `json.dumps` over already-parsed JSON,
/// which cannot fail (`comment_json` is not `binary`, and the
/// `is_json_string` arm needs HTML-form input); absence skips (`comment_json`
/// is optional, so `partial` is moot).
#[derive(Debug, Clone, PartialEq, Eq)]
enum JsonError {
    /// Explicit JSON null (`allow_null=False`).
    Null,
}

impl JsonError {
    fn message(&self) -> String {
        match self {
            JsonError::Null => MSG_NULL.to_owned(),
        }
    }
}

/// Port of DRF `JSONField` validation for `comment_json`
/// (`fields.py:JSONField` + `Field.validate_empty_values`): absent → skip,
/// null → null failure, anything else passes through verbatim.
/// `value=None` is the absent key.
fn validate_json(value: Option<&Value>) -> Result<JsonOutcome, JsonError> {
    let Some(value) = value else {
        return Ok(JsonOutcome::Skip);
    };
    if value.is_null() {
        return Err(JsonError::Null);
    }
    Ok(JsonOutcome::Value(value.clone()))
}

/// Strip DRF `IntegerField.re_decimal` (`\.0*\s*$`): the match — if any — always
/// starts at the LAST dot (an earlier dot's tail would contain that dot, which
/// `0*`/`\s*` cannot span), so cut there iff the tail is zeros THEN whitespace
/// (`"1. 0"` does NOT match — the space precedes the zero). Slice math is safe
/// (`.` is one ASCII byte).
fn strip_decimal_suffix(text: &str) -> &str {
    if let Some(dot) = text.rfind('.') {
        let tail = &text[dot + 1..];
        if tail.trim_start_matches('0').chars().all(py_is_space) {
            return &text[..dot];
        }
    }
    text
}

/// CPython `int(text, 10)` acceptance over ASCII digits (live-probed):
/// int-whitespace stripped (the Rust-`trim()` set), one optional sign, then
/// ASCII digits with single `_` between digits only. Values must fit `i64`
/// (past it → `None`, the documented approximation); non-ASCII decimal
/// digits → `None` (documented approximation).
fn parse_py_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (unsigned, negative) = match trimmed.strip_prefix('+') {
        Some(rest) => (rest, false),
        None => match trimmed.strip_prefix('-') {
            Some(rest) => (rest, true),
            None => (trimmed, false),
        },
    };
    if unsigned.is_empty() {
        return None;
    }
    // Accumulate towards the sign so `-9223372036854775808` (`i64::MIN`,
    // whose magnitude overflows `i64`) parses — the checked ops bound the
    // range exactly.
    let mut value: i64 = 0;
    // `int()` requires digits around every underscore.
    let mut prev_underscore = true;
    let mut seen_digit = false;
    for c in unsigned.chars() {
        if c == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if c.is_ascii_digit() {
            let digit = (c as u8 - b'0') as i64;
            value = value.checked_mul(10)?;
            value = if negative {
                value.checked_sub(digit)?
            } else {
                value.checked_add(digit)?
            };
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

/// Every failure DRF `IntegerField` validation can produce, in check order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum IntegerError {
    /// Key absent (`size` is always required; the upload has no partial path).
    Required,
    /// Explicit JSON null.
    Null,
    /// Over-1000-code-point string input (strings only).
    TooLarge,
    /// Anything `int()` rejects.
    Invalid,
}

impl IntegerError {
    fn message(&self) -> String {
        match self {
            IntegerError::Required => MSG_REQUIRED.to_owned(),
            IntegerError::Null => MSG_NULL.to_owned(),
            IntegerError::TooLarge => MSG_STRING_TOO_LARGE.to_owned(),
            IntegerError::Invalid => MSG_INVALID_INTEGER.to_owned(),
        }
    }
}

/// Port of DRF `IntegerField` validation (`fields.py:IntegerField` +
/// `Field.validate_empty_values`): absent → required, null → null failure,
/// over-long strings → `max_string_length`, else
/// `int(re_decimal.sub('', str(data)))` — strings verbatim, numbers via
/// `str()` (so `123.0` validates as `123`), bools as `True`/`False` (which
/// `int()` rejects), composites always rejected (their `str()` never parses).
/// `value=None` is the absent key.
fn validate_integer(value: Option<&Value>) -> Result<i64, IntegerError> {
    let Some(value) = value else {
        return Err(IntegerError::Required);
    };
    if value.is_null() {
        return Err(IntegerError::Null);
    }
    match value {
        Value::String(text) => {
            if text.chars().count() > MAX_INTEGER_STRING_CHARS {
                return Err(IntegerError::TooLarge);
            }
            parse_py_int(strip_decimal_suffix(text)).ok_or(IntegerError::Invalid)
        }
        Value::Number(number) => {
            // `str()` sees the VALUE's layout, not the literal's: render via
            // the shared helper first (`1e3` → `1000.0` → valid;
            // `10000000000000000.0` → `1e+16` → invalid).
            parse_py_int(strip_decimal_suffix(&python_number_str(number)))
                .ok_or(IntegerError::Invalid)
        }
        // `str(True)`/`str([...])` never parse as int — invalid without
        // building the string.
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => Err(IntegerError::Invalid),
        Value::Null => Err(IntegerError::Null),
    }
}

/// Outcome of [`validate_labels`] for one input.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LabelsOutcome {
    /// Key absent (never required): omitted from `validated_data`.
    Skip,
    /// The stripped items.
    List(Vec<String>),
}

/// Every failure `labels` (`ListField`, the `ArrayField` mapping) validation
/// can produce, in check order. The `ArrayField(size=8)` cap is NOT mapped —
/// DRF ignores it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LabelsError {
    /// Explicit JSON null.
    Null,
    /// Non-list input; carries the Python type name.
    NotAList(&'static str),
    /// Per-index child failures, in enumeration order.
    Items(Vec<(usize, Vec<String>)>),
}

/// Port of DRF `ListField` validation for `labels` (the `ArrayField` mapping,
/// `serializers.py:939,1303` + `fields.py:ListField` + child `CharField` with
/// the base field's `max_length=32`, no blank, no null): absent → skip, null
/// → null failure, non-list → `not_a_list`, else per-item child validation
/// collecting `{index: [messages]}` (`run_child_validation`). `value=None` is
/// the absent key.
fn validate_labels(value: Option<&Value>) -> Result<LabelsOutcome, LabelsError> {
    let Some(value) = value else {
        // Never required (`blank=True, default=list`).
        return Ok(LabelsOutcome::Skip);
    };
    if value.is_null() {
        return Err(LabelsError::Null);
    }
    let Value::Array(items) = value else {
        return Err(LabelsError::NotAList(json_type_name(value)));
    };
    // `allow_empty` defaults True — `[]` validates to `[]`.
    let mut validated = Vec::with_capacity(items.len());
    let mut failures: Vec<(usize, Vec<String>)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        match validate_char(Some(item), false, false, false, Some(MAX_LABEL_ITEM_CHARS)) {
            Ok(CharOutcome::Text(text)) => validated.push(text),
            // Present non-null items yield `Text` or `Err` only.
            Ok(_) => unreachable!("labels child validated a present item to Skip/Null"),
            Err(error) => failures.push((idx, vec![error.message()])),
        }
    }
    if !failures.is_empty() {
        return Err(LabelsError::Items(failures));
    }
    Ok(LabelsOutcome::List(validated))
}

/// Shape one field failure for [`field_errors_body`].
fn field_entry(field: &'static str, message: String) -> (&'static str, Value) {
    (field, Value::Array(vec![Value::String(message)]))
}

/// `IssueAttachmentUploadSerializer(data)` input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadWriteInput<'a> {
    /// The request body as parsed JSON.
    pub body: &'a Value,
}

/// Validated upload write (`validated_data` shape): `None` = key omitted
/// (absent input — no field here carries a default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedUpload {
    pub name: String,
    pub type_: Option<String>,
    pub size: i64,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
}

/// Every failure [`validate_upload_write`] can produce. Both arms carry the
/// byte-exact 400 body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UploadWriteError {
    /// Non-object body.
    #[error("{0}")]
    NotADict(String),
    /// Combined field errors in [`UPLOAD_FIELDS`] order.
    #[error("{0}")]
    Fields(String),
}

impl UploadWriteError {
    /// HTTP status Django answers with: always 400 on this serializer.
    pub fn status(&self) -> u16 {
        400
    }

    /// Byte-exact response body.
    pub fn body(&self) -> &str {
        match self {
            UploadWriteError::NotADict(body) | UploadWriteError::Fields(body) => body,
        }
    }
}

/// Port of `IssueAttachmentUploadSerializer` field validation
/// (`serializers/issue.py:1119-1136` over DRF `Serializer`): every field runs
/// even after failures, errors combine in declaration order, unknown input
/// keys are silently ignored.
pub fn validate_upload_write(
    input: &UploadWriteInput<'_>,
) -> Result<ValidatedUpload, UploadWriteError> {
    let Some(obj) = input.body.as_object() else {
        return Err(UploadWriteError::NotADict(non_dict_body(input.body)));
    };
    let mut errors: Vec<(&'static str, Value)> = Vec::new();

    // `name = CharField()`: required, no null, no blank, no length cap.
    let name = match validate_char(obj.get("name"), true, false, false, None) {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("name", error.message()));
            None
        }
    };
    // `type = CharField(required=False)`: optional, no null, no blank.
    let type_ = match validate_char(obj.get("type"), false, false, false, None) {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("type", error.message()));
            None
        }
    };
    // `size = IntegerField()`: required.
    let size = match validate_integer(obj.get("size")) {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(field_entry("size", error.message()));
            None
        }
    };
    // `external_id`/`external_source = CharField(required=False)`: optional,
    // no null, no blank, no length cap (plain `Serializer` fields, no model).
    let external_id = match validate_char(obj.get("external_id"), false, false, false, None) {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("external_id", error.message()));
            None
        }
    };
    let external_source = match validate_char(obj.get("external_source"), false, false, false, None)
    {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("external_source", error.message()));
            None
        }
    };

    if !errors.is_empty() {
        return Err(UploadWriteError::Fields(field_errors_body(&errors)));
    }
    Ok(ValidatedUpload {
        name: name.expect("name validated when no field errors"),
        type_,
        size: size.expect("size validated when no field errors"),
        external_id,
        external_source,
    })
}

/// `IssueCommentCreateSerializer(instance, data, partial=...)` input
/// (`api/views/issue.py:1916,2063`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentWriteInput<'a> {
    /// The request body as parsed JSON.
    pub body: &'a Value,
    /// PATCH (`partial=True`) vs POST: behavior is identical either way here
    /// (no field is required, so absent keys always skip); carried so the two
    /// view call sites (`:1916` full, `:2063` partial) share one function.
    pub partial: bool,
}

/// Validated comment write (`validated_data` shape): `None` = key omitted
/// (absent input); the double-`Option` fields distinguish explicit null.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedComment {
    pub comment_json: Option<Value>,
    pub comment_html: Option<String>,
    pub access: Option<String>,
    pub external_source: Option<Option<String>>,
    pub external_id: Option<Option<String>>,
    pub labels: Option<Vec<String>>,
    pub speaker_type: Option<String>,
    pub speaker_label: Option<String>,
    /// Canonical UUID string when set.
    pub speaker_agent_run_id: Option<Option<String>>,
}

/// Every failure [`validate_comment_create`] can produce. Both arms carry the
/// byte-exact 400 body. (`validate()` is NOT run here: the create serializer
/// defines no override — the lxml `validate()` lives on the full
/// `IssueCommentSerializer`, see [`validate_comment_html`].)
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommentWriteError {
    /// Non-object body.
    #[error("{0}")]
    NotADict(String),
    /// Combined field errors in [`COMMENT_CREATE_FIELDS`] order.
    #[error("{0}")]
    Fields(String),
}

impl CommentWriteError {
    /// HTTP status Django answers with: always 400 on this serializer.
    pub fn status(&self) -> u16 {
        400
    }

    /// Byte-exact response body.
    pub fn body(&self) -> &str {
        match self {
            CommentWriteError::NotADict(body) | CommentWriteError::Fields(body) => body,
        }
    }
}

/// Port of `IssueCommentCreateSerializer` field validation
/// (`serializers/issue.py:928-962` over DRF `ModelSerializer`, no `validate()`
/// override): every writable field runs even after failures, errors combine
/// in `Meta.fields` order, unknown input keys are silently ignored, and no
/// call site passes `fields=` (so all nine fields always run).
pub fn validate_comment_create(
    input: &CommentWriteInput<'_>,
) -> Result<ValidatedComment, CommentWriteError> {
    let Some(obj) = input.body.as_object() else {
        return Err(CommentWriteError::NotADict(non_dict_body(input.body)));
    };
    let mut errors: Vec<(&'static str, Value)> = Vec::new();

    // `comment_json = JSONField(blank=True, default=dict)`: optional, no null.
    let comment_json = match validate_json(obj.get("comment_json")) {
        Ok(JsonOutcome::Value(value)) => Some(value),
        Ok(JsonOutcome::Skip) => None,
        Err(error) => {
            errors.push(field_entry("comment_json", error.message()));
            None
        }
    };
    // `comment_html = TextField(blank=True, default="<p></p>")`: optional,
    // blank ok, no null, no length cap.
    let comment_html = match validate_char(obj.get("comment_html"), false, false, true, None) {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("comment_html", error.message()));
            None
        }
    };
    // `access = CharField(choices, default="INTERNAL")`: optional, no null.
    let access = match validate_choice(obj.get("access"), ACCESS_CHOICES) {
        Ok(ChoiceOutcome::Choice(value)) => Some(value),
        Ok(ChoiceOutcome::Skip) => None,
        Err(error) => {
            errors.push(field_entry("access", error.message()));
            None
        }
    };
    // `external_source`/`external_id`: optional, null ok, blank ok, 255 cap.
    let external_source = match validate_char(
        obj.get("external_source"),
        false,
        true,
        true,
        Some(MAX_EXTERNAL_CHARS),
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
        Some(MAX_EXTERNAL_CHARS),
    ) {
        Ok(CharOutcome::Text(value)) => Some(Some(value)),
        Ok(CharOutcome::Null) => Some(None),
        Ok(CharOutcome::Skip) => None,
        Err(error) => {
            errors.push(field_entry("external_id", error.message()));
            None
        }
    };
    // `labels = ArrayField(CharField(max_length=32), blank=True,
    // default=list)`: optional, no null.
    let labels = match validate_labels(obj.get("labels")) {
        Ok(LabelsOutcome::List(items)) => Some(items),
        Ok(LabelsOutcome::Skip) => None,
        Err(LabelsError::Null) => {
            errors.push(field_entry("labels", MSG_NULL.to_owned()));
            None
        }
        Err(LabelsError::NotAList(py_type)) => {
            // Built as a detail (not via `not_a_list_body`, which renders a
            // whole body) so it combines in field order; the spelling is
            // asserted byte-identical against that helper in the tests.
            let detail = Value::Array(vec![Value::String(format!(
                "Expected a list of items but got type \"{py_type}\"."
            ))]);
            errors.push(("labels", detail));
            None
        }
        Err(LabelsError::Items(failures)) => {
            let mut indexed = Map::with_capacity(failures.len());
            for (idx, messages) in &failures {
                let details: Vec<Value> =
                    messages.iter().map(|m| Value::String(m.clone())).collect();
                indexed.insert(idx.to_string(), Value::Array(details));
            }
            errors.push(("labels", Value::Object(indexed)));
            None
        }
    };
    // `speaker_type = CharField(choices, default=HUMAN)`: optional, no null.
    let speaker_type = match validate_choice(obj.get("speaker_type"), SPEAKER_TYPE_CHOICES) {
        Ok(ChoiceOutcome::Choice(value)) => Some(value),
        Ok(ChoiceOutcome::Skip) => None,
        Err(error) => {
            errors.push(field_entry("speaker_type", error.message()));
            None
        }
    };
    // `speaker_label = CharField(max_length=128, blank=True, default="")`:
    // optional, blank ok, no null.
    let speaker_label = match validate_char(
        obj.get("speaker_label"),
        false,
        false,
        true,
        Some(MAX_SPEAKER_LABEL_CHARS),
    ) {
        Ok(CharOutcome::Text(value)) => Some(value),
        Ok(_) => None,
        Err(error) => {
            errors.push(field_entry("speaker_label", error.message()));
            None
        }
    };
    // `speaker_agent_run_id = UUIDField(null=True, blank=True)`: optional,
    // null ok.
    let speaker_agent_run_id = match validate_uuid(obj.get("speaker_agent_run_id"), true) {
        Ok(UuidOutcome::Uuid(value)) => Some(Some(value)),
        Ok(UuidOutcome::Null) => Some(None),
        Ok(UuidOutcome::Skip) => None,
        Err(error) => {
            errors.push(field_entry("speaker_agent_run_id", error.message()));
            None
        }
    };

    if !errors.is_empty() {
        return Err(CommentWriteError::Fields(field_errors_body(&errors)));
    }
    Ok(ValidatedComment {
        comment_json,
        comment_html,
        access,
        external_source,
        external_id,
        labels,
        speaker_type,
        speaker_label,
        speaker_agent_run_id,
    })
}

/// `IssueCommentSerializer.validate()` input (`issue.py:1008-1017`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentHtmlInput<'a> {
    /// `data.get("comment_html")`: `None` = key absent or JSON null —
    /// passed through untouched.
    pub value: Option<&'a str>,
    /// lxml `tostring(fromstring(value), encoding="unicode")`; `None` = the
    /// round-trip raised — including the empty string (`html.fromstring("")`
    /// raises `ParserError: Document is empty`). Caller-supplied (the handler
    /// layer owns the HTML parser), same seam as `shape_issue`.
    pub roundtripped: Option<&'a str>,
}

/// Every failure [`validate_comment_html`] can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CommentValidateError {
    /// lxml round-trip raised (`:1015-1016`). Raised as a bare message, hence
    /// `non_field_errors` — byte-identical to the issue serializer's body.
    #[error("Invalid HTML passed")]
    InvalidHtml,
}

impl CommentValidateError {
    /// HTTP status Django answers with: always 400 on this serializer.
    pub fn status(&self) -> u16 {
        400
    }

    /// Byte-exact response body.
    pub fn body(&self) -> &'static str {
        match self {
            CommentValidateError::InvalidHtml => INVALID_HTML_BODY,
        }
    }
}

/// Port of `IssueCommentSerializer.validate()` (`issue.py:1008-1017`): absent
/// or null `comment_html` passes through untouched; otherwise the lxml
/// round-trip either substitutes the round-tripped string into the validated
/// data or fails the whole object with `Invalid HTML passed`. (Unreachable
/// from api-v1 views — no view constructs the full serializer with `data=` —
/// but pinned by the fixture, so ported.)
pub fn validate_comment_html(
    input: &CommentHtmlInput<'_>,
) -> Result<Option<String>, CommentValidateError> {
    if input.value.is_none() {
        return Ok(None);
    }
    match input.roundtripped {
        Some(roundtripped) => Ok(Some(roundtripped.to_owned())),
        None => Err(CommentValidateError::InvalidHtml),
    }
}

/// Failure modes of the `render_*` functions: the caller-contract arms are
/// 500-class for the handler to map (mirroring `shape_issue::RenderError`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SocialRenderError {
    /// Nested `fields=` dict (`TypeError` parity, see [`filter_fields`]).
    #[error("fields filter failed: {0}")]
    Fields(#[from] FilterError),
    /// Non-finite float: `serde_json` cannot render NaN/Infinity (Postgres
    /// `float8` admits them; Django emits the literal tokens).
    #[error("{field} is not finite (Django emits the NaN/Infinity literal)")]
    NonFiniteFloat {
        /// The offending field (`size`, `epoch`).
        field: &'static str,
    },
    /// A map-hit `expand` name with no caller value (Python always renders
    /// the related object, or `{}` when the FK is null).
    #[error("expand '{0}' needs its rendered value (None renders {{}})")]
    MissingExpansion(String),
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::String(text.to_string()),
        None => Value::Null,
    }
}

fn str_list(items: &[Option<&str>]) -> Value {
    // `ListField.to_representation`: None items pass through as null
    // (unreachable via the API — the child forbids null — but Postgres
    // arrays admit NULLs, so the row type stays faithful).
    Value::Array(items.iter().map(|item| opt_str(*item)).collect())
}

fn finite_float(value: f64, field: &'static str) -> Result<Value, SocialRenderError> {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or(SocialRenderError::NonFiniteFloat { field })
}

/// Base expansion (`base.py:76-116`), shared by the four renders: for each
/// `expand` name in request order that survived `fields=`, a map hit
/// ([`BASE_EXPANSION_NAMES`]) renders the caller value (or `{}` for a null
/// FK), anything else takes `passthrough(name)` — `Some` for the FK fields
/// outside the map (the `getattr(instance, f"{expand}_id", None)` rule, an
/// observable no-op) — or `null`.
fn apply_expansion(
    out: &mut Map<String, Value>,
    kept: &[String],
    expand: &[&str],
    expansions: &[(&str, Option<Value>)],
    passthrough: &impl Fn(&str) -> Option<Value>,
) -> Result<(), SocialRenderError> {
    for name in expand {
        if !kept.iter().any(|kept| kept == name) {
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
                None => return Err(SocialRenderError::MissingExpansion(name.to_string())),
            }
        } else if let Some(value) = passthrough(name) {
            out.insert(name.to_string(), value);
        } else {
            out.insert(name.to_string(), Value::Null);
        }
    }
    Ok(())
}

/// One `FileAsset` row for [`render_attachment`]. Datetimes arrive
/// pre-formatted (DRF ISO-8601 with `Z`); UUIDs as canonical strings.
pub struct AttachmentRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    /// `attributes` JSON (`NOT NULL`, `default=dict`).
    pub attributes: &'a Value,
    /// Stored asset key; empty renders `null` (DRF `FileField`: falsy → `None`).
    pub asset: &'a str,
    pub entity_type: Option<&'a str>,
    pub entity_identifier: Option<&'a str>,
    pub is_deleted: bool,
    pub is_archived: bool,
    pub external_id: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub size: f64,
    pub is_uploaded: bool,
    pub storage_metadata: Option<&'a Value>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub draft_issue: Option<&'a str>,
    pub project: Option<&'a str>,
    pub issue: Option<&'a str>,
    pub comment: Option<&'a str>,
    pub page: Option<&'a str>,
}

/// `IssueAttachmentSerializer.to_representation()` input (`issue.py:907-927`
/// over the `BaseSerializer` passes, `api/serializers/base.py:19-30,72-117`).
pub struct AttachmentRepresentationInput<'a> {
    /// The asset row.
    pub row: &'a AttachmentRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// Rendered values for map-hit `expand` names among this serializer's
    /// fields (`created_by`, `updated_by`, `user`, `workspace`, `project`,
    /// `issue`): `Some(value)` renders the object, `None` renders `{}` (null
    /// FK). Looked up only for names in [`BASE_EXPANSION_NAMES`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `IssueAttachmentSerializer.to_representation()` (`issue.py:907-927`)
/// over the `BaseSerializer` passes (`base.py:19-30,72-117`). Non-map
/// `expand` names null their scalar, except the FK passthroughs
/// (`comment`, `page`, `draft_issue`), which re-emit their id.
pub fn render_attachment(
    input: &AttachmentRepresentationInput<'_>,
) -> Result<Map<String, Value>, SocialRenderError> {
    let kept = filter_fields(ATTACHMENT_READ_FIELDS, input.fields)?;

    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "deleted_at" => opt_str(row.deleted_at),
            "attributes" => row.attributes.clone(),
            // `S3Storage.url()` returns the bare name; empty (no file) → null.
            "asset" => {
                if row.asset.is_empty() {
                    Value::Null
                } else {
                    Value::String(row.asset.to_string())
                }
            }
            "entity_type" => opt_str(row.entity_type),
            "entity_identifier" => opt_str(row.entity_identifier),
            "is_deleted" => Value::Bool(row.is_deleted),
            "is_archived" => Value::Bool(row.is_archived),
            "external_id" => opt_str(row.external_id),
            "external_source" => opt_str(row.external_source),
            "size" => finite_float(row.size, "size")?,
            "is_uploaded" => Value::Bool(row.is_uploaded),
            "storage_metadata" => row.storage_metadata.cloned().unwrap_or(Value::Null),
            "created_by" => opt_str(row.created_by),
            "updated_by" => opt_str(row.updated_by),
            "user" => opt_str(row.user),
            "workspace" => opt_str(row.workspace),
            "draft_issue" => opt_str(row.draft_issue),
            "project" => opt_str(row.project),
            "issue" => opt_str(row.issue),
            "comment" => opt_str(row.comment),
            "page" => opt_str(row.page),
            // `filter_fields` only ever yields `ATTACHMENT_READ_FIELDS` names,
            // and every one is matched above.
            _ => unreachable!("render_attachment matched every kept readable field"),
        };
        out.insert(name.clone(), value);
    }

    apply_expansion(
        &mut out,
        &kept,
        input.expand,
        input.expansions,
        &|name| match name {
            "comment" => Some(opt_str(row.comment)),
            "page" => Some(opt_str(row.page)),
            "draft_issue" => Some(opt_str(row.draft_issue)),
            _ => None,
        },
    )?;

    Ok(out)
}

/// One `IssueComment` row for [`render_comment`]. Datetimes arrive
/// pre-formatted (DRF ISO-8601 with `Z`); UUIDs as canonical strings.
pub struct CommentRow<'a> {
    pub id: &'a str,
    /// The `is_member` queryset annotation (`views/issue.py:1817,1973`):
    /// `None` = un-annotated row → DRF `SkipField` drops the key.
    pub is_member: Option<bool>,
    /// `get_url()` output (`None` = unconfigured base or missing part).
    pub url: Option<String>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    /// `NOT NULL` (`blank=True`, `default="<p></p>"`) — always present.
    pub comment_html: &'a str,
    /// `NOT NULL` (`blank=True`, `default=list`) — always present.
    pub attachments: &'a [Option<&'a str>],
    /// `NOT NULL` (`blank=True`, `default=list`) — always present.
    pub labels: &'a [Option<&'a str>],
    /// `NOT NULL` (`default="INTERNAL"`) — always present.
    pub access: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    /// `NOT NULL` (`default=HUMAN`) — always present.
    pub speaker_type: &'a str,
    /// `NOT NULL` (`blank=True`, `default=""`) — always present.
    pub speaker_label: &'a str,
    pub speaker_agent_run_id: Option<&'a str>,
    pub edited_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    /// `NOT NULL` — always present.
    pub project: &'a str,
    /// `NOT NULL` — always present.
    pub workspace: &'a str,
    pub description: Option<&'a str>,
    /// `NOT NULL` — always present.
    pub issue: &'a str,
    pub actor: Option<&'a str>,
    pub parent: Option<&'a str>,
}

/// `IssueCommentSerializer.to_representation()` input (`issue.py:965-1007`
/// over the `BaseSerializer` passes, `api/serializers/base.py:19-30,72-117`).
pub struct CommentRepresentationInput<'a> {
    /// The comment row.
    pub row: &'a CommentRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// Rendered values for map-hit `expand` names among this serializer's
    /// fields (`created_by`, `updated_by`, `project`, `workspace`, `issue`,
    /// `actor`, `parent`): `Some(value)` renders the object, `None` renders
    /// `{}` (null FK). Looked up only for names in [`BASE_EXPANSION_NAMES`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `IssueCommentSerializer.to_representation()` (`issue.py:1001-1007`)
/// over the `BaseSerializer` passes (`base.py:19-30,72-117`). `is_member`
/// renders only when the annotation fact is present; `url` is omitted when
/// null — after expansion, so `?expand=url` removes the key. The non-map
/// `description` FK re-emits its id through the passthrough.
pub fn render_comment(
    input: &CommentRepresentationInput<'_>,
) -> Result<Map<String, Value>, SocialRenderError> {
    let kept = filter_fields(COMMENT_READ_FIELDS, input.fields)?;

    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            // `SkipField` when the annotation is missing (un-annotated row).
            "is_member" => match row.is_member {
                Some(member) => Value::Bool(member),
                None => continue,
            },
            "url" => opt_str(row.url.as_deref()),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "deleted_at" => opt_str(row.deleted_at),
            "comment_html" => Value::String(row.comment_html.to_string()),
            "attachments" => str_list(row.attachments),
            "labels" => str_list(row.labels),
            "access" => Value::String(row.access.to_string()),
            "external_source" => opt_str(row.external_source),
            "external_id" => opt_str(row.external_id),
            "speaker_type" => Value::String(row.speaker_type.to_string()),
            "speaker_label" => Value::String(row.speaker_label.to_string()),
            "speaker_agent_run_id" => opt_str(row.speaker_agent_run_id),
            "edited_at" => opt_str(row.edited_at),
            "created_by" => opt_str(row.created_by),
            "updated_by" => opt_str(row.updated_by),
            "project" => Value::String(row.project.to_string()),
            "workspace" => Value::String(row.workspace.to_string()),
            "description" => opt_str(row.description),
            "issue" => Value::String(row.issue.to_string()),
            "actor" => opt_str(row.actor),
            "parent" => opt_str(row.parent),
            // `filter_fields` only ever yields `COMMENT_READ_FIELDS` names,
            // and every one is matched above.
            _ => unreachable!("render_comment matched every kept readable field"),
        };
        out.insert(name.clone(), value);
    }

    apply_expansion(
        &mut out,
        &kept,
        input.expand,
        input.expansions,
        &|name| match name {
            "description" => Some(opt_str(row.description)),
            _ => None,
        },
    )?;

    // Omit `url` when null (`issue.py:1003-1005`) — after expansion, so
    // `?expand=url` removes the key.
    if out.get("url").is_none_or(Value::is_null) {
        out.remove("url");
    }

    Ok(out)
}

/// One `IssueComment` row for [`render_comment_create`] (the
/// `IssueCommentCreateSerializer.data` read shape, embedded in the activity
/// payload at `views/issue.py:1930`).
pub struct CommentCreateRow<'a> {
    /// `comment_json` (`NOT NULL`, `default=dict`).
    pub comment_json: &'a Value,
    /// `NOT NULL` — always present.
    pub comment_html: &'a str,
    /// `NOT NULL` — always present.
    pub access: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    /// `NOT NULL` — always present.
    pub labels: &'a [Option<&'a str>],
    /// `NOT NULL` — always present.
    pub speaker_type: &'a str,
    /// `NOT NULL` — always present.
    pub speaker_label: &'a str,
    pub speaker_agent_run_id: Option<&'a str>,
}

/// `IssueCommentCreateSerializer.to_representation()` input (the nine
/// `Meta.fields`, `issue.py:938-948`, over the `BaseSerializer` passes).
pub struct CommentCreateRepresentationInput<'a> {
    /// The comment row.
    pub row: &'a CommentCreateRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// No field of this serializer is in [`BASE_EXPANSION_NAMES`], so every
    /// `expand` name takes the else branch (`null` — none of the nine has a
    /// `<name>_id` attribute); carried for the shared call shape and never
    /// consulted.
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of the `IssueCommentCreateSerializer` read shape (the nine
/// `Meta.fields` over the `BaseSerializer` passes). Every `expand` name
/// nulls its scalar (no map hits, no FK passthroughs among the nine).
pub fn render_comment_create(
    input: &CommentCreateRepresentationInput<'_>,
) -> Result<Map<String, Value>, SocialRenderError> {
    let kept = filter_fields(COMMENT_CREATE_FIELDS, input.fields)?;

    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "comment_json" => row.comment_json.clone(),
            "comment_html" => Value::String(row.comment_html.to_string()),
            "access" => Value::String(row.access.to_string()),
            "external_source" => opt_str(row.external_source),
            "external_id" => opt_str(row.external_id),
            "labels" => str_list(row.labels),
            "speaker_type" => Value::String(row.speaker_type.to_string()),
            "speaker_label" => Value::String(row.speaker_label.to_string()),
            "speaker_agent_run_id" => opt_str(row.speaker_agent_run_id),
            // `filter_fields` only ever yields `COMMENT_CREATE_FIELDS` names,
            // and every one is matched above.
            _ => unreachable!("render_comment_create matched every kept readable field"),
        };
        out.insert(name.clone(), value);
    }

    apply_expansion(&mut out, &kept, input.expand, input.expansions, &|_| None)?;

    Ok(out)
}

/// One `IssueActivity` row for [`render_activity`]. Datetimes arrive
/// pre-formatted (DRF ISO-8601 with `Z`); UUIDs as canonical strings.
pub struct ActivityRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    /// `NOT NULL` (`default="created"`) — always present.
    pub verb: &'a str,
    pub field: Option<&'a str>,
    pub old_value: Option<&'a str>,
    pub new_value: Option<&'a str>,
    /// `NOT NULL` (`blank=True`) — always present.
    pub comment: &'a str,
    /// `NOT NULL` (`blank=True`, `default=list`) — always present.
    pub attachments: &'a [Option<&'a str>],
    pub old_identifier: Option<&'a str>,
    pub new_identifier: Option<&'a str>,
    pub epoch: Option<f64>,
    /// `NOT NULL` — always present.
    pub project: &'a str,
    /// `NOT NULL` — always present.
    pub workspace: &'a str,
    pub issue: Option<&'a str>,
    pub issue_comment: Option<&'a str>,
    pub actor: Option<&'a str>,
}

/// `IssueActivitySerializer.to_representation()` input (`issue.py:1020-1031`
/// over the `BaseSerializer` passes, `api/serializers/base.py:19-30,72-117`).
pub struct ActivityRepresentationInput<'a> {
    /// The activity row.
    pub row: &'a ActivityRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// Rendered values for map-hit `expand` names among this serializer's
    /// fields (`project`, `workspace`, `issue`, `actor`): `Some(value)`
    /// renders the object, `None` renders `{}` (null FK). Looked up only for
    /// names in [`BASE_EXPANSION_NAMES`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `IssueActivitySerializer.to_representation()` (`issue.py:1020-1031`)
/// over the `BaseSerializer` passes (`base.py:19-30,72-117`). Non-map
/// `expand` names null their scalar, except the `issue_comment` FK
/// passthrough, which re-emits its id.
pub fn render_activity(
    input: &ActivityRepresentationInput<'_>,
) -> Result<Map<String, Value>, SocialRenderError> {
    let kept = filter_fields(ACTIVITY_READ_FIELDS, input.fields)?;

    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "deleted_at" => opt_str(row.deleted_at),
            "verb" => Value::String(row.verb.to_string()),
            "field" => opt_str(row.field),
            "old_value" => opt_str(row.old_value),
            "new_value" => opt_str(row.new_value),
            "comment" => Value::String(row.comment.to_string()),
            "attachments" => str_list(row.attachments),
            "old_identifier" => opt_str(row.old_identifier),
            "new_identifier" => opt_str(row.new_identifier),
            "epoch" => match row.epoch {
                Some(epoch) => finite_float(epoch, "epoch")?,
                None => Value::Null,
            },
            "project" => Value::String(row.project.to_string()),
            "workspace" => Value::String(row.workspace.to_string()),
            "issue" => opt_str(row.issue),
            "issue_comment" => opt_str(row.issue_comment),
            "actor" => opt_str(row.actor),
            // `filter_fields` only ever yields `ACTIVITY_READ_FIELDS` names,
            // and every one is matched above.
            _ => unreachable!("render_activity matched every kept readable field"),
        };
        out.insert(name.clone(), value);
    }

    apply_expansion(
        &mut out,
        &kept,
        input.expand,
        input.expansions,
        &|name| match name {
            "issue_comment" => Some(opt_str(row.issue_comment)),
            _ => None,
        },
    )?;

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::shape_issue::{
        issue_url, list_index_errors_body, not_a_list_body, web_base_url,
    };
    use super::*;

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

    fn out_keys(out: &Map<String, Value>) -> Vec<&str> {
        out.keys().map(String::as_str).collect()
    }

    /// Golden `render` values are Python `repr()`s: `"None"`/`"True"`/`"False"`
    /// for null/bools, `'...'`-quoted strings, JSON spellings (`[]`, `{}`,
    /// `10.0`) for the rest, bare text otherwise.
    fn fx_val(value: &Value) -> Value {
        let text = value.as_str().expect("fixture render values are strings");
        match text {
            "None" => Value::Null,
            "True" => Value::Bool(true),
            "False" => Value::Bool(false),
            quoted if quoted.len() >= 2 && quoted.starts_with('\'') && quoted.ends_with('\'') => {
                Value::String(quoted[1..quoted.len() - 1].to_string())
            }
            _ => serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string())),
        }
    }

    /// Raw golden repr string for a `render` key.
    fn fx_raw<'a>(render: &'a Value, key: &str) -> &'a str {
        render[key].as_str().expect("render value is a string")
    }

    /// `None` for `"None"`, else the bare string (for UUID/text-or-null keys,
    /// which the fixture records unquoted).
    fn fx_opt<'a>(render: &'a Value, key: &str) -> Option<&'a str> {
        let text = fx_raw(render, key);
        if text == "None" {
            None
        } else {
            Some(text)
        }
    }

    fn fx_bool(render: &Value, key: &str) -> bool {
        match fx_raw(render, key) {
            "True" => true,
            "False" => false,
            other => panic!("expected a bool repr, got {other:?}"),
        }
    }

    /// Fixture validated strings are Python reprs; every recorded value here
    /// is quote-free, so `repr(s) == 's'`.
    fn py_repr(text: &str) -> String {
        format!("'{text}'")
    }

    fn includes(names: &[&str]) -> Vec<FieldSpec> {
        names
            .iter()
            .map(|name| FieldSpec::Include(name.to_string()))
            .collect()
    }

    /// Assert a render against a golden `render` object: key order equals the
    /// golden `render_keys`, then per-key value equality through [`fx_val`].
    fn assert_render(out: &Map<String, Value>, unit: &Value) {
        assert_eq!(str_list(&unit["render_keys"]), out_keys(out));
        let render = unit["render"].as_object().expect("render is an object");
        for (key, golden) in render {
            assert_eq!(
                out.get(key),
                Some(&fx_val(golden)),
                "render[{key}] matches the golden"
            );
        }
    }

    fn attachment_row<'a>(
        render: &'a Value,
        attributes: &'a Value,
        storage_metadata: Option<&'a Value>,
    ) -> AttachmentRow<'a> {
        AttachmentRow {
            id: fx_raw(render, "id"),
            created_at: fx_raw(render, "created_at"),
            updated_at: fx_raw(render, "updated_at"),
            deleted_at: fx_opt(render, "deleted_at"),
            attributes,
            asset: fx_raw(render, "asset"),
            entity_type: fx_opt(render, "entity_type"),
            entity_identifier: fx_opt(render, "entity_identifier"),
            is_deleted: fx_bool(render, "is_deleted"),
            is_archived: fx_bool(render, "is_archived"),
            external_id: fx_opt(render, "external_id"),
            external_source: fx_opt(render, "external_source"),
            size: fx_val(&render["size"]).as_f64().expect("size is a float"),
            is_uploaded: fx_bool(render, "is_uploaded"),
            storage_metadata,
            created_by: fx_opt(render, "created_by"),
            updated_by: fx_opt(render, "updated_by"),
            user: fx_opt(render, "user"),
            workspace: fx_opt(render, "workspace"),
            draft_issue: fx_opt(render, "draft_issue"),
            project: fx_opt(render, "project"),
            issue: fx_opt(render, "issue"),
            comment: fx_opt(render, "comment"),
            page: fx_opt(render, "page"),
        }
    }

    // ---- F18-03 replays: IssueAttachmentSerializer --------------------------

    #[test]
    fn attachment_keys_match_f18_03() {
        let fx = fixture(F18_03);
        let att = unit(&fx, "IssueAttachmentSerializer");
        assert_eq!(str_list(&att["render_keys"]), ATTACHMENT_READ_FIELDS);
    }

    #[test]
    fn attachment_render_matches_f18_03() {
        let fx = fixture(F18_03);
        let att = unit(&fx, "IssueAttachmentSerializer");
        let attributes = fx_val(&att["render"]["attributes"]);
        // `storage_metadata` is `"None"` in the golden.
        assert_eq!(fx_raw(&att["render"], "storage_metadata"), "None");
        let row = attachment_row(&att["render"], &attributes, None);
        let out = render_attachment(&AttachmentRepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("fixture row renders");
        assert_render(&out, att);
        // Literal spots (independent of the repr converter).
        assert_eq!(out["asset"], Value::String("contract-asset".to_string()));
        assert_eq!(
            out["entity_type"],
            Value::String("ISSUE_ATTACHMENT".to_string())
        );
        assert_eq!(out["size"], serde_json::json!(10.0));
        assert_eq!(out["is_deleted"], Value::Bool(false));
        assert_eq!(out["is_uploaded"], Value::Bool(true));
        assert_eq!(out["attributes"], serde_json::json!({}));
        assert_eq!(out["deleted_at"], Value::Null);
    }

    #[test]
    fn attachment_fields_subset_keeps_wire_order() {
        let fx = fixture(F18_03);
        let att = unit(&fx, "IssueAttachmentSerializer");
        let attributes = fx_val(&att["render"]["attributes"]);
        let row = attachment_row(&att["render"], &attributes, None);
        let specs = includes(&["asset", "id", "nope"]);
        let out = render_attachment(&AttachmentRepresentationInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            expansions: &[],
        })
        .expect("subset renders");
        assert_eq!(out_keys(&out), vec!["id", "asset"]);
    }

    #[test]
    fn attachment_expand_behaviors() {
        let fx = fixture(F18_03);
        let att = unit(&fx, "IssueAttachmentSerializer");
        let attributes = fx_val(&att["render"]["attributes"]);
        let row = attachment_row(&att["render"], &attributes, None);
        // Map hit with a value renders the object; null FK renders `{}`.
        let lite = serde_json::json!({"id": "u1"});
        let out = render_attachment(&AttachmentRepresentationInput {
            row: &row,
            fields: None,
            expand: &["user", "created_by", "asset", "comment"],
            expansions: &[("user", Some(lite.clone())), ("created_by", None)],
        })
        .expect("expansions render");
        assert_eq!(out["user"], lite);
        assert_eq!(out["created_by"], serde_json::json!({}));
        // Non-map scalar nulls; null-FK passthrough re-emits null.
        assert_eq!(out["asset"], Value::Null);
        assert_eq!(out["comment"], Value::Null);

        // FK passthrough with a value re-emits the id (observable no-op).
        let attributes2 = fx_val(&att["render"]["attributes"]);
        let mut row2 = attachment_row(&att["render"], &attributes2, None);
        row2.comment = Some("c0ffee00-0000-0000-0000-000000000000");
        let out = render_attachment(&AttachmentRepresentationInput {
            row: &row2,
            fields: None,
            expand: &["comment", "page", "draft_issue"],
            expansions: &[],
        })
        .expect("passthroughs render");
        assert_eq!(
            out["comment"],
            Value::String("c0ffee00-0000-0000-0000-000000000000".to_string())
        );
        assert_eq!(out["page"], Value::Null);
        assert_eq!(out["draft_issue"], Value::Null);

        // Expand on a non-kept field is ignored; missing caller value errors.
        let specs = includes(&["id"]);
        let out = render_attachment(&AttachmentRepresentationInput {
            row: &row,
            fields: Some(&specs),
            expand: &["user"],
            expansions: &[],
        })
        .expect("non-kept expand is ignored");
        assert_eq!(out_keys(&out), vec!["id"]);
        let err = render_attachment(&AttachmentRepresentationInput {
            row: &row,
            fields: None,
            expand: &["user"],
            expansions: &[],
        })
        .expect_err("missing caller value errors");
        assert_eq!(err, SocialRenderError::MissingExpansion("user".to_string()));
    }

    #[test]
    fn attachment_edge_cases() {
        let fx = fixture(F18_03);
        let att = unit(&fx, "IssueAttachmentSerializer");
        let attributes = fx_val(&att["render"]["attributes"]);
        let row = attachment_row(&att["render"], &attributes, None);
        // Empty asset (no file) renders null.
        let attributes2 = fx_val(&att["render"]["attributes"]);
        let storage = serde_json::json!({"bucket": "b"});
        let mut row2 = attachment_row(&att["render"], &attributes2, Some(&storage));
        row2.asset = "";
        let out = render_attachment(&AttachmentRepresentationInput {
            row: &row2,
            fields: Some(&includes(&["asset"])),
            expand: &[],
            expansions: &[],
        })
        .expect("empty asset renders");
        assert_eq!(out["asset"], Value::Null);
        // Present `storage_metadata` passes through verbatim.
        let out = render_attachment(&AttachmentRepresentationInput {
            row: &row2,
            fields: Some(&includes(&["storage_metadata"])),
            expand: &[],
            expansions: &[],
        })
        .expect("storage_metadata renders");
        assert_eq!(out["storage_metadata"], serde_json::json!({"bucket": "b"}));
        // Non-finite size is a 500-class caller-contract failure.
        row2.size = f64::NAN;
        let err = render_attachment(&AttachmentRepresentationInput {
            row: &row2,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect_err("NaN size errors");
        assert_eq!(err, SocialRenderError::NonFiniteFloat { field: "size" });
        // Nested `fields=` dict raises (TypeError parity).
        let specs = vec![FieldSpec::Nested("id".to_string(), vec![])];
        let err = render_attachment(&AttachmentRepresentationInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            expansions: &[],
        })
        .expect_err("nested fields= raises");
        assert!(matches!(err, SocialRenderError::Fields(_)));
    }

    // ---- F18-03 replays: IssueAttachmentUploadSerializer --------------------

    fn upload(body: &Value) -> Result<ValidatedUpload, UploadWriteError> {
        validate_upload_write(&UploadWriteInput { body })
    }

    #[test]
    fn upload_ok_matches_f18_03() {
        let fx = fixture(F18_03);
        let up = unit(&fx, "IssueAttachmentUploadSerializer");
        let body = serde_json::json!({"name": "a.png", "type": "image/png", "size": 123});
        let validated = upload(&body).expect("ok is valid");
        assert_eq!(validated.name, "a.png");
        assert_eq!(validated.type_.as_deref(), Some("image/png"));
        assert_eq!(validated.size, 123);
        assert_eq!(validated.external_id, None);
        assert_eq!(validated.external_source, None);
        // Golden cross-checks (repr spellings + validity flag).
        let golden = &up["ok"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            golden["validated"]["name"].as_str(),
            Some(py_repr("a.png")).as_deref()
        );
        assert_eq!(
            golden["validated"]["type"].as_str(),
            Some(py_repr("image/png")).as_deref()
        );
        assert_eq!(golden["validated"]["size"].as_str(), Some("123"));
    }

    #[test]
    fn upload_minimal_matches_f18_03() {
        let fx = fixture(F18_03);
        let up = unit(&fx, "IssueAttachmentUploadSerializer");
        let body = serde_json::json!({"name": "a.png", "size": 123});
        let validated = upload(&body).expect("minimal is valid");
        assert_eq!(validated.name, "a.png");
        assert_eq!(validated.type_, None);
        assert_eq!(validated.size, 123);
        let golden = &up["minimal"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            golden["validated"]["name"].as_str(),
            Some(py_repr("a.png")).as_deref()
        );
        assert_eq!(golden["validated"]["size"].as_str(), Some("123"));
    }

    #[test]
    fn upload_missing_name_matches_f18_03() {
        let fx = fixture(F18_03);
        let up = unit(&fx, "IssueAttachmentUploadSerializer");
        let body = serde_json::json!({"size": 123});
        let err = upload(&body).expect_err("missing name fails");
        assert_eq!(err.status(), 400);
        assert_eq!(err.body(), r#"{"name":["This field is required."]}"#);
        let golden = &up["missing_name"]["errors"]["name"][0];
        assert_eq!(golden["message"].as_str(), Some("This field is required."));
        assert_eq!(golden["code"].as_str(), Some("required"));
    }

    #[test]
    fn upload_missing_size_matches_f18_03() {
        let fx = fixture(F18_03);
        let up = unit(&fx, "IssueAttachmentUploadSerializer");
        let body = serde_json::json!({"name": "a.png"});
        let err = upload(&body).expect_err("missing size fails");
        assert_eq!(err.body(), r#"{"size":["This field is required."]}"#);
        let golden = &up["missing_size"]["errors"]["size"][0];
        assert_eq!(golden["message"].as_str(), Some("This field is required."));
        assert_eq!(golden["code"].as_str(), Some("required"));
    }

    #[test]
    fn upload_bad_size_matches_f18_03() {
        let fx = fixture(F18_03);
        let up = unit(&fx, "IssueAttachmentUploadSerializer");
        // The golden records the error, not the input; every non-integer
        // spelling lands on the same body (live-probed).
        for bad in [
            serde_json::json!("abc"),
            serde_json::json!(1.5),
            serde_json::json!(true),
            serde_json::json!(""),
            serde_json::json!("12.5"),
            serde_json::json!([1]),
            serde_json::json!({"n": 1}),
        ] {
            let body = serde_json::json!({"name": "a.png", "size": bad});
            let err = upload(&body).expect_err("bad size fails");
            assert_eq!(err.body(), r#"{"size":["A valid integer is required."]}"#);
        }
        let golden = &up["bad_size"]["errors"]["size"][0];
        assert_eq!(
            golden["message"].as_str(),
            Some("A valid integer is required.")
        );
        assert_eq!(golden["code"].as_str(), Some("invalid"));
    }

    #[test]
    fn upload_integer_grammar() {
        // Valid spellings (live-probed against DRF `IntegerField`).
        for (raw, want) in [
            (serde_json::json!("123"), 123),
            (serde_json::json!(123), 123),
            (serde_json::json!(123.0), 123),
            (serde_json::json!("12.0"), 12),
            (serde_json::json!("12."), 12),
            (serde_json::json!(" 12 "), 12),
            (serde_json::json!("+12"), 12),
            (serde_json::json!("-12"), -12),
            (serde_json::json!("1_0"), 10),
            (serde_json::json!("0.000"), 0),
            // Trailing whitespace AFTER the zeros still strips (`0*\s*$`).
            (serde_json::json!("1.0 "), 1),
            (serde_json::json!("9223372036854775807"), i64::MAX),
            (serde_json::json!("-9223372036854775808"), i64::MIN),
        ] {
            let body = serde_json::json!({"name": "a.png", "size": raw});
            let validated = upload(&body).expect("integer spelling is valid");
            assert_eq!(validated.size, want, "input {raw}");
        }
        // Invalid spellings.
        for raw in [
            serde_json::json!("0x10"),
            serde_json::json!("1__0"),
            serde_json::json!("_1"),
            serde_json::json!("1_"),
            serde_json::json!("--1"),
            serde_json::json!("1.0e3"),
            serde_json::json!("5.0.0"),
            // Order-mixed tails never match `0*\s*$` (review finding:
            // whitespace-before-zero must NOT strip).
            serde_json::json!("1. 0"),
            serde_json::json!("1.0 0"),
            serde_json::json!("1.\t0"),
            // Fullwidth digits parse in CPython `int()` but fail here —
            // documented approximation.
            serde_json::json!("１２"),
            // Past `i64` — documented approximation (Python keeps it exact).
            serde_json::json!("9999999999999999999999"),
            serde_json::json!("9223372036854775808"),
            serde_json::json!("-9223372036854775809"),
        ] {
            let body = serde_json::json!({"name": "a.png", "size": raw});
            let err = upload(&body).expect_err("integer spelling fails");
            assert_eq!(
                err.body(),
                r#"{"size":["A valid integer is required."]}"#,
                "input {raw}"
            );
        }
        // Null fails `null`; over-1000-code-point strings fail
        // `max_string_length` (strings only — the guard never fires for JSON
        // numbers).
        let body = serde_json::json!({"name": "a.png", "size": Value::Null});
        assert_eq!(
            upload(&body).expect_err("null size fails").body(),
            r#"{"size":["This field may not be null."]}"#
        );
        let body = serde_json::json!({"name": "a.png", "size": "9".repeat(1001)});
        assert_eq!(
            upload(&body).expect_err("over-long size fails").body(),
            r#"{"size":["String value too large."]}"#
        );
        let body = serde_json::json!({"name": "a.png", "size": "9".repeat(1000)});
        // 1000 code points clears the guard; the value overflows `i64`
        // (documented approximation) — but the GUARD verdict is what this
        // asserts: `invalid`, not `max_string_length`.
        assert_eq!(
            upload(&body).expect_err("1000-digit size fails").body(),
            r#"{"size":["A valid integer is required."]}"#
        );
    }

    #[test]
    fn upload_char_edges_and_combination() {
        // Blank/null/type/trim edges on `name`.
        for (raw, body) in [
            (
                serde_json::json!(""),
                r#"{"name":["This field may not be blank."]}"#,
            ),
            (
                serde_json::json!("   "),
                r#"{"name":["This field may not be blank."]}"#,
            ),
            (
                serde_json::json!(Value::Null),
                r#"{"name":["This field may not be null."]}"#,
            ),
            (
                serde_json::json!(true),
                r#"{"name":["Not a valid string."]}"#,
            ),
            (
                serde_json::json!(["a"]),
                r#"{"name":["Not a valid string."]}"#,
            ),
            (
                serde_json::json!("a\x00b"),
                r#"{"name":["Null characters are not allowed."]}"#,
            ),
        ] {
            let body_in = serde_json::json!({"name": raw, "size": 1});
            assert_eq!(upload(&body_in).expect_err("bad name fails").body(), body);
        }
        // Numeric coercion + trim; no `max_length` on upload fields.
        let body = serde_json::json!({"name": 5, "size": 1});
        assert_eq!(upload(&body).expect("numeric name coerces").name, "5");
        let body = serde_json::json!({"name": "  x  ", "size": 1});
        assert_eq!(upload(&body).expect("name trims").name, "x");
        let body = serde_json::json!({"name": "n".repeat(5000), "size": 1});
        assert_eq!(upload(&body).expect("long name passes").name.len(), 5000);
        // Optional chars: blank/null fail (`allow_blank=False`,
        // `allow_null=False`), missing skips.
        let body = serde_json::json!({"name": "a", "size": 1, "type": ""});
        assert_eq!(
            upload(&body).expect_err("blank type fails").body(),
            r#"{"type":["This field may not be blank."]}"#
        );
        let body = serde_json::json!({"name": "a", "size": 1, "external_id": Value::Null});
        assert_eq!(
            upload(&body).expect_err("null external_id fails").body(),
            r#"{"external_id":["This field may not be null."]}"#
        );
        // Unknown keys ignored; `{}` combines both required failures in
        // declaration order.
        let body = serde_json::json!({"name": "a", "size": 1, "bogus": true});
        assert!(upload(&body).is_ok());
        let body = serde_json::json!({});
        assert_eq!(
            upload(&body).expect_err("empty body fails").body(),
            r#"{"name":["This field is required."],"size":["This field is required."]}"#
        );
    }

    #[test]
    fn upload_non_dict_bodies() {
        for (raw, body) in [
            (
                serde_json::json!([1]),
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}"#,
            ),
            (
                serde_json::json!("x"),
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got str."]}"#,
            ),
            (
                serde_json::json!(5),
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got int."]}"#,
            ),
            (
                serde_json::json!(true),
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got bool."]}"#,
            ),
            (
                serde_json::json!(1.5),
                r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got float."]}"#,
            ),
            (Value::Null, r#"{"non_field_errors":["No data provided"]}"#),
        ] {
            let err = upload(&raw).expect_err("non-dict body fails");
            assert_eq!(err.status(), 400);
            assert_eq!(err.body(), body);
        }
    }

    // ---- F18-03 replays: IssueCommentCreateSerializer -----------------------

    fn create(body: &Value, partial: bool) -> Result<ValidatedComment, CommentWriteError> {
        validate_comment_create(&CommentWriteInput { body, partial })
    }

    #[test]
    fn create_fields_match_f18_03() {
        let fx = fixture(F18_03);
        let create_unit = unit(&fx, "IssueCommentCreateSerializer");
        assert_eq!(str_list(&create_unit["fields"]), COMMENT_CREATE_FIELDS);
    }

    #[test]
    fn create_ok_matches_f18_03() {
        let fx = fixture(F18_03);
        let create_unit = unit(&fx, "IssueCommentCreateSerializer");
        let body = serde_json::json!({
            "comment_json": {"a": 1},
            "comment_html": "<p>hi</p>",
            "access": "EXTERNAL",
        });
        let validated = create(&body, false).expect("ok is valid");
        assert_eq!(validated.comment_json, Some(serde_json::json!({"a": 1})));
        assert_eq!(validated.comment_html.as_deref(), Some("<p>hi</p>"));
        assert_eq!(validated.access.as_deref(), Some("EXTERNAL"));
        assert_eq!(validated.external_source, None);
        assert_eq!(validated.external_id, None);
        assert_eq!(validated.labels, None);
        assert_eq!(validated.speaker_type, None);
        assert_eq!(validated.speaker_label, None);
        assert_eq!(validated.speaker_agent_run_id, None);
        // Golden cross-checks (repr spellings + validity flag).
        let golden = &create_unit["ok"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert_eq!(
            golden["validated"]["comment_json"].as_str(),
            Some("{'a': 1}")
        );
        assert_eq!(
            golden["validated"]["comment_html"].as_str(),
            Some(py_repr("<p>hi</p>")).as_deref()
        );
        assert_eq!(
            golden["validated"]["access"].as_str(),
            Some(py_repr("EXTERNAL")).as_deref()
        );
    }

    #[test]
    fn create_empty_ok_matches_f18_03() {
        let fx = fixture(F18_03);
        let create_unit = unit(&fx, "IssueCommentCreateSerializer");
        let body = serde_json::json!({});
        let validated = create(&body, false).expect("empty is valid");
        // Every field is optional; DRF propagates no model default into
        // `validated_data`, so all keys are omitted.
        assert_eq!(validated.comment_json, None);
        assert_eq!(validated.comment_html, None);
        assert_eq!(validated.access, None);
        assert_eq!(validated.external_source, None);
        assert_eq!(validated.external_id, None);
        assert_eq!(validated.labels, None);
        assert_eq!(validated.speaker_type, None);
        assert_eq!(validated.speaker_label, None);
        assert_eq!(validated.speaker_agent_run_id, None);
        let golden = &create_unit["empty_ok"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        assert!(golden["validated"]
            .as_object()
            .expect("validated is an object")
            .is_empty());
    }

    #[test]
    fn create_choice_edges() {
        // Both choices valid; case-sensitive.
        for access in ["INTERNAL", "EXTERNAL"] {
            let body = serde_json::json!({"access": access});
            assert_eq!(
                create(&body, false)
                    .expect("access choice is valid")
                    .access
                    .as_deref(),
                Some(access)
            );
        }
        for speaker in ["human", "agent", "system", "integration"] {
            let body = serde_json::json!({"speaker_type": speaker});
            assert_eq!(
                create(&body, false)
                    .expect("speaker choice is valid")
                    .speaker_type
                    .as_deref(),
                Some(speaker)
            );
        }
        // `invalid_choice` echoes the Python `str()` of the input.
        for (field, raw, want) in [
            (
                "access",
                serde_json::json!("bogus"),
                r#"{"access":["\"bogus\" is not a valid choice."]}"#,
            ),
            (
                "access",
                serde_json::json!(""),
                r#"{"access":["\"\" is not a valid choice."]}"#,
            ),
            (
                "access",
                serde_json::json!(5),
                r#"{"access":["\"5\" is not a valid choice."]}"#,
            ),
            (
                "access",
                serde_json::json!(true),
                r#"{"access":["\"True\" is not a valid choice."]}"#,
            ),
            (
                "access",
                serde_json::json!({"a": 1}),
                r#"{"access":["\"{'a': 1}\" is not a valid choice."]}"#,
            ),
            (
                "access",
                serde_json::json!(["x"]),
                r#"{"access":["\"['x']\" is not a valid choice."]}"#,
            ),
            (
                "speaker_type",
                serde_json::json!("HUMAN"),
                r#"{"speaker_type":["\"HUMAN\" is not a valid choice."]}"#,
            ),
        ] {
            let body = serde_json::json!({field: raw});
            let err = create(&body, false).expect_err("bad choice fails");
            assert_eq!(err.status(), 400);
            assert_eq!(err.body(), want, "field {field} input {raw}");
        }
        // Null fails `null` (neither choice field allows null).
        let body = serde_json::json!({"access": Value::Null});
        assert_eq!(
            create(&body, false).expect_err("null access fails").body(),
            r#"{"access":["This field may not be null."]}"#
        );
    }

    #[test]
    fn create_char_edges() {
        // `comment_html`: blank ok (validates as `""`), null fails.
        let body = serde_json::json!({"comment_html": ""});
        assert_eq!(
            create(&body, false)
                .expect("blank html passes")
                .comment_html
                .as_deref(),
            Some("")
        );
        let body = serde_json::json!({"comment_html": "   "});
        assert_eq!(
            create(&body, false)
                .expect("whitespace html passes")
                .comment_html
                .as_deref(),
            Some("")
        );
        let body = serde_json::json!({"comment_html": Value::Null});
        assert_eq!(
            create(&body, false).expect_err("null html fails").body(),
            r#"{"comment_html":["This field may not be null."]}"#
        );
        // `external_*`: null ok (explicit null), blank ok, 255 cap.
        let body = serde_json::json!({"external_source": Value::Null});
        assert_eq!(
            create(&body, false)
                .expect("null external passes")
                .external_source,
            Some(None)
        );
        let body = serde_json::json!({"external_id": "x".repeat(255)});
        assert!(create(&body, false).is_ok(), "255 chars clears max_length");
        let body = serde_json::json!({"external_id": "x".repeat(256)});
        assert_eq!(
            create(&body, false).expect_err("256 chars fails").body(),
            r#"{"external_id":["Ensure this field has no more than 255 characters."]}"#
        );
        // The cap runs on the TRIMMED value.
        let body = serde_json::json!({"external_id": format!("  {}  ", "x".repeat(255))});
        assert!(create(&body, false).is_ok(), "cap runs after trim");
        // `speaker_label`: null fails, 128 cap, numeric coercion.
        let body = serde_json::json!({"speaker_label": Value::Null});
        assert_eq!(
            create(&body, false).expect_err("null label fails").body(),
            r#"{"speaker_label":["This field may not be null."]}"#
        );
        let body = serde_json::json!({"speaker_label": "x".repeat(129)});
        assert_eq!(
            create(&body, false).expect_err("129 chars fails").body(),
            r#"{"speaker_label":["Ensure this field has no more than 128 characters."]}"#
        );
        let body = serde_json::json!({"speaker_label": 5});
        assert_eq!(
            create(&body, false)
                .expect("numeric label coerces")
                .speaker_label
                .as_deref(),
            Some("5")
        );
        let body = serde_json::json!({"speaker_label": true});
        assert_eq!(
            create(&body, false).expect_err("bool label fails").body(),
            r#"{"speaker_label":["Not a valid string."]}"#
        );
        // Null byte fails after `max_length`.
        let body = serde_json::json!({"comment_html": "a\x00b"});
        assert_eq!(
            create(&body, false).expect_err("null byte fails").body(),
            r#"{"comment_html":["Null characters are not allowed."]}"#
        );
    }

    #[test]
    fn create_uuid_edges() {
        // Canonical, braced, urn, uppercase, and int forms all canonicalize
        // (live-probed against `uuid.UUID`).
        for (raw, want) in [
            (
                "6353d7bf-012d-45ba-82a1-d2ff209af168",
                "6353d7bf-012d-45ba-82a1-d2ff209af168",
            ),
            (
                "{6353d7bf-012d-45ba-82a1-d2ff209af168}",
                "6353d7bf-012d-45ba-82a1-d2ff209af168",
            ),
            (
                "urn:uuid:6353d7bf-012d-45ba-82a1-d2ff209af168",
                "6353d7bf-012d-45ba-82a1-d2ff209af168",
            ),
            (
                "6353D7BF-012D-45BA-82A1-D2FF209AF168",
                "6353d7bf-012d-45ba-82a1-d2ff209af168",
            ),
            (
                "6353d7bf012d45ba82a1d2ff209af168",
                "6353d7bf-012d-45ba-82a1-d2ff209af168",
            ),
        ] {
            let body = serde_json::json!({"speaker_agent_run_id": raw});
            assert_eq!(
                create(&body, false)
                    .expect("uuid form is valid")
                    .speaker_agent_run_id,
                Some(Some(want.to_string())),
                "input {raw}"
            );
        }
        // The bool quirk: `isinstance(True, int)` takes the int arm.
        let body = serde_json::json!({"speaker_agent_run_id": true});
        assert_eq!(
            create(&body, false)
                .expect("bool uuid passes")
                .speaker_agent_run_id,
            Some(Some("00000000-0000-0000-0000-000000000001".to_string()))
        );
        let body = serde_json::json!({"speaker_agent_run_id": 1});
        assert_eq!(
            create(&body, false)
                .expect("int uuid passes")
                .speaker_agent_run_id,
            Some(Some("00000000-0000-0000-0000-000000000001".to_string()))
        );
        // Null ok; everything else fails with the one un-interpolated message.
        let body = serde_json::json!({"speaker_agent_run_id": Value::Null});
        assert_eq!(
            create(&body, false)
                .expect("null uuid passes")
                .speaker_agent_run_id,
            Some(None)
        );
        for raw in [
            serde_json::json!("not-a-uuid"),
            serde_json::json!(""),
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!(["x"]),
            serde_json::json!("  6353d7bf-012d-45ba-82a1-d2ff209af168  "),
        ] {
            let body = serde_json::json!({"speaker_agent_run_id": raw});
            let err = create(&body, false).expect_err("bad uuid fails");
            assert_eq!(
                err.body(),
                r#"{"speaker_agent_run_id":["Must be a valid UUID."]}"#
            );
        }
    }

    #[test]
    fn create_json_edges() {
        // Any JSON value passes through verbatim; null fails.
        for raw in [
            serde_json::json!({"a": 1}),
            serde_json::json!([1, "x"]),
            serde_json::json!("text"),
            serde_json::json!(5),
            serde_json::json!(true),
        ] {
            let body = serde_json::json!({"comment_json": raw});
            assert_eq!(
                create(&body, false).expect("json passes").comment_json,
                Some(raw.clone())
            );
        }
        let body = serde_json::json!({"comment_json": Value::Null});
        assert_eq!(
            create(&body, false).expect_err("null json fails").body(),
            r#"{"comment_json":["This field may not be null."]}"#
        );
    }

    #[test]
    fn create_labels_edges() {
        // Items validate as `CharField(max_length=32)`: numerics coerce.
        let body = serde_json::json!({"labels": ["a", 5]});
        assert_eq!(
            create(&body, false).expect("label items pass").labels,
            Some(vec!["a".to_string(), "5".to_string()])
        );
        let body = serde_json::json!({"labels": []});
        assert_eq!(
            create(&body, false).expect("empty list passes").labels,
            Some(vec![])
        );
        // Child failures collect as `{index: [messages]}` — byte-identical to
        // the shared single-body helper.
        let body = serde_json::json!({"labels": ["", "x"]});
        let err = create(&body, false).expect_err("blank item fails");
        assert_eq!(
            err.body(),
            r#"{"labels":{"0":["This field may not be blank."]}}"#
        );
        assert_eq!(
            err.body(),
            list_index_errors_body(
                "labels",
                &[(0, vec!["This field may not be blank.".to_string()])]
            )
        );
        let body = serde_json::json!({"labels": ["ok", "y".repeat(33)]});
        assert_eq!(
            create(&body, false).expect_err("long item fails").body(),
            r#"{"labels":{"1":["Ensure this field has no more than 32 characters."]}}"#
        );
        let body = serde_json::json!({"labels": [true]});
        assert_eq!(
            create(&body, false).expect_err("bool item fails").body(),
            r#"{"labels":{"0":["Not a valid string."]}}"#
        );
        // Non-list inputs fail `not_a_list` — byte-identical to the shared
        // single-body helper.
        let body = serde_json::json!({"labels": "x"});
        let err = create(&body, false).expect_err("str labels fails");
        assert_eq!(
            err.body(),
            r#"{"labels":["Expected a list of items but got type \"str\"."]}"#
        );
        assert_eq!(err.body(), not_a_list_body("labels", "str"));
        let body = serde_json::json!({"labels": {"a": 1}});
        assert_eq!(
            create(&body, false).expect_err("dict labels fails").body(),
            r#"{"labels":["Expected a list of items but got type \"dict\"."]}"#
        );
        // Null fails `null`.
        let body = serde_json::json!({"labels": Value::Null});
        assert_eq!(
            create(&body, false).expect_err("null labels fails").body(),
            r#"{"labels":["This field may not be null."]}"#
        );
    }

    #[test]
    fn create_partial_and_combination() {
        // PATCH (`partial=True`): absent keys skipped; present-but-invalid
        // keys still fail; unknown keys ignored.
        let body = serde_json::json!({});
        assert!(create(&body, true).is_ok());
        let body = serde_json::json!({"access": "bogus", "bogus": 1});
        assert_eq!(
            create(&body, true)
                .expect_err("partial bad access fails")
                .body(),
            r#"{"access":["\"bogus\" is not a valid choice."]}"#
        );
        // Errors combine in `Meta.fields` order across all nine fields.
        let body = serde_json::json!({
            "speaker_agent_run_id": "zz",
            "labels": "x",
            "access": "bogus",
            "comment_json": Value::Null,
        });
        assert_eq!(
            create(&body, false).expect_err("combined fails").body(),
            concat!(
                r#"{"comment_json":["This field may not be null."],"#,
                r#""access":["\"bogus\" is not a valid choice."],"#,
                r#""labels":["Expected a list of items but got type \"str\"."],"#,
                r#""speaker_agent_run_id":["Must be a valid UUID."]}"#,
            )
        );
        // Non-dict bodies (incl. the null edge) fail before field validation.
        for raw in [serde_json::json!([1]), Value::Null] {
            let err = create(&raw, false).expect_err("non-dict fails");
            assert_eq!(err.status(), 400);
        }
        assert_eq!(
            create(&Value::Null, false)
                .expect_err("null body fails")
                .body(),
            r#"{"non_field_errors":["No data provided"]}"#
        );
        assert_eq!(
            create(&serde_json::json!("x"), true)
                .expect_err("str body fails")
                .body(),
            r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got str."]}"#
        );
    }

    #[test]
    fn py_str_shapes() {
        // Container `str()` spellings used by `invalid_choice` messages.
        assert_eq!(py_str(&serde_json::json!({"a": 1})), "{'a': 1}");
        assert_eq!(
            py_str(&serde_json::json!({"a": [1, true, null, "x'y"]})),
            r#"{'a': [1, True, None, "x'y"]}"#
        );
        assert_eq!(py_repr_str("plain"), "'plain'");
        assert_eq!(py_repr_str("a'b"), "\"a'b\"");
        assert_eq!(py_repr_str("a'b\"c"), "'a\\'b\"c'");
        assert_eq!(py_repr_str("a\nb"), "'a\\nb'");
        assert_eq!(py_repr_str(""), "''");
    }

    // ---- F18-03 replays: IssueCommentSerializer.validate --------------------

    #[test]
    fn validate_html_ok_matches_f18_03() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        let golden = &comment["validate_html_ok"];
        assert!(golden["valid"].as_bool().expect("valid is a bool"));
        let out = validate_comment_html(&CommentHtmlInput {
            value: Some("<p>x</p>"),
            roundtripped: Some("<p>x</p>"),
        })
        .expect("round-trip substitutes");
        assert_eq!(out.as_deref(), Some("<p>x</p>"));
        assert_eq!(
            golden["validated"]["comment_html"].as_str(),
            Some(py_repr("<p>x</p>")).as_deref()
        );
    }

    #[test]
    fn validate_html_empty_matches_f18_03() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        // Empty input raises in lxml (`roundtripped: None`).
        let err = validate_comment_html(&CommentHtmlInput {
            value: Some(""),
            roundtripped: None,
        })
        .expect_err("empty html fails");
        assert_eq!(err.status(), 400);
        assert_eq!(
            err.body(),
            r#"{"non_field_errors":["Invalid HTML passed"]}"#
        );
        let golden = &comment["validate_html_empty"]["errors"]["non_field_errors"][0];
        assert_eq!(golden["message"].as_str(), Some("Invalid HTML passed"));
        assert_eq!(golden["code"].as_str(), Some("invalid"));
    }

    #[test]
    fn validate_html_passthrough() {
        // Absent/null `comment_html` passes through untouched, whatever the
        // parser fact says (the branch never runs).
        for roundtripped in [None, Some("<p>x</p>")] {
            let out = validate_comment_html(&CommentHtmlInput {
                value: None,
                roundtripped,
            })
            .expect("absent passes");
            assert_eq!(out, None);
        }
        // Present values substitute the round-tripped string verbatim.
        let out = validate_comment_html(&CommentHtmlInput {
            value: Some("<p>x"),
            roundtripped: Some("<p>x</p>"),
        })
        .expect("substitution passes");
        assert_eq!(out.as_deref(), Some("<p>x</p>"));
    }

    // ---- F18-03 replays: IssueCommentSerializer render ----------------------

    fn comment_row<'a>(
        render: &'a Value,
        is_member: Option<bool>,
        url: Option<String>,
        attachments: &'a [Option<&'a str>],
        labels: &'a [Option<&'a str>],
    ) -> CommentRow<'a> {
        CommentRow {
            id: fx_raw(render, "id"),
            is_member,
            url,
            created_at: fx_raw(render, "created_at"),
            updated_at: fx_raw(render, "updated_at"),
            deleted_at: fx_opt(render, "deleted_at"),
            comment_html: fx_raw(render, "comment_html"),
            attachments,
            labels,
            access: fx_raw(render, "access"),
            external_source: fx_opt(render, "external_source"),
            external_id: fx_opt(render, "external_id"),
            speaker_type: fx_raw(render, "speaker_type"),
            speaker_label: fx_raw(render, "speaker_label"),
            speaker_agent_run_id: fx_opt(render, "speaker_agent_run_id"),
            edited_at: fx_opt(render, "edited_at"),
            created_by: fx_opt(render, "created_by"),
            updated_by: fx_opt(render, "updated_by"),
            project: fx_raw(render, "project"),
            workspace: fx_raw(render, "workspace"),
            description: fx_opt(render, "description"),
            issue: fx_raw(render, "issue"),
            actor: fx_opt(render, "actor"),
            parent: fx_opt(render, "parent"),
        }
    }

    #[test]
    fn comment_keys_match_f18_03() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        // The golden probe row carried no `is_member` annotation, so DRF
        // `SkipField` dropped it: golden keys == read fields minus that slot.
        let mut expected: Vec<&str> = COMMENT_READ_FIELDS.to_vec();
        assert_eq!(expected.remove(1), "is_member");
        assert_eq!(str_list(&comment["render_keys"]), expected);
    }

    #[test]
    fn comment_render_matches_f18_03() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        let render = &comment["render"];
        let empty: [Option<&str>; 0] = [];
        // Both golden lists are `[]`.
        assert_eq!(fx_raw(render, "attachments"), "[]");
        assert_eq!(fx_raw(render, "labels"), "[]");
        let row = comment_row(
            render,
            None,
            Some(fx_raw(render, "url").to_string()),
            &empty,
            &empty,
        );
        let out = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("fixture row renders");
        assert_render(&out, comment);
        // Literal spots (independent of the repr converter).
        assert_eq!(
            out["url"],
            Value::String("http://127.0.0.1:18359/ws-conv659-2/browse/CT00003-1".to_string())
        );
        assert_eq!(
            out["comment_html"],
            Value::String("<p>test comment</p>".to_string())
        );
        assert_eq!(out["access"], Value::String("EXTERNAL".to_string()));
        assert_eq!(out["speaker_type"], Value::String("human".to_string()));
        assert_eq!(out["speaker_label"], Value::String(String::new()));
        assert_eq!(out["attachments"], serde_json::json!([]));
        assert_eq!(out["labels"], serde_json::json!([]));
        assert!(
            !out.contains_key("is_member"),
            "un-annotated row drops is_member"
        );
    }

    #[test]
    fn comment_is_member_annotated() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        let render = &comment["render"];
        let empty: [Option<&str>; 0] = [];
        // Annotated rows carry `is_member` as the 2nd key.
        for member in [true, false] {
            let row = comment_row(
                render,
                Some(member),
                Some(fx_raw(render, "url").to_string()),
                &empty,
                &empty,
            );
            let out = render_comment(&CommentRepresentationInput {
                row: &row,
                fields: None,
                expand: &[],
                expansions: &[],
            })
            .expect("annotated row renders");
            assert_eq!(out_keys(&out), COMMENT_READ_FIELDS);
            assert_eq!(out["is_member"], Value::Bool(member));
        }
        // It is also `fields=`-addressable.
        let row = comment_row(
            render,
            Some(true),
            Some(fx_raw(render, "url").to_string()),
            &empty,
            &empty,
        );
        let out = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: Some(&includes(&["is_member", "id"])),
            expand: &[],
            expansions: &[],
        })
        .expect("is_member subset renders");
        assert_eq!(out_keys(&out), vec!["id", "is_member"]);
    }

    #[test]
    fn comment_lists_render_items() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        let render = &comment["render"];
        let attachments = [Some("https://files.example/a.png")];
        let labels = [Some("fold")];
        let row = comment_row(
            render,
            None,
            Some(fx_raw(render, "url").to_string()),
            &attachments,
            &labels,
        );
        let out = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: Some(&includes(&["attachments", "labels"])),
            expand: &[],
            expansions: &[],
        })
        .expect("lists render");
        assert_eq!(
            out["attachments"],
            serde_json::json!(["https://files.example/a.png"])
        );
        assert_eq!(out["labels"], serde_json::json!(["fold"]));
        // A Postgres NULL array element renders null (DRF `to_representation`
        // passes None items through).
        let with_null = [Some("fold"), None];
        let row = comment_row(
            render,
            None,
            Some(fx_raw(render, "url").to_string()),
            &attachments,
            &with_null,
        );
        let out = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: Some(&includes(&["labels"])),
            expand: &[],
            expansions: &[],
        })
        .expect("null item renders");
        assert_eq!(out["labels"], serde_json::json!(["fold", null]));
    }

    #[test]
    fn comment_url_omitted_when_unconfigured() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        assert!(comment["url_omitted_when_unconfigured"]
            .as_bool()
            .expect("flag is a bool"));
        let render = &comment["render"];
        let empty: [Option<&str>; 0] = [];
        let row = comment_row(render, None, None, &empty, &empty);
        let out = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("unconfigured row renders");
        assert!(!out.contains_key("url"), "null url is omitted, not nulled");
    }

    #[test]
    fn comment_get_url_matches_f18_03() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        // The golden URL decomposes into the `get_url` arguments
        // (`workspace.slug`, `project.identifier`, `issue.sequence_id`).
        let base = web_base_url(Some("http://127.0.0.1:18359"), None);
        let url = issue_url(
            base.as_deref(),
            Some("ws-conv659-2"),
            Some("CT00003"),
            Some(1),
        );
        assert_eq!(url.as_deref(), comment["get_url"].as_str());
        assert_eq!(
            url.as_deref(),
            Some("http://127.0.0.1:18359/ws-conv659-2/browse/CT00003-1")
        );
        // Unconfigured base or a missing part yields `None` (omitted).
        assert_eq!(issue_url(None, Some("ws"), Some("P"), Some(1)), None);
        assert_eq!(
            issue_url(
                web_base_url(None, None).as_deref(),
                Some("ws"),
                Some("P"),
                Some(1)
            ),
            None
        );
    }

    #[test]
    fn comment_expand_behaviors() {
        let fx = fixture(F18_03);
        let comment = unit(&fx, "IssueCommentSerializer");
        let render = &comment["render"];
        let empty: [Option<&str>; 0] = [];
        let row = comment_row(
            render,
            Some(true),
            Some(fx_raw(render, "url").to_string()),
            &empty,
            &empty,
        );
        // Map hit renders the caller value; null FK renders `{}`.
        let actor = serde_json::json!({"id": "a1"});
        let out = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: None,
            expand: &["actor", "parent", "comment_html"],
            expansions: &[("actor", Some(actor.clone())), ("parent", None)],
        })
        .expect("expansions render");
        assert_eq!(out["actor"], actor);
        assert_eq!(out["parent"], serde_json::json!({}));
        assert_eq!(out["comment_html"], Value::Null);
        // `?expand=url` removes the key (nulled, then popped).
        let out = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: None,
            expand: &["url"],
            expansions: &[],
        })
        .expect("expand url renders");
        assert!(!out.contains_key("url"));
        // `description` is the FK passthrough (not in the map).
        let out = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: None,
            expand: &["description"],
            expansions: &[],
        })
        .expect("passthrough renders");
        assert_eq!(out["description"], Value::Null);
        // Missing caller value errors.
        let err = render_comment(&CommentRepresentationInput {
            row: &row,
            fields: None,
            expand: &["actor"],
            expansions: &[],
        })
        .expect_err("missing caller value errors");
        assert_eq!(
            err,
            SocialRenderError::MissingExpansion("actor".to_string())
        );
    }

    #[test]
    fn comment_create_render_shape() {
        // No golden pins this read shape (it feeds the activity payload at
        // `views/issue.py:1930`); keys and values follow the nine `Meta.fields`.
        let json = serde_json::json!({"a": 1});
        let labels = [Some("fold")];
        let row = CommentCreateRow {
            comment_json: &json,
            comment_html: "<p>hi</p>",
            access: "EXTERNAL",
            external_source: Some("github"),
            external_id: None,
            labels: &labels,
            speaker_type: "agent",
            speaker_label: "bot",
            speaker_agent_run_id: Some("6353d7bf-012d-45ba-82a1-d2ff209af168"),
        };
        let out = render_comment_create(&CommentCreateRepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("create shape renders");
        assert_eq!(out_keys(&out), COMMENT_CREATE_FIELDS);
        assert_eq!(out["comment_json"], serde_json::json!({"a": 1}));
        assert_eq!(out["comment_html"], Value::String("<p>hi</p>".to_string()));
        assert_eq!(out["access"], Value::String("EXTERNAL".to_string()));
        assert_eq!(out["external_source"], Value::String("github".to_string()));
        assert_eq!(out["external_id"], Value::Null);
        assert_eq!(out["labels"], serde_json::json!(["fold"]));
        assert_eq!(out["speaker_type"], Value::String("agent".to_string()));
        assert_eq!(out["speaker_label"], Value::String("bot".to_string()));
        assert_eq!(
            out["speaker_agent_run_id"],
            Value::String("6353d7bf-012d-45ba-82a1-d2ff209af168".to_string())
        );
        // Every `expand` name nulls its scalar (no map hits among the nine).
        let out = render_comment_create(&CommentCreateRepresentationInput {
            row: &row,
            fields: Some(&includes(&["access", "labels"])),
            expand: &["access"],
            expansions: &[],
        })
        .expect("create expand renders");
        assert_eq!(out_keys(&out), vec!["access", "labels"]);
        assert_eq!(out["access"], Value::Null);
    }

    // ---- F18-03 replays: IssueActivitySerializer ----------------------------

    fn activity_row<'a>(render: &'a Value, attachments: &'a [Option<&'a str>]) -> ActivityRow<'a> {
        ActivityRow {
            id: fx_raw(render, "id"),
            created_at: fx_raw(render, "created_at"),
            updated_at: fx_raw(render, "updated_at"),
            deleted_at: fx_opt(render, "deleted_at"),
            verb: fx_raw(render, "verb"),
            field: fx_opt(render, "field"),
            old_value: fx_opt(render, "old_value"),
            new_value: fx_opt(render, "new_value"),
            comment: fx_raw(render, "comment"),
            attachments,
            old_identifier: fx_opt(render, "old_identifier"),
            new_identifier: fx_opt(render, "new_identifier"),
            epoch: match fx_raw(render, "epoch") {
                "None" => None,
                other => Some(other.parse().expect("epoch is a float")),
            },
            project: fx_raw(render, "project"),
            workspace: fx_raw(render, "workspace"),
            issue: fx_opt(render, "issue"),
            issue_comment: fx_opt(render, "issue_comment"),
            actor: fx_opt(render, "actor"),
        }
    }

    #[test]
    fn activity_keys_match_f18_03() {
        let fx = fixture(F18_03);
        let act = unit(&fx, "IssueActivitySerializer");
        assert_eq!(str_list(&act["render_keys"]), ACTIVITY_READ_FIELDS);
    }

    #[test]
    fn activity_render_matches_f18_03() {
        let fx = fixture(F18_03);
        let act = unit(&fx, "IssueActivitySerializer");
        let render = &act["render"];
        assert_eq!(fx_raw(render, "attachments"), "[]");
        let empty: [Option<&str>; 0] = [];
        let row = activity_row(render, &empty);
        let out = render_activity(&ActivityRepresentationInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("fixture row renders");
        assert_render(&out, act);
        // Literal spots (independent of the repr converter).
        assert_eq!(out["verb"], Value::String("created".to_string()));
        assert_eq!(
            out["comment"],
            Value::String("contract activity".to_string())
        );
        assert_eq!(out["attachments"], serde_json::json!([]));
        assert_eq!(out["epoch"], Value::Null);
        assert_eq!(out["old_identifier"], Value::Null);
        assert!(!out.contains_key("created_by"), "created_by is excluded");
        assert!(!out.contains_key("updated_by"), "updated_by is excluded");
    }

    #[test]
    fn activity_expand_and_edges() {
        let fx = fixture(F18_03);
        let act = unit(&fx, "IssueActivitySerializer");
        let render = &act["render"];
        let files = [Some("https://files.example/b.png")];
        let row = activity_row(render, &files);
        // Map hit with a value; scalar else-branch nulls; `issue_comment` is
        // the FK passthrough; items render.
        let issue = serde_json::json!({"id": "i1"});
        let out = render_activity(&ActivityRepresentationInput {
            row: &row,
            fields: None,
            expand: &["issue", "verb", "issue_comment"],
            expansions: &[("issue", Some(issue.clone()))],
        })
        .expect("expansions render");
        assert_eq!(out["issue"], issue);
        assert_eq!(out["verb"], Value::Null);
        assert_eq!(out["issue_comment"], Value::Null);
        assert_eq!(
            out["attachments"],
            serde_json::json!(["https://files.example/b.png"])
        );
        // Finite epoch renders; non-finite is a 500-class failure.
        let mut row2 = activity_row(render, &[]);
        row2.epoch = Some(1.5);
        let out = render_activity(&ActivityRepresentationInput {
            row: &row2,
            fields: Some(&includes(&["epoch"])),
            expand: &[],
            expansions: &[],
        })
        .expect("epoch renders");
        assert_eq!(out["epoch"], serde_json::json!(1.5));
        row2.epoch = Some(f64::INFINITY);
        let err = render_activity(&ActivityRepresentationInput {
            row: &row2,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect_err("infinite epoch errors");
        assert_eq!(err, SocialRenderError::NonFiniteFloat { field: "epoch" });
        // Missing caller value errors.
        let err = render_activity(&ActivityRepresentationInput {
            row: &row,
            fields: None,
            expand: &["actor"],
            expansions: &[],
        })
        .expect_err("missing caller value errors");
        assert_eq!(
            err,
            SocialRenderError::MissingExpansion("actor".to_string())
        );
    }

    #[test]
    fn char_validators_use_python_float_spelling() {
        // `str(1e100)` is `1e+100` (PIDASHCONV-758). `serde` keeps
        // non-canonical layouts verbatim; the small-exponent cases
        // diverge even without `arbitrary_precision` (`zmij` spells
        // `0.00001` and `1e-7`).
        let float: Value = serde_json::from_str("1e100").expect("parses");
        assert_eq!(
            validate_char(Some(&float), false, false, false, None),
            Ok(CharOutcome::Text("1e+100".to_owned()))
        );
        let tiny: Value = serde_json::from_str("0.00001").expect("parses");
        assert_eq!(
            validate_char(Some(&tiny), false, false, false, None),
            Ok(CharOutcome::Text("1e-05".to_owned()))
        );
        assert_eq!(
            validate_choice(Some(&float), &["a"]),
            Err(ChoiceError::InvalidChoice("1e+100".to_owned()))
        );
        let neg: Value = serde_json::from_str("1e-7").expect("parses");
        assert_eq!(py_str(&float), "1e+100");
        assert_eq!(py_str(&tiny), "1e-05");
        assert_eq!(py_str(&neg), "1e-07");
    }
}
