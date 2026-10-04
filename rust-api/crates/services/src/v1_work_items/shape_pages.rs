#![forbid(unsafe_code)]

//! Page serializer shapes (D-18 serializers G, PIDASHCONV-666).
//!
//! Ports `apps/api/pi_dash/api/serializers/page.py:27-112`
//! (`PageLiteSerializer` `:27-49`, `PageDetailSerializer` `:50-71`,
//! `PageWriteSerializer` `:72-95`, `PageCreateSerializer` `:96-101`,
//! `PageUpdateSerializer` `:102-112`) over the `BaseSerializer`
//! passes (`api/serializers/base.py`: `fields=` filtering at `:19-30`
//! and expansion at `:72-117`).
//!
//! Fixture: F18-04
//! (`rust-api/fixtures/v1_work_items/serializers/F18-04.page_shapes.golden.json`).
//! Every replay `#[test]` below replays it: golden in/out byte-identical,
//! including validation error strings and codes. The fixture records read
//! values as Python reprs (`"None"`, `"False"`, `"0"`) and write values the
//! same way (`"'n'"`, `"UUID('…')"`); the tests interpret those reprs
//! explicitly rather than comparing them as JSON.
//!
//! This module is pure, following the S1 precedent ([`shape_issue`]): every
//! check that needs the database, the markdown converter or a sibling-owned
//! render in Python takes the already-resolved fact as an argument. The
//! queries and handler layers supply those facts; the error bodies, key
//! orders and check order here are the contract they must honor.
//!
//! Seams (caller-supplied, resolved outside this module):
//!
//! * HTML → markdown conversion (`utils/markdown_converter.py`,
//!   `TiptapMarkdownConverter` over the `markdownify` wheel). No Rust port
//!   exists and no issue owns it; [`PageRow::description_markdown`] takes
//!   the converted text as a plain `&str` and this module ports the key
//!   order and the never-raises contract around it. Handler issue
//!   PIDASHCONV-679 needs the real converter for byte-identical
//!   `description_markdown` reads.
//! * Datetimes/dates cross this boundary already rendered as DRF strings
//!   (the handler owns `USE_TZ`/`TIME_ZONE = UTC` conversion and DRF
//!   ISO-8601: `2026-10-02T23:17:12.040770Z`, fraction dropped when zero,
//!   `settings/common.py:361-362`) — rendering here is a byte-exact
//!   passthrough, following the `app_issues` precedent.
//! * The `expand=parent` render (the `IssueLite` read shape is
//!   PIDASHCONV-661's scope): [`PageReadInput::expansions`] carries the
//!   caller-rendered value, `None` rendering `{}` for a null FK.
//!
//! Reused, not forked: [`filter_fields`] (the `?fields=` kernel this
//! module's `mod.rs` owns) and [`crate::v1_projects::ser_collab`]
//! `UserLite` (the `expand=owned_by` render — the same
//! `api/serializers/user.py:13-38` unit D-19 ported, following the S1
//! precedent). D-30's app-plane page shapes (`app_pages::shape`) cover
//! different serializers with different field lists, so nothing there is
//! byte-identical; this module ports inline.
//!
//! JSON rendering notes:
//!
//! * `to_representation` builds a `serde_json::Map` in wire order; key order
//!   is insertion order (`preserve_order`, declared on this crate's
//!   `serde_json` dependency). Byte-exact order assertions in the tests
//!   guard it. DRF's compact separators (`(',', ':')`) match `serde_json`
//!   compact output.
//! * DRF wraps `ValidationError("msg")` raised in `validate()` as
//!   `{"non_field_errors": ["msg"]}`, and per-field failures as
//!   `{"<field>": ["<message>", ...]}` in field-declaration order. Codes
//!   (`required`, `blank`, `invalid_choice`, …) travel on the Rust error
//!   type for the fixture replay but are never rendered to the wire —
//!   DRF renders `ErrorDetail` strings only.
//! * `validate()` runs only when NO field error exists (DRF
//!   `Serializer.to_internal_value` raises before `run_validation` reaches
//!   `validate()`), so field errors and `non_field_errors` never mix.
//! * All three write serializers share one field order — `name`,
//!   `description_markdown`, `description_html`, `parent`, `access` —
//!   because the metaclass pops the subclass attrs before computing `known`
//!   (`serializers.py:285-305`, DRF 3.15.2): the overridden `name` keeps
//!   its base slot with the subclass value. Verified by probe (all three
//!   list `name` first; multi-error bodies lead with `name`).
//!
//! Ported quirks (translate, don't redesign — all verified by probe
//! against the venv Django/DRF 3.15.2):
//!
//! * `expand=parent` renders `IssueLiteSerializer` over a `Page`
//!   (`base.py:104` maps `parent` to `IssueLiteSerializer`): only `id`
//!   survives (`sequence_id`/`project_id` `SkipField`), a null parent
//!   renders `{}`.
//! * `expand=<scalar>` (`name`, `access`, …) nulls that scalar
//!   (`getattr(instance, f"{expand}_id", None)`, `base.py:116`).
//! * `expand=owned_by` with no owner 500s (`RelatedObjectDoesNotExist` —
//!   the FK descriptor raises for a `None` id on a non-nullable field),
//!   unlike the nullable `parent` which renders `{}`.
//! * `parent` accepts JSON booleans and ints (`isinstance(data, int)` is
//!   true for `bool`; `uuid.UUID(int=…)`): `true` validates to
//!   `00000000-0000-0000-0000-000000000001`.
//! * The UUID string grammar is CPython's (`uuid.py:174-179` +
//!   `int(x, 16)` leniency): lowercase `urn:`/`uuid:` prefixes stripped,
//!   braces stripped at the ends only, hyphens removed, length 32, then
//!   surrounding Unicode whitespace, one `+` sign, one `0x` prefix, and
//!   single interior underscores (plus one after `0x`) all accepted.
//! * `access` errors quote Python `str()` of the input:
//!   `"True" is not a valid choice.`, `"0.0" is not a valid choice.`.
//! * `str.strip()` (name trimming) strips `\x1c`-`\x1f` and `\x85`, which
//!   Rust's Unicode `trim` leaves — matched with an explicit 29-codepoint
//!   table generated from CPython's `str.isspace`.
//!
//! [`shape_issue`]: super::shape_issue

use serde_json::{Map, Value};

use crate::v1_projects::ser_collab::{user_lite_to_representation, UserLiteRow};

use super::{filter_fields, FieldSpec, FilterError};

// ---------------------------------------------------------------------------
// Read shapes: PageLiteSerializer / PageDetailSerializer
// ---------------------------------------------------------------------------

/// `PageLiteSerializer.Meta.fields`, in wire order
/// (`api/serializers/page.py:36-46`). `id` renders the UUID pk
/// (`BaseSerializer.id`, `base.py:17`).
pub const PAGE_LITE_FIELDS: &[&str] = &[
    "id",
    "name",
    "parent",
    "owned_by",
    "access",
    "is_locked",
    "archived_at",
    "created_at",
    "updated_at",
];

/// `PageDetailSerializer.Meta.fields`, in wire order (`page.py:60-65`):
/// the lite fields plus the stored HTML, the stripped text, and the derived
/// markdown.
pub const PAGE_DETAIL_FIELDS: &[&str] = &[
    "id",
    "name",
    "parent",
    "owned_by",
    "access",
    "is_locked",
    "archived_at",
    "created_at",
    "updated_at",
    "description_html",
    "description_stripped",
    "description_markdown",
];

/// A database row for the page `to_representation` pair
/// (`db/models/page.py`, `pages` table — column types/nullability per
/// F18-05). The pk and FKs are canonical UUID strings
/// (`PrimaryKeyRelatedField`, read-only); datetimes and the date cross this
/// boundary already rendered as DRF strings (see the module docs) —
/// rendering here is a byte-exact passthrough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    /// Canonical UUID string, or `None` (SQL NULL → `null`).
    pub parent: Option<&'a str>,
    /// Canonical UUID string. `None` renders `null` (corrupt data — the
    /// column is non-nullable); `expand=owned_by` over it 500s in Django
    /// (see [`RenderError::MissingOwner`]).
    pub owned_by: Option<&'a str>,
    /// `PositiveSmallIntegerField` with choices (`page.py` model `:37`):
    /// DRF renders the int as-is (`ChoiceField.to_representation` falls
    /// back to the value itself for unlisted ints).
    pub access: i64,
    pub is_locked: bool,
    /// Pre-rendered `YYYY-MM-DD`, or `None` (SQL NULL → `null`).
    pub archived_at: Option<&'a str>,
    /// Pre-rendered DRF ISO-8601 (`…T…Z`, fraction dropped when zero).
    pub created_at: &'a str,
    /// Pre-rendered DRF ISO-8601.
    pub updated_at: &'a str,
    /// Stored Tiptap HTML, or `None` (SQL NULL → `null`; the column is
    /// `blank=True` without `null=True`, so live rows carry a string).
    pub description_html: Option<&'a str>,
    /// Tag-stripped text (`None` renders `null`).
    pub description_stripped: Option<&'a str>,
    /// Caller-supplied `html_to_markdown(description_html)` output (seam —
    /// see the module docs). Always a string in Python (`""` for empty
    /// input; the converter never raises).
    pub description_markdown: &'a str,
}

