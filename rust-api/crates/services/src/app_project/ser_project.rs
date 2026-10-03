//! Project serializers, core (D-25 L1, PIDASHCONV-563).
//!
//! Ports `apps/api/pi_dash/app/serializers/project.py:30-190,259-266`
//! (`ProjectSerializer` `:30-117`, `ProjectLiteSerializer` `:120-133`,
//! `ProjectListSerializer` `:136-168`, `ProjectDetailSerializer` `:171-190`,
//! `DeployBoardSerializer` `:259-266`) over `DynamicBaseSerializer`
//! (`app/serializers/base.py:12-201`).
//!
//! * `validate_name` / `validate_identifier` (`:47-83`) — the
//!   `FORBIDDEN_IDENTIFIER_CHARS_PATTERN` check
//!   (`db/models/project.py:226`) plus the same-workspace duplicate check
//!   with self-exclusion on update. The duplicate verdict arrives as a
//!   caller-supplied boolean; the `exclude(id)` + soft-delete-scoped
//!   `exists()` SQL belongs to the queries layer (PIDASHCONV-568).
//! * `validate` (`:85-108`) — the `default_agent_executor == "cloud_agent"`
//!   gate (D-11, consumed — never reimplemented — from
//!   `crate::dispatch`), the is-default-unset guard, and the
//!   `description_html` sanitize branch (`utils/content_validator.py:211-241`,
//!   whose `nh3.clean` verdict arrives caller-supplied; the handler layer
//!   runs the cleaner).
//! * `create` (`:110-117`) — the `Project` row plus the `ProjectIdentifier`
//!   row. The handler performs the writes; here are the identifier
//!   normalisation and the identifier-row name derivation.
//! * `get_members` / `get_next_work_item_sequence` (`:154-168`) — pure
//!   kernels over caller-supplied rows / `Max` verdict.
//! * `get_agent_executor_options` (`:35-40,:147-152,:181-186`) — the
//!   request-user derivation (`None` when missing or unauthenticated) plus
//!   delegation to D-11's [`policy::agent_executor_options`](crate::dispatch::policy::agent_executor_options).
//! * `DynamicBaseSerializer` `fields=` / `expand=` (`base.py:12-201`) — the
//!   fields-dead rule, the `__init__` addition kernel with its
//!   KeyError/TypeError edges, and the `to_representation` overwrite rule.
//! * Read shapes — `Serialize` structs declaring fields in DRF wire order
//!   for all five serializers, over caller-supplied already-rendered
//!   values (datetimes and UUID/FK keys as strings, JSON columns and
//!   nested objects as [`Value`]).
//!
//! D-11 calls, consumed verbatim (PIDASHCONV-484/485, both Done):
//! [`cloud_agent_is_configured`](crate::dispatch::policy::cloud_agent_is_configured),
//! [`agent_executor_options`](crate::dispatch::policy::agent_executor_options)
//! (+ [`UserFlags`](crate::dispatch::policy::UserFlags),
//! [`ManagedAvailability`](crate::dispatch::policy::ManagedAvailability),
//! [`ExecutorOption`](crate::dispatch::policy::ExecutorOption)) and
//! [`CloudAgentUnavailableBody`](crate::dispatch::admission::CloudAgentUnavailableBody)
//! (+
//! [`CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS`](crate::dispatch::admission::CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS)).
//!
//! Wire rules (all verified against live DRF through the fixture or the
//! repo Python: DRF 3.15.2 in the fixture env, 3.18.1 locally):
//!
//! * Key order is DRF order: `id` (declared on `BaseSerializer`), then the
//!   serializer's declared fields, then the model fields. The fixture's
//!   `key_order` arrays are authoritative; the structs below declare fields
//!   in exactly that order so the serialized bytes match (struct field
//!   order is preserved without `preserve_order`; multi-key bodies are
//!   never built with `json!`).
//! * UUID and FK primary keys render as strings (`PrimaryKeyRelatedField`,
//!   read-only); a null FK renders `null`. `workspace_detail` /
//!   `project_details` / `default_assignee` / `project_lead` nest the D-24
//!   lite shapes (or `null` when the FK is null); those shapes belong to
//!   other domains and are referenced here by name only, arriving as
//!   caller-supplied [`Value`]s already rendered in DRF key order
//!   (insertion order is the wire order under `preserve_order`).
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings — formatting owns to the DB edge, so rendering here is a
//!   byte-exact passthrough.
//! * A read-only field whose attribute is missing on the instance raises
//!   DRF `SkipField` and the key is ABSENT, not `null`. Live on the create
//!   path (`app/views/project/base.py:310` renders `ProjectListSerializer`
//!   on a fresh instance with no `is_favorite` / `sort_order` /
//!   `member_role` / `anchor` annotations), so those four list/detail
//!   fields are presence-`Option`s with `skip_serializing_if`. A present
//!   annotation holding `None` (e.g. no `DeployBoard` row for `anchor`)
//!   still renders `null` — hence the double `Option` on the nullable
//!   three.
//! * `sort_order` renders as a JSON number (`65535.0`); `f64` prints whole
//!   values with `.0` exactly like Python.
//! * Field-validator failures (`detail="..."`) render `{"field":
//!   ["..."]}`; a `validate()` string raise renders
//!   `{"non_field_errors": ["..."]}`; a `validate()` dict raise renders
//!   each value list-wrapped (`{"error": ["html content is not valid"]}`).
//!   The `CloudAgentUnavailableAPI` raise is NOT a `ValidationError` — it
//!   propagates past the serializer as its own 409 response.
//! * DRF `CharField` input runs before `validate_name` /
//!   `validate_identifier`: absent → required, explicit `null` → null,
//!   `""` → blank, bool/dict/list → invalid, int/float → `str()`, strip,
//!   then `max_length` (code points). The stripped value is what the
//!   `validate_*` checks and `validated_data` carry.
//! * `validate()` runs only when no field errored; inside it the first
//!   raise wins, in source order (executor gate, is-default guard, html
//!   branch). Field errors across fields collect (`{"name": [...],
//!   "identifier": [...]}`).
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * B-fields-dead (`base.py:14-18`): `DynamicBaseSerializer.__init__`
//!   pops the `fields` kwarg and immediately overwrites it with
//!   `self.expand`, so `?fields=` on the project list
//!   (`app/views/project/base.py:102,140`, the only D-25 `fields=` call
//!   site — `expand=` is never passed) is dead and full objects always
//!   render. [`effective_expand`] ports the overwrite.
//! * B-case-dup (`serializers/project.py:66-83` vs
//!   `db/models/project.py:258`): `validate_identifier` matches the
//!   raw (stripped, still lowercased) identifier case-sensitively, but
//!   `Project.save()` uppercases before the insert — so `abc` against a
//!   stored `ABC` passes validation and then hits the unique index
//!   (500/`IntegrityError` at the DB layer, handler-owned).
//! * B-nested-expand (`base.py:41-42`): `_filter_fields` recurses as
//!   `self._filter_fields(self.fields[key], value)`, passing a `Field`
//!   where the `fields` list belongs — an unknown key raises `KeyError`
//!   first (`self.fields[key]`), a known key raises `TypeError` (3 args
//!   for 2 params), so ANY `{key: [...]}` `expand=` entry raises.
//!   [`filter_field_additions`] returns [`ExpandError`] for that case. No
//!   D-25 view passes `expand=`, so the kernel path is latent here.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` constrain writes, of which the shape ports have none
//! (kept as `*_READ_ONLY_FIELDS` consts for the handler layer).
//! `Project.save()` (`db/models/project.py:255-299`, incl. the identifier
//! uppercase, the workspace-timezone default, the first-project default
//! and the atomic default swap + model backstop) belongs to the models
//! layer (PIDASHCONV-567); `create()` relies on it and the docs cite the
//! exact lines. The `issue_attachments` appendage in
//! `DynamicBaseSerializer.to_representation` (`base.py:183-199`, a
//! `FileAsset` query) is unreachable for project instances —
//! `ProjectListSerializer` has no such field and no D-25 view passes that
//! `expand=` — so it is documented, not ported.
//!
//! Fixture: `rust-api/fixtures/app_project/FX-APROJ-01.serializers_project.json`
//! (FX-APROJ-01).

use crate::dispatch::admission::{CloudAgentUnavailableBody, CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS};
use crate::dispatch::policy::{ExecutorOption, ManagedAvailability, UserFlags};
use pidash_db::config::CloudAgentSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Input field parsing: name / identifier (`:47-83` run after DRF CharField)
// ---------------------------------------------------------------------------

/// `Project.name` column limit (`db/models/project.py:74`,
/// `max_length=255`), enforced by DRF's `CharField` before `validate_name`.
pub const MAX_NAME_CHARS: usize = 255;

/// `Project.identifier` column limit (`db/models/project.py:80`,
/// `max_length=12`), enforced by DRF's `CharField` before
/// `validate_identifier`.
pub const MAX_IDENTIFIER_CHARS: usize = 12;

/// Missing required field (DRF `CharField`, `required=True` default).
pub const MSG_REQUIRED: &str = "This field is required.";
/// Explicit JSON null (DRF `CharField`, `allow_null=False` default).
pub const MSG_NULL: &str = "This field may not be null.";
/// Empty value (DRF `CharField`, `allow_blank=False` default).
pub const MSG_BLANK: &str = "This field may not be blank.";
/// Bool/dict/list value (DRF `CharField.to_internal_value`; int/float
/// coerce via `str()` instead).
pub const MSG_INVALID: &str = "Not a valid string.";
/// Over-`max_length` name (DRF `CharField`, `max_length=255` from the model).
pub const MSG_NAME_MAX_LENGTH: &str = "Ensure this field has no more than 255 characters.";
/// Over-`max_length` identifier (`max_length=12` from the model).
pub const MSG_IDENTIFIER_MAX_LENGTH: &str = "Ensure this field has no more than 12 characters.";

/// `validate_name` forbidden-characters detail
/// (`serializers/project.py:51`).
pub const NAME_FORBIDDEN_DETAIL: &str = "PROJECT_NAME_CANNOT_CONTAIN_SPECIAL_CHARACTERS";
/// `validate_name` duplicate detail (`:59-62`).
pub const NAME_TAKEN_DETAIL: &str = "PROJECT_NAME_ALREADY_EXIST";
/// `validate_identifier` forbidden-characters detail (`:70`).
pub const IDENTIFIER_FORBIDDEN_DETAIL: &str =
    "PROJECT_IDENTIFIER_CANNOT_CONTAIN_SPECIAL_CHARACTERS";
/// `validate_identifier` duplicate detail (`:78-81`).
pub const IDENTIFIER_TAKEN_DETAIL: &str = "PROJECT_IDENTIFIER_ALREADY_EXIST";

/// Renders a single-field error body, `{"<field>":["<detail>"]}` — the DRF
/// shape for both field-machinery and `validate_<field>` failures. Every
/// detail below is fixed ASCII without quotes or backslashes, so the
/// formatted bytes are exact.
pub fn field_error_body(field: &str, detail: &str) -> String {
    format!("{{\"{field}\":[\"{detail}\"]}}")
}

/// The empty-payload envelope (`is_valid_envelopes.empty`): both required
/// fields missing, in field order.
pub const EMPTY_REQUIRED_BODY: &str =
    "{\"name\":[\"This field is required.\"],\"identifier\":[\"This field is required.\"]}";

/// Failure of DRF `CharField` input parsing for `name` / `identifier`,
/// before `validate_*` runs. Variants are in check order (absent, null,
/// blank, wrong type, over length).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldInputError {
    /// The key is absent (DRF `required`).
    Required,
    /// Explicit JSON null (DRF `allow_null=False`).
    Null,
    /// `""` (DRF `allow_blank=False`).
    Blank,
    /// Bool/dict/list (DRF `invalid`).
    InvalidType,
    /// Stripped value over `max_length` (code points).
    MaxLength,
}