/// Failure modes of [`render_page_lite`] / [`render_page_detail`]:
/// caller-contract violations plus the two 500-class render failures. None
/// of these are 400 wire bodies — handlers map them (the `Missing*` arms
/// reproduce Django 500s / contract violations; the `Fields` arm is the
/// shared kernel's `TypeError` parity).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// Nested `fields=` dict (`TypeError` parity, see [`filter_fields`]).
    #[error("fields filter failed: {0}")]
    Fields(#[from] FilterError),
    /// `expand=owned_by` with no owner row: Django's FK descriptor raises
    /// `RelatedObjectDoesNotExist` (`Page has no owned_by.`) when the id is
    /// `None` on a non-nullable field (probe-verified; a 500). The column
    /// is non-nullable, so live handlers always supply the row.
    #[error("expand 'owned_by' with no owner row (Django raises RelatedObjectDoesNotExist here)")]
    MissingOwner,
    /// A map-hit `expand` name with no caller value (Python always renders
    /// the related object, or `{}` when the FK is null).
    #[error("expand '{0}' needs its rendered value (None renders {{}})")]
    MissingExpansion(String),
}

/// `to_representation()` input for both page read shapes (`page.py:27-71` +
/// the Base passes).
#[derive(Debug, Clone, PartialEq)]
pub struct PageReadInput<'a> {
    /// The page row.
    pub row: &'a PageRow<'a>,
    /// The `fields=` argument (`None` = all fields; see [`filter_fields`]).
    /// Plain names only reach here in practice (comma-split query strings).
    pub fields: Option<&'a [FieldSpec]>,
    /// The `expand=` names in request order (comma-split query string).
    pub expand: &'a [&'a str],
    /// Owner row for `expand=owned_by`, rendered via the reused D-19
    /// `UserLite`. Looked up only when the expand hits; `None` there is
    /// [`RenderError::MissingOwner`] (Django 500s).
    pub owner: Option<&'a UserLiteRow<'a>>,
    /// Caller-rendered `IssueLite` expansion for `expand=parent`
    /// (PIDASHCONV-661's scope): `Some(value)` renders the object, `None`
    /// renders `{}` (null FK). Looked up only for `"parent"`; a missing
    /// entry is [`RenderError::MissingExpansion`].
    pub expansions: &'a [(&'a str, Option<Value>)],
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::String(text.to_string()),
        None => Value::Null,
    }
}

/// Port of `PageLiteSerializer.to_representation()` (`page.py:27-49`) over
/// the `BaseSerializer` passes (`base.py:19-30` `fields=` filtering,
/// `:72-117` expansion).
///
/// Key order is wire order ([`PAGE_LITE_FIELDS`], filtered). Base-expansion
/// rules, in `expand` order, for names in the kept fields:
///
/// * `owned_by` → the reused D-19 `UserLite` render (missing owner row is
///   [`RenderError::MissingOwner`]);
/// * `parent` → the caller value, or `{}` for a null FK (missing caller
///   value is [`RenderError::MissingExpansion`]);
/// * anything else kept → `null` (`getattr(instance, f"{expand}_id", None)`
///   — no page scalar has a `<name>_id` attribute, `base.py:116`).
///
/// Non-field names (e.g. `?expand=url`) are ignored: expansion only
/// overwrites keys already rendered, never adds any (unlike
/// `IssueSerializer`, pages define no custom `to_representation`).
pub fn render_page_lite(input: &PageReadInput<'_>) -> Result<Map<String, Value>, RenderError> {
    render_page(input, PAGE_LITE_FIELDS)
}

/// Port of `PageDetailSerializer.to_representation()` (`page.py:50-71`):
/// the lite render plus `description_html`, `description_stripped` and the
/// caller-supplied `description_markdown`, under the same Base passes.
pub fn render_page_detail(input: &PageReadInput<'_>) -> Result<Map<String, Value>, RenderError> {
    render_page(input, PAGE_DETAIL_FIELDS)
}

fn render_page(
    input: &PageReadInput<'_>,
    available: &[&str],
) -> Result<Map<String, Value>, RenderError> {
    let kept = filter_fields(available, input.fields)?;
    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "name" => Value::String(row.name.to_string()),
            "parent" => opt_str(row.parent),
            "owned_by" => opt_str(row.owned_by),
            "access" => Value::Number(row.access.into()),
            "is_locked" => Value::Bool(row.is_locked),
            "archived_at" => opt_str(row.archived_at),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "description_html" => opt_str(row.description_html),
            "description_stripped" => opt_str(row.description_stripped),
            "description_markdown" => Value::String(row.description_markdown.to_string()),
            // `filter_fields` only ever yields `available` names, and every
            // one is matched above.
            _ => unreachable!("kept field is always a known page field"),
        };
        out.insert(name.clone(), value);
    }
    // Base expansion pass (`base.py:76-116`), in `expand` order. `if expand
    // in self.fields` is the kept set — `out` holds exactly the kept
    // readable fields (no write-only fields on read serializers).
    for name in input.expand {
        if !out.contains_key(*name) {
            continue;
        }
        if *name == "owned_by" {
            let owner = input.owner.ok_or(RenderError::MissingOwner)?;
            let view = user_lite_to_representation(owner);
            let rendered =
                serde_json::to_value(view).expect("UserLite view is always serializable");
            out.insert(name.to_string(), rendered);
        } else if *name == "parent" {
            let found = input
                .expansions
                .iter()
                .find(|(key, _)| *key == *name)
                .map(|(_, value)| value.clone());
            match found {
                Some(Some(value)) => {
                    out.insert(name.to_string(), value);
                }
                Some(None) => {
                    out.insert(name.to_string(), Value::Object(Map::new()));
                }
                None => return Err(RenderError::MissingExpansion(name.to_string())),
            }
        } else {
            // `base.py:114-116` else branch: the `<name>_id` attribute
            // lookup misses for every other page field, defaulting None.
            out.insert(name.to_string(), Value::Null);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Write shapes: PageWriteSerializer / PageCreateSerializer / PageUpdateSerializer
// ---------------------------------------------------------------------------

/// Write field order shared by all three write serializers
/// (`page.py:80-84`; the `PageCreateSerializer` override keeps the base
/// slot — see the module docs). Field errors render in this order.
pub const WRITE_FIELDS_IN_ORDER: &[&str] = &[
    "name",
    "description_markdown",
    "description_html",
    "parent",
    "access",
];

/// `PageWriteSerializer.validate` both-bodies message (`page.py:88`).
pub const BOTH_BODIES_MESSAGE: &str =
    "Send the body as description_markdown or description_html, not both.";
/// `PageUpdateSerializer.validate` empty message (`page.py:108-111`).
pub const EMPTY_UPDATE_MESSAGE: &str =
    "Nothing to update: send at least one of name, description_markdown, description_html, parent, access.";
/// `Serializer.to_internal_value` non-dict message (`serializers.py:339-341`,
/// DRF 3.15.2), formatted with the JSON datatype name.
pub const NOT_A_DICT_PREFIX: &str = "Invalid data. Expected a dictionary, but got ";
/// Whole-body `null` message (`as_serializer_error`, `serializers.py:580`).
pub const NO_DATA_MESSAGE: &str = "No data provided";

/// Which write serializer to validate as (`page.py:72-112`). The field
/// loop is identical for all three; only `name` requiredness and the
/// object-level `validate()` differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// `PageWriteSerializer`: every field optional; both-bodies check only.
    Base,
    /// `PageCreateSerializer`: `name` required (`page.py:99`).
    Create,
    /// `PageUpdateSerializer`: every field optional, but at least one must
    /// validate (`page.py:105-112`).
    Update,
}

/// One validation failure, with the DRF wire message and the DRF code
/// (codes ride along for the fixture replay; DRF renders message strings
/// only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldFailure {
    /// `This field is required.` / `required`.
    Required,
    /// `This field may not be null.` / `null`.
    Null,
    /// `This field may not be blank.` / `blank` (CharField, `trim` variant).
    Blank,
    /// `Not a valid string.` / `invalid` (CharField non-string).
    InvalidString,
    /// `Must be a valid UUID.` / `invalid` (UUIDField).
    InvalidUuid,
    /// `"{input}" is not a valid choice.` / `invalid_choice`, carrying
    /// Python `str()` of the input.
    InvalidChoice(String),
    /// `Null characters are not allowed.` / `null_characters_not_allowed`
    /// (Django validator on every `CharField`).
    NullCharacters,
    /// [`BOTH_BODIES_MESSAGE`] / `invalid` (`non_field_errors`).
    BothBodies,
    /// [`EMPTY_UPDATE_MESSAGE`] / `invalid` (`non_field_errors`).
    EmptyUpdate,
    /// [`NO_DATA_MESSAGE`] / `null` (whole body is JSON `null`).
    NoData,
    /// [`NOT_A_DICT_PREFIX`] + datatype / `invalid` (whole body is not an
    /// object; carries the Python type name: `bool`, `int`, `float`,
    /// `str`, `list`).
    NotADict(&'static str),
}

impl FieldFailure {
    /// Byte-exact DRF message.
    pub fn message(&self) -> String {
        match self {
            FieldFailure::Required => "This field is required.".to_string(),
            FieldFailure::Null => "This field may not be null.".to_string(),
            FieldFailure::Blank => "This field may not be blank.".to_string(),
            FieldFailure::InvalidString => "Not a valid string.".to_string(),
            FieldFailure::InvalidUuid => "Must be a valid UUID.".to_string(),
            FieldFailure::InvalidChoice(input) => format!("\"{input}\" is not a valid choice."),
            FieldFailure::NullCharacters => "Null characters are not allowed.".to_string(),
            FieldFailure::BothBodies => BOTH_BODIES_MESSAGE.to_string(),
            FieldFailure::EmptyUpdate => EMPTY_UPDATE_MESSAGE.to_string(),
            FieldFailure::NoData => NO_DATA_MESSAGE.to_string(),
            FieldFailure::NotADict(datatype) => format!("{NOT_A_DICT_PREFIX}{datatype}."),
        }
    }

    /// DRF code (fixture replay; never rendered to the wire).
    pub fn code(&self) -> &'static str {
        match self {
            FieldFailure::Required => "required",
            FieldFailure::Null => "null",
            FieldFailure::Blank => "blank",
            FieldFailure::InvalidString => "invalid",
            FieldFailure::InvalidUuid => "invalid",
            FieldFailure::InvalidChoice(_) => "invalid_choice",
            FieldFailure::NullCharacters => "null_characters_not_allowed",
            FieldFailure::BothBodies => "invalid",
            FieldFailure::EmptyUpdate => "invalid",
            FieldFailure::NoData => "null",
            FieldFailure::NotADict(_) => "invalid",
        }
    }
}

/// The validated write payload (`validated_data`, `page.py:72-112`).
/// Unknown input keys are ignored (DRF iterates writable fields, never
/// the input dict).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidatedPageWrite {
    /// Trimmed name (present unless absent).
    pub name: Option<String>,
    /// Untrimmed body text (`allow_blank`, `trim_whitespace=False`).
    pub description_markdown: Option<String>,
    /// Untrimmed body HTML.
    pub description_html: Option<String>,
    /// Outer `None` = key absent (untouched); `Some(None)` = explicit
    /// `null` (clears the parent); `Some(Some(_))` = canonical lowercase
    /// hyphenated UUID. The views branch on presence (`"parent" in data`,
    /// `views/page.py:424,454-456`), so the three states survive.
    pub parent: Option<Option<String>>,
    /// Validated choice value (`0` or `1`).
    pub access: Option<i64>,
}

impl ValidatedPageWrite {
    /// Port of `PageWriteSerializer.has_body` (`page.py:91-93`): either
    /// body key validated — including an empty string (`allow_blank`).
    pub fn has_body(&self) -> bool {
        self.description_markdown.is_some() || self.description_html.is_some()
    }

    /// Whether no field validated (the `PageUpdateSerializer` empty check,
    /// `page.py:107`: `if not attrs`).
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description_markdown.is_none()
            && self.description_html.is_none()
            && self.parent.is_none()
            && self.access.is_none()
    }
}

/// The `is_valid() == False` outcome: per-field failures in
/// [`WRITE_FIELDS_IN_ORDER`] plus at most one object-level failure.
/// `validate()` runs only when the field list is empty, so the two never
/// mix (DRF `Serializer.run_validation`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageWriteErrors {
    /// `(field, failures)` in field order.
    pub field_errors: Vec<(String, Vec<FieldFailure>)>,
    /// `validate()` failures (`non_field_errors` on the wire).
    pub non_field_errors: Vec<FieldFailure>,
}

impl PageWriteErrors {
    fn add(&mut self, field: &str, failure: FieldFailure) {
        match self.field_errors.iter_mut().find(|(name, _)| name == field) {
            Some((_, failures)) => failures.push(failure),
            None => self.field_errors.push((field.to_string(), vec![failure])),
        }
    }

    fn is_empty(&self) -> bool {
        self.field_errors.is_empty() && self.non_field_errors.is_empty()
    }

    /// Byte-exact 400 body: `{"<field>": ["<message>", …], …}` in field
    /// order, or `{"non_field_errors": ["<message>"]}`.
    pub fn body(&self) -> String {
        let mut map = Map::with_capacity(self.field_errors.len() + 1);
        for (field, failures) in &self.field_errors {
            let messages: Vec<Value> = failures
                .iter()
                .map(|failure| Value::String(failure.message()))
                .collect();
            map.insert(field.clone(), Value::Array(messages));
        }
        if !self.non_field_errors.is_empty() {
            let messages: Vec<Value> = self
                .non_field_errors
                .iter()
                .map(|failure| Value::String(failure.message()))
                .collect();
            map.insert("non_field_errors".to_string(), Value::Array(messages));
        }
        serde_json::to_string(&map).expect("error bodies are always serializable")
    }
}

/// Python `type(data).__name__` for a non-object JSON body (the
/// `NotADict` datatype slot).
fn json_datatype(value: &Value) -> &'static str {
    match value {
        Value::Null | Value::Object(_) => unreachable!("caller handles null and object"),
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
    }
}

/// Port of `PageWriteSerializer.is_valid()` + `validated_data` for all
/// three write serializers (`page.py:72-112`).
///
/// Field loop first, in [`WRITE_FIELDS_IN_ORDER`] (`to_internal_value`
/// iterates `_writable_fields`; unknown keys ignored); then `validate()`
/// iff no field failed — the both-bodies check for every mode
/// (`page.py:86-89`), then the non-empty check for [`WriteMode::Update`]
/// (`page.py:105-112`, after `super().validate()`).
pub fn validate_page_write(
    data: &Value,
    mode: WriteMode,
) -> Result<ValidatedPageWrite, PageWriteErrors> {
    let mut errors = PageWriteErrors::default();
    let obj = match data {
        Value::Null => {
            errors.non_field_errors.push(FieldFailure::NoData);
            return Err(errors);
        }
        Value::Object(map) => map,
        other => {
            errors
                .non_field_errors
                .push(FieldFailure::NotADict(json_datatype(other)));
            return Err(errors);
        }
    };
    let mut out = ValidatedPageWrite::default();
    match validate_name(obj.get("name"), matches!(mode, WriteMode::Create)) {
        Ok(value) => out.name = value,
        Err(failure) => errors.add("name", failure),
    }
    match validate_body_text(obj.get("description_markdown")) {
        Ok(value) => out.description_markdown = value,
        Err(failure) => errors.add("description_markdown", failure),
    }
    match validate_body_text(obj.get("description_html")) {
        Ok(value) => out.description_html = value,
        Err(failure) => errors.add("description_html", failure),
    }
    match validate_parent(obj.get("parent")) {
        Ok(value) => out.parent = value,
        Err(failure) => errors.add("parent", failure),
    }
    match validate_access(obj.get("access")) {
        Ok(value) => out.access = value,
        Err(failure) => errors.add("access", failure),
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    if out.description_markdown.is_some() && out.description_html.is_some() {
        errors.non_field_errors.push(FieldFailure::BothBodies);
    } else if matches!(mode, WriteMode::Update) && out.is_empty() {
        errors.non_field_errors.push(FieldFailure::EmptyUpdate);
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// `name = CharField(required=?, allow_blank=False, trim_whitespace=True)`
/// (`page.py:80,99`).
///
/// Absent with `required=false` skips (`SkipField` — no default); `null`
/// fails (`allow_null` unset); bools/lists/dicts fail (`Not a valid
/// string.`); ints/floats coerce via Python `str()`; strings strip
/// (Python set) and fail when nothing remains (`blank`), then run the
/// null-characters validator on the stripped value (`fields.py:745-763`,
/// DRF 3.15.2).
fn validate_name(value: Option<&Value>, required: bool) -> Result<Option<String>, FieldFailure> {
    let Some(value) = value else {
        if required {
            return Err(FieldFailure::Required);
        }
        return Ok(None);
    };
    let text = match value {
        Value::Null => return Err(FieldFailure::Null),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            return Err(FieldFailure::InvalidString);
        }
        Value::Number(number) => python_number_str(number),
        Value::String(text) => text.clone(),
    };
    // Blank pre-check (`data == '' or str(data).strip() == ''`,
    // `fields.py:749`) and `to_internal_value` strip in one: a number's
    // `str()` never strips to empty, a string fails exactly when its
    // strip is empty.
    let stripped = python_strip(&text);
    if stripped.is_empty() {
        return Err(FieldFailure::Blank);
    }
    check_no_null_chars(stripped)?;
    Ok(Some(stripped.to_string()))
}

/// `description_markdown` / `description_html = CharField(required=False,
/// allow_blank=True, trim_whitespace=False)` (`page.py:81-82`).
///
/// Same shape as [`validate_name`] but never required, never trimmed, and
/// never blank: `""` validates (and counts for `has_body`).
/// `null`/bool/list/dict fail exactly like `name`.
fn validate_body_text(value: Option<&Value>) -> Result<Option<String>, FieldFailure> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = match value {
        Value::Null => return Err(FieldFailure::Null),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            return Err(FieldFailure::InvalidString);
        }
        Value::Number(number) => python_number_str(number),
        Value::String(text) => text.clone(),
    };
    check_no_null_chars(&text)?;
    Ok(Some(text))
}