impl FieldInputError {
    /// The DRF message for `name` inputs.
    pub fn name_detail(self) -> &'static str {
        match self {
            FieldInputError::Required => MSG_REQUIRED,
            FieldInputError::Null => MSG_NULL,
            FieldInputError::Blank => MSG_BLANK,
            FieldInputError::InvalidType => MSG_INVALID,
            FieldInputError::MaxLength => MSG_NAME_MAX_LENGTH,
        }
    }

    /// The DRF message for `identifier` inputs.
    pub fn identifier_detail(self) -> &'static str {
        match self {
            FieldInputError::Required => MSG_REQUIRED,
            FieldInputError::Null => MSG_NULL,
            FieldInputError::Blank => MSG_BLANK,
            FieldInputError::InvalidType => MSG_INVALID,
            FieldInputError::MaxLength => MSG_IDENTIFIER_MAX_LENGTH,
        }
    }
}

/// Ports DRF `CharField` input parsing for `name` (`None` = key absent):
/// absent → required, null → null, `""` → blank, bool/dict/list →
/// invalid, int/float → `str()`, strip, over 255 chars → max_length.
/// Returns the stripped value `validate_name` checks.
pub fn parse_name_input(value: Option<&Value>) -> Result<String, FieldInputError> {
    parse_char_input(value, MAX_NAME_CHARS)
}

/// Ports DRF `CharField` input parsing for `identifier`, same rules with
/// `max_length=12`. Returns the stripped (NOT uppercased — B-case-dup)
/// value `validate_identifier` checks.
pub fn parse_identifier_input(value: Option<&Value>) -> Result<String, FieldInputError> {
    parse_char_input(value, MAX_IDENTIFIER_CHARS)
}

fn parse_char_input(value: Option<&Value>, max_chars: usize) -> Result<String, FieldInputError> {
    let Some(value) = value else {
        return Err(FieldInputError::Required);
    };
    if value.is_null() {
        return Err(FieldInputError::Null);
    }
    // DRF coerces int/float via str() but rejects bools: `isinstance(data,
    // bool) or not isinstance(data, (str, int, float))` fails 'invalid'.
    // serde_json booleans are distinct from numbers, so match arms stay
    // exact (a JSON 1/0 is a number, never a bool).
    let raw = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return Err(FieldInputError::InvalidType),
    };
    if raw.is_empty() {
        return Err(FieldInputError::Blank);
    }
    // `trim_whitespace=True` (DRF default); `str::trim` strips Unicode
    // whitespace like Python's `strip`.
    let stripped = raw.trim().to_string();
    if stripped.chars().count() > max_chars {
        return Err(FieldInputError::MaxLength);
    }
    Ok(stripped)
}

// ---------------------------------------------------------------------------
// validate_name / validate_identifier (`:47-83`)
// ---------------------------------------------------------------------------

/// `Project.FORBIDDEN_IDENTIFIER_CHARS_PATTERN`
/// (`db/models/project.py:226`), applied with `re.match`. (The v1
/// `ProjectCreateSerializer` port carries the same table for
/// `api/serializers/project.py`; each domain ports what its own units
/// check.)
pub const FORBIDDEN_IDENTIFIER_CHARS_PATTERN: &str = r"^.*[&+,:;$^}{*=?@#|'<>.()%!-].*$";

/// The 24 forbidden characters of the class above, in pattern order.
/// Kept as a table so no `regex` dependency is needed.
pub const FORBIDDEN_CHARS: &[char] = &[
    '&', '+', ',', ':', ';', '$', '^', '}', '{', '*', '=', '?', '@', '#', '|', '\'', '<', '>', '.',
    '(', ')', '%', '!', '-',
];

/// Ports `re.match(FORBIDDEN_IDENTIFIER_CHARS_PATTERN, value)` byte for
/// byte. Python `.` never crosses `\n` and `$` only anchors at the end, so
/// a value with an interior newline can never match (verified against
/// CPython: `"a&b\nc"`, `"a\nb&c"`, `"&\n&"` all pass while `"a&b"`,
/// `"a&b\n"`, `"a\rb&c"` are rejected). Equivalent rule: strip one trailing
/// newline; reject iff the rest is single-line and holds a forbidden char.
pub fn contains_forbidden_chars(value: &str) -> bool {
    let single_line = value.strip_suffix('\n').unwrap_or(value);
    if single_line.contains('\n') {
        return false;
    }
    single_line.chars().any(|c| FORBIDDEN_CHARS.contains(&c))
}

/// Failure of `validate_name` (`:47-64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// Forbidden characters (`:51`).
    Forbidden,
    /// Same-workspace duplicate (`:59-62`).
    Taken,
}

impl NameError {
    /// The wire detail (`{"name": ["..."]}` via [`field_error_body`]).
    pub fn detail(self) -> &'static str {
        match self {
            NameError::Forbidden => NAME_FORBIDDEN_DETAIL,
            NameError::Taken => NAME_TAKEN_DETAIL,
        }
    }
}

/// Ports `validate_name` over the parsed (stripped) value: forbidden
/// characters first, then the duplicate verdict. `name_taken` is the
/// queries-layer `Project.objects.filter(name=..., workspace_id=...)`
/// (`.exclude(id)` on update, soft-delete-scoped default manager)
/// `.exists()` verdict.
pub fn check_name_value(name: &str, name_taken: bool) -> Result<(), NameError> {
    if contains_forbidden_chars(name) {
        return Err(NameError::Forbidden);
    }
    if name_taken {
        return Err(NameError::Taken);
    }
    Ok(())
}

/// Failure of `validate_identifier` (`:66-83`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierError {
    /// Forbidden characters (`:70`).
    Forbidden,
    /// Same-workspace duplicate (`:78-81`).
    Taken,
}

impl IdentifierError {
    /// The wire detail (`{"identifier": ["..."]}` via [`field_error_body`]).
    pub fn detail(self) -> &'static str {
        match self {
            IdentifierError::Forbidden => IDENTIFIER_FORBIDDEN_DETAIL,
            IdentifierError::Taken => IDENTIFIER_TAKEN_DETAIL,
        }
    }
}

/// Ports `validate_identifier` over the parsed (stripped, still
/// lowercased) value: forbidden characters first, then the duplicate
/// verdict. `identifier_taken` is the queries-layer exact-match
/// `.exists()` verdict (`.exclude(id)` on update) — exact-case, which is
/// the B-case-dup gap versus `save()`'s uppercase.
pub fn check_identifier_value(
    identifier: &str,
    identifier_taken: bool,
) -> Result<(), IdentifierError> {
    if contains_forbidden_chars(identifier) {
        return Err(IdentifierError::Forbidden);
    }
    if identifier_taken {
        return Err(IdentifierError::Taken);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// validate() (`:85-108`): executor gate, is-default guard, html branch
// ---------------------------------------------------------------------------

/// `AgentExecutorKind.CLOUD_AGENT` value
/// (`core/agent_execution.py:7-16`), the only gated kind in this
/// serializer's `validate` (there is no managed branch here, unlike the
/// v1 serializers).
pub const EXECUTOR_CLOUD_AGENT: &str = "cloud_agent";

/// Outcome of the `validate()` executor gate (`:86-92`):
/// `data.get("default_agent_executor") == "cloud_agent"` while
/// `cloud_agent_is_configured()` is false raises
/// `CloudAgentUnavailableAPI`, which propagates as its own 409 response —
/// never as a serializer 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorGate {
    /// Any other kind, or cloud while configured.
    Ok,
    /// `cloud_agent` while unconfigured: the handler answers 409 with
    /// [`CloudAgentUnavailableBody`] ([`CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS`]).
    CloudUnavailable,
}

/// Ports the executor gate: `executor` is the raw
/// `default_agent_executor` input (`None` when absent — `data.get(...)`
/// misses, and `None != "cloud_agent"` passes), `cloud_configured` is the
/// D-11 [`cloud_agent_is_configured`](crate::dispatch::policy::cloud_agent_is_configured)
/// verdict. Exact string equality, like Python's `==`.
pub fn check_executor_gate(executor: Option<&str>, cloud_configured: bool) -> ExecutorGate {
    match executor {
        Some(e) if e == EXECUTOR_CLOUD_AGENT && !cloud_configured => ExecutorGate::CloudUnavailable,
        _ => ExecutorGate::Ok,
    }
}

/// The 409 body for [`ExecutorGate::CloudUnavailable`], consumed from D-11
/// (L4) rather than redefined: `{"error": "Pi Dash Cloud Agent is not
/// currently available", "code": "cloud_agent_unavailable"}`.
pub fn cloud_unavailable_body() -> CloudAgentUnavailableBody {
    CloudAgentUnavailableBody::new()
}

/// The 409 status for [`ExecutorGate::CloudUnavailable`], consumed from
/// D-11 (L4).
pub const CLOUD_UNAVAILABLE_STATUS: u16 = CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS;

/// `validate()` is-default-unset detail (`:93-96`): raised as a bare
/// string, so DRF wraps it as `non_field_errors` (unlike the model
/// backstop at `db/models/project.py:290`, which raises a Django
/// `ValidationError` — same message, different envelope, owned by L5).
pub const UNSET_DEFAULT_DETAIL: &str =
    "Default project cannot be unset without assigning another default project.";

/// The wire body for the is-default-unset guard.
pub const UNSET_DEFAULT_BODY: &str =
    "{\"non_field_errors\":[\"Default project cannot be unset without assigning another default project.\"]}";

/// Failure of the is-default-unset guard (`:93-96`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultUnsetError;

impl DefaultUnsetError {
    /// The wire body ([`UNSET_DEFAULT_BODY`]).
    pub fn body(self) -> &'static str {
        UNSET_DEFAULT_BODY
    }
}

/// Ports the guard `self.instance and self.instance.is_default and
/// data.get("is_default") is False`: `instance_is_default` is false both
/// when there is no instance (create) and when it is not the default (both
/// skip); the `is False` identity means only a literal `Some(false)` —
/// post-DRF-`BooleanField` coercion — trips it (`None`/missing passes).
pub fn check_is_default_unset(
    instance_is_default: bool,
    new_is_default: Option<bool>,
) -> Result<(), DefaultUnsetError> {
    if instance_is_default && new_is_default == Some(false) {
        return Err(DefaultUnsetError);
    }
    Ok(())
}

/// `validate()` html-branch detail (`:99-106`): raised as
/// `ValidationError({"error": ...})`, so DRF renders each value
/// list-wrapped.
pub const HTML_INVALID_DETAIL: &str = "html content is not valid";

/// The wire body for the html branch.
pub const HTML_INVALID_BODY: &str = "{\"error\":[\"html content is not valid\"]}";

/// Failure of the `description_html` branch (`:99-106`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtmlInvalidError;

impl HtmlInvalidError {
    /// The wire body ([`HTML_INVALID_BODY`]).
    pub fn body(self) -> &'static str {
        HTML_INVALID_BODY
    }
}

/// The `validate_html_content` verdict (`utils/content_validator.py:211-241`):
/// `(is_valid, error_message, clean_html)`. The message is unused by the
/// serializer (the raise carries the fixed detail), so only validity and
/// the sanitized replacement cross this boundary. The handler layer runs
/// the cleaner (size check + `nh3.clean` over `ALLOWED_TAGS` /
/// `ATTRIBUTES`); the serializer consumes the verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlVerdict {
    /// `is_valid` element.
    pub is_valid: bool,
    /// `clean_html` element; written back over the input when `Some`
    /// (which on failure is `None`, so failures never replace).
    pub sanitized: Option<String>,
}

/// Ports Python truthiness for a validated `description_html` JSON value
/// (`description_html` is a `JSONField`, so any JSON type reaches the
/// `if ... data["description_html"]` gate): `null` / `false` / numeric
/// zero / `""` / `[]` / `{}` are falsy, everything else truthy.
pub fn json_is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Ports the `description_html` branch (`:99-106`): `None` (key absent)
/// or a falsy value skips without consulting the verdict; a truthy value
/// runs it (`validate_html_content(str(value))` in Python — the handler
/// stringifies the same way before cleaning), writes back `sanitized`
/// when `Some`, and raises [`HtmlInvalidError`] when invalid. Returns the
/// replacement for `data["description_html"]` (`None` = no write-back).
/// The `FnOnce` verdict preserves the Python short-circuit (the cleaner
/// runs only when the gate passes) and keeps it provable in tests.
pub fn check_description_html(
    value: Option<&Value>,
    verdict: impl FnOnce() -> HtmlVerdict,
) -> Result<Option<String>, HtmlInvalidError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !json_is_truthy(value) {
        return Ok(None);
    }
    let HtmlVerdict {
        is_valid,
        sanitized,
    } = verdict();
    if !is_valid {
        return Err(HtmlInvalidError);
    }
    Ok(sanitized)
}