/// The `CharField` null-characters validator
/// (`django.core.validators.ProhibitNullCharactersValidator`, wired at
/// DRF `fields.py:742`). The sibling surrogate validator
/// (`ProhibitSurrogateCharactersValidator`) is vacuous here: Rust `str`
/// cannot hold surrogates, and `serde_json` rejects lone `\uD800`-`\uDFFF`
/// escapes at parse (asserted in the tests), so validated text reaching
/// this module is always surrogate-free — while Python's `json` accepts
/// lone surrogates and 400s on them one layer down, at this module's
/// input boundary that input cannot exist.
fn check_no_null_chars(text: &str) -> Result<(), FieldFailure> {
    if text.contains('\0') {
        return Err(FieldFailure::NullCharacters);
    }
    Ok(())
}

/// `parent = UUIDField(required=False, allow_null=True)` (`page.py:83`).
///
/// Absent skips; explicit `null` validates to `None`; bools and ints go
/// through `uuid.UUID(int=…)` (so `true` is `…0001`); strings go through
/// the CPython hex grammar; floats/lists/dicts fail (`fields.py:837-848`,
/// DRF 3.15.2). Success yields the canonical lowercase hyphenated form
/// (`str(uuid)`).
///
/// Known edge: a JSON integer above `u64::MAX` (up to `2**128 - 1`, which
/// Python accepts) demotes to `f64` in `serde_json` without
/// `arbitrary_precision` and reads as `invalid` here. Closing it needs a
/// `serde_json` feature flip — a foundation-crate change, so a new issue
/// if it ever matters; no fixture or contract vector goes there.
fn validate_parent(value: Option<&Value>) -> Result<Option<Option<String>>, FieldFailure> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(Some(None)),
        Value::Bool(flag) => Ok(Some(Some(
            uuid::Uuid::from_u128(u128::from(*flag as u8))
                .hyphenated()
                .to_string(),
        ))),
        Value::Number(number) => match number.as_u64() {
            // Every `u64` is `< 2**128`; negatives and floats fail.
            Some(int) => Ok(Some(Some(
                uuid::Uuid::from_u128(u128::from(int))
                    .hyphenated()
                    .to_string(),
            ))),
            None => Err(FieldFailure::InvalidUuid),
        },
        Value::String(text) => match python_uuid_value(text) {
            Some(int) => Ok(Some(Some(
                uuid::Uuid::from_u128(int).hyphenated().to_string(),
            ))),
            None => Err(FieldFailure::InvalidUuid),
        },
        Value::Array(_) | Value::Object(_) => Err(FieldFailure::InvalidUuid),
    }
}

/// `access = ChoiceField(choices=Page.ACCESS_CHOICES, required=False)`
/// (`page.py:84`; choices `((1, "Private"), (0, "Public"))`,
/// `db/models/page.py:28`).
///
/// `choice_strings_to_values` is `{"1": 1, "0": 0}`: `str(data)` selects,
/// so JSON `0` and `"0"` both validate to int `0`. Anything else fails
/// with Python `str()` of the input quoted (`fields.py`, `ChoiceField`,
/// DRF 3.15.2).
fn validate_access(value: Option<&Value>) -> Result<Option<i64>, FieldFailure> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Err(FieldFailure::Null);
    }
    let text = python_str(value);
    match text.as_str() {
        "0" => Ok(Some(0)),
        "1" => Ok(Some(1)),
        _ => Err(FieldFailure::InvalidChoice(text)),
    }
}

// ---------------------------------------------------------------------------
// Python kernels (exact ports of the stdlib/DRF primitives the shapes call)
// ---------------------------------------------------------------------------