// ---------------------------------------------------------------------------
// create() (`:110-117`)
// ---------------------------------------------------------------------------

/// Ports the `create()` normalisation path (`:110-117` + model
/// `save()` at `db/models/project.py:258`): strip then upper-case.
/// `str::trim` and `to_uppercase` are Unicode-aware like Python's
/// `strip`/`upper`. `Project.save()` repeats the same normalisation, so
/// the row stores upper-case even though `validated_data` (and the
/// `validate_identifier` duplicate check — B-case-dup) holds the raw
/// stripped value.
pub fn normalize_identifier(raw: &str) -> String {
    raw.trim().to_uppercase()
}

/// Ports the `ProjectIdentifier` row `create()` writes (`:115`):
/// `name=project.identifier` — the POST-`save()` (uppercased) identifier —
/// plus the project and `workspace_id`. Takes the validated (stripped)
/// identifier and returns the row's `name`.
pub fn create_identifier_row_name(validated_identifier: &str) -> String {
    normalize_identifier(validated_identifier)
}

// ---------------------------------------------------------------------------
// get_members / get_next_work_item_sequence (`:154-168`)
// ---------------------------------------------------------------------------

/// One `members_list` prefetch row (`app/views/project/base.py:89-97`:
/// active `ProjectMember`s of the workspace with `select_related("member")`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberRow {
    /// `member.member_id`.
    pub member_id: Uuid,
    /// `member.is_active`.
    pub is_active: bool,
    /// `member.member.is_bot`.
    pub member_is_bot: bool,
}

/// Ports `get_members` (`:154-160`): `getattr(obj, "members_list", None)`
/// is `None` (fresh instances, e.g. the create-201 render) → `[]`;
/// otherwise the `member_id`s of the active non-bot rows, in prefetch
/// order.
pub fn project_list_members(rows: Option<&[MemberRow]>) -> Vec<Uuid> {
    match rows {
        None => Vec::new(),
        Some(rows) => rows
            .iter()
            .filter(|row| row.is_active && !row.member_is_bot)
            .map(|row| row.member_id)
            .collect(),
    }
}

/// Ports `get_next_work_item_sequence` (`:161-164`):
/// `IssueSequence.objects.filter(project_id=...).aggregate(Max("sequence"))`
/// then `(max + 1) if max else 1`. `max_sequence` is the queries-layer
/// `Max` verdict (`None` = no rows). Python's `if max_sequence` treats `0`
/// as missing, but `0 + 1 == 1` anyway, so the rule is exact for every
/// input.
pub fn next_work_item_sequence(max_sequence: Option<i64>) -> i64 {
    match max_sequence {
        Some(max) if max != 0 => max + 1,
        _ => 1,
    }
}

// ---------------------------------------------------------------------------
// get_agent_executor_options (`:35-40,:147-152,:181-186`, D-11 delegation)
// ---------------------------------------------------------------------------

/// Ports the request-user derivation all three `get_agent_executor_options`
/// methods share: `user = getattr(request, "user", None) if request else
/// None`, then `user if user and user.is_authenticated else None`.
/// `user` is the request user when the handler has one (`None` covers both
/// "no request in context" and "no user on the request"); a present but
/// unauthenticated user (e.g. `AnonymousUser`) also maps to `None`.
pub fn resolve_options_user(user: Option<UserFlags>, is_authenticated: bool) -> Option<UserFlags> {
    match user {
        Some(flags) if is_authenticated => Some(flags),
        _ => None,
    }
}

/// Ports `get_agent_executor_options(project)` end to end by delegating to
/// D-11's [`agent_executor_options`](crate::dispatch::policy::agent_executor_options):
/// the cloud verdict consults the `has_usable_llm_config` EE seam only for
/// authenticated users on configured instances (see D-11 docs);
/// `local_runner_exists` is the queries-layer
/// `LOCAL_RUNNER_EXISTS_SQL` verdict and `managed` the L4
/// `managed_runner_availability` verdict, both passed through verbatim.
/// Rows are always `[cloud_agent, local_runner, managed_runner]`.
pub fn project_executor_options<F>(
    cloud: &CloudAgentSettings,
    user: Option<UserFlags>,
    is_authenticated: bool,
    has_usable_llm_config: F,
    local_runner_exists: bool,
    managed: ManagedAvailability,
) -> [ExecutorOption; 3]
where
    F: FnOnce() -> bool,
{
    crate::dispatch::policy::agent_executor_options(
        cloud,
        resolve_options_user(user, is_authenticated),
        has_usable_llm_config,
        local_runner_exists,
        managed,
    )
}

// ---------------------------------------------------------------------------
// DynamicBaseSerializer fields= / expand= (`base.py:12-201`)
// ---------------------------------------------------------------------------

/// One entry of an `expand=` argument (`base.py:33-53`): a plain field
/// name, a `{name: <non-list>}` dict entry (kept with no recursion), or a
/// `{name: [...]}` dict entry (always raises — B-nested-expand). Even an
/// EMPTY list raises in Python (`isinstance([], list)` is true), so the
/// list case carries its items only for documentation.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandItem {
    /// A plain field name (`isinstance(item, str)`, `base.py:48-49`).
    Name(String),
    /// A `{name: value}` dict entry with a non-list value (`base.py:41-42`
    /// guard skips recursion; the key is kept like a name). Note it still
    /// never renders: `to_representation` evaluates `expand in self.fields`
    /// (`base.py:128`) with the dict itself, which raises `TypeError:
    /// unhashable type` — so every dict `expand=` entry crashes, list
    /// ones in `__init__` and scalar ones at render.
    DictValue(String),
    /// A `{name: [...]}` dict entry: the key is kept in `allowed`
    /// (`base.py:52-53`) but the recursion raises first — see
    /// [`ExpandError`].
    DictList(String, Vec<ExpandItem>),
}

/// Failure modes of [`filter_field_additions`], mirroring the Python
/// raises in evaluation order: `self.fields[key]` is evaluated before the
/// recursive call, so an unknown nested key raises `KeyError` while a
/// known one raises `TypeError` (B-nested-expand, `base.py:41-42`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpandError {
    /// `KeyError`: the nested key is not a serializer field.
    #[error("unknown field: {0}")]
    UnknownField(String),
    /// `TypeError` parity (B-nested-expand): `{key: [...]}` always raises
    /// in Python, so it always fails here.
    #[error("nested expand= entry always raises TypeError in Python (base.py:42): {0}")]
    NestedNotSupported(String),
}

/// The `__init__` expansion map (`base.py:74-96`): an `expand=` name that
/// is NOT already a serializer field is ADDED under the mapped nested
/// serializer. (The `to_representation` map at `base.py:148-171` adds one
/// more key, [`EXPANSION_RENDER_EXTRA`]; the many-list at `base.py:103-115`
/// names [`EXPANSION_MANY_FIELDS`].)
pub const EXPANSION_ADD_FIELDS: &[&str] = &[
    "user",
    "workspace",
    "project",
    "default_assignee",
    "project_lead",
    "state",
    "created_by",
    "issue",
    "actor",
    "owned_by",
    "members",
    "assignees",
    "labels",
    "issue_cycle",
    "parent",
    "issue_relation",
    "issue_intake",
    "issue_related",
    "issue_reactions",
    "issue_link",
    "sub_issues",
];

/// The extra key of the `to_representation` expansion map
/// (`base.py:148-171`) over [`EXPANSION_ADD_FIELDS`]: `__init__` never
/// adds it, but `to_representation` would overwrite it if it were a
/// field. Latent everywhere in D-25 (no view passes `expand=`).
pub const EXPANSION_RENDER_EXTRA: &str = "issue_attachment";

/// Entries of [`EXPANSION_ADD_FIELDS`] added with `many=True`
/// (`base.py:100-117`). (`issue_attachment` is named in the many-list but
/// is not an add-map key, so that entry is dead.)
pub const EXPANSION_MANY_FIELDS: &[&str] = &[
    "members",
    "assignees",
    "labels",
    "issue_cycle",
    "issue_relation",
    "issue_intake",
    "issue_reactions",
    "issue_link",
    "sub_issues",
    "issue_related",
];

/// Whether an `expand=` name renders through the `to_representation`
/// expansion map (`base.py:148-171` = [`EXPANSION_ADD_FIELDS`] plus
/// [`EXPANSION_RENDER_EXTRA`]).
pub fn is_render_expandable(name: &str) -> bool {
    name == EXPANSION_RENDER_EXTRA || EXPANSION_ADD_FIELDS.contains(&name)
}

/// A field `__init__` adds for an `expand=` name (`base.py:98-118`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedField {
    /// The added field name.
    pub name: String,
    /// Whether it is added with `many=True`.
    pub many: bool,
}

/// Ports `DynamicBaseSerializer.__init__` + `_filter_fields`
/// (`base.py:12-120`). `available` is the serializer's field list in wire
/// order; `expand` is the effective `expand=` argument (see
/// [`effective_expand`]). Returns the fields `__init__` ADDS — names that
/// are neither already fields nor in [`EXPANSION_ADD_FIELDS`] are silently
/// ignored, and NOTHING is ever removed (there is no popping loop: unlike
/// the space `DynamicBaseSerializer`, the app one only ever adds).
pub fn filter_field_additions(
    available: &[&str],
    expand: &[ExpandItem],
) -> Result<Vec<AddedField>, ExpandError> {
    let mut added = Vec::new();
    for item in expand {
        match item {
            ExpandItem::Name(name) | ExpandItem::DictValue(name) => {
                if !available.contains(&name.as_str())
                    && EXPANSION_ADD_FIELDS.contains(&name.as_str())
                {
                    added.push(AddedField {
                        name: name.clone(),
                        many: EXPANSION_MANY_FIELDS.contains(&name.as_str()),
                    });
                }
            }
            ExpandItem::DictList(key, _) => {
                if !available.contains(&key.as_str()) {
                    return Err(ExpandError::UnknownField(key.clone()));
                }
                return Err(ExpandError::NestedNotSupported(key.clone()));
            }
        }
    }
    Ok(added)
}

/// Ports the `fields=` → `expand=` overwrite (`base.py:14-18`):
/// `__init__` pops `fields` and immediately replaces it with
/// `self.expand` (`kwargs.pop("expand", []) or []`), so the `fields`
/// content — the `?fields=` values the project list view passes
/// (`app/views/project/base.py:102,140`) — is discarded ENTIRELY
/// (B-fields-dead) and only `expand` selects. `None` (kwarg absent, or
/// the view's `fields if fields else None` falling through) behaves as
/// `[]`. Callers pass parsed lists (every Python call site does); a raw
/// string `expand=` would iterate chars in Python — no call site does
/// this.
pub fn effective_expand(_fields_present: bool, expand: Option<Vec<ExpandItem>>) -> Vec<ExpandItem> {
    expand.unwrap_or_default()
}

/// What `to_representation` does to one `expand=` name (`base.py:122-181`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepresentationOverride {
    /// In fields and in the render map, current value is a list: re-render
    /// `getattr(instance, expand)` with the mapped serializer, `many=True`.
    NestedMany,
    /// In fields and in the render map, current value is not a list:
    /// re-render with the mapped serializer, `many=False`.
    NestedOne,
    /// In fields but NOT in the render map: overwrite with
    /// `getattr(instance, "<expand>_id", None)` (e.g. `expand=name` nulls
    /// the name — there is no `name_id` attribute).
    IdAttribute,
    /// Not a field (and not added by `__init__`): untouched.
    Unchanged,
}