/// CPython `str.isspace` set — the 29 codepoints `str.strip()` trims
/// (generated from CPython: `[hex(ord(c)) for c in map(chr,
/// range(0x110000)) if c.isspace()]`). Rust's `char::is_whitespace`
/// (Unicode `White_Space`) misses `\x1c`-`\x1f` and `\x85`, so the table
/// is explicit. `int(x, 16)` strips this same set.
fn is_python_space(ch: char) -> bool {
    matches!(
        ch,
        '\u{9}'
            | '\u{a}'
            | '\u{b}'
            | '\u{c}'
            | '\u{d}'
            | '\u{1c}'
            | '\u{1d}'
            | '\u{1e}'
            | '\u{1f}'
            | '\u{20}'
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'
            | '\u{2001}'
            | '\u{2002}'
            | '\u{2003}'
            | '\u{2004}'
            | '\u{2005}'
            | '\u{2006}'
            | '\u{2007}'
            | '\u{2008}'
            | '\u{2009}'
            | '\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// CPython `str.strip()` (no args).
fn python_strip(text: &str) -> &str {
    text.trim_matches(is_python_space)
}

/// CPython `uuid.UUID(hex=…)` string path (`uuid.py:174-179,210-212`):
/// strip every lowercase `urn:`/`uuid:` prefix, strip braces at the ends
/// only, remove hyphens, require length 32 (code points), then
/// `int(x, 16)` leniency, then the `0 <= v < 2**128` range check.
/// Returns the 128-bit value; the caller hyphenates.
fn python_uuid_value(text: &str) -> Option<u128> {
    let no_prefix = text.replace("urn:", "").replace("uuid:", "");
    let no_braces = no_prefix.trim_matches(|ch| ch == '{' || ch == '}');
    let hex: String = no_braces.chars().filter(|ch| *ch != '-').collect();
    if hex.chars().count() != 32 {
        return None;
    }
    python_int_hex(&hex)
}

/// CPython `int(x, 16)` (`long_from_string_with_base`): strip surrounding
/// Python whitespace, one optional `+` sign (a `-` cannot survive the
/// hyphen removal, so negatives are unreachable here), one optional
/// `0x`/`0X` prefix, then hex digits with single interior underscores —
/// plus one optional underscore directly after the `0x` prefix
/// (`int('0x_12', 16)` is valid; `int('0x__12')`, `int('+_12')` and
/// `int('1__2')` are not — all probe-verified).
fn python_int_hex(text: &str) -> Option<u128> {
    let text = python_strip(text);
    if text.is_empty() {
        return None;
    }
    let text = text.strip_prefix('+').unwrap_or(text);
    let (text, prefixed) = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(rest) => (rest, true),
        None => (text, false),
    };
    let text = if prefixed {
        text.strip_prefix('_').unwrap_or(text)
    } else {
        text
    };
    if text.is_empty() {
        return None;
    }
    let mut value: u128 = 0;
    let mut digits = 0u32;
    for part in text.split('_') {
        if part.is_empty() {
            return None;
        }
        for ch in part.chars() {
            let digit = ch.to_digit(16)?;
            value = value.checked_mul(16)?.checked_add(u128::from(digit))?;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    Some(value)
}

/// Python `str()` of a JSON number: ints render decimal, floats render
/// [`python_float_repr`].
///
/// Known edge (the same `serde_json` demotion [`validate_parent`]
/// documents): a JSON integer above `u64::MAX` arrives here as `f64` and
/// renders as a float (`1e+30`) where Python renders the full digits —
/// observed in coerced `name`/body text and in `invalid_choice` messages
/// for huge-int `access` input. Same foundation flip to close.
fn python_number_str(number: &serde_json::Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(int) = number.as_u64() {
        return int.to_string();
    }
    python_float_repr(number.as_f64().expect("a JSON number is i64, u64 or f64"))
}

/// CPython `repr(float)` (== `str(float)`): shortest round-trip digits —
/// Rust's `{:?}` agrees digit-for-digit — with the exponent reformatted
/// (`1e16` → `1e+16`, `1e-5` → `1e-05`): always signed, at least two
/// digits. `inf`/`nan` are unreachable from parsed JSON (`serde_json`
/// rejects them) but render exactly anyway.
fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let rendered = format!("{value:?}");
    let Some(exp_at) = rendered.find('e') else {
        return rendered;
    };
    let (mantissa, exponent) = rendered.split_at(exp_at);
    let exp_value: i32 = exponent[1..]
        .parse()
        .expect("Rust formats the float exponent as a signed integer");
    format!("{mantissa}e{exp_value:+03}")
}

/// Python `str()` of a JSON value (the `ChoiceField` lookup key and the
/// `invalid_choice` message slot). Scalars render bare; arrays/objects
/// render with Python `repr` element formatting (single-quoted strings).
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(flag) => {
            if *flag {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Number(number) => python_number_str(number),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", python_repr_string(key), python_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a JSON value (nested positions only).
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => python_repr_string(text),
        _ => python_str(value),
    }
}

/// CPython string `repr`: single quotes unless the string contains `'`
/// and not `"`; short escapes for `\t`/`\n`/`\r`; `\\` and the active
/// quote escaped; other C0/C1 controls and DEL as `\xXX`; everything
/// else via Rust `escape_debug`, whose `\u{…}` sequences are reformatted
/// to Python `\u…`/`\U…`.
/// (`\x85` rides the `\xXX` arm — it is C1, non-printable in CPython.)
/// Printable-set edge differences vs CPython for exotic non-ASCII chars
/// (format/separator/unassigned codepoints) are a known approximation:
/// ASCII and everyday Unicode are exact, and only `invalid_choice`
/// messages with nested non-ASCII input can observe it.
fn python_repr_string(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        if ch == quote {
            out.push('\\');
            out.push(ch);
        } else if ch == '\'' || ch == '"' {
            // The inactive quote is never escaped (`"it's"`, `'say "hi"'`).
            out.push(ch);
        } else if ch == '\\' {
            out.push_str("\\\\");
        } else if ch == '\n' {
            out.push_str("\\n");
        } else if ch == '\r' {
            out.push_str("\\r");
        } else if ch == '\t' {
            out.push_str("\\t");
        } else if ch.is_control() || ch == '\u{7f}' {
            out.push_str(&format!("\\x{:02x}", ch as u32));
        } else {
            reformat_debug_escapes(&mut out, ch);
        }
    }
    out.push(quote);
    out
}

/// Append `ch`'s `escape_debug` rendering with `\u{…}` sequences
/// reformatted to CPython form (`\uHHHH` / `\UHHHHHHHH`, lowercase hex).
/// The caller pre-handles quotes, backslash, short escapes and controls,
/// so only a literal char or a `\u{…}` sequence can arrive here.
fn reformat_debug_escapes(out: &mut String, ch: char) {
    let debug = ch.escape_debug().to_string();
    let Some(rest) = debug.strip_prefix("\\u{") else {
        out.push_str(&debug);
        return;
    };
    let Some(hex) = rest.strip_suffix('}') else {
        out.push_str(&debug);
        return;
    };
    if hex.len() <= 2 {
        out.push_str(&format!("\\x{hex:0>2}"));
    } else if hex.len() <= 4 {
        out.push_str(&format!("\\u{hex:0>4}"));
    } else {
        out.push_str(&format!("\\U{hex:0>8}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const F18_04: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/serializers/F18-04.page_shapes.golden.json"
    );

    fn golden() -> serde_json::Value {
        let raw = std::fs::read_to_string(F18_04).expect("fixture golden exists");
        serde_json::from_str(&raw).expect("fixture golden is valid JSON")
    }

    fn str_list(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_array()
            .expect("golden carries a string list")
            .iter()
            .map(|item| item.as_str().expect("entries are strings"))
            .collect()
    }

    fn failure_list(failures: &[FieldFailure]) -> Value {
        Value::Array(
            failures
                .iter()
                .map(|failure| json!({"message": failure.message(), "code": failure.code()}))
                .collect(),
        )
    }

    /// `PageWriteErrors` in the fixture's `{field: [{message, code}]}` form.
    fn errors_to_golden(errors: &PageWriteErrors) -> Value {
        let mut map = Map::new();
        for (field, failures) in &errors.field_errors {
            map.insert(field.clone(), failure_list(failures));
        }
        if !errors.non_field_errors.is_empty() {
            map.insert(
                "non_field_errors".to_string(),
                failure_list(&errors.non_field_errors),
            );
        }
        Value::Object(map)
    }

    /// `ValidatedPageWrite` in the fixture's Python-repr form. F18-04 pins
    /// `name`/`description_*` reprs, the `access` int repr, and (by
    /// absence) that no other key validates; it carries no
    /// parent-validated vector, so this helper covers only what the
    /// golden pins (parent validation is asserted in the probe tables).
    fn validated_to_golden(validated: &ValidatedPageWrite) -> Value {
        let mut map = Map::new();
        if let Some(name) = &validated.name {
            map.insert("name".to_string(), Value::String(python_repr_string(name)));
        }
        if let Some(markdown) = &validated.description_markdown {
            map.insert(
                "description_markdown".to_string(),
                Value::String(python_repr_string(markdown)),
            );
        }
        if let Some(html) = &validated.description_html {
            map.insert(
                "description_html".to_string(),
                Value::String(python_repr_string(html)),
            );
        }
        if let Some(access) = validated.access {
            map.insert("access".to_string(), Value::String(access.to_string()));
        }
        Value::Object(map)
    }

    fn validate_ok(data: Value, mode: WriteMode) -> ValidatedPageWrite {
        match validate_page_write(&data, mode) {
            Ok(validated) => validated,
            Err(errors) => panic!("expected valid, got {}", errors.body()),
        }
    }

    fn validate_err(data: Value, mode: WriteMode) -> PageWriteErrors {
        match validate_page_write(&data, mode) {
            Ok(validated) => panic!("expected errors, got valid {validated:?}"),
            Err(errors) => errors,
        }
    }

    fn fixture_row<'a>(parsed: &'a serde_json::Value) -> PageRow<'a> {
        let render = &parsed["units"]["PageLiteSerializer"]["render"];
        PageRow {
            id: render["id"].as_str().expect("golden carries id"),
            name: render["name"].as_str().expect("golden carries name"),
            // Fixture reprs: "None" -> null, "0" -> 0, "False" -> false.
            parent: None,
            owned_by: Some(
                render["owned_by"]
                    .as_str()
                    .expect("golden carries owned_by"),
            ),
            access: 0,
            is_locked: false,
            archived_at: None,
            created_at: render["created_at"]
                .as_str()
                .expect("golden carries created_at"),
            updated_at: render["updated_at"]
                .as_str()
                .expect("golden carries updated_at"),
            description_html: None,
            description_stripped: None,
            description_markdown: "",
        }
    }

    // --- F18-04 replay: read shapes -------------------------------------

    #[test]
    fn lite_fields_match_f18_04_in_order() {
        let parsed = golden();
        let expected = str_list(&parsed["units"]["PageLiteSerializer"]["render_keys"]);
        assert_eq!(PAGE_LITE_FIELDS, expected.as_slice());
    }

    #[test]
    fn lite_render_matches_f18_04() {
        let parsed = golden();
        let row = fixture_row(&parsed);
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &[],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_lite(&input).expect("plain render succeeds");
        let render = &parsed["units"]["PageLiteSerializer"]["render"];
        let mut expected = Map::new();
        expected.insert(
            "id".to_string(),
            Value::String(render["id"].as_str().expect("id").to_string()),
        );
        expected.insert(
            "name".to_string(),
            Value::String(render["name"].as_str().expect("name").to_string()),
        );
        expected.insert("parent".to_string(), Value::Null);
        expected.insert(
            "owned_by".to_string(),
            Value::String(render["owned_by"].as_str().expect("owned_by").to_string()),
        );
        expected.insert("access".to_string(), Value::Number(0.into()));
        expected.insert("is_locked".to_string(), Value::Bool(false));
        expected.insert("archived_at".to_string(), Value::Null);
        expected.insert(
            "created_at".to_string(),
            Value::String(
                render["created_at"]
                    .as_str()
                    .expect("created_at")
                    .to_string(),
            ),
        );
        expected.insert(
            "updated_at".to_string(),
            Value::String(
                render["updated_at"]
                    .as_str()
                    .expect("updated_at")
                    .to_string(),
            ),
        );
        assert_eq!(rendered, expected);
        let keys: Vec<&str> = rendered.keys().map(String::as_str).collect();
        let expected_keys = str_list(&parsed["units"]["PageLiteSerializer"]["render_keys"]);
        assert_eq!(keys, expected_keys);
    }

    #[test]
    fn detail_keys_and_markdown_match_f18_04() {
        let parsed = golden();
        let detail = &parsed["units"]["PageDetailSerializer"];
        let expected_keys = str_list(&detail["render_keys"]);
        assert_eq!(PAGE_DETAIL_FIELDS, expected_keys.as_slice());
        let html = detail["description_html"]
            .as_str()
            .expect("golden carries description_html");
        let stripped = detail["description_stripped"]
            .as_str()
            .expect("golden carries description_stripped");
        let markdown = detail["description_markdown"]
            .as_str()
            .expect("golden carries description_markdown");
        let mut row = fixture_row(&parsed);
        row.description_html = Some(html);
        row.description_stripped = Some(stripped);
        row.description_markdown = markdown;
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &[],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_detail(&input).expect("plain render succeeds");
        assert_eq!(
            rendered.get("description_html"),
            Some(&Value::String(html.to_string()))
        );
        assert_eq!(
            rendered.get("description_stripped"),
            Some(&Value::String(stripped.to_string()))
        );
        assert_eq!(
            rendered.get("description_markdown"),
            Some(&Value::String(markdown.to_string()))
        );
        let keys: Vec<&str> = rendered.keys().map(String::as_str).collect();
        assert_eq!(keys, expected_keys);
    }

    // --- F18-04 replay: write shapes ------------------------------------

    #[test]
    fn write_vectors_match_f18_04() {
        let parsed = golden();
        let unit = &parsed["units"]["PageWriteSerializer"];
        let validated = validate_ok(
            json!({"name": "n", "description_markdown": "# hi"}),
            WriteMode::Base,
        );
        assert_eq!(
            validated_to_golden(&validated),
            unit["markdown_only"]["validated"]
        );
        assert!(validated.has_body());
        let validated = validate_ok(json!({"description_html": "<p>hi</p>"}), WriteMode::Base);
        assert_eq!(
            validated_to_golden(&validated),
            unit["html_only"]["validated"]
        );
        assert!(validated.has_body());
        let errors = validate_err(
            json!({"description_markdown": "a", "description_html": "b"}),
            WriteMode::Base,
        );
        assert_eq!(errors_to_golden(&errors), unit["both_bodies"]["errors"]);
        let errors = validate_err(json!({"name": ""}), WriteMode::Base);
        assert_eq!(errors_to_golden(&errors), unit["blank_name"]["errors"]);
        let errors = validate_err(json!({"access": "99"}), WriteMode::Base);
        assert_eq!(errors_to_golden(&errors), unit["bad_access"]["errors"]);
        // `access_values: [true, true]` — both choices validate.
        for access in [0, 1] {
            let validated = validate_ok(json!({"access": access}), WriteMode::Base);
            assert_eq!(validated.access, Some(access));
        }
        assert!(validate_ok(json!({"description_markdown": "x"}), WriteMode::Base).has_body());
        assert!(validate_ok(json!({"description_html": "x"}), WriteMode::Base).has_body());
        assert!(!validate_ok(json!({"name": "n"}), WriteMode::Base).has_body());
        assert_eq!(unit["has_body_markdown"], Value::Bool(true));
        assert_eq!(unit["has_body_true"], Value::Bool(true));
        assert_eq!(unit["has_body_false"], Value::Bool(false));
    }

    #[test]
    fn create_vectors_match_f18_04() {
        let parsed = golden();
        let unit = &parsed["units"]["PageCreateSerializer"];
        let validated = validate_ok(json!({"name": "New page"}), WriteMode::Create);
        assert_eq!(validated_to_golden(&validated), unit["ok"]["validated"]);
        let errors = validate_err(json!({}), WriteMode::Create);
        assert_eq!(errors_to_golden(&errors), unit["missing_name"]["errors"]);
        let errors = validate_err(json!({"name": ""}), WriteMode::Create);
        assert_eq!(errors_to_golden(&errors), unit["blank_name"]["errors"]);
    }

    #[test]
    fn update_vectors_match_f18_04() {
        let parsed = golden();
        let unit = &parsed["units"]["PageUpdateSerializer"];
        let validated = validate_ok(json!({"access": 0}), WriteMode::Update);
        assert_eq!(
            validated_to_golden(&validated),
            unit["ok_subset"]["validated"]
        );
        let errors = validate_err(json!({}), WriteMode::Update);
        assert_eq!(errors_to_golden(&errors), unit["empty"]["errors"]);
        let errors = validate_err(
            json!({"description_markdown": "a", "description_html": "b"}),
            WriteMode::Update,
        );
        assert_eq!(errors_to_golden(&errors), unit["both_bodies"]["errors"]);
    }

    // --- Probe tables (venv Django/DRF 3.15.2, no DB) --------------------

    const PROBE_UUID: &str = "255f3646-9ace-4f46-9620-788071f7a05e";
    const PROBE_OWNER: &str = "79c81d76-5a93-4d3d-894d-5935576834b6";

    fn probe_row() -> PageRow<'static> {
        PageRow {
            id: PROBE_UUID,
            name: "Fixture page",
            parent: None,
            owned_by: Some(PROBE_OWNER),
            access: 0,
            is_locked: false,
            archived_at: None,
            created_at: "2026-10-02T23:17:12.040770Z",
            updated_at: "2026-10-02T23:17:12.040770Z",
            description_html: Some(
                "<h1>Title</h1><p>para with <strong>bold</strong></p><ul><li>a</li><li>b</li></ul>",
            ),
            description_stripped: Some("Titlepara with boldab"),
            description_markdown: "# Title\n\npara with **bold**\n\n- a\n- b",
        }
    }

    fn probe_owner() -> UserLiteRow<'static> {
        UserLiteRow {
            id: PROBE_OWNER,
            first_name: "",
            last_name: "",
            email: Some("o@example.com"),
            avatar: "",
            avatar_url: None,
            display_name: "",
        }
    }

    #[test]
    fn multi_error_bodies_lead_with_name_in_every_mode() {
        // Probe: `{"name": "", "access": 99}` errors `["name", "access"]`
        // however the input is ordered, in all three modes (shared field
        // order — the `PageCreateSerializer.name` override keeps the base
        // slot).
        let expected = "{\"name\":[\"This field may not be blank.\"],\
             \"access\":[\"\\\"99\\\" is not a valid choice.\"]}";
        for mode in [WriteMode::Base, WriteMode::Create, WriteMode::Update] {
            let errors = validate_err(json!({"name": "", "access": 99}), mode);
            assert_eq!(errors.body(), expected);
            assert_eq!(
                errors
                    .field_errors
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
                vec!["name", "access"]
            );
        }
    }

    #[test]
    fn char_validators_match_probes() {
        // name: trim + blank + coercions.
        assert_eq!(
            validate_ok(json!({"name": "  x  "}), WriteMode::Update).name,
            Some("x".to_string())
        );
        assert_eq!(
            validate_ok(json!({"name": 123}), WriteMode::Update).name,
            Some("123".to_string())
        );
        assert_eq!(
            validate_ok(json!({"name": 1.5}), WriteMode::Update).name,
            Some("1.5".to_string())
        );
        for data in [
            json!({"name": "   "}),
            json!({"name": true}),
            json!({"name": null}),
            json!({"name": ["x"]}),
            json!({"name": {"x": 1}}),
        ] {
            assert!(validate_page_write(&data, WriteMode::Update).is_err());
        }
        assert_eq!(
            validate_err(json!({"name": "   "}), WriteMode::Update).body(),
            "{\"name\":[\"This field may not be blank.\"]}"
        );
        assert_eq!(
            validate_err(json!({"name": true}), WriteMode::Update).body(),
            "{\"name\":[\"Not a valid string.\"]}"
        );
        assert_eq!(
            validate_err(json!({"name": null}), WriteMode::Update).body(),
            "{\"name\":[\"This field may not be null.\"]}"
        );
        assert_eq!(
            validate_err(json!({"name": "a\x00b"}), WriteMode::Update).body(),
            "{\"name\":[\"Null characters are not allowed.\"]}"
        );
        // Bodies: no trim, blank allowed, same type rules.
        let validated = validate_ok(json!({"description_markdown": "  x  "}), WriteMode::Update);
        assert_eq!(validated.description_markdown, Some("  x  ".to_string()));
        assert!(validated.has_body());
        let validated = validate_ok(json!({"description_markdown": ""}), WriteMode::Update);
        assert_eq!(validated.description_markdown, Some(String::new()));
        assert!(validated.has_body());
        assert_eq!(
            validate_ok(json!({"description_markdown": 5}), WriteMode::Update).description_markdown,
            Some("5".to_string())
        );
        assert_eq!(
            validate_err(json!({"description_markdown": null}), WriteMode::Update).body(),
            "{\"description_markdown\":[\"This field may not be null.\"]}"
        );
        assert_eq!(
            validate_err(json!({"description_html": ["x"]}), WriteMode::Update).body(),
            "{\"description_html\":[\"Not a valid string.\"]}"
        );
    }

    #[test]
    fn parent_uuid_grammar_matches_probes() {
        // Every accepted spelling canonicalizes; the rest 400.
        let canonical = PROBE_UUID.to_string();
        let spellings = vec![
            PROBE_UUID.to_string(),
            PROBE_UUID.to_uppercase(),
            PROBE_UUID.replace('-', ""),
            format!("{{{PROBE_UUID}}}"),
            format!("urn:uuid:{PROBE_UUID}"),
            format!("uuid:{PROBE_UUID}"),
            format!("urn:uuid:urn:uuid:{PROBE_UUID}"),
        ];
        for text in &spellings {
            let validated = validate_ok(json!({"parent": text}), WriteMode::Update);
            assert_eq!(validated.parent, Some(Some(canonical.clone())), "{text}");
        }
        let rejects = vec![
            "zzz".to_string(),
            "URN:UUID:".to_string() + PROBE_UUID,
            "  ".to_string() + PROBE_UUID + "  ",
            PROBE_UUID[..8].to_string() + "{fill}" + &PROBE_UUID[8..],
        ];
        for text in &rejects {
            let errors = validate_err(json!({"parent": text}), WriteMode::Update);
            assert_eq!(
                errors.body(),
                "{\"parent\":[\"Must be a valid UUID.\"]}",
                "{text}"
            );
        }
        // Grammar internals (probe-verified against CPython `uuid.py`):
        // `0x` prefix, single interior underscores, and a `+` sign pass
        // the 32-length gate; doubles, misplaced underscores, interior
        // whitespace, and `-` (eaten by hyphen removal, failing the
        // length gate) do not.
        let hex32 = PROBE_UUID.replace('-', "");
        for text in [
            format!("0x{}", &hex32[..30]),
            format!("0X{}", &hex32[..30]),
            format!("0x_{}", &hex32[..29]),
            format!("+{}", &hex32[..31]),
            format!(" {}", &hex32[..31]),
            format!("{} ", &hex32[..31]),
            format!("\u{a0}{}", &hex32[..31]),
            format!("{}_5e", &hex32[..29]),
        ] {
            assert_eq!(text.chars().count(), 32, "{text:?}");
            assert!(
                validate_page_write(&json!({"parent": text}), WriteMode::Update).is_ok(),
                "{text:?} validates"
            );
        }
        for text in [
            format!("0x__{}", &hex32[..28]),
            format!("+_{}", &hex32[..30]),
            format!("{}__5", &hex32[..29]),
            format!("_{}", &hex32[..31]),
            format!("{}_", &hex32[..31]),
            format!("-{}", "0".repeat(31)),
            format!("{} {}", &hex32[..16], &hex32[16..31]),
        ] {
            assert_eq!(text.chars().count(), 32, "{text:?}");
            let errors = validate_err(json!({"parent": text}), WriteMode::Update);
            assert_eq!(
                errors.body(),
                "{\"parent\":[\"Must be a valid UUID.\"]}",
                "{text:?}"
            );
        }
        // Non-string inputs: floats/lists/dicts fail.
        for data in [
            json!({"parent": 1.5}),
            json!({"parent": ["x"]}),
            json!({"parent": {}}),
        ] {
            assert_eq!(
                validate_err(data, WriteMode::Update).body(),
                "{\"parent\":[\"Must be a valid UUID.\"]}"
            );
        }
    }

    #[test]
    fn parent_int_bool_null_match_probes() {
        // `uuid.UUID(int=…)` incl. the bool-is-int quirk; explicit null
        // validates to `None` (`allow_null`); absent stays absent.
        let validated = validate_ok(json!({"parent": 5}), WriteMode::Update);
        assert_eq!(
            validated.parent,
            Some(Some("00000000-0000-0000-0000-000000000005".to_string()))
        );
        let validated = validate_ok(json!({"parent": true}), WriteMode::Update);
        assert_eq!(
            validated.parent,
            Some(Some("00000000-0000-0000-0000-000000000001".to_string()))
        );
        let validated = validate_ok(json!({"parent": false}), WriteMode::Update);
        assert_eq!(
            validated.parent,
            Some(Some("00000000-0000-0000-0000-000000000000".to_string()))
        );
        let validated = validate_ok(json!({"parent": null}), WriteMode::Update);
        assert_eq!(validated.parent, Some(None));
        assert!(!validated.is_empty());
        let validated = validate_ok(json!({"name": "n"}), WriteMode::Update);
        assert_eq!(validated.parent, None);
        assert_eq!(
            validate_err(json!({"parent": -1}), WriteMode::Update).body(),
            "{\"parent\":[\"Must be a valid UUID.\"]}"
        );
    }

    #[test]
    fn access_choice_table_matches_probes() {
        for (data, expected) in [
            (json!({"access": 0}), Some(0)),
            (json!({"access": 1}), Some(1)),
            (json!({"access": "0"}), Some(0)),
            (json!({"access": "1"}), Some(1)),
        ] {
            assert_eq!(
                validate_ok(data, WriteMode::Update).access,
                expected.map(i64::from)
            );
        }
        for (data, message) in [
            (json!({"access": 99}), "\"99\" is not a valid choice."),
            (json!({"access": "99"}), "\"99\" is not a valid choice."),
            (json!({"access": true}), "\"True\" is not a valid choice."),
            (json!({"access": ""}), "\"\" is not a valid choice."),
            (json!({"access": 0.0}), "\"0.0\" is not a valid choice."),
            (json!({"access": 1.5}), "\"1.5\" is not a valid choice."),
            (json!({"access": [0]}), "\"[0]\" is not a valid choice."),
            (
                json!({"access": {"a": 1}}),
                "\"{'a': 1}\" is not a valid choice.",
            ),
        ] {
            let errors = validate_err(data, WriteMode::Update);
            let quoted = message
                .strip_suffix(" is not a valid choice.")
                .expect("table message shape");
            let inner = quoted
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
                .expect("table message quotes");
            assert_eq!(
                errors.field_errors,
                vec![(
                    "access".to_string(),
                    vec![FieldFailure::InvalidChoice(inner.to_string())]
                )]
            );
            let body = errors.body();
            assert_eq!(
                body,
                format!(
                    "{{\"access\":[{}]}}",
                    serde_json::to_string(message).unwrap()
                )
            );
        }
        assert_eq!(
            validate_err(json!({"access": null}), WriteMode::Update).body(),
            "{\"access\":[\"This field may not be null.\"]}"
        );
    }

    #[test]
    fn unknown_keys_ignored_and_validate_gated() {
        // Unknown keys never validate and never error; an update of only
        // unknowns is still an empty update; field errors suppress
        // `validate()` (both bodies + blank name = name error only).
        let validated = validate_ok(json!({"nope": 1}), WriteMode::Base);
        assert!(validated.is_empty());
        assert!(!validated.has_body());
        let errors = validate_err(json!({"nope": 1}), WriteMode::Update);
        assert_eq!(errors.non_field_errors, vec![FieldFailure::EmptyUpdate]);
        let errors = validate_err(
            json!({"name": "", "description_markdown": "a", "description_html": "b"}),
            WriteMode::Update,
        );
        assert_eq!(errors.non_field_errors, vec![]);
        assert_eq!(
            errors.field_errors,
            vec![("name".to_string(), vec![FieldFailure::Blank])]
        );
        // Create without a name fails on `name` even when a body validates.
        let errors = validate_err(
            json!({"description_markdown": "a", "description_html": "b"}),
            WriteMode::Create,
        );
        assert_eq!(
            errors.field_errors,
            vec![("name".to_string(), vec![FieldFailure::Required])]
        );
    }

    #[test]
    fn non_dict_bodies_match_probes() {
        for (data, message, code) in [
            (
                json!([]),
                "Invalid data. Expected a dictionary, but got list.",
                "invalid",
            ),
            (
                json!("x"),
                "Invalid data. Expected a dictionary, but got str.",
                "invalid",
            ),
            (
                json!(5),
                "Invalid data. Expected a dictionary, but got int.",
                "invalid",
            ),
            (
                json!(5.5),
                "Invalid data. Expected a dictionary, but got float.",
                "invalid",
            ),
            (
                json!(true),
                "Invalid data. Expected a dictionary, but got bool.",
                "invalid",
            ),
            (json!(null), "No data provided", "null"),
        ] {
            for mode in [WriteMode::Base, WriteMode::Create, WriteMode::Update] {
                let errors = validate_err(data.clone(), mode);
                assert!(errors.field_errors.is_empty());
                assert_eq!(errors.non_field_errors.len(), 1);
                assert_eq!(errors.non_field_errors[0].message(), message);
                assert_eq!(errors.non_field_errors[0].code(), code);
                assert_eq!(
                    errors.body(),
                    format!(
                        "{{\"non_field_errors\":[{}]}}",
                        serde_json::to_string(message).unwrap()
                    )
                );
            }
        }
    }

    #[test]
    fn expand_behaviors_match_probes() {
        let row = probe_row();
        let owner = probe_owner();
        // `expand=owned_by` inlines the D-19 UserLite, key order kept.
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &["owned_by"],
            owner: Some(&owner),
            expansions: &[],
        };
        let rendered = render_page_lite(&input).expect("owned_by expands");
        assert_eq!(
            rendered.get("owned_by"),
            Some(&json!({
                "id": PROBE_OWNER,
                "first_name": "",
                "last_name": "",
                "email": "o@example.com",
                "avatar": "",
                "avatar_url": null,
                "display_name": "",
            }))
        );
        let keys: Vec<&str> = rendered.keys().map(String::as_str).collect();
        assert_eq!(keys, PAGE_LITE_FIELDS);
        // `expand=parent` over a null parent renders `{}`.
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &["parent"],
            owner: None,
            expansions: &[("parent", None)],
        };
        let rendered = render_page_lite(&input).expect("null parent expands");
        assert_eq!(rendered.get("parent"), Some(&json!({})));
        // ... over a set parent renders the caller value (IssueLite over a
        // Page keeps only `id` — the caller, PIDASHCONV-661's shape, owns
        // that render).
        let mut child = probe_row();
        child.parent = Some(PROBE_UUID);
        child.name = "x";
        let input = PageReadInput {
            row: &child,
            fields: None,
            expand: &["parent"],
            owner: None,
            expansions: &[("parent", Some(json!({"id": PROBE_UUID})))],
        };
        let rendered = render_page_lite(&input).expect("set parent expands");
        assert_eq!(rendered.get("parent"), Some(&json!({"id": PROBE_UUID})));
        // Missing caller value is a contract error, not `{}`.
        let input = PageReadInput {
            row: &child,
            fields: None,
            expand: &["parent"],
            owner: None,
            expansions: &[],
        };
        assert_eq!(
            render_page_lite(&input),
            Err(RenderError::MissingExpansion("parent".to_string()))
        );
        // `expand=<scalar>` nulls the scalar; unknown names are ignored.
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &["name"],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_lite(&input).expect("scalar expand nulls");
        assert_eq!(rendered.get("name"), Some(&Value::Null));
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &["url"],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_lite(&input).expect("unknown expand ignored");
        assert_eq!(rendered.len(), PAGE_LITE_FIELDS.len());
        // Expansion applies to kept fields only (`fields=[name]` drops
        // `owned_by` before the pass runs).
        let specs = vec![FieldSpec::Include("name".to_string())];
        let input = PageReadInput {
            row: &row,
            fields: Some(&specs),
            expand: &["owned_by"],
            owner: Some(&owner),
            expansions: &[],
        };
        let rendered = render_page_lite(&input).expect("filtered expand");
        assert_eq!(
            rendered,
            Map::from_iter([(
                "name".to_string(),
                Value::String("Fixture page".to_string())
            )])
        );
        // `expand=owned_by` with no owner row is the Django 500 parity arm.
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &["owned_by"],
            owner: None,
            expansions: &[],
        };
        assert_eq!(render_page_lite(&input), Err(RenderError::MissingOwner));
    }

    #[test]
    fn fields_filtering_matches_probes() {
        let row = probe_row();
        // Reversed request order still yields wire order (pop-in-place).
        let specs = vec![
            FieldSpec::Include("name".to_string()),
            FieldSpec::Include("id".to_string()),
        ];
        let input = PageReadInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_lite(&input).expect("subset filters");
        let keys: Vec<&str> = rendered.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["id", "name"]);
        // All-unknown keeps nothing (never raises).
        let specs = vec![FieldSpec::Include("nope".to_string())];
        let input = PageReadInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_lite(&input).expect("unknown filters");
        assert!(rendered.is_empty());
        // A nested entry raises before allowance (shared kernel).
        let specs = vec![FieldSpec::Nested("id".to_string(), vec![])];
        let input = PageReadInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            owner: None,
            expansions: &[],
        };
        assert!(matches!(
            render_page_lite(&input),
            Err(RenderError::Fields(_))
        ));
        // The detail shape filters over all twelve fields.
        let specs = vec![FieldSpec::Include("description_markdown".to_string())];
        let input = PageReadInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_detail(&input).expect("detail filters");
        assert_eq!(
            rendered,
            Map::from_iter([(
                "description_markdown".to_string(),
                Value::String("# Title\n\npara with **bold**\n\n- a\n- b".to_string())
            )])
        );
    }

    #[test]
    fn detail_null_and_empty_html_match_probes() {
        // `description_html: null` renders null and still derives `""`
        // (`html_to_markdown(None)`); `""` renders `""` with `""`.
        let mut row = probe_row();
        row.description_html = None;
        row.description_stripped = None;
        row.description_markdown = "";
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &[],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_detail(&input).expect("null html renders");
        assert_eq!(rendered.get("description_html"), Some(&Value::Null));
        assert_eq!(rendered.get("description_stripped"), Some(&Value::Null));
        assert_eq!(
            rendered.get("description_markdown"),
            Some(&Value::String(String::new()))
        );
        row.description_html = Some("");
        let input = PageReadInput {
            row: &row,
            fields: None,
            expand: &[],
            owner: None,
            expansions: &[],
        };
        let rendered = render_page_detail(&input).expect("empty html renders");
        assert_eq!(
            rendered.get("description_html"),
            Some(&Value::String(String::new()))
        );
        assert_eq!(
            rendered.get("description_markdown"),
            Some(&Value::String(String::new()))
        );
    }

    #[test]
    fn serde_json_rejects_lone_surrogates_at_parse() {
        // Seam assertion for `check_no_null_chars`: Python's `json`
        // accepts lone surrogates (DRF then 400s via
        // `ProhibitSurrogateCharactersValidator`), but this module's
        // inputs arrive through `serde_json`, which refuses to parse
        // them — so the surrogate arm is unreachable past the boundary.
        assert!(serde_json::from_str::<Value>(r#""a�b""#).is_ok());
        assert!(serde_json::from_str::<Value>(r#""a\ud800b""#).is_err());
        assert!(serde_json::from_str::<Value>(r#""\udc00""#).is_err());
    }

    // --- Python kernel tables -------------------------------------------

    #[test]
    fn strip_table_matches_cpython_full_range() {
        // The 29 codepoints from `[hex(ord(c)) for c in map(chr,
        // range(0x110000)) if c.isspace()]`: `is_python_space` agrees on
        // every char, and `python_strip` trims exactly that set.
        let expected = [
            0x9u32, 0xa, 0xb, 0xc, 0xd, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x85, 0xa0, 0x1680, 0x2000,
            0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028,
            0x2029, 0x202f, 0x205f, 0x3000,
        ];
        assert_eq!(expected.len(), 29);
        for code in 0u32..0x110000 {
            let Some(ch) = char::from_u32(code) else {
                continue;
            };
            assert_eq!(
                is_python_space(ch),
                expected.contains(&code),
                "U+{code:04X}"
            );
        }
        assert_eq!(python_strip("\u{1c}\u{85} x \u{a0}\u{3000}"), "x");
        assert_eq!(python_strip(""), "");
        assert_eq!(python_strip("x"), "x");
    }

    #[test]
    fn float_repr_table_matches_cpython() {
        // `repr(f)` for each value, generated by CPython.
        let table: &[(f64, &str)] = &[
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.5, "1.5"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (1e16, "1e+16"),
            (1e21, "1e+21"),
            (1.5e-5, "1.5e-05"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (123456.789, "123456.789"),
            (2.5, "2.5"),
            (100.0, "100.0"),
            (1e300, "1e+300"),
            (1e-300, "1e-300"),
            (std::f64::consts::PI, "3.141592653589793"),
            (0.30000000000000004, "0.30000000000000004"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (123456789012345680.0, "1.2345678901234568e+17"),
            (1.0 / 3.0, "0.3333333333333333"),
            (10.0f64.powi(-7), "1e-07"),
            (123.456e10, "1234560000000.0"),
        ];
        for (value, expected) in table {
            assert_eq!(&python_float_repr(*value), expected, "{value:?}");
        }
        assert_eq!(python_float_repr(f64::INFINITY), "inf");
        assert_eq!(python_float_repr(f64::NEG_INFINITY), "-inf");
        assert_eq!(python_float_repr(f64::NAN), "nan");
    }

    #[test]
    fn python_str_table_matches_cpython() {
        // `str(v)` for each JSON value, generated by CPython.
        let table: &[(Value, &str)] = &[
            (json!(null), "None"),
            (json!(true), "True"),
            (json!(false), "False"),
            (json!(0), "0"),
            (json!(-12), "-12"),
            (json!(99), "99"),
            (json!("99"), "99"),
            (json!(""), ""),
            (json!(0.0), "0.0"),
            (json!(1.5), "1.5"),
            (json!(1e16), "1e+16"),
            (
                json!([1, "a", true, null, 1.5]),
                "[1, 'a', True, None, 1.5]",
            ),
            (json!({"a": 1}), "{'a': 1}"),
            (json!({"a": "b"}), "{'a': 'b'}"),
            (json!([]), "[]"),
            (json!({}), "{}"),
            (json!([["x"]]), "[['x']]"),
            (json!({"n": [1.5, null]}), "{'n': [1.5, None]}"),
            (
                json!(["a\nb", "q\\q", "it's", "say \"hi\""]),
                "['a\\nb', 'q\\\\q', \"it's\", 'say \"hi\"']",
            ),
            (json!({"k\"k": "v"}), "{'k\"k': 'v'}"),
            (json!(["\x01\x7f\u{85}é"]), "['\\x01\\x7f\\x85é']"),
        ];
        for (value, expected) in table {
            assert_eq!(
                &python_str(value),
                expected,
                "{}",
                serde_json::to_string(value).unwrap()
            );
        }
    }
}