/// Ports the per-name `to_representation` rule: `in_fields` is `expand in
/// self.fields` (after the `__init__` additions), `render_expandable` is
/// [`is_render_expandable`], `current_is_list` is `isinstance(response.get(expand),
/// list)`.
pub fn representation_override(
    in_fields: bool,
    render_expandable: bool,
    current_is_list: bool,
) -> RepresentationOverride {
    if !in_fields {
        return RepresentationOverride::Unchanged;
    }
    if !render_expandable {
        return RepresentationOverride::IdAttribute;
    }
    if current_is_list {
        RepresentationOverride::NestedMany
    } else {
        RepresentationOverride::NestedOne
    }
}

// ---------------------------------------------------------------------------
// Serializer metadata: key orders + read-only fields
// ---------------------------------------------------------------------------

/// `ProjectSerializer.Meta.read_only_fields` (`:45`).
pub const PROJECT_READ_ONLY_FIELDS: &[&str] = &["workspace", "deleted_at"];

/// `ProjectSerializer` wire order (`:30-45` over `fields = "__all__"`):
/// `id`, the declared `workspace_detail` / `inbox_view` /
/// `agent_executor_options`, then the model fields.
pub const PROJECT_KEY_ORDER: &[&str] = &[
    "id",
    "workspace_detail",
    "inbox_view",
    "agent_executor_options",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "network",
    "identifier",
    "emoji",
    "icon_prop",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "is_time_tracking_enabled",
    "is_issue_type_enabled",
    "is_default",
    "guest_view_all_features",
    "members_can_edit_states",
    "cover_image",
    "archive_in",
    "close_in",
    "logo_props",
    "archived_at",
    "timezone",
    "external_source",
    "external_id",
    "repo_url",
    "base_branch",
    "agent_default_interval_seconds",
    "agent_default_max_ticks",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
    "agent_ticking_enabled",
    "default_agent_executor",
    "created_by",
    "updated_by",
    "workspace",
    "default_assignee",
    "project_lead",
    "cover_image_asset",
    "estimate",
    "default_state",
];

/// `ProjectListSerializer` wire order (`:136-168` over `fields =
/// "__all__"`): `id`, the declared annotation/method fields, then the
/// model fields.
pub const PROJECT_LIST_KEY_ORDER: &[&str] = &[
    "id",
    "is_favorite",
    "sort_order",
    "member_role",
    "anchor",
    "members",
    "cover_image_url",
    "inbox_view",
    "next_work_item_sequence",
    "agent_executor_options",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "network",
    "identifier",
    "emoji",
    "icon_prop",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "is_time_tracking_enabled",
    "is_issue_type_enabled",
    "is_default",
    "guest_view_all_features",
    "members_can_edit_states",
    "cover_image",
    "archive_in",
    "close_in",
    "logo_props",
    "archived_at",
    "timezone",
    "external_source",
    "external_id",
    "repo_url",
    "base_branch",
    "agent_default_interval_seconds",
    "agent_default_max_ticks",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
    "agent_ticking_enabled",
    "default_agent_executor",
    "created_by",
    "updated_by",
    "workspace",
    "default_assignee",
    "project_lead",
    "cover_image_asset",
    "estimate",
    "default_state",
];

/// `ProjectLiteSerializer.Meta.fields` (`:123-132`), in list order (also
/// the `read_only_fields`, `:133`).
pub const PROJECT_LITE_FIELDS: &[&str] = &[
    "id",
    "identifier",
    "name",
    "cover_image",
    "cover_image_url",
    "logo_props",
    "description",
    "is_default",
];

/// `ProjectDetailSerializer` wire order (`:171-190` over `fields =
/// "__all__"`): `id`, the declared nested/annotated fields (the nested
/// `default_assignee` / `project_lead` REPLACE the PK rendering — no
/// trailing PK keys), then the model fields.
pub const PROJECT_DETAIL_KEY_ORDER: &[&str] = &[
    "id",
    "default_assignee",
    "project_lead",
    "is_favorite",
    "sort_order",
    "member_role",
    "anchor",
    "agent_executor_options",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "network",
    "identifier",
    "emoji",
    "icon_prop",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "is_time_tracking_enabled",
    "is_issue_type_enabled",
    "is_default",
    "guest_view_all_features",
    "members_can_edit_states",
    "cover_image",
    "archive_in",
    "close_in",
    "logo_props",
    "archived_at",
    "timezone",
    "external_source",
    "external_id",
    "repo_url",
    "base_branch",
    "agent_default_interval_seconds",
    "agent_default_max_ticks",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
    "agent_ticking_enabled",
    "default_agent_executor",
    "created_by",
    "updated_by",
    "workspace",
    "cover_image_asset",
    "estimate",
    "default_state",
];

/// `DeployBoardSerializer.Meta.read_only_fields` (`:266`).
pub const DEPLOY_BOARD_READ_ONLY_FIELDS: &[&str] = &["workspace", "project", "anchor"];

/// `DeployBoardSerializer` wire order (`:259-266` over `fields =
/// "__all__"`): `id`, the declared `project_details` / `workspace_detail`,
/// then the model fields.
pub const DEPLOY_BOARD_KEY_ORDER: &[&str] = &[
    "id",
    "project_details",
    "workspace_detail",
    "created_at",
    "updated_at",
    "deleted_at",
    "entity_identifier",
    "entity_name",
    "anchor",
    "is_comments_enabled",
    "is_reactions_enabled",
    "is_votes_enabled",
    "view_props",
    "is_activity_enabled",
    "is_disabled",
    "created_by",
    "updated_by",
    "workspace",
    "project",
    "intake",
];

/// `DeployBoardSerializer().data` with no instance and no input: DRF
/// returns `get_initial()` — the 12 writable keys with field initials
/// (read-only `id` / nested / auto / `read_only_fields` excluded).
/// Byte-exact, in fixture order.
pub const DEPLOY_BOARD_INITIAL_BODY: &str = "{\"deleted_at\":null,\"entity_identifier\":null,\"entity_name\":\"\",\"is_comments_enabled\":false,\"is_reactions_enabled\":false,\"is_votes_enabled\":false,\"view_props\":null,\"is_activity_enabled\":false,\"is_disabled\":false,\"created_by\":null,\"updated_by\":null,\"intake\":null}";

// ---------------------------------------------------------------------------
// Read shapes (fields in DRF wire order; see the key-order consts)
// ---------------------------------------------------------------------------

/// `ProjectSerializer` read shape (`:30-45`, [`PROJECT_KEY_ORDER`]).
/// Datetimes and UUID/FK keys arrive already rendered as strings;
/// `workspace_detail` and the JSON columns arrive as caller-supplied
/// [`Value`]s already rendered in DRF key order. `Deserialize` supports
/// the fixture replays (golden → struct → bytes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectRead {
    /// PK (`PrimaryKeyRelatedField`, read-only).
    pub id: String,
    /// `WorkspaceLiteSerializer(source="workspace")` (D-24 shape, by value).
    pub workspace_detail: Value,
    /// `source="intake_view"` alias.
    pub inbox_view: bool,
    /// D-11 executor rows.
    pub agent_executor_options: Vec<ExecutorOption>,
    /// DRF iso-8601.
    pub created_at: String,
    /// DRF iso-8601.
    pub updated_at: String,
    /// DRF iso-8601 or null.
    pub deleted_at: Option<String>,
    /// `CharField(max_length=255)`.
    pub name: String,
    /// `TextField(blank=True)`.
    pub description: String,
    /// `JSONField(blank=True, null=True)`.
    pub description_text: Option<Value>,
    /// `JSONField(blank=True, null=True)`.
    pub description_html: Option<Value>,
    /// `PositiveSmallIntegerField` (`NETWORK_CHOICES` 0/2).
    pub network: i64,
    /// `CharField(max_length=12)`, stored upper-cased.
    pub identifier: String,
    /// `CharField(null=True, blank=True)`.
    pub emoji: Option<String>,
    /// `JSONField(null=True)`.
    pub icon_prop: Option<Value>,
    /// Feature flags, in model order.
    pub module_view: bool,
    /// Feature flags, in model order.
    pub cycle_view: bool,
    /// Feature flags, in model order.
    pub issue_views_view: bool,
    /// Feature flags, in model order (`default=True`).
    pub page_view: bool,
    /// Feature flags, in model order.
    pub intake_view: bool,
    /// `default=False`.
    pub is_time_tracking_enabled: bool,
    /// `default=False`.
    pub is_issue_type_enabled: bool,
    /// `default=False`.
    pub is_default: bool,
    /// `default=False`.
    pub guest_view_all_features: bool,
    /// `default=True`.
    pub members_can_edit_states: bool,
    /// `TextField(blank=True, null=True)`.
    pub cover_image: Option<String>,
    /// `IntegerField(default=0)`.
    pub archive_in: i64,
    /// `IntegerField(default=0)`.
    pub close_in: i64,
    /// `JSONField(default=dict)`.
    pub logo_props: Value,
    /// `DateTimeField(null=True)`.
    pub archived_at: Option<String>,
    /// `CharField(default="UTC")`.
    pub timezone: String,
    /// `CharField(null=True, blank=True)`.
    pub external_source: Option<String>,
    /// `CharField(blank=True, null=True)`.
    pub external_id: Option<String>,
    /// `CharField(blank=True, default="")`.
    pub repo_url: String,
    /// `CharField(blank=True, default="main")`.
    pub base_branch: String,
    /// `IntegerField(default=10800)`.
    pub agent_default_interval_seconds: i64,
    /// `IntegerField(default=10)`.
    pub agent_default_max_ticks: i64,
    /// `IntegerField(default=10800)`.
    pub agent_review_default_interval_seconds: i64,
    /// `IntegerField(default=10800)`.
    pub agent_test_default_interval_seconds: i64,
    /// `BooleanField(default=True)`.
    pub agent_ticking_enabled: bool,
    /// `CharField` over `AgentExecutorKind`.
    pub default_agent_executor: String,
    /// Audit FK (`PrimaryKeyRelatedField`).
    pub created_by: Option<String>,
    /// Audit FK (`PrimaryKeyRelatedField`).
    pub updated_by: Option<String>,
    /// Workspace FK (`PrimaryKeyRelatedField`).
    pub workspace: String,
    /// User FK (`PrimaryKeyRelatedField`).
    pub default_assignee: Option<String>,
    /// User FK (`PrimaryKeyRelatedField`).
    pub project_lead: Option<String>,
    /// `FileAsset` FK (`PrimaryKeyRelatedField`).
    pub cover_image_asset: Option<String>,
    /// `Estimate` FK (`PrimaryKeyRelatedField`).
    pub estimate: Option<String>,
    /// `State` FK (`PrimaryKeyRelatedField`).
    pub default_state: Option<String>,
}

/// `ProjectListSerializer` read shape (`:136-168`,
/// [`PROJECT_LIST_KEY_ORDER`]). The four queryset annotations are
/// presence-`Option`s: on rows rendered without them (the create-201
/// fresh instance, `app/views/project/base.py:310`) DRF `SkipField`
/// omits the keys. The nullable three are double-`Option`s (outer =
/// annotation present, inner = value vs `None` → `null`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectListRead {
    /// PK.
    pub id: String,
    /// `Exists(UserFavorite …)` annotation; absent without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_favorite: Option<bool>,
    /// `ProjectUserProperty` subquery annotation; absent without it, null
    /// when the viewer has no row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<Option<f64>>,
    /// `ProjectMember.role` subquery annotation; absent without it, null
    /// when the viewer is not a member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_role: Option<Option<i64>>,
    /// `DeployBoard.anchor` subquery annotation; absent without it, null
    /// when no board row exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Option<String>>,
    /// [`project_list_members`] output, stringified.
    pub members: Vec<String>,
    /// Model `cover_image_url` property (asset URL, else `cover_image`,
    /// else `None`) — always present, unlike the annotations.
    pub cover_image_url: Option<String>,
    /// `source="intake_view"` alias.
    pub inbox_view: bool,
    /// [`next_work_item_sequence`] output.
    pub next_work_item_sequence: i64,
    /// D-11 executor rows.
    pub agent_executor_options: Vec<ExecutorOption>,
    /// DRF iso-8601.
    pub created_at: String,
    /// DRF iso-8601.
    pub updated_at: String,
    /// DRF iso-8601 or null.
    pub deleted_at: Option<String>,
    /// `CharField(max_length=255)`.
    pub name: String,
    /// `TextField(blank=True)`.
    pub description: String,
    /// `JSONField(blank=True, null=True)`.
    pub description_text: Option<Value>,
    /// `JSONField(blank=True, null=True)`.
    pub description_html: Option<Value>,
    /// `PositiveSmallIntegerField`.
    pub network: i64,
    /// `CharField(max_length=12)`, stored upper-cased.
    pub identifier: String,
    /// `CharField(null=True, blank=True)`.
    pub emoji: Option<String>,
    /// `JSONField(null=True)`.
    pub icon_prop: Option<Value>,
    /// Feature flags, in model order.
    pub module_view: bool,
    /// Feature flags, in model order.
    pub cycle_view: bool,
    /// Feature flags, in model order.
    pub issue_views_view: bool,
    /// Feature flags, in model order (`default=True`).
    pub page_view: bool,
    /// Feature flags, in model order.
    pub intake_view: bool,
    /// `default=False`.
    pub is_time_tracking_enabled: bool,
    /// `default=False`.
    pub is_issue_type_enabled: bool,
    /// `default=False`.
    pub is_default: bool,
    /// `default=False`.
    pub guest_view_all_features: bool,
    /// `default=True`.
    pub members_can_edit_states: bool,
    /// `TextField(blank=True, null=True)`.
    pub cover_image: Option<String>,
    /// `IntegerField(default=0)`.
    pub archive_in: i64,
    /// `IntegerField(default=0)`.
    pub close_in: i64,
    /// `JSONField(default=dict)`.
    pub logo_props: Value,
    /// `DateTimeField(null=True)`.
    pub archived_at: Option<String>,
    /// `CharField(default="UTC")`.
    pub timezone: String,
    /// `CharField(null=True, blank=True)`.
    pub external_source: Option<String>,
    /// `CharField(blank=True, null=True)`.
    pub external_id: Option<String>,
    /// `CharField(blank=True, default="")`.
    pub repo_url: String,
    /// `CharField(blank=True, default="main")`.
    pub base_branch: String,
    /// `IntegerField(default=10800)`.
    pub agent_default_interval_seconds: i64,
    /// `IntegerField(default=10)`.
    pub agent_default_max_ticks: i64,
    /// `IntegerField(default=10800)`.
    pub agent_review_default_interval_seconds: i64,
    /// `IntegerField(default=10800)`.
    pub agent_test_default_interval_seconds: i64,
    /// `BooleanField(default=True)`.
    pub agent_ticking_enabled: bool,
    /// `CharField` over `AgentExecutorKind`.
    pub default_agent_executor: String,
    /// Audit FK.
    pub created_by: Option<String>,
    /// Audit FK.
    pub updated_by: Option<String>,
    /// Workspace FK.
    pub workspace: String,
    /// User FK (PK rendering — the list serializer does not nest).
    pub default_assignee: Option<String>,
    /// User FK (PK rendering).
    pub project_lead: Option<String>,
    /// `FileAsset` FK.
    pub cover_image_asset: Option<String>,
    /// `Estimate` FK.
    pub estimate: Option<String>,
    /// `State` FK.
    pub default_state: Option<String>,
}

/// `ProjectLiteSerializer` read shape (`:120-133`,
/// [`PROJECT_LITE_FIELDS`]): the 8-key `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectLiteRead {
    /// PK.
    pub id: String,
    /// `CharField(max_length=12)`.
    pub identifier: String,
    /// `CharField(max_length=255)`.
    pub name: String,
    /// `TextField(blank=True, null=True)`.
    pub cover_image: Option<String>,
    /// Model `cover_image_url` property (read-only `ReadOnlyField` here).
    pub cover_image_url: Option<String>,
    /// `JSONField(default=dict)`.
    pub logo_props: Value,
    /// `TextField(blank=True)`.
    pub description: String,
    /// `default=False`.
    pub is_default: bool,
}

/// `ProjectDetailSerializer` read shape (`:171-190`,
/// [`PROJECT_DETAIL_KEY_ORDER`]), ported as-is though unreferenced
/// outside `__init__`. The nested `default_assignee` / `project_lead`
/// (`UserLiteSerializer`, D-24 shapes by value, `null` when the FK is
/// null) replace the PK rendering; the four annotations follow the same
/// presence rules as [`ProjectListRead`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectDetailRead {
    /// PK.
    pub id: String,
    /// `UserLiteSerializer(read_only=True)` (D-24 shape, by value).
    pub default_assignee: Option<Value>,
    /// `UserLiteSerializer(read_only=True)` (D-24 shape, by value).
    pub project_lead: Option<Value>,
    /// `Exists(UserFavorite …)` annotation; absent without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_favorite: Option<bool>,
    /// `ProjectUserProperty` subquery annotation; absent without it, null
    /// when the viewer has no row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<Option<f64>>,
    /// `ProjectMember.role` subquery annotation; absent without it, null
    /// when the viewer is not a member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_role: Option<Option<i64>>,
    /// `DeployBoard.anchor` subquery annotation; absent without it, null
    /// when no board row exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Option<String>>,
    /// D-11 executor rows.
    pub agent_executor_options: Vec<ExecutorOption>,
    /// DRF iso-8601.
    pub created_at: String,
    /// DRF iso-8601.
    pub updated_at: String,
    /// DRF iso-8601 or null.
    pub deleted_at: Option<String>,
    /// `CharField(max_length=255)`.
    pub name: String,
    /// `TextField(blank=True)`.
    pub description: String,
    /// `JSONField(blank=True, null=True)`.
    pub description_text: Option<Value>,
    /// `JSONField(blank=True, null=True)`.
    pub description_html: Option<Value>,
    /// `PositiveSmallIntegerField`.
    pub network: i64,
    /// `CharField(max_length=12)`, stored upper-cased.
    pub identifier: String,
    /// `CharField(null=True, blank=True)`.
    pub emoji: Option<String>,
    /// `JSONField(null=True)`.
    pub icon_prop: Option<Value>,
    /// Feature flags, in model order.
    pub module_view: bool,
    /// Feature flags, in model order.
    pub cycle_view: bool,
    /// Feature flags, in model order.
    pub issue_views_view: bool,
    /// Feature flags, in model order (`default=True`).
    pub page_view: bool,
    /// Feature flags, in model order.
    pub intake_view: bool,
    /// `default=False`.
    pub is_time_tracking_enabled: bool,
    /// `default=False`.
    pub is_issue_type_enabled: bool,
    /// `default=False`.
    pub is_default: bool,
    /// `default=False`.
    pub guest_view_all_features: bool,
    /// `default=True`.
    pub members_can_edit_states: bool,
    /// `TextField(blank=True, null=True)`.
    pub cover_image: Option<String>,
    /// `IntegerField(default=0)`.
    pub archive_in: i64,
    /// `IntegerField(default=0)`.
    pub close_in: i64,
    /// `JSONField(default=dict)`.
    pub logo_props: Value,
    /// `DateTimeField(null=True)`.
    pub archived_at: Option<String>,
    /// `CharField(default="UTC")`.
    pub timezone: String,
    /// `CharField(null=True, blank=True)`.
    pub external_source: Option<String>,
    /// `CharField(blank=True, null=True)`.
    pub external_id: Option<String>,
    /// `CharField(blank=True, default="")`.
    pub repo_url: String,
    /// `CharField(blank=True, default="main")`.
    pub base_branch: String,
    /// `IntegerField(default=10800)`.
    pub agent_default_interval_seconds: i64,
    /// `IntegerField(default=10)`.
    pub agent_default_max_ticks: i64,
    /// `IntegerField(default=10800)`.
    pub agent_review_default_interval_seconds: i64,
    /// `IntegerField(default=10800)`.
    pub agent_test_default_interval_seconds: i64,
    /// `BooleanField(default=True)`.
    pub agent_ticking_enabled: bool,
    /// `CharField` over `AgentExecutorKind`.
    pub default_agent_executor: String,
    /// Audit FK.
    pub created_by: Option<String>,
    /// Audit FK.
    pub updated_by: Option<String>,
    /// Workspace FK.
    pub workspace: String,
    /// `FileAsset` FK.
    pub cover_image_asset: Option<String>,
    /// `Estimate` FK.
    pub estimate: Option<String>,
    /// `State` FK.
    pub default_state: Option<String>,
}

/// `DeployBoardSerializer` read shape (`:259-266`,
/// [`DEPLOY_BOARD_KEY_ORDER`]). No annotated fields: all 20 keys always
/// render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeployBoardRead {
    /// PK.
    pub id: String,
    /// `ProjectLiteSerializer(source="project")` (this domain's lite
    /// shape, by value); `null` when the project FK is null.
    pub project_details: Option<Value>,
    /// `WorkspaceLiteSerializer(source="workspace")` (D-24 shape, by value).
    pub workspace_detail: Value,
    /// DRF iso-8601.
    pub created_at: String,
    /// DRF iso-8601.
    pub updated_at: String,
    /// DRF iso-8601 or null.
    pub deleted_at: Option<String>,
    /// `UUIDField(null=True)`, stringified.
    pub entity_identifier: Option<String>,
    /// `CharField(max_length=30, null=True, blank=True)`.
    pub entity_name: Option<String>,
    /// `CharField(default=get_anchor, unique=True)`.
    pub anchor: String,
    /// `BooleanField(default=False)`.
    pub is_comments_enabled: bool,
    /// `BooleanField(default=False)`.
    pub is_reactions_enabled: bool,
    /// `BooleanField(default=False)`.
    pub is_votes_enabled: bool,
    /// `JSONField(default=dict)`.
    pub view_props: Value,
    /// `BooleanField(default=True)`.
    pub is_activity_enabled: bool,
    /// `BooleanField(default=False)`.
    pub is_disabled: bool,
    /// Audit FK.
    pub created_by: Option<String>,
    /// Audit FK.
    pub updated_by: Option<String>,
    /// Workspace FK.
    pub workspace: String,
    /// Project FK.
    pub project: Option<String>,
    /// `Intake` FK.
    pub intake: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/app_project/FX-APROJ-01.serializers_project.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn str_list(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect()
    }

    /// Order-insensitive canonical form for value equality.
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), canonical(&map[k]));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }

    /// Top-level keys of a `{...}` JSON object string, in byte order.
    /// Depth- and string-aware; proves wire order without `preserve_order`.
    fn object_keys(span: &str) -> Vec<String> {
        let chars: Vec<char> = span.chars().collect();
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '"' => {
                    let mut j = i + 1;
                    let mut s = String::new();
                    while j < chars.len() && chars[j] != '"' {
                        if chars[j] == '\\' {
                            j += 1;
                            if j < chars.len() {
                                s.push(chars[j]);
                                j += 1;
                            }
                        } else {
                            s.push(chars[j]);
                            j += 1;
                        }
                    }
                    let mut k = j + 1;
                    while k < chars.len() && chars[k].is_whitespace() {
                        k += 1;
                    }
                    if depth == 1 && k < chars.len() && chars[k] == ':' {
                        keys.push(s);
                    }
                    i = j + 1;
                }
                '{' | '[' => {
                    depth += 1;
                    i += 1;
                }
                '}' | ']' => {
                    depth = depth.saturating_sub(1);
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
        keys
    }

    /// Every `"key":` in byte order at any depth (for fixed-shape arrays
    /// of objects, chunked per row by the caller).
    fn all_key_sequence(span: &str) -> Vec<String> {
        let chars: Vec<char> = span.chars().collect();
        let mut keys = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '"' {
                let mut j = i + 1;
                let mut s = String::new();
                while j < chars.len() && chars[j] != '"' {
                    if chars[j] == '\\' {
                        j += 1;
                        if j < chars.len() {
                            s.push(chars[j]);
                            j += 1;
                        }
                    } else {
                        s.push(chars[j]);
                        j += 1;
                    }
                }
                let mut k = j + 1;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                if k < chars.len() && chars[k] == ':' {
                    keys.push(s);
                }
                i = j + 1;
            } else {
                i += 1;
            }
        }
        keys
    }

    /// Byte span of a top-level key's value in a `{...}` object string.
    fn value_span<'a>(obj: &'a str, key: &str) -> &'a str {
        let bytes = obj.as_bytes();
        let mut depth = 0usize;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] != b'"' {
                        if bytes[j] == b'\\' {
                            j += 1;
                        }
                        j += 1;
                    }
                    let s = &obj[i + 1..j];
                    let mut k = j + 1;
                    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    if depth == 1 && k < bytes.len() && bytes[k] == b':' && s == key {
                        let mut v = k + 1;
                        while v < bytes.len() && bytes[v].is_ascii_whitespace() {
                            v += 1;
                        }
                        let start = v;
                        if bytes[v] == b'{' || bytes[v] == b'[' {
                            let (open, close) = if bytes[v] == b'{' {
                                (b'{', b'}')
                            } else {
                                (b'[', b']')
                            };
                            let mut d = 0;
                            while v < bytes.len() {
                                if bytes[v] == b'"' {
                                    v += 1;
                                    while v < bytes.len() && bytes[v] != b'"' {
                                        if bytes[v] == b'\\' {
                                            v += 1;
                                        }
                                        v += 1;
                                    }
                                    v += 1;
                                } else if bytes[v] == open {
                                    d += 1;
                                    v += 1;
                                } else if bytes[v] == close {
                                    d -= 1;
                                    v += 1;
                                    if d == 0 {
                                        break;
                                    }
                                } else {
                                    v += 1;
                                }
                            }
                            return &obj[start..v];
                        }
                        while v < bytes.len()
                            && bytes[v] != b','
                            && bytes[v] != b'}'
                            && bytes[v] != b']'
                        {
                            if bytes[v] == b'"' {
                                v += 1;
                                while v < bytes.len() && bytes[v] != b'"' {
                                    if bytes[v] == b'\\' {
                                        v += 1;
                                    }
                                    v += 1;
                                }
                                v += 1;
                            } else {
                                v += 1;
                            }
                        }
                        let mut end = v;
                        while end > start && bytes[end - 1].is_ascii_whitespace() {
                            end -= 1;
                        }
                        return &obj[start..end];
                    }
                    i = j + 1;
                }
                b'{' | b'[' => {
                    depth += 1;
                    i += 1;
                }
                b'}' | b']' => {
                    depth = depth.saturating_sub(1);
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
        panic!("key {key} not found at top level");
    }

    /// Django-default cloud settings with the kill switch off, as in the
    /// fixture env (`CLOUD_AGENT_ENABLED` unset). Only `enabled` feeds
    /// this layer's gate; the rest are Django defaults (see D-11 tests).
    fn cloud_settings(enabled: bool) -> CloudAgentSettings {
        CloudAgentSettings {
            enabled,
            writes_enabled: false,
            github_tools_enabled: true,
            disabled_tools: Vec::new(),
            reconcile_interval_secs: 30,
            model_request_timeout_secs: 60,
            execution_timeout_secs: 285,
            run_soft_limit_secs: 300,
            run_hard_limit_secs: 330,
            stale_grace_secs: 60,
            dispatch_lease_secs: 60,
            dispatch_backoff_secs: 10,
            dispatch_scan_interval_secs: 10,
            sweep_interval_secs: 30,
            dispatch_scan_batch: 100,
            max_queue_age_secs: 900,
            model_request_limit: 25,
            tool_call_limit: 20,
            write_call_limit: 3,
            input_token_limit: 144_000,
            output_token_limit: 16_000,
            total_token_limit: 160_000,
            max_output_tokens_per_request: 4096,
            max_queued_per_workspace: 20,
            max_running_per_workspace: 2,
            user_creation_rate_per_minute: 6,
            workspace_creation_rate_per_minute: 30,
            tool_timeout_secs: 20,
            max_tool_result_bytes: 65536,
            max_prompt_bytes: 262_144,
            max_final_result_bytes: 65536,
            max_events: 500,
            block_private_urls: true,
        }
    }

    fn managed_disabled() -> ManagedAvailability {
        ManagedAvailability {
            available: false,
            reason_code: "managed_runner_disabled".to_string(),
        }
    }

    #[test]
    fn forbidden_pattern_const_is_verbatim() {
        let fx = fixture();
        assert_eq!(
            FORBIDDEN_IDENTIFIER_CHARS_PATTERN,
            fx.get("forbidden_pattern")
                .expect("pattern")
                .as_str()
                .expect("str")
        );
        assert_eq!(FORBIDDEN_CHARS.len(), 24);
    }

    #[test]
    fn forbidden_table_matches_re_match_including_newline_quirk() {
        for c in FORBIDDEN_CHARS {
            assert!(
                contains_forbidden_chars(&format!("ab{c}cd")),
                "char {c:?} should be forbidden"
            );
        }
        assert!(!contains_forbidden_chars("Beta 962c4176"));
        assert!(!contains_forbidden_chars("My Project 962c4176"));
        assert!(!contains_forbidden_chars("A_B_962c4176"));
        assert!(!contains_forbidden_chars("A/B 962c4176"));
        assert!(!contains_forbidden_chars("ENG"));
        assert!(!contains_forbidden_chars(""));
        // Interior newline: Python `re.match` can never match (`.` does not
        // cross `\n`, `$` anchors at the end), so these pass.
        assert!(!contains_forbidden_chars("a&b\nc"));
        assert!(!contains_forbidden_chars("a\nb&c"));
        assert!(!contains_forbidden_chars("&\n&"));
        assert!(!contains_forbidden_chars("ok\n&x"));
        // Trailing newline still anchors `$`: rejected.
        assert!(contains_forbidden_chars("a&b\n"));
        // `\r` is matched by `.`: rejected.
        assert!(contains_forbidden_chars("a\rb&c"));
    }

    #[test]
    fn field_input_parsing_matches_drf() {
        // Absent → required; null → null; "" → blank.
        assert_eq!(parse_name_input(None), Err(FieldInputError::Required));
        assert_eq!(
            parse_name_input(Some(&Value::Null)),
            Err(FieldInputError::Null)
        );
        assert_eq!(
            parse_name_input(Some(&json!(""))),
            Err(FieldInputError::Blank)
        );
        // Bool/dict/list → invalid; int/float coerce via str().
        for bad in [json!(true), json!(false), json!({"a": 1}), json!([1])] {
            assert_eq!(
                parse_name_input(Some(&bad)),
                Err(FieldInputError::InvalidType),
                "input {bad}"
            );
        }
        assert_eq!(parse_name_input(Some(&json!(5))), Ok("5".to_string()));
        assert_eq!(parse_name_input(Some(&json!(5.5))), Ok("5.5".to_string()));
        // Strip (`trim_whitespace=True` default).
        assert_eq!(
            parse_name_input(Some(&json!("  ab  "))),
            Ok("ab".to_string())
        );
        // max_length is inclusive, in code points.
        assert!(parse_name_input(Some(&json!("x".repeat(255).as_str()))).is_ok());
        assert_eq!(
            parse_name_input(Some(&json!("x".repeat(256).as_str()))),
            Err(FieldInputError::MaxLength)
        );
        assert!(parse_identifier_input(Some(&json!("x".repeat(12).as_str()))).is_ok());
        assert_eq!(
            parse_identifier_input(Some(&json!("x".repeat(13).as_str()))),
            Err(FieldInputError::MaxLength)
        );
        // Messages (probed against live DRF 3.18.1).
        assert_eq!(
            FieldInputError::Required.name_detail(),
            "This field is required."
        );
        assert_eq!(
            FieldInputError::Null.name_detail(),
            "This field may not be null."
        );
        assert_eq!(
            FieldInputError::Blank.name_detail(),
            "This field may not be blank."
        );
        assert_eq!(
            FieldInputError::InvalidType.name_detail(),
            "Not a valid string."
        );
        assert_eq!(
            FieldInputError::MaxLength.name_detail(),
            "Ensure this field has no more than 255 characters."
        );
        assert_eq!(
            FieldInputError::MaxLength.identifier_detail(),
            "Ensure this field has no more than 12 characters."
        );
    }

    #[test]
    fn validate_name_vectors_match_fixture() {
        let fx = fixture();
        let nodes = fx.get("validate_name").expect("validate_name");
        for (node, case) in nodes.as_object().expect("object") {
            let ok = case.get("ok").expect("ok").as_bool().expect("bool");
            if ok {
                let value = case.get("value").expect("value").as_str().expect("str");
                assert_eq!(check_name_value(value, false), Ok(()), "node {node}");
            } else {
                let detail = case
                    .get("detail")
                    .expect("detail")
                    .as_array()
                    .expect("array")[0]
                    .as_str()
                    .expect("str");
                // dup_same_workspace is the Taken vector; every other
                // failing node names its forbidden character.
                let input: &str = match node.as_str() {
                    "dup_same_workspace" => "Beta 962c4176",
                    "ampersand" => "a&b",
                    "plus" => "a+b",
                    "comma" => "a,b",
                    "colon" => "a:b",
                    "semicolon" => "a;b",
                    "dollar" => "a$b",
                    "caret" => "a^b",
                    "lbrace" => "a{b",
                    "rbrace" => "a}b",
                    "star" => "a*b",
                    "equals" => "a=b",
                    "question" => "a?b",
                    "at" => "a@b",
                    "hash" => "a#b",
                    "pipe" => "a|b",
                    "squote" => "a'b",
                    "lt" => "a<b",
                    "gt" => "a>b",
                    "dot" => "a.b",
                    "lparen" => "a(b",
                    "rparen" => "a)b",
                    "percent" => "a%b",
                    "bang" => "a!b",
                    "dash" => "a-b",
                    other => panic!("unmapped validate_name node {other}"),
                };
                let taken = node == "dup_same_workspace";
                let err = check_name_value(input, taken).expect_err("fails");
                assert_eq!(err.detail(), detail, "node {node}");
                assert_eq!(
                    field_error_body("name", err.detail()),
                    format!("{{\"name\":[\"{detail}\"]}}"),
                    "node {node}"
                );
            }
        }
        assert_eq!(
            NameError::Forbidden.detail(),
            "PROJECT_NAME_CANNOT_CONTAIN_SPECIAL_CHARACTERS"
        );
        assert_eq!(NameError::Taken.detail(), "PROJECT_NAME_ALREADY_EXIST");
    }

    #[test]
    fn validate_identifier_vectors_match_fixture() {
        let fx = fixture();
        let nodes = fx.get("validate_identifier").expect("validate_identifier");
        for (node, case) in nodes.as_object().expect("object") {
            let ok = case.get("ok").expect("ok").as_bool().expect("bool");
            if ok {
                let value = case.get("value").expect("value").as_str().expect("str");
                assert_eq!(check_identifier_value(value, false), Ok(()), "node {node}");
            } else {
                let detail = case
                    .get("detail")
                    .expect("detail")
                    .as_array()
                    .expect("array")[0]
                    .as_str()
                    .expect("str");
                let taken = node == "dup_same_workspace";
                let input: &str = match node.as_str() {
                    "dup_same_workspace" => "Z9Q",
                    "special" => "a&b",
                    "dot" => "a.b",
                    "dash" => "a-b",
                    other => panic!("unmapped validate_identifier node {other}"),
                };
                let err = check_identifier_value(input, taken).expect_err("fails");
                assert_eq!(err.detail(), detail, "node {node}");
                assert_eq!(
                    field_error_body("identifier", err.detail()),
                    format!("{{\"identifier\":[\"{detail}\"]}}"),
                    "node {node}"
                );
            }
        }
        // Lowercase passes validation unstripped of case (B-case-dup: the
        // duplicate check runs pre-uppercase).
        assert_eq!(check_identifier_value("ab12", false), Ok(()));
        assert_eq!(
            IdentifierError::Forbidden.detail(),
            "PROJECT_IDENTIFIER_CANNOT_CONTAIN_SPECIAL_CHARACTERS"
        );
        assert_eq!(
            IdentifierError::Taken.detail(),
            "PROJECT_IDENTIFIER_ALREADY_EXIST"
        );
    }

    #[test]
    fn is_valid_envelopes_match_fixture() {
        let fx = fixture();
        let env = fx.get("is_valid_envelopes").expect("envelopes");
        let dup = env.get("dup_name").expect("dup_name");
        assert_eq!(
            canonical(
                &serde_json::from_str(&field_error_body("name", NAME_TAKEN_DETAIL)).expect("json")
            ),
            canonical(dup)
        );
        let special = env.get("special_name").expect("special_name");
        assert_eq!(
            canonical(
                &serde_json::from_str(&field_error_body("name", NAME_FORBIDDEN_DETAIL))
                    .expect("json")
            ),
            canonical(special)
        );
        let empty = env.get("empty").expect("empty");
        assert_eq!(
            canonical(&serde_json::from_str(EMPTY_REQUIRED_BODY).expect("json")),
            canonical(empty)
        );
        assert_eq!(
            EMPTY_REQUIRED_BODY,
            "{\"name\":[\"This field is required.\"],\"identifier\":[\"This field is required.\"]}"
        );
    }

    #[test]
    fn executor_gate_vectors_match_fixture() {
        let fx = fixture();
        let validate = fx.get("validate").expect("validate");
        // cloud_agent while unconfigured → CloudAgentUnavailableAPI shape.
        assert!(!fx
            .get("validate_cloud_setting")
            .expect("setting")
            .as_bool()
            .expect("bool"));
        assert_eq!(
            check_executor_gate(Some("cloud_agent"), false),
            ExecutorGate::CloudUnavailable
        );
        let cloud = validate
            .get("cloud_agent_unconfigured")
            .expect("cloud node");
        assert_eq!(cloud.get("exc").expect("exc"), "CloudAgentUnavailableAPI");
        assert_eq!(CLOUD_UNAVAILABLE_STATUS, 409);
        assert_eq!(
            CLOUD_UNAVAILABLE_STATUS,
            cloud
                .get("status_code")
                .expect("status")
                .as_u64()
                .expect("u64") as u16
        );
        let body = serde_json::to_string(&cloud_unavailable_body()).expect("serializes");
        assert_eq!(
            canonical(&serde_json::from_str(&body).expect("json")),
            canonical(cloud.get("detail").expect("detail"))
        );
        assert_eq!(
            body,
            "{\"error\":\"Pi Dash Cloud Agent is not currently available\",\"code\":\"cloud_agent_unavailable\"}"
        );
        // The fixture's standalone exception node agrees.
        let exc = fx.get("cloud_exception").expect("cloud_exception");
        assert_eq!(exc.get("status_code").expect("s"), 409);
        assert_eq!(
            canonical(&serde_json::from_str(&body).expect("json")),
            canonical(exc.get("detail").expect("detail"))
        );
        // Ungated kinds and absent input pass.
        assert_eq!(
            check_executor_gate(Some("local_runner"), false),
            ExecutorGate::Ok
        );
        assert_eq!(check_executor_gate(None, false), ExecutorGate::Ok);
        assert_eq!(
            check_executor_gate(Some("cloud_agent"), true),
            ExecutorGate::Ok
        );
        assert!(validate
            .get("local_runner_ok")
            .expect("lr")
            .get("ok")
            .expect("ok")
            .as_bool()
            .expect("b"));
        assert!(validate
            .get("no_executor_ok")
            .expect("ne")
            .get("ok")
            .expect("ok")
            .as_bool()
            .expect("b"));
    }

    #[test]
    fn is_default_guard_matches_fixture() {
        let fx = fixture();
        let validate = fx.get("validate").expect("validate");
        let guard = validate.get("is_default_unset_guard").expect("guard");
        assert!(!guard.get("ok").expect("ok").as_bool().expect("bool"));
        assert_eq!(guard.get("exc").expect("exc"), "ValidationError");
        assert_eq!(
            check_is_default_unset(true, Some(false)),
            Err(DefaultUnsetError)
        );
        assert_eq!(
            DefaultUnsetError.body(),
            "{\"non_field_errors\":[\"Default project cannot be unset without assigning another default project.\"]}"
        );
        // Non-default instances and missing/true values pass.
        assert_eq!(check_is_default_unset(false, Some(false)), Ok(()));
        assert_eq!(check_is_default_unset(true, None), Ok(()));
        assert_eq!(check_is_default_unset(true, Some(true)), Ok(()));
        assert_eq!(check_is_default_unset(false, None), Ok(()));
        let other = validate.get("is_default_unset_other_ok").expect("other");
        assert!(other.get("ok").expect("ok").as_bool().expect("bool"));
    }

    #[test]
    fn json_truthiness_matches_python() {
        for falsy in [
            json!(null),
            json!(false),
            json!(0),
            json!(0.0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(!json_is_truthy(&falsy), "value {falsy}");
        }
        for truthy in [
            json!(true),
            json!(1),
            json!(-2),
            json!(0.5),
            json!("x"),
            json!([null]),
            json!({"a": 1}),
        ] {
            assert!(json_is_truthy(&truthy), "value {truthy}");
        }
    }

    #[test]
    fn description_html_branch_matches_fixture() {
        let fx = fixture();
        let validate = fx.get("validate").expect("validate");
        // Clean HTML passes with the sanitized write-back.
        let clean = validate.get("description_html_clean").expect("clean");
        let verdict = HtmlVerdict {
            is_valid: true,
            sanitized: Some("<p>hello</p>".to_string()),
        };
        assert_eq!(
            check_description_html(Some(&json!("<p>hello</p>")), || verdict.clone()),
            Ok(Some("<p>hello</p>".to_string()))
        );
        assert_eq!(
            clean
                .get("data")
                .expect("data")
                .get("description_html")
                .expect("h"),
            "<p>hello</p>"
        );
        // Stripped input passes with the cleaned replacement.
        let script = validate.get("description_html_script").expect("script");
        assert_eq!(
            check_description_html(Some(&json!("<p>x</p><script>y</script>")), || HtmlVerdict {
                is_valid: true,
                sanitized: Some("<p>x</p>".to_string())
            }),
            Ok(Some("<p>x</p>".to_string()))
        );
        assert_eq!(
            script
                .get("data")
                .expect("data")
                .get("description_html")
                .expect("h"),
            "<p>x</p>"
        );
        // Empty string and absent keys skip WITHOUT consulting the verdict.
        let no_call = || panic!("verdict must not run for falsy/absent input");
        assert_eq!(check_description_html(Some(&json!("")), no_call), Ok(None));
        fn no_call_value() -> HtmlVerdict {
            panic!("verdict must not run for falsy/absent input")
        }
        assert_eq!(check_description_html(None, no_call_value), Ok(None));
        let empty = validate
            .get("description_html_empty_string")
            .expect("empty");
        assert_eq!(
            empty
                .get("data")
                .expect("data")
                .get("description_html")
                .expect("h"),
            ""
        );
        // Invalid verdicts (e.g. the over-10MB size refusal) raise with the
        // fixed body and never write back.
        assert_eq!(
            check_description_html(Some(&json!("x".repeat(11 * 1024 * 1024).as_str())), || {
                HtmlVerdict {
                    is_valid: false,
                    sanitized: None,
                }
            }),
            Err(HtmlInvalidError)
        );
        assert_eq!(
            HtmlInvalidError.body(),
            "{\"error\":[\"html content is not valid\"]}"
        );
        let boundary = fx.get("html_size_boundary").expect("boundary");
        let over = boundary.get("over_10MB").expect("over");
        assert!(!over.get("is_valid").expect("iv").as_bool().expect("bool"));
        assert_eq!(
            canonical(&serde_json::from_str(HtmlInvalidError.body()).expect("json")),
            canonical(over.get("errors").expect("errors"))
        );
    }

    #[test]
    fn create_normalisation_matches_fixture() {
        let fx = fixture();
        assert_eq!(normalize_identifier(" eng "), "ENG");
        assert_eq!(normalize_identifier("eng"), "ENG");
        assert_eq!(normalize_identifier("ENG"), "ENG");
        // Lowercase identifiers validate raw but store upper (B-case-dup).
        let lower = fx.get("create_lowercase_identifier").expect("lower");
        let sent = lower.get("sent").expect("sent").as_str().expect("str");
        assert_eq!(
            parse_identifier_input(Some(&json!(sent))).expect("parses"),
            sent
        );
        assert_eq!(
            normalize_identifier(sent),
            lower
                .get("stored_project_identifier")
                .expect("stored")
                .as_str()
                .expect("str")
        );
        assert_eq!(
            create_identifier_row_name(sent),
            lower
                .get("stored_identifier_row")
                .expect("rows")
                .as_array()
                .expect("array")[0]
                .get("name")
                .expect("name")
                .as_str()
                .expect("str")
        );
        // The create() project row stores the uppercased identifier.
        let create = fx.get("create").expect("create");
        let row = create.get("project_row").expect("row");
        assert_eq!(row.get("identifier").expect("id"), "A962C417");
        assert_eq!(
            create
                .get("identifier_rows")
                .expect("irows")
                .as_array()
                .expect("array")[0]
                .get("name")
                .expect("name"),
            "A962C417"
        );
    }

    #[test]
    fn members_kernel_matches_fixture() {
        let fx = fixture();
        // Fresh instances (no members_list attribute) render [].
        assert_eq!(project_list_members(None), Vec::<Uuid>::new());
        let member_id: Uuid = fx
            .get("list_serializer")
            .expect("list")
            .get("members")
            .expect("members")
            .as_array()
            .expect("array")[0]
            .as_str()
            .expect("str")
            .parse()
            .expect("uuid");
        let other = Uuid::new_v4();
        let bot = Uuid::new_v4();
        let rows = [
            MemberRow {
                member_id,
                is_active: true,
                member_is_bot: false,
            },
            MemberRow {
                member_id: other,
                is_active: false,
                member_is_bot: false,
            },
            MemberRow {
                member_id: bot,
                is_active: true,
                member_is_bot: true,
            },
        ];
        assert_eq!(project_list_members(Some(&rows)), vec![member_id]);
    }

    #[test]
    fn next_sequence_kernel_matches_fixture() {
        let fx = fixture();
        let list = fx.get("list_serializer").expect("list");
        assert_eq!(list.get("next_seq_empty").expect("empty"), 1);
        assert_eq!(next_work_item_sequence(None), 1);
        assert_eq!(next_work_item_sequence(Some(0)), 1);
        assert_eq!(list.get("next_seq_after_rows").expect("after"), 8);
        assert_eq!(next_work_item_sequence(Some(7)), 8);
        assert_eq!(
            list.get("full")
                .expect("full")
                .get("next_work_item_sequence")
                .expect("seq"),
            1
        );
    }

    #[test]
    fn options_user_resolution_matches_serializer() {
        let flags = UserFlags {
            is_active: true,
            is_bot: false,
        };
        // No request / no user → None.
        assert_eq!(resolve_options_user(None, true), None);
        assert_eq!(resolve_options_user(None, false), None);
        // Present but unauthenticated → None.
        assert_eq!(resolve_options_user(Some(flags), false), None);
        // Authenticated → passed through.
        assert_eq!(resolve_options_user(Some(flags), true), Some(flags));
    }

    #[test]
    fn executor_options_rows_match_fixture() {
        let fx = fixture();
        let list = fx.get("list_serializer").expect("list");
        // Fixture env: cloud switch off, no local runner, managed switch
        // off — all three request-user variants collapse to the same rows.
        let expected = list.get("agent_options_unauthenticated").expect("unauth");
        assert_eq!(
            list.get("agent_options_authenticated").expect("auth"),
            expected
        );
        assert_eq!(
            list.get("agent_options_no_request").expect("noreq"),
            expected
        );
        let flags = UserFlags {
            is_active: true,
            is_bot: false,
        };
        for (user, authed) in [
            (None, false),
            (None, true),
            (Some(flags), false),
            (Some(flags), true),
        ] {
            // The EE seam must not run: cloud is off, so D-11 short-circuits
            // before consulting it.
            let rows = project_executor_options(
                &cloud_settings(false),
                user,
                authed,
                || panic!("seam must not run while cloud is off"),
                false,
                managed_disabled(),
            );
            let actual: Value = serde_json::to_value(&rows).expect("serializes");
            assert_eq!(
                canonical(&actual),
                canonical(expected),
                "user {user:?} authed {authed}"
            );
        }
        let rows = project_executor_options(
            &cloud_settings(false),
            None,
            false,
            || panic!("seam must not run while cloud is off"),
            false,
            managed_disabled(),
        );
        let bytes = serde_json::to_string(&rows).expect("serializes");
        assert_eq!(
            bytes,
            "[{\"kind\":\"cloud_agent\",\"available\":false,\"reason_code\":\"cloud_agent_unavailable\"},{\"kind\":\"local_runner\",\"available\":false,\"reason_code\":\"no_local_runner\"},{\"kind\":\"managed_runner\",\"available\":false,\"reason_code\":\"managed_runner_disabled\"}]"
        );
    }

    #[test]
    fn key_order_consts_match_fixture() {
        let fx = fixture();
        assert_eq!(PROJECT_KEY_ORDER.len(), 49);
        assert_eq!(
            PROJECT_KEY_ORDER,
            str_list(
                fx.get("project_serializer")
                    .expect("ps")
                    .get("key_order")
                    .expect("ko")
            )
            .as_slice()
        );
        assert_eq!(PROJECT_LIST_KEY_ORDER.len(), 55);
        assert_eq!(
            PROJECT_LIST_KEY_ORDER,
            str_list(
                fx.get("list_serializer")
                    .expect("ls")
                    .get("key_order")
                    .expect("ko")
            )
            .as_slice()
        );
        assert_eq!(
            PROJECT_LITE_FIELDS,
            str_list(fx.get("lite").expect("lite").get("key_order").expect("ko")).as_slice()
        );
        assert_eq!(PROJECT_DETAIL_KEY_ORDER.len(), 51);
        assert_eq!(
            PROJECT_DETAIL_KEY_ORDER,
            str_list(
                fx.get("detail")
                    .expect("detail")
                    .get("key_order")
                    .expect("ko")
            )
            .as_slice()
        );
        assert_eq!(
            DEPLOY_BOARD_KEY_ORDER,
            str_list(
                fx.get("deploy_board")
                    .expect("board")
                    .get("key_order")
                    .expect("ko")
            )
            .as_slice()
        );
        assert_eq!(PROJECT_READ_ONLY_FIELDS, &["workspace", "deleted_at"]);
        assert_eq!(
            DEPLOY_BOARD_READ_ONLY_FIELDS,
            &["workspace", "project", "anchor"]
        );
    }

    /// Full byte proof for a read shape: struct → bytes, then top-level
    /// key order scanned from the bytes, values compared canonically, and
    /// every nested object/array key order scanned too.
    fn assert_shape_replay<T>(
        shape: &T,
        golden: &Value,
        key_order: &[&str],
        nested_orders: &[(&str, Vec<&str>)],
    ) where
        T: Serialize,
    {
        let actual = serde_json::to_string(shape).expect("serializes");
        assert_eq!(object_keys(&actual), key_order, "top-level key order");
        let produced: Value = serde_json::from_str(&actual).expect("reparses");
        assert_eq!(canonical(&produced), canonical(golden), "values");
        for (key, order) in nested_orders {
            let span = value_span(&actual, key);
            if span.starts_with('{') {
                assert_eq!(object_keys(span), *order, "nested order of {key}");
            } else if span.starts_with('[') {
                // Fixed-shape rows: the key sequence repeats per row.
                let seq = all_key_sequence(span);
                let rows = seq.len() / order.len();
                assert_ne!(rows, 0, "empty array for {key}");
                for row in seq.chunks(order.len()) {
                    assert_eq!(row, *order, "row order of {key}");
                }
            } else {
                panic!("nested {key} is neither object nor array: {span}");
            }
        }
    }

    #[test]
    fn project_shape_replays_golden_byte_for_byte() {
        let fx = fixture();
        let node = fx.get("project_serializer").expect("ps");
        // inbox_view is the intake_view alias: same value, two keys.
        assert_eq!(
            node.get("golden")
                .expect("g")
                .get("inbox_view")
                .expect("iv"),
            node.get("golden")
                .expect("g")
                .get("intake_view")
                .expect("tv")
        );
        let shape: ProjectRead =
            serde_json::from_value(node.get("golden").expect("golden").clone()).expect("fits");
        assert_shape_replay(
            &shape,
            node.get("golden").expect("golden"),
            &str_list(node.get("key_order").expect("ko")),
            &[
                ("workspace_detail", vec!["name", "slug", "id", "logo_url"]),
                (
                    "agent_executor_options",
                    vec!["kind", "available", "reason_code"],
                ),
            ],
        );
    }

    #[test]
    fn list_shape_replays_golden_byte_for_byte() {
        let fx = fixture();
        let node = fx.get("list_serializer").expect("ls");
        let full = node.get("full").expect("full");
        assert_eq!(
            full.get("inbox_view").expect("iv"),
            full.get("intake_view").expect("tv")
        );
        let mut shape: ProjectListRead = serde_json::from_value(full.clone()).expect("fits");
        // serde collapses explicit null to None on Deserialize; restore the
        // present-null (Some(None)) the golden pins for the nullable
        // annotations (absent keys stay None).
        if full.get("anchor") == Some(&Value::Null) {
            shape.anchor = Some(None);
        }
        if full.get("sort_order") == Some(&Value::Null) {
            shape.sort_order = Some(None);
        }
        if full.get("member_role") == Some(&Value::Null) {
            shape.member_role = Some(None);
        }
        assert_shape_replay(
            &shape,
            full,
            &str_list(node.get("key_order").expect("ko")),
            &[(
                "agent_executor_options",
                vec!["kind", "available", "reason_code"],
            )],
        );
        // Fresh instances (create-201) omit the four annotations.
        let mut fresh = full.clone();
        for key in ["is_favorite", "sort_order", "member_role", "anchor"] {
            fresh.as_object_mut().expect("obj").remove(key);
        }
        fresh
            .as_object_mut()
            .expect("obj")
            .insert("members".to_string(), json!([]));
        let shape: ProjectListRead = serde_json::from_value(fresh).expect("fresh fits");
        let actual = serde_json::to_string(&shape).expect("serializes");
        let keys = object_keys(&actual);
        for key in ["is_favorite", "sort_order", "member_role", "anchor"] {
            assert!(!keys.contains(&key.to_string()), "absent {key}");
        }
        assert_eq!(value_span(&actual, "members"), "[]");
        assert_eq!(value_span(&actual, "next_work_item_sequence"), "1");
    }

    #[test]
    fn lite_shape_replays_golden_byte_for_byte() {
        let fx = fixture();
        let node = fx.get("lite").expect("lite");
        let shape: ProjectLiteRead =
            serde_json::from_value(node.get("data").expect("data").clone()).expect("fits");
        assert_shape_replay(
            &shape,
            node.get("data").expect("data"),
            &str_list(node.get("key_order").expect("ko")),
            &[],
        );
    }

    #[test]
    fn detail_shape_replays_golden_byte_for_byte() {
        let fx = fixture();
        let node = fx.get("detail").expect("detail");
        let data = node.get("data").expect("data");
        let mut shape: ProjectDetailRead = serde_json::from_value(data.clone()).expect("fits");
        // Same present-null restore as the list shape.
        if data.get("anchor") == Some(&Value::Null) {
            shape.anchor = Some(None);
        }
        if data.get("sort_order") == Some(&Value::Null) {
            shape.sort_order = Some(None);
        }
        if data.get("member_role") == Some(&Value::Null) {
            shape.member_role = Some(None);
        }
        assert_shape_replay(
            &shape,
            data,
            &str_list(node.get("key_order").expect("ko")),
            &[(
                "agent_executor_options",
                vec!["kind", "available", "reason_code"],
            )],
        );
    }

    #[test]
    fn deploy_board_shape_replays_golden_byte_for_byte() {
        let fx = fixture();
        let node = fx.get("deploy_board").expect("board");
        let shape: DeployBoardRead =
            serde_json::from_value(node.get("data").expect("data").clone()).expect("fits");
        assert_shape_replay(
            &shape,
            node.get("data").expect("data"),
            &str_list(node.get("key_order").expect("ko")),
            &[("workspace_detail", vec!["name", "slug", "id", "logo_url"])],
        );
        // The unbound `.data` is DRF `get_initial()`: 12 writable keys.
        let none = fx.get("deploy_board_none_instance").expect("none");
        assert_eq!(none.get("repr_type").expect("rt"), "ReturnDict");
        let order = str_list(none.get("key_order").expect("ko"));
        assert_eq!(
            order,
            [
                "deleted_at",
                "entity_identifier",
                "entity_name",
                "is_comments_enabled",
                "is_reactions_enabled",
                "is_votes_enabled",
                "view_props",
                "is_activity_enabled",
                "is_disabled",
                "created_by",
                "updated_by",
                "intake"
            ]
        );
        let data = none.get("data").expect("data");
        let parts: Vec<String> = order
            .iter()
            .map(|k| format!("\"{k}\":{}", serde_json::to_string(&data[k]).expect("v")))
            .collect();
        assert_eq!(
            format!("{{{}}}", parts.join(",")),
            DEPLOY_BOARD_INITIAL_BODY
        );
    }

    #[test]
    fn effective_expand_discards_fields() {
        // B-fields-dead: whatever ?fields= carried, only expand selects.
        assert_eq!(effective_expand(true, None), Vec::<ExpandItem>::new());
        assert_eq!(effective_expand(false, None), Vec::<ExpandItem>::new());
        let expand = vec![ExpandItem::Name("members".to_string())];
        assert_eq!(effective_expand(true, Some(expand.clone())), expand);
    }

    #[test]
    fn filter_additions_match_base_py() {
        // "project" is not a list field but is in the add map → added, many=false.
        assert_eq!(
            filter_field_additions(
                PROJECT_LIST_KEY_ORDER,
                &[ExpandItem::Name("project".to_string())]
            ),
            Ok(vec![AddedField {
                name: "project".to_string(),
                many: false
            }])
        );
        // "members" IS a list field → no addition (render overwrite instead).
        assert_eq!(
            filter_field_additions(
                PROJECT_LIST_KEY_ORDER,
                &[ExpandItem::Name("members".to_string())]
            ),
            Ok(vec![])
        );
        // Unknown names are silently ignored.
        assert_eq!(
            filter_field_additions(
                PROJECT_LIST_KEY_ORDER,
                &[ExpandItem::Name("nope".to_string())]
            ),
            Ok(vec![])
        );
        // {name: scalar} behaves like a name.
        assert_eq!(
            filter_field_additions(
                PROJECT_LIST_KEY_ORDER,
                &[ExpandItem::DictValue("labels".to_string())]
            ),
            Ok(vec![AddedField {
                name: "labels".to_string(),
                many: true
            }])
        );
        // {name: [...]} always raises: KeyError for unknown keys …
        assert_eq!(
            filter_field_additions(
                PROJECT_LIST_KEY_ORDER,
                &[ExpandItem::DictList("nope".to_string(), vec![])]
            ),
            Err(ExpandError::UnknownField("nope".to_string()))
        );
        // … TypeError for known ones (even with an empty list).
        assert_eq!(
            filter_field_additions(
                PROJECT_LIST_KEY_ORDER,
                &[ExpandItem::DictList("members".to_string(), vec![])]
            ),
            Err(ExpandError::NestedNotSupported("members".to_string()))
        );
        assert_eq!(EXPANSION_ADD_FIELDS.len(), 21);
        assert_eq!(EXPANSION_MANY_FIELDS.len(), 10);
        assert!(is_render_expandable("members"));
        assert!(is_render_expandable("issue_attachment"));
        assert!(!is_render_expandable("nope"));
    }

    #[test]
    fn representation_override_matches_base_py() {
        use RepresentationOverride::*;
        // Not a field → untouched.
        assert_eq!(representation_override(false, true, false), Unchanged);
        assert_eq!(representation_override(false, false, true), Unchanged);
        // In fields but outside the render map → {expand}_id getattr.
        assert_eq!(representation_override(true, false, false), IdAttribute);
        assert_eq!(representation_override(true, false, true), IdAttribute);
        // In fields and in the map → nested re-render, many iff list.
        assert_eq!(representation_override(true, true, true), NestedMany);
        assert_eq!(representation_override(true, true, false), NestedOne);
    }
}
