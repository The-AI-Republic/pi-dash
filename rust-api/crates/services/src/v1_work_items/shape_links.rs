#![forbid(unsafe_code)]

//! Link shapes (D-18 serializers C, PIDASHCONV-662).
//!
//! Ports `apps/api/pi_dash/api/serializers/issue.py:580-728`:
//! `IssueLinkCreateSerializer` (`:580-622`, incl. `validate_url` `:602-616`
//! and the `create` duplicate guard `:617-622`),
//! `IssueLinkUpdateSerializer` (`:623-648`, incl. `update` `:638-648`),
//! `IssueLinkSerializer` (`:649-671`),
//! `GithubPullRequestLinkSerializer` (`:672-699`) and
//! `GitCodeReviewLinkSerializer` (`:700-728`).
//!
//! Fixture: the F18-02 link subset (`rust-api/fixtures/v1_work_items/`
//! `serializers/F18-02.label_link_relation.golden.json`). Every `#[test]`
//! below replays it: golden in/out byte-identical, including the field
//! orders and the URL-validation error strings.
//!
//! Reused, not forked: [`filter_fields`]/[`FieldSpec`]/[`FilterError`]
//! (the shared `?fields=` kernel) and [`field_errors_body`] +
//! [`BASE_EXPANSION_NAMES`] from the sibling `shape_issue` module. The
//! `is_python_space`/`python_strip`/`json_type_name`/`py_float_repr`
//! helpers are per-module copies of the `shape_pages`/`shape_relations`
//! versions, which is the codebase precedent for private helpers.
//!
//! Write-path reachability (all verified against `api/views/issue.py`,
//! `api/views/github_pr.py`, `api/views/git_code_review.py` plus live
//! probes against the pinned DRF 3.15.2 / Django 4.2.30 sources):
//!
//! * `IssueLinkCreateSerializer` — link POST (`views/issue.py:1632`):
//!   [`validate_link_create`]. No `partial`, so a missing `url` always
//!   fails. Invalid → `Response(serializer.errors, 400)`; valid →
//!   `save(project_id=…, issue_id=…)` (the save kwargs are the SOLE source
//!   of the row's `issue_id` — input `issue_id` never reaches
//!   `validated_data`, see below), the `create` duplicate guard, the crawl
//!   + activity tasks, then the Show render with 201.
//! * `IssueLinkUpdateSerializer` — no reachable view path: PATCH
//!   (`views/issue.py:1746`) constructs the full `IssueLinkSerializer`
//!   with `partial=True` instead (so PATCH has NO `validate_url` and NO
//!   duplicate guard — handler issue PIDASHCONV-674 owns that quirk), and
//!   the Update serializer otherwise appears only as OpenAPI docs
//!   (`views/issue.py:1724`). [`validate_link_update`] is ported for
//!   direct-call parity and fixture replay.
//! * `IssueLinkSerializer` — list/detail GETs (with `fields=`/`expand=`,
//!   `views/issue.py:1602,1704-1713`), the POST 201 render (`:1648`) and
//!   the PATCH read/write/verify cycle (`:1745-1760`).
//!   [`render_link`] ports its read shape with full `fields=`/`expand=`
//!   parity; the PATCH partial-write rules are documented below for 674.
//! * `GithubPullRequestLinkSerializer` / `GitCodeReviewLinkSerializer` —
//!   read-only everywhere (every `Meta.fields` entry is also in
//!   `read_only_fields`): list GETs (`many=True`, no `fields=`/`expand=`
//!   kwargs) and the attach POST responses (`github_pr.py:55,74`,
//!   `git_code_review.py:50,74`). [`render_pr_link`] and
//!   [`render_review_link`] take `fields=`/`expand=` for full
//!   `BaseSerializer` parity although the views pass none.
//!
//! Field rules (probed live; model: `db/models/issue.py:471-475`):
//!
//! * `title` (`CharField(max_length=255, null=True, blank=True)`):
//!   `required=False`, `allow_null`, `allow_blank`, `max_length=255`,
//!   `trim_whitespace`. Validators in order: `MaxLengthValidator`,
//!   `ProhibitNullCharactersValidator`, `ProhibitSurrogateCharactersValidator`
//!   (vacuous here — see below). ALL validator failures are collected,
//!   not just the first (probed: a 256-char title with a null char reports
//!   both messages).
//! * `url` (`TextField()`): `required=True`, no null/blank, no max length,
//!   `trim_whitespace`. Validators: the two prohibit validators, then
//!   `validate_url` (skipped when field validation already failed).
//! * `issue_id`: not a model field name (the FK attname), so DRF builds a
//!   `ReadOnlyField` for it (probed type name) — every input value, of any
//!   JSON type, is silently ignored and the key never validates.
//! * CharField order per field: blank pre-check (`data == ''` or the
//!   stripped Python-`str()` of the input is empty — the latter is
//!   unreachable for JSON scalars, whose reprs are never blank) → null →
//!   coerce (strings pass; ints/floats render Python `str()` — floats via
//!   [`py_float_repr`], so `1e100` validates as `'1e+100'`; bools, lists
//!   and dicts fail `invalid`) → strip → validators → `validate_url`.
//! * The surrogate validator is vacuous: Rust `str` cannot hold
//!   surrogates and `serde_json` rejects lone `\uD800`-`\uDFFF` escapes at
//!   parse, so validated text is always surrogate-free (same rationale as
//!   `shape_pages`; Python's `json` accepts lone surrogates and 400s on
//!   them, but that input cannot exist at this module's boundary).
//! * Combined errors follow declared field order (`title`, then `url`);
//!   unknown input keys are silently ignored.
//!
//! `validate_url` (`issue.py:602-614`) runs Django's `URLValidator` and
//! then requires a case-sensitive `http(s)://` prefix. The port
//! ([`validate_link_url`]) mirrors `URLValidator.__call__`
//! (Django 4.2 `validators.py`) step by step — length, unsafe chars,
//! scheme pre-check, `urlsplit` failure modes, the host regex (hand-parsed:
//! its `(?<!-)`/`(?!-)` lookarounds have no `regex`-crate equivalent),
//! the IDN retry, the IPv6 re-verify and the 253-char hostname cap — and
//! every branch below is pinned by a live probe (see the tests).
//!
//! Duplicate guards (handler contract for PIDASHCONV-674; the SQL lives
//! with the endpoint wiring — `queries_sub` ports the list/detail
//! querysets but no `EXISTS` helper):
//!
//! * create: `IssueLink.objects.filter(url=…, issue_id=…)` where `issue_id`
//!   is the view's save kwarg (path id), never the input — on a hit the
//!   `create()` raise renders 400 [`duplicate_url_body`].
//! * update: same filter with the INSTANCE's `issue_id`, excluding the
//!   instance pk — same body.
//!
//! PATCH partial-write rules for 674 (`IssueLinkSerializer` with
//! `partial=True`, `views/issue.py:1746-1748`): writable fields are
//! `deleted_at` (`DateTimeField`, `required=False`, `allow_null` —
//! probed live on the Show serializer; it is NOT in
//! `Meta.read_only_fields`), `title`, `url` and `metadata` (every other
//! `__all__` field is in `Meta.read_only_fields`); missing keys are
//! skipped (`SkipField`);
//! present `title`/`url` follow the create field rules above EXCEPT there
//! is no `validate_url` call and no duplicate guard, so PATCH accepts
//! `ftp://…` URLs and duplicate `(url, issue_id)` pairs that POST rejects.
//! `metadata` is a default-DRF `JSONField` (`required=False` via the
//! `default=dict`, `allow_null=False`).
//!
//! Ported quirks (translate, don't redesign — all verified live):
//!
//! * `IssueLinkUpdateSerializer.Meta.fields` lists `issue_id` twice
//!   (`issue.py:633-635` appends it to the create list); DRF keys fields
//!   by name so it collapses — ported as [`LINK_UPDATE_FIELDS`], equal to
//!   [`LINK_CREATE_FIELDS`].
//! * The scheme check is a case-sensitive `startswith`: `HTTP://…`
//!   passes `URLValidator` (which lowercases first) but fails here with
//!   "Invalid URL scheme."
//! * The IDN retry skips only the IPv6 re-verify (Django's `except`
//!   branch); the 253-char hostname cap below it in `validators.py` still
//!   applies to the ORIGINAL hostname — a 253-char astral host validates,
//!   254 fails (both probed).
//! * A scheme that passes the pre-check but is not `scheme_chars`-clean
//!   (only reachable via non-ASCII folds, e.g. `HTTPſ://…`) fails with
//!   "Invalid URL format.": Python's scheme check lowercases `ſ` to
//!   itself, so it never passes — and any scheme that DOES pass is pure
//!   ASCII, which makes the `urlsplit` scheme parse infallible here.
//! * `float` titles stringify with Python `str()` (`1e100` → `'1e+100'`),
//!   not `serde_json` rendering (`1e100`).
//!
//! Known edges (shared precedent, documented not fixed):
//!
//! * JSON integers above `u64::MAX` arrive as `f64` under `serde_json`'s
//!   default precision and stringify via [`py_float_repr`], while Python's
//!   unbounded `int` would render exactly. Only reachable with absurd
//!   titles; the handler parses the body, so no shape-local code can
//!   recover the digits.
//! * The IDN retry maps non-ASCII labels through UTS-46 (`idna` crate)
//!   where Python uses its IDNA-2003 codec: identical on every probe
//!   (including astral hosts and userinfo/port/mixed cases, all pinned),
//!   diverging only on theoretical ~60-char deviation-label mixes.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::net::Ipv6Addr;

use serde_json::{Map, Value};

use super::shape_issue::{field_errors_body, BASE_EXPANSION_NAMES};
use super::{filter_fields, FieldSpec, FilterError};

/// `IssueLinkCreateSerializer.Meta.fields` (`issue.py:588-590`): the
/// declared field order (combined field errors follow it).
pub const LINK_CREATE_FIELDS: &[&str] = &["title", "url", "issue_id"];

/// `IssueLinkUpdateSerializer.Meta.fields` (`issue.py:631-635`), collapsed:
/// the source appends `issue_id` to the create list (so it appears twice)
/// but DRF keys fields by name — probed live as
/// `['title', 'url', 'issue_id']`, matching the F18-02 "rendered once" note.
pub const LINK_UPDATE_FIELDS: &[&str] = &["title", "url", "issue_id"];

/// `IssueLinkSerializer` `fields="__all__"` order (`issue.py:657-659`):
/// pk first, then model order (probed live; pinned by F18-02 `render_keys`).
pub const LINK_SHOW_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "title",
    "url",
    "metadata",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
];

/// `GithubPullRequestLinkSerializer.Meta.fields` (`issue.py:681-696`).
pub const PR_LINK_FIELDS: &[&str] = &[
    "id",
    "issue",
    "repo_owner",
    "repo_name",
    "pr_number",
    "url",
    "title",
    "state",
    "merged",
    "draft",
    "pr_updated_at",
    "created_at",
    "updated_at",
    "created_by",
];

/// `GitCodeReviewLinkSerializer.Meta.fields` (`issue.py:705-725`).
pub const REVIEW_LINK_FIELDS: &[&str] = &[
    "id",
    "issue",
    "provider",
    "host_url",
    "namespace",
    "repo_name",
    "repo_external_id",
    "external_id",
    "external_iid",
    "url",
    "title",
    "state",
    "merged",
    "draft",
    "remote_updated_at",
    "metadata",
    "created_at",
    "updated_at",
    "created_by",
];

/// Missing input where `required=True` (DRF `required`).
pub const MSG_REQUIRED: &str = "This field is required.";
/// Explicit JSON null where `allow_null=False` (DRF `null`).
pub const MSG_NULL: &str = "This field may not be null.";
/// Empty/whitespace-only input where `allow_blank=False` (DRF `blank`).
pub const MSG_BLANK: &str = "This field may not be blank.";
/// Bool/list/dict input to a `CharField` (DRF `invalid`).
pub const MSG_INVALID_STRING: &str = "Not a valid string.";
/// `title` over 255 code points (`MaxLengthValidator`).
pub const MSG_TITLE_MAX_LENGTH: &str = "Ensure this field has no more than 255 characters.";
/// Null char in `title`/`url` (Django `ProhibitNullCharactersValidator`).
pub const MSG_NULL_CHARACTERS: &str = "Null characters are not allowed.";
/// `URLValidator` rejection (`validate_url`, `issue.py:608`).
pub const MSG_URL_FORMAT: &str = "Invalid URL format.";
/// Non-`http(s)://` prefix after `URLValidator` passed (`issue.py:612`).
pub const MSG_URL_SCHEME: &str = "Invalid URL scheme.";
/// Create/update duplicate guard (`issue.py:619,644`).
pub const MSG_DUPLICATE_URL: &str = "URL already exists for this Issue";
/// Null request body (DRF serializer-level `null`).
pub const MSG_NO_DATA: &str = "No data provided";
/// Non-object request body prefix (DRF serializer-level `invalid`).
pub const MSG_NOT_A_DICT_PREFIX: &str = "Invalid data. Expected a dictionary, but got ";

/// `IssueLink.title` (`CharField(max_length=255)`): code-point cap.
pub const TITLE_MAX_LENGTH: usize = 255;
/// `URLValidator.max_length`: code points of the stripped value.
pub const URL_MAX_LENGTH: usize = 2048;
/// `URLValidator` hostname cap (`validators.py`: 253 per RFC 1034 §3.1),
/// counted on the lowercased hostname (the İ→i̇ inflation is load-bearing).
pub const HOSTNAME_MAX_LENGTH: usize = 253;

/// Validated link write (`validated_data` shape, shared by the create and
/// update serializers — their field objects are identical).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedLinkWrite {
    /// `title`: `None` = key absent (DRF `SkipField`, never in
    /// `validated_data`); `Some(None)` = explicit JSON null (valid —
    /// `allow_null`, the row stores NULL); `Some(Some(_))` = stripped text
    /// (blank allowed, so `""` and `"   "` both validate as `""`).
    pub title: Option<Option<String>>,
    /// `url`: always present on success (required), stripped.
    pub url: String,
}

/// Every failure [`validate_link_create`]/[`validate_link_update`] can
/// produce. Both arms carry the byte-exact 400 body (the views answer
/// `Response(serializer.errors, 400)`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkWriteError {
    /// Null body (`No data provided`) or non-object body.
    #[error("{0}")]
    NotADict(String),
    /// Field errors in [`LINK_CREATE_FIELDS`] order.
    #[error("{0}")]
    Fields(String),
}

impl LinkWriteError {
    /// The byte-exact 400 response body.
    pub fn body(&self) -> &str {
        match self {
            LinkWriteError::NotADict(body) | LinkWriteError::Fields(body) => body,
        }
    }
}

/// Port of `IssueLinkCreateSerializer.is_valid()` field validation
/// (`serializers/issue.py:580-616` over DRF `ModelSerializer`).
///
/// The body must be an object (null → `No data provided`, anything else
/// non-object → `invalid` with the JSON type name). Both writable fields
/// run even after failures and errors combine in declared order; `issue_id`
/// input (any JSON type) and unknown keys are silently ignored. The POST
/// view (`views/issue.py:1632`) constructs without `partial`, so a missing
/// `url` always fails.
pub fn validate_link_create(body: &Value) -> Result<ValidatedLinkWrite, LinkWriteError> {
    validate_link_write(body)
}

/// Port of `IssueLinkUpdateSerializer.is_valid()` field validation
/// (`serializers/issue.py:623-636`): identical rules to
/// [`validate_link_create`] (the `Meta.fields` duplicate collapses and no
/// field option differs) — a separate entry point for direct-call parity.
/// No view constructs this serializer (PATCH uses the Show serializer).
pub fn validate_link_update(body: &Value) -> Result<ValidatedLinkWrite, LinkWriteError> {
    validate_link_write(body)
}

fn validate_link_write(body: &Value) -> Result<ValidatedLinkWrite, LinkWriteError> {
    if body.is_null() {
        return Err(LinkWriteError::NotADict(field_errors_body(&[(
            "non_field_errors",
            Value::Array(vec![Value::String(MSG_NO_DATA.to_string())]),
        )])));
    }
    let Some(obj) = body.as_object() else {
        return Err(LinkWriteError::NotADict(field_errors_body(&[(
            "non_field_errors",
            Value::Array(vec![Value::String(format!(
                "{}{}.",
                MSG_NOT_A_DICT_PREFIX,
                json_type_name(body)
            ))]),
        )])));
    };
    let mut errors: Vec<(&str, Value)> = Vec::new();

    let title = match validate_title(obj.get("title")) {
        Ok(title) => Some(title),
        Err(messages) => {
            errors.push(("title", messages_body(messages)));
            None
        }
    };
    let url = match validate_url_value(obj.get("url")) {
        Ok(url) => Some(url),
        Err(messages) => {
            errors.push(("url", messages_body(messages)));
            None
        }
    };

    if !errors.is_empty() {
        return Err(LinkWriteError::Fields(field_errors_body(&errors)));
    }
    Ok(ValidatedLinkWrite {
        title: title.expect("no errors means title parsed"),
        url: url.expect("no errors means url parsed"),
    })
}

fn messages_body(messages: Vec<String>) -> Value {
    Value::Array(messages.into_iter().map(Value::String).collect())
}

/// Validates `title` (`CharField`, `required=False`, `allow_null`,
/// `allow_blank`, `max_length=255`, `trim_whitespace` — derived from
/// `CharField(max_length=255, null=True, blank=True)` via DRF
/// `field_mapping.get_field_kwargs`, probed live).
///
/// Order: absent → skip; null → `None`; blank pre-check (empty or
/// whitespace-only → `""`); coerce; strip; `MaxLengthValidator` then
/// `ProhibitNullCharactersValidator` (all failures collected, probed).
fn validate_title(raw: Option<&Value>) -> Result<Option<Option<String>>, Vec<String>> {
    let Some(value) = raw else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(Some(None));
    }
    if let Value::String(text) = value {
        // `CharField.run_validation` blank pre-check
        // (`fields.py:749-757`): `data == '' or str(data).strip() == ''`.
        // Only strings can take this arm — every other JSON scalar has a
        // non-blank Python `str()` (`'True'`, `'0'`, `'[]'`, …).
        if python_strip(text).is_empty() {
            return Ok(Some(Some(String::new())));
        }
    }
    let coerced = match coerce_charfield(value) {
        Ok(text) => text,
        Err(()) => return Err(vec![MSG_INVALID_STRING.to_string()]),
    };
    let stripped = python_strip(&coerced);
    let mut failures = Vec::new();
    if stripped.chars().count() > TITLE_MAX_LENGTH {
        failures.push(MSG_TITLE_MAX_LENGTH.to_string());
    }
    if stripped.contains('\0') {
        failures.push(MSG_NULL_CHARACTERS.to_string());
    }
    // The surrogate validator is vacuous here (see module docs).
    if failures.is_empty() {
        Ok(Some(Some(stripped.to_string())))
    } else {
        Err(failures)
    }
}

/// Validates `url` (`CharField`, `required=True`, no null/blank, no max
/// length, `trim_whitespace` — from `TextField()`, probed live).
///
/// Same order as [`validate_title`], then `validate_url`
/// (`issue.py:602-614`) — which runs only when field validation passed
/// (probed: `'notaurl\x00'` reports only the null-char message).
fn validate_url_value(raw: Option<&Value>) -> Result<String, Vec<String>> {
    let Some(value) = raw else {
        return Err(vec![MSG_REQUIRED.to_string()]);
    };
    if value.is_null() {
        return Err(vec![MSG_NULL.to_string()]);
    }
    if let Value::String(text) = value {
        if python_strip(text).is_empty() {
            return Err(vec![MSG_BLANK.to_string()]);
        }
    }
    let coerced = match coerce_charfield(value) {
        Ok(text) => text,
        Err(()) => return Err(vec![MSG_INVALID_STRING.to_string()]),
    };
    let stripped = python_strip(&coerced);
    if stripped.contains('\0') {
        return Err(vec![MSG_NULL_CHARACTERS.to_string()]);
    }
    match validate_link_url(stripped) {
        Ok(()) => Ok(stripped.to_string()),
        Err(error) => Err(vec![error.message().to_string()]),
    }
}

/// DRF `CharField.to_internal_value` minus the strip (`fields.py:759-766`):
/// bools fail (checked before the numeric coercion), strings pass
/// through, integers render decimal, floats render Python `str()`
/// ([`py_float_repr`]); lists, dicts and (defensively) null fail.
fn coerce_charfield(value: &Value) -> Result<String, ()> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Ok(int.to_string())
            } else if let Some(uint) = number.as_u64() {
                Ok(uint.to_string())
            } else {
                Ok(py_float_repr(number.as_f64().expect("float number")))
            }
        }
        Value::Bool(_) | Value::Array(_) | Value::Object(_) | Value::Null => Err(()),
    }
}

/// DRF type name for the non-object-body message, mirroring Python's
/// `type(data).__name__` for JSON-decoded values (mirrors
/// `shape_relations::json_type_name`, a per-module copy by precedent).
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

/// Python `str()` of a float: shortest round-trip digits (taken from
/// serde's ryu rendering, which implements the same shortest spec as
/// CPython's `float_repr_style short`) laid out by Python's rules —
/// fixed notation for decimal exponents `-3..=16` with a mandatory `.0`
/// on integral values, else `d[.ddd]e±XX` with a signed, ≥2-digit
/// exponent. Mirrors `shape_relations::py_float_repr` (a per-module copy
/// by precedent); probed: `1e100` → `'1e+100'`, `100.0` → `'100.0'`,
/// `-0.0` → `'-0.0'`.
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

/// CPython `str.strip()` (no args) member set — and, verified by an
/// exhaustive scan of all 0x110000 code points, EXACTLY Python `re` `\s`
/// too (the `regex` path and strip paths share this predicate).
/// Mirrors `shape_pages::is_python_space` (a per-module copy by precedent).
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

/// CPython `str.strip()` (no args). Mirrors `shape_pages::python_strip`.
fn python_strip(text: &str) -> &str {
    text.trim_matches(is_python_space)
}

/// `IssueLinkCreateSerializer.validate_url` failures (`issue.py:602-614`),
/// in check order: Django `URLValidator` first, then the scheme prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkUrlError {
    /// `URLValidator` rejected the value (`issue.py:604-608`).
    Format,
    /// Passed `URLValidator` but is not `http(s)://` (`issue.py:611-612`).
    Scheme,
}

impl LinkUrlError {
    /// Byte-exact DRF message.
    pub fn message(self) -> &'static str {
        match self {
            LinkUrlError::Format => MSG_URL_FORMAT,
            LinkUrlError::Scheme => MSG_URL_SCHEME,
        }
    }
}

/// Port of `validate_url` (`issue.py:602-614`). `value` is the stripped
/// field value (DRF strips in `to_internal_value`, before validators run).
pub fn validate_link_url(value: &str) -> Result<(), LinkUrlError> {
    if !url_format_ok(value) {
        return Err(LinkUrlError::Format);
    }
    // Case-SENSITIVE prefix (`issue.py:611`): `HTTP://…` passes
    // `URLValidator` (which lowercases first) yet fails here — probed.
    if !value.starts_with("http://") && !value.starts_with("https://") {
        return Err(LinkUrlError::Scheme);
    }
    Ok(())
}

/// Django `URLValidator.__call__` equivalence (Django 4.2
/// `validators.py`), step by step: length → unsafe chars → scheme
/// pre-check → `urlsplit` failure modes → host regex (hand-parsed — its
/// `(?<!-)`/`(?!-)` lookarounds have no `regex`-crate equivalent) → IDN
/// retry → IPv6 re-verify → hostname cap.
fn url_format_ok(value: &str) -> bool {
    // Length (code points) then unsafe chars.
    if value.chars().count() > URL_MAX_LENGTH {
        return false;
    }
    if value.contains(['\t', '\r', '\n']) {
        return false;
    }
    // Scheme pre-check: `value.split("://")[0].lower()` in
    // `["http", "https", "ftp", "ftps"]`.
    let Some(scheme_end) = value.find("://") else {
        return false;
    };
    let scheme = &value[..scheme_end];
    if !is_url_scheme(scheme) {
        return false;
    }
    // Authority = up to the first `/?#` (`_splitnetloc`).
    let after_scheme = &value[scheme_end + 3..];
    let auth_len = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..auth_len];
    let rest = &after_scheme[auth_len..];
    // `urlsplit` `ValueError` parity (netloc checks run before the regex).
    if bracketed_netloc_invalid(authority) {
        return false;
    }
    if authority.chars().any(is_nfkc_dangerous) {
        return false;
    }
    if authority_path_matches(authority, rest) {
        // Direct match → IPv6 re-verify (the `else:` branch) + the
        // unconditional hostname cap.
        if let Some(inner) = bracketed_verify_inner(authority) {
            // Load-bearing for userinfo + brackets (`[u]@[::1]`: the
            // direct match passes but Django's greedy `(.+)` re-verify
            // rejects — probed FORMAT); on plain `[inner]` inputs the
            // urlsplit stage above already strict-validated the same
            // string.
            if inner.parse::<Ipv6Addr>().is_err() {
                return false;
            }
        }
        let Some(hostname) = split_hostname(authority) else {
            // `splitted_url.hostname is None` — unreachable on a direct
            // match (the host grammar is never empty and `netloc` is never
            // empty here: any scheme passing the pre-check is pure ASCII,
            // so the `urlsplit` scheme parse cannot fail — see
            // [`is_url_scheme`]).
            return false;
        };
        return hostname.chars().count() <= HOSTNAME_MAX_LENGTH;
    }
    // IDN retry: `punycode(netloc)` then the regex. The `except` branch
    // skips only the IPv6 re-verify — the 253-char hostname cap sits BELOW
    // the try/except in `validators.py`, so it runs UNCONDITIONALLY on the
    // ORIGINAL hostname (probed: a 253-char astral host validates, 254
    // fails — a 400-char astral host does NOT validate).
    let Some(retry_authority) = punycode_netloc(authority) else {
        return false;
    };
    if !authority_path_matches(&retry_authority, rest) {
        return false;
    }
    let Some(hostname) = split_hostname(authority) else {
        return false;
    };
    hostname.chars().count() <= HOSTNAME_MAX_LENGTH
}

/// `value.split("://")[0].lower() in ["http", "https", "ftp", "ftps"]`,
/// exactly: a scheme passing Python's check is necessarily pure ASCII
/// (the only non-ASCII char whose `.lower()` is ASCII is U+212A → `k`,
/// and no scheme name contains `k`; multi-char lowers like İ→i̇ can never
/// concatenate to a pure-ASCII name), so ASCII case-insensitive equality
/// against the four names is equivalent — no Unicode lowering needed.
fn is_url_scheme(scheme: &str) -> bool {
    matches!(scheme.len(), 3..=5)
        && (scheme.eq_ignore_ascii_case("http")
            || scheme.eq_ignore_ascii_case("https")
            || scheme.eq_ignore_ascii_case("ftp")
            || scheme.eq_ignore_ascii_case("ftps"))
}

/// `urlsplit` `ValueError` parity for bracketed netlocs, probed against
/// `parse.py:511-516` (`_check_bracketed_netloc`, `_check_bracketed_host`).
/// Rejects bracket mismatch, data before `[` / after `]` (after the LAST
/// `@`), and bracketed hosts that are not valid IPv6.
///
/// Strict `Ipv6Addr` here is verdict-equivalent to Python's
/// `ipaddress.ip_address` + IPvFuture acceptance: IPvFuture (`[v…]`) and
/// `%`-scoped inners pass `urlsplit` but can never match the Django host
/// regex next (`v`/`%` are outside `[0-9a-f:.]`), so both paths reject
/// (probed `[v1.fe]`, `[fe80::1%eth0]` → INVALID). Brackets confined to
/// the userinfo likewise always fail: the else-branch hostname (no `[`,
/// cut at the first `:`) can never IPv6-parse.
fn bracketed_netloc_invalid(authority: &str) -> bool {
    let has_open = authority.contains('[');
    let has_close = authority.contains(']');
    if has_open != has_close {
        return true;
    }
    if !has_open {
        return false;
    }
    // After the LAST `@` (`rpartition`), like `_check_bracketed_netloc`.
    let hostinfo = authority.rsplit('@').next().unwrap_or(authority);
    match hostinfo.find('[') {
        Some(0) => {
            let bracketed = &hostinfo[1..];
            let Some(end) = bracketed.find(']') else {
                return true;
            };
            let after = &bracketed[end + 1..];
            if !after.is_empty() && !after.starts_with(':') {
                return true;
            }
            // Port content is NOT validated here (`port` is lazily
            // parsed); the regex stage rejects non-digit ports next.
            bracketed[..end].parse::<Ipv6Addr>().is_err()
        }
        // Data before `[`, or brackets confined to the userinfo (whose
        // unbracketed hostname then must IPv6-parse — impossible, so
        // this arm always fails, exactly like `_check_bracketed_host`).
        Some(_) => true,
        None => {
            let hostname = hostinfo.split(':').next().unwrap_or("");
            hostname.parse::<Ipv6Addr>().is_err()
        }
    }
}

/// `_checknetloc` equivalence (`parse.py:421-437`): `urlsplit` raises when
/// NFKC normalization introduces a `/?#@:` char. Verified by an exhaustive
/// `unicodedata` scan of all planes: exactly these 19 chars (BMP-only, no
/// astral hits) expand to a string containing one — fullwidth
/// `＃／：？＠`, small-form `﹕﹖﹟﹫`, vertical `︓︖`, `⁇⁈⁉`, `℀℁℅℆`, `⩴`.
/// Per-char checking is exact (NFKC composition never CREATES an ASCII
/// special from chars that lack one).
fn is_nfkc_dangerous(ch: char) -> bool {
    matches!(
        ch,
        '\u{2047}'
            | '\u{2048}'
            | '\u{2049}'
            | '\u{2100}'
            | '\u{2101}'
            | '\u{2105}'
            | '\u{2106}'
            | '\u{2a74}'
            | '\u{fe13}'
            | '\u{fe16}'
            | '\u{fe55}'
            | '\u{fe56}'
            | '\u{fe5f}'
            | '\u{fe6b}'
            | '\u{ff03}'
            | '\u{ff0f}'
            | '\u{ff1a}'
            | '\u{ff1f}'
            | '\u{ff20}'
    )
}

/// Django `URLValidator.regex` equivalence on the authority + path parts.
/// The scheme part is guaranteed by the pre-checks (any passing scheme is
/// pure ASCII matching `[a-z0-9.+-]*`, and `://` was split on), so this
/// covers auth + host + port + path + end anchor.
fn authority_path_matches(authority: &str, rest: &str) -> bool {
    // Auth split at the FIRST `@`: the regex auth group is greedy and its
    // user class admits no `@`, while the host grammar admits none either
    // — so a present `@` MUST delimit auth (`a@b@c` fails both ways).
    let hostport = match authority.find('@') {
        Some(at) => {
            if !userinfo_ok(&authority[..at]) {
                return false;
            }
            &authority[at + 1..]
        }
        None => authority,
    };
    let Some(tail) = parse_host(hostport) else {
        return false;
    };
    // Optional port: `(?::[0-9]{1,5})?`. No backtracking can help a bad
    // port (no host grammar admits `:`), so the first-`:` split is exact.
    let tail = match tail.strip_prefix(':') {
        Some(port) => {
            if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit())
            {
                return false;
            }
            ""
        }
        None => tail,
    };
    if !tail.is_empty() {
        return false;
    }
    // Path: empty, or `[/?#][^\s]*` to the end (`\Z`).
    if rest.is_empty() {
        return true;
    }
    let mut chars = rest.chars();
    match chars.next() {
        Some('/' | '?' | '#') => chars.all(|c| !is_python_space(c)),
        _ => false,
    }
}

/// `(?:[^\s:@/]+(?::[^\s:@/]*)?@)` user grammar (the `@` itself was split
/// on, so it cannot appear here).
fn userinfo_ok(userinfo: &str) -> bool {
    if userinfo.is_empty() {
        return false;
    }
    let mut parts = userinfo.splitn(2, ':');
    let user = parts.next().unwrap_or("");
    if user.is_empty() || user.chars().any(is_forbidden_auth) {
        return false;
    }
    if let Some(pass) = parts.next() {
        // A second `:` lands in `pass` and fails here, as in the regex.
        if pass.chars().any(is_forbidden_auth) {
            return false;
        }
    }
    true
}

fn is_forbidden_auth(ch: char) -> bool {
    is_python_space(ch) || ch == ':' || ch == '@' || ch == '/'
}

/// The host alternatives (`ipv4 | ipv6-simple | hostname-labels |
/// localhost`), returning the unmatched tail (port/path territory) on
/// success. Alternation order is irrelevant — backtracking makes this a
/// union, and the alternatives are outcome-disjoint.
fn parse_host(hostport: &str) -> Option<&str> {
    if let Some(stripped) = hostport.strip_prefix('[') {
        // `\[[0-9a-f:.]+\]`: 1+ ASCII hex/colon/dot (IGNORECASE → A-F
        // match; non-ASCII folds are moot — the urlsplit stage above
        // already rejected non-IPv6 inners), then `]`.
        let mut end = 0usize;
        for (index, ch) in stripped.char_indices() {
            if ch.is_ascii_hexdigit() || ch == ':' || ch == '.' {
                end = index + ch.len_utf8();
            } else {
                break;
            }
        }
        if end == 0 {
            return None;
        }
        return stripped[end..].strip_prefix(']');
    }
    // Non-bracketed: the host runs to the first `:` or the end.
    let host_end = hostport.find(':').unwrap_or(hostport.len());
    let host = &hostport[..host_end];
    let tail = &hostport[host_end..];
    if is_strict_ipv4(host) {
        return Some(tail);
    }
    if is_localhost_fold(host) {
        return Some(tail);
    }
    if hostname_labels_ok(host) {
        return Some(tail);
    }
    None
}

/// `(?:0|25[0-5]|2[0-4][0-9]|1[0-9]?[0-9]?|[1-9][0-9]?)(?:\....){3}` —
/// exactly four strict octets (no leading zeros; probed `01`/`00` fail).
fn is_strict_ipv4(host: &str) -> bool {
    let mut parts = host.split('.');
    for _ in 0..4 {
        match parts.next() {
            Some(part) if is_octet(part) => {}
            _ => return false,
        }
    }
    parts.next().is_none()
}

fn is_octet(part: &str) -> bool {
    if part.is_empty() || part.len() > 3 || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    match part.len() {
        1 => true,
        2 => !part.starts_with('0'),
        3 => !part.starts_with('0') && part.parse::<u32>().unwrap_or(999) <= 255,
        _ => false,
    }
}

/// The `localhost` alternative under `re.IGNORECASE`: ASCII folding plus
/// the full non-ASCII fold set — verified by an exhaustive scan as exactly
/// U+0130/U+0131 → `i`, U+017F → `s`, U+212A → `k` (probed `localhoſt`
/// VALID).
fn is_localhost_fold(host: &str) -> bool {
    host.chars().count() == 9
        && host
            .chars()
            .zip("localhost".chars())
            .all(|(got, want)| fold_case(got) == want)
}

/// Case-fold for `re.IGNORECASE` equivalence: ASCII upper → lower plus the
/// four verified non-ASCII folds (see [`is_localhost_fold`]).
fn fold_case(ch: char) -> char {
    match ch {
        '\u{130}' | '\u{131}' => 'i',
        '\u{17f}' => 's',
        '\u{212a}' => 'k',
        ch if ch.is_ascii_uppercase() => ch.to_ascii_lowercase(),
        ch => ch,
    }
}

/// `hostname + domain* + tld`: dots are unambiguous separators, so the
/// greedy/backtracking regex is equivalent to this label split. One
/// trailing dot allowed (`tld_re` ends `\.?`).
fn hostname_labels_ok(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let mut labels = host.split('.');
    let Some(first) = labels.next() else {
        return false;
    };
    if !is_dns_label(first) {
        return false;
    }
    let rest: Vec<&str> = labels.collect();
    if rest.is_empty() {
        return false;
    }
    let (middle, last) = rest.split_at(rest.len() - 1);
    let tld = last[0];
    if !(is_tld_alpha(tld) || is_tld_puny(tld)) {
        return false;
    }
    middle.iter().all(|label| is_dns_label(label))
}

/// `hostname_re`/`domain_re` label shape (identical constraints): 1–63
/// chars of `[a-z0-9-]` (ASCII case-insensitive) plus U+00A1–U+FFFF, with
/// no leading/trailing dash (`(?!-)`/`(?<!-)`).
fn is_dns_label(label: &str) -> bool {
    let count = label.chars().count();
    if count == 0 || count > 63 {
        return false;
    }
    if !label.chars().all(is_label_char) {
        return false;
    }
    let mut chars = label.chars();
    let first = chars.next().unwrap_or('-');
    let last = label.chars().last().unwrap_or('-');
    first != '-' && last != '-'
}

fn is_label_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || is_ul(ch)
}

/// Django `ul = "\u00a1-\uffff"` (NOT a raw string in the source — the
/// escape is real).
fn is_ul(ch: char) -> bool {
    ('\u{a1}'..='\u{ffff}').contains(&ch)
}

/// Alpha TLD: `\.(?!-)(?:[a-z\ul-]{2,63})(?<!-)` — 2–63 chars, no digits,
/// no edge dash (probed: `c`/`123` fail, `straß…`/CJK pass).
fn is_tld_alpha(tld: &str) -> bool {
    let count = tld.chars().count();
    if !(2..=63).contains(&count) {
        return false;
    }
    if !tld
        .chars()
        .all(|ch| ch.is_ascii_alphabetic() || ch == '-' || is_ul(ch))
    {
        return false;
    }
    let first = tld.chars().next().unwrap_or('-');
    let last = tld.chars().last().unwrap_or('-');
    first != '-' && last != '-'
}

/// Punycode TLD: `xn--[a-z0-9]{1,59}` under `re.IGNORECASE` plus the
/// trailing `(?<!-)` (probed: `xn--p1ai`/`xn--123` pass, `xn--` fails,
/// and the folds apply — `xn--ſ1`, `xn--ı1`, `xn--K1` all VALID).
fn is_tld_puny(tld: &str) -> bool {
    let mut chars = tld.chars();
    for expected in ['x', 'n', '-', '-'] {
        match chars.next() {
            Some(got) if fold_case(got) == expected => {}
            _ => return false,
        }
    }
    let rest: Vec<char> = chars.collect();
    if rest.is_empty() || rest.len() > 59 {
        return false;
    }
    rest.iter().all(|ch| fold_case(*ch).is_ascii_alphanumeric())
        && *rest.last().unwrap_or(&'-') != '-'
}

/// Django's post-match IPv6 re-verify target:
/// `re.search(r"^\[(.+)\](?::[0-9]{1,5})?$", netloc)` — the greedy `.+`
/// runs to the LAST `]`. `None` = no verify (pass).
fn bracketed_verify_inner(authority: &str) -> Option<&str> {
    let stripped = authority.strip_prefix('[')?;
    let end = stripped.rfind(']')?;
    let inner = &stripped[..end];
    if inner.is_empty() {
        return None;
    }
    let after = &stripped[end + 1..];
    if after.is_empty() {
        return Some(inner);
    }
    let port = after.strip_prefix(':')?;
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(inner)
}

/// `splitted_url.hostname` on the direct-match path: after the LAST `@`
/// (`rpartition` — at most one `@` can exist on a direct match, since the
/// host grammar admits none), bracket-stripped (up to the FIRST `]`, like
/// `_hostinfo`) or port-stripped, lowercased. The `%`-zone split is
/// unreachable (`%` admits no host grammar). Lowercasing is load-bearing:
/// İ lowercases to TWO code points (probed: raw-252 → VALID, raw-253 →
/// INVALID). Rust `to_lowercase` and Python `.lower()` agree here (İ is
/// the only multi-char lowercase in Unicode).
fn split_hostname(authority: &str) -> Option<String> {
    let hostinfo = authority.rsplit('@').next().unwrap_or(authority);
    let hostname = if let Some(stripped) = hostinfo.strip_prefix('[') {
        let end = stripped.find(']')?;
        stripped[..end].to_string()
    } else {
        hostinfo.split(':').next().unwrap_or("").to_string()
    };
    if hostname.is_empty() {
        return None;
    }
    Some(hostname.to_lowercase())
}

/// Django `punycode()` (`django/utils/encoding.py:211-213`,
/// `domain.encode("idna")`) on the authority, label by label: ASCII
/// labels pass through byte-identical (empty or >63 chars →
/// `UnicodeError`, i.e. `None` — but a wholly empty authority encodes to
/// `""`, probed); non-ASCII labels go through UTS-46 (`idna` crate).
/// `None` = `UnicodeError` (the retry fails, the original verdict stands).
///
/// Known delta (see module docs): Python uses its IDNA-2003 codec while
/// `idna` implements UTS-46 — identical on every probe.
fn punycode_netloc(authority: &str) -> Option<String> {
    if authority.is_empty() {
        return Some(String::new());
    }
    // One trailing root dot is tolerated and preserved (`höst.com.` →
    // `xn--….com.`, probed); two (`a.com..`) raise, as do leading and
    // interior empties.
    let (body, dot) = match authority.strip_suffix('.') {
        Some(body) => (body, "."),
        None => (authority, ""),
    };
    if body.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for label in body.split('.') {
        if label.is_ascii() {
            if label.is_empty() || label.len() > 63 {
                return None;
            }
            out.push(label.to_string());
        } else {
            out.push(idna::domain_to_ascii(label).ok()?);
        }
    }
    let mut joined = out.join(".");
    joined.push_str(dot);
    Some(joined)
}

/// The 400 body for the create/update duplicate guard: the `create()` /
/// `update()` raise is `ValidationError({"error": …})` out of
/// `create()`/`update()` (`issue.py:617-622,638-648`), which DRF's
/// `exception_handler` returns as-is — a dict detail keeps its scalar
/// value, so the wire body is `{"error": "URL already exists for this
/// Issue"}` (STRING form), unlike `is_valid()` field errors, which take
/// the list form.
///
/// F18-02 `duplicate_create.detail` / `update_save_duplicate.detail`
/// record the exception *internals* (`{"error": [{"message", "code"}]}`),
/// not the wire bytes: `ErrorDetail` subclasses `str` and serializes as a
/// bare string. Live-verified 2026-10-05 (Django runserver, stock test
/// settings; PIDASHCONV-674 differential): POST duplicate URL → 400
/// `{"error":"URL already exists for this Issue"}`.
pub fn duplicate_url_body() -> String {
    field_errors_body(&[("error", Value::String(MSG_DUPLICATE_URL.to_string()))])
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::String(text.to_string()),
        None => Value::Null,
    }
}

/// Failure modes of [`render_link`], [`render_pr_link`] and
/// [`render_review_link`]: the caller-contract arm is 500-class for the
/// handler to map (mirroring `shape_issue::RenderError` and the sibling
/// `RelationShowError`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkShowError {
    /// Nested `fields=` dict (`TypeError` parity, see [`filter_fields`]).
    #[error("fields filter failed: {0}")]
    Fields(#[from] FilterError),
    /// A map-hit `expand` name with no caller value (Python always renders
    /// the related object, or `{}` when the FK is null).
    #[error("expand '{0}' needs its rendered value (None renders {{}})")]
    MissingExpansion(String),
}

/// One `IssueLink` row for `IssueLinkSerializer.to_representation`
/// (`issue.py:649-671`). Datetimes cross this boundary already
/// DRF-formatted (mirroring `shape_issue`, where `created_at` is a `&str`
/// passthrough — the queries layer formats); ids are canonical
/// lowercase-hyphenated UUID strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub title: Option<&'a str>,
    pub url: &'a str,
    pub metadata: &'a Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
}

/// Input for [`render_link`]: the row plus the `BaseSerializer` read
/// kwargs. `expand` names are in request order (comma-split query
/// string); `expansions` carries rendered values for map-hit names among
/// the kept fields only — `Some(value)` renders the related object,
/// `None` renders `{}` (null FK).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkShowInput<'a> {
    pub row: &'a LinkRow<'a>,
    pub fields: Option<&'a [FieldSpec]>,
    pub expand: &'a [&'a str],
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `IssueLinkSerializer.to_representation` (`issue.py:649-671`
/// over `BaseSerializer`): the [`LINK_SHOW_FIELDS`] keys in wire order
/// (`PrimaryKeyRelatedField` FKs render pk strings, `None` renders null,
/// `metadata` passes its JSON through), filtered by `fields=`, then the
/// `expand=` rules (map hit → caller value or `{}` for a null FK;
/// anything else → `null` — no non-map field of this serializer has a
/// `<name>_id` attribute on the `IssueLink` instance, so the
/// `base.py:114-116` passthrough always yields `None`).
pub fn render_link(input: &LinkShowInput<'_>) -> Result<Map<String, Value>, LinkShowError> {
    let kept = filter_fields(LINK_SHOW_FIELDS, input.fields)?;
    let kept_contains = |name: &str| kept.iter().any(|kept| kept == name);
    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "deleted_at" => opt_str(row.deleted_at),
            "title" => opt_str(row.title),
            "url" => Value::String(row.url.to_string()),
            "metadata" => row.metadata.clone(),
            "created_by" => opt_str(row.created_by),
            "updated_by" => opt_str(row.updated_by),
            "project" => Value::String(row.project.to_string()),
            "workspace" => Value::String(row.workspace.to_string()),
            "issue" => Value::String(row.issue.to_string()),
            _ => unreachable!("kept names are a subset of LINK_SHOW_FIELDS"),
        };
        out.insert(name.clone(), value);
    }
    apply_link_expansion(&mut out, &kept_contains, input.expand, input.expansions)?;
    Ok(out)
}

/// One `GithubPullRequestLink` row for
/// `GithubPullRequestLinkSerializer.to_representation` (`issue.py:672-699`):
/// same boundary conventions as [`LinkRow`]; `pr_number` is `int4`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrLinkRow<'a> {
    pub id: &'a str,
    pub issue: &'a str,
    pub repo_owner: &'a str,
    pub repo_name: &'a str,
    pub pr_number: i32,
    pub url: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub merged: bool,
    pub draft: bool,
    pub pr_updated_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
}

/// Input for [`render_pr_link`]: same `fields=`/`expand=` contract as
/// [`LinkShowInput`] (the views pass neither — taken for full
/// `BaseSerializer` parity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrLinkShowInput<'a> {
    pub row: &'a PrLinkRow<'a>,
    pub fields: Option<&'a [FieldSpec]>,
    pub expand: &'a [&'a str],
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `GithubPullRequestLinkSerializer.to_representation`
/// (`issue.py:672-699`): the [`PR_LINK_FIELDS`] keys in declared order
/// with the same `fields=`/`expand=` rules as [`render_link`] (map hits
/// here are `issue` and `created_by`; every other field expands to `null`).
pub fn render_pr_link(input: &PrLinkShowInput<'_>) -> Result<Map<String, Value>, LinkShowError> {
    let kept = filter_fields(PR_LINK_FIELDS, input.fields)?;
    let kept_contains = |name: &str| kept.iter().any(|kept| kept == name);
    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "issue" => Value::String(row.issue.to_string()),
            "repo_owner" => Value::String(row.repo_owner.to_string()),
            "repo_name" => Value::String(row.repo_name.to_string()),
            "pr_number" => Value::Number(row.pr_number.into()),
            "url" => Value::String(row.url.to_string()),
            "title" => Value::String(row.title.to_string()),
            "state" => Value::String(row.state.to_string()),
            "merged" => Value::Bool(row.merged),
            "draft" => Value::Bool(row.draft),
            "pr_updated_at" => opt_str(row.pr_updated_at),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "created_by" => opt_str(row.created_by),
            _ => unreachable!("kept names are a subset of PR_LINK_FIELDS"),
        };
        out.insert(name.clone(), value);
    }
    apply_link_expansion(&mut out, &kept_contains, input.expand, input.expansions)?;
    Ok(out)
}

/// One `GitCodeReviewLink` row for
/// `GitCodeReviewLinkSerializer.to_representation` (`issue.py:700-728`):
/// same boundary conventions as [`LinkRow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewLinkRow<'a> {
    pub id: &'a str,
    pub issue: &'a str,
    pub provider: &'a str,
    pub host_url: &'a str,
    pub namespace: &'a str,
    pub repo_name: &'a str,
    pub repo_external_id: &'a str,
    pub external_id: &'a str,
    pub external_iid: &'a str,
    pub url: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub merged: bool,
    pub draft: bool,
    pub remote_updated_at: Option<&'a str>,
    pub metadata: &'a Value,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
}

/// Input for [`render_review_link`]: same `fields=`/`expand=` contract as
/// [`LinkShowInput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewLinkShowInput<'a> {
    pub row: &'a ReviewLinkRow<'a>,
    pub fields: Option<&'a [FieldSpec]>,
    pub expand: &'a [&'a str],
    pub expansions: &'a [(&'a str, Option<Value>)],
}

/// Port of `GitCodeReviewLinkSerializer.to_representation`
/// (`issue.py:700-728`): the [`REVIEW_LINK_FIELDS`] keys in declared order
/// with the same `fields=`/`expand=` rules as [`render_link`] (map hits
/// are `issue` and `created_by`; `repo_external_id`/`external_id`/
/// `external_iid` expand to `null` — `getattr(instance, "external_id_id",
/// None)` is `None`).
pub fn render_review_link(
    input: &ReviewLinkShowInput<'_>,
) -> Result<Map<String, Value>, LinkShowError> {
    let kept = filter_fields(REVIEW_LINK_FIELDS, input.fields)?;
    let kept_contains = |name: &str| kept.iter().any(|kept| kept == name);
    let row = input.row;
    let mut out = Map::with_capacity(kept.len());
    for name in &kept {
        let value = match name.as_str() {
            "id" => Value::String(row.id.to_string()),
            "issue" => Value::String(row.issue.to_string()),
            "provider" => Value::String(row.provider.to_string()),
            "host_url" => Value::String(row.host_url.to_string()),
            "namespace" => Value::String(row.namespace.to_string()),
            "repo_name" => Value::String(row.repo_name.to_string()),
            "repo_external_id" => Value::String(row.repo_external_id.to_string()),
            "external_id" => Value::String(row.external_id.to_string()),
            "external_iid" => Value::String(row.external_iid.to_string()),
            "url" => Value::String(row.url.to_string()),
            "title" => Value::String(row.title.to_string()),
            "state" => Value::String(row.state.to_string()),
            "merged" => Value::Bool(row.merged),
            "draft" => Value::Bool(row.draft),
            "remote_updated_at" => opt_str(row.remote_updated_at),
            "metadata" => row.metadata.clone(),
            "created_at" => Value::String(row.created_at.to_string()),
            "updated_at" => Value::String(row.updated_at.to_string()),
            "created_by" => opt_str(row.created_by),
            _ => unreachable!("kept names are a subset of REVIEW_LINK_FIELDS"),
        };
        out.insert(name.clone(), value);
    }
    apply_link_expansion(&mut out, &kept_contains, input.expand, input.expansions)?;
    Ok(out)
}

/// Base expansion (`base.py:76-116`), shared by the three renders: in
/// `expand` order, names outside the kept fields are skipped; map hits
/// render the caller value (`{}` for a null FK,
/// [`LinkShowError::MissingExpansion`] when the caller passed none);
/// anything else renders `null`. (All three serializers render scalars
/// only, so the `many=True` list arm at `base.py:109` is unreachable.)
fn apply_link_expansion(
    out: &mut Map<String, Value>,
    kept_contains: &dyn Fn(&str) -> bool,
    expand: &[&str],
    expansions: &[(&str, Option<Value>)],
) -> Result<(), LinkShowError> {
    for name in expand {
        if !kept_contains(name) {
            continue;
        }
        if BASE_EXPANSION_NAMES.contains(name) {
            match expansions.iter().find(|(key, _)| key == name) {
                Some((_, Some(value))) => {
                    out.insert(name.to_string(), value.clone());
                }
                Some((_, None)) => {
                    out.insert(name.to_string(), Value::Object(Map::new()));
                }
                None => return Err(LinkShowError::MissingExpansion(name.to_string())),
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
    use serde_json::json;

    const F18_02: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/serializers/F18-02.label_link_relation.golden.json"
    );

    const UID: &str = "25ace52e-c64d-4043-a700-911b1e42bffc";
    const URL: &str = "https://example.com/spec";

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

    fn valid_body(title: &str, url: &str) -> Value {
        json!({"title": title, "url": url})
    }

    // ---- F18-02 create replays --------------------------------------------

    #[test]
    fn create_fields_match_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueLinkCreateSerializer");
        assert_eq!(LINK_CREATE_FIELDS, str_list(&create["fields"]).as_slice());
        // The update duplicate collapses to the same live field list.
        assert_eq!(LINK_UPDATE_FIELDS, LINK_CREATE_FIELDS);
    }

    #[test]
    fn create_ok_replays() {
        let validated = validate_link_create(&valid_body("Spec", URL)).expect("valid");
        assert_eq!(
            validated,
            ValidatedLinkWrite {
                title: Some(Some("Spec".to_string())),
                url: URL.to_string(),
            }
        );
        // Absent title validates to nothing (validated keys are only
        // title+url when present — the F18-02 `issue_id_note` shape).
        let validated = validate_link_create(&json!({"url": URL})).expect("valid");
        assert_eq!(
            validated,
            ValidatedLinkWrite {
                title: None,
                url: URL.to_string(),
            }
        );
    }

    #[test]
    fn create_url_errors_replay_byte_identical() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueLinkCreateSerializer");
        // bad_format ← "notaurl", bad_scheme ← "ftp://…", missing_url ← {}.
        let bad_format = validate_link_create(&json!({"url": "notaurl"}));
        assert_eq!(
            bad_format.expect_err("format fails").body(),
            expected_field_body(&create["bad_format"]["errors"], "url")
        );
        let bad_scheme = validate_link_create(&json!({"url": "ftp://example.com/x"}));
        assert_eq!(
            bad_scheme.expect_err("scheme fails").body(),
            expected_field_body(&create["bad_scheme"]["errors"], "url")
        );
        let missing = validate_link_create(&json!({}));
        assert_eq!(
            missing.expect_err("missing fails").body(),
            expected_field_body(&create["missing_url"]["errors"], "url")
        );
    }

    #[test]
    fn duplicate_guard_body_matches_f18_02() {
        let fx = fixture(F18_02);
        let create = unit(&fx, "IssueLinkCreateSerializer");
        let update = unit(&fx, "IssueLinkUpdateSerializer");
        // F18-02 records the exception internals ({message, code}); the wire
        // body carries the message as a bare string (DRF exception_handler
        // returns the dict detail as-is — see the helper docs), so replay
        // the golden message into the string form, not the list form.
        for detail in [
            &create["duplicate_create"]["detail"],
            &update["update_save_duplicate"]["detail"],
        ] {
            let message = detail["error"][0]["message"]
                .as_str()
                .expect("golden error carries a message");
            let want = format!(
                "{{\"error\":{}}}",
                serde_json::to_string(message).expect("message serializes")
            );
            assert_eq!(duplicate_url_body(), want);
        }
        assert_eq!(
            duplicate_url_body(),
            r#"{"error":"URL already exists for this Issue"}"#
        );
    }

    // ---- field rules (live-probe pins) -------------------------------------

    #[test]
    fn title_null_blank_and_coercions() {
        // Explicit null validates to None (row stores NULL).
        let validated = validate_link_create(&json!({"title": null, "url": URL})).expect("valid");
        assert_eq!(validated.title, Some(None));
        // Blank and whitespace-only validate as "" (allow_blank).
        for title in ["", "   ", "\t \n"] {
            let validated =
                validate_link_create(&json!({"title": title, "url": URL})).expect("valid");
            assert_eq!(validated.title, Some(Some(String::new())), "{title:?}");
        }
        // Int/float coerce via Python str(); bools fail.
        let validated = validate_link_create(&json!({"title": 7, "url": URL})).expect("valid");
        assert_eq!(validated.title, Some(Some("7".to_string())));
        for (input, want) in [
            (json!(1e100), "1e+100"),
            (json!(100.0), "100.0"),
            (json!(-0.0), "-0.0"),
            (json!(1.5), "1.5"),
            (json!(0), "0"),
            (json!(-5), "-5"),
            (json!(18446744073709551615u64), "18446744073709551615"),
        ] {
            let validated =
                validate_link_create(&json!({"title": input, "url": URL})).expect("valid");
            assert_eq!(validated.title, Some(Some(want.to_string())), "{input}");
        }
        for input in [json!(true), json!(false), json!([1]), json!({"t": 1})] {
            let err =
                validate_link_create(&json!({"title": input, "url": URL})).expect_err("must fail");
            assert_eq!(
                err.body(),
                r#"{"title":["Not a valid string."]}"#,
                "{input}"
            );
        }
    }

    #[test]
    fn title_max_length_counts_code_points() {
        let ok = "\u{e9}".repeat(255);
        validate_link_create(&valid_body(&ok, URL)).expect("255 code points pass");
        let long = "a".repeat(256);
        let err = validate_link_create(&valid_body(&long, URL)).expect_err("256 chars fail");
        assert_eq!(
            err.body(),
            r#"{"title":["Ensure this field has no more than 255 characters."]}"#
        );
        let long = "\u{e9}".repeat(256);
        validate_link_create(&valid_body(&long, URL)).expect_err("256 é fail");
    }

    #[test]
    fn null_chars_and_validator_order() {
        // Null char alone reports once; over-long + null reports BOTH in
        // validator order (MaxLengthValidator before ProhibitNull — probed).
        let err = validate_link_create(&json!({"title": "a\x00b", "url": URL}))
            .expect_err("null char fails");
        assert_eq!(
            err.body(),
            r#"{"title":["Null characters are not allowed."]}"#
        );
        let both = format!("{}\x00", "a".repeat(256));
        let err = validate_link_create(&valid_body(&both, URL)).expect_err("both fail");
        assert_eq!(
            err.body(),
            r#"{"title":["Ensure this field has no more than 255 characters.","Null characters are not allowed."]}"#
        );
        // Field validators win over validate_url (probed: no format message).
        let err =
            validate_link_create(&json!({"url": "notaurl\x00"})).expect_err("null char fails");
        assert_eq!(
            err.body(),
            r#"{"url":["Null characters are not allowed."]}"#
        );
    }

    #[test]
    fn url_required_null_blank_and_types() {
        let cases: &[(&str, Value, &str)] = &[
            (
                "missing",
                json!({}),
                r#"{"url":["This field is required."]}"#,
            ),
            (
                "null",
                json!({"url": null}),
                r#"{"url":["This field may not be null."]}"#,
            ),
            (
                "blank",
                json!({"url": ""}),
                r#"{"url":["This field may not be blank."]}"#,
            ),
            (
                "whitespace",
                json!({"url": "   "}),
                r#"{"url":["This field may not be blank."]}"#,
            ),
            (
                "bool",
                json!({"url": true}),
                r#"{"url":["Not a valid string."]}"#,
            ),
            (
                "list",
                json!({"url": ["https://example.com/"]}),
                r#"{"url":["Not a valid string."]}"#,
            ),
            (
                "dict",
                json!({"url": {"u": URL}}),
                r#"{"url":["Not a valid string."]}"#,
            ),
            (
                "int",
                json!({"url": 123}),
                r#"{"url":["Invalid URL format."]}"#,
            ),
            (
                "float",
                json!({"url": 1.5}),
                r#"{"url":["Invalid URL format."]}"#,
            ),
        ];
        for (label, body, want) in cases {
            let err = validate_link_create(body).expect_err("must fail");
            assert_eq!(err.body(), *want, "{label}");
        }
        // Padded URLs validate stripped (incl. the exotic strip set).
        for pad in [
            " ", "\t", "\u{1c}", "\u{85}", "\u{a0}", "\u{3000}", "\u{b}", "\u{c}",
        ] {
            let body = json!({"url": format!("{pad}{URL}{pad}")});
            let validated = validate_link_create(&body).expect("padded valid");
            assert_eq!(validated.url, URL, "{pad:?}");
        }
        // …while zero-width space / BOM are NOT stripped (probed).
        for pad in ["\u{200b}", "\u{feff}"] {
            let body = json!({"url": format!("{pad}{URL}{pad}")});
            let err = validate_link_create(&body).expect_err("must fail");
            assert_eq!(err.body(), r#"{"url":["Invalid URL format."]}"#, "{pad:?}");
        }
    }

    #[test]
    fn combined_errors_follow_field_order() {
        let err =
            validate_link_create(&json!({"title": true, "url": "notaurl"})).expect_err("both fail");
        assert_eq!(
            err.body(),
            r#"{"title":["Not a valid string."],"url":["Invalid URL format."]}"#
        );
    }

    #[test]
    fn issue_id_and_unknown_keys_ignored() {
        for issue_id in [
            json!(UID),
            json!("nota-uuid"),
            json!(123),
            json!(null),
            json!({"x": 1}),
            json!(["x"]),
        ] {
            let validated = validate_link_create(&json!({"url": URL, "issue_id": issue_id}))
                .expect("issue_id ignored");
            assert_eq!(validated.title, None);
            assert_eq!(validated.url, URL);
        }
        // Unknown keys (incl. model fields like metadata) are ignored too.
        let validated =
            validate_link_create(&json!({"url": URL, "nope": 1, "metadata": {}})).expect("valid");
        assert_eq!(validated.url, URL);
    }

    #[test]
    fn null_and_non_dict_bodies() {
        let err = validate_link_create(&Value::Null).expect_err("null fails");
        assert_eq!(err.body(), r#"{"non_field_errors":["No data provided"]}"#);
        for (body, datatype) in [
            (json!([1]), "list"),
            (json!("x"), "str"),
            (json!(5), "int"),
            (json!(true), "bool"),
            (json!(1.5), "float"),
        ] {
            let err = validate_link_create(&body).expect_err("non-dict fails");
            assert_eq!(
                err.body(),
                format!(
                    r#"{{"non_field_errors":["Invalid data. Expected a dictionary, but got {datatype}."]}}"#
                ),
                "{body}"
            );
        }
    }

    #[test]
    fn update_matches_create() {
        // Same field rules through the update entry point.
        let via_create = validate_link_create(&valid_body("t", URL)).expect("valid");
        let via_update = validate_link_update(&valid_body("t", URL)).expect("valid");
        assert_eq!(via_create, via_update);
        let err_create = validate_link_create(&json!({"url": "notaurl"})).expect_err("fails");
        let err_update = validate_link_update(&json!({"url": "notaurl"})).expect_err("fails");
        assert_eq!(err_create.body(), err_update.body());
        // F18-02 update vectors: duplicate-target URL validates (the guard
        // fires at save, with the shared body); title-only write validates.
        let fx = fixture(F18_02);
        let update = unit(&fx, "IssueLinkUpdateSerializer");
        assert_eq!(
            update["update_to_duplicate"]["valid"],
            Value::Bool(true),
            "fixture pins update_to_duplicate valid"
        );
        validate_link_update(&json!({"url": URL})).expect("update url validates");
        validate_link_update(&json!({"title": "renamed", "url": URL}))
            .expect("update title validates");
    }

    // ---- validate_url battery (every vector probed live) --------------------

    fn url_case(url: &str, want: Result<(), LinkUrlError>) {
        assert_eq!(validate_link_url(url), want, "{url:?}");
    }

    #[test]
    fn url_valid_vectors() {
        for url in [
            "https://example.com/spec",
            "http://example.com/",
            "http://example.com",
            "http://example.com?x",
            "http://example.com#x",
            "http://example.com//a",
            "https://example.com/a?b=c#d",
            "http://127.0.0.1:8000/x",
            "http://0.0.0.0/",
            "http://localhost:8000/x",
            "http://LOCALHOST/",
            "http://localhost:8080/",
            "http://user@localhost/",
            "http://localho\u{17f}t/",
            "http://[::1]/",
            "http://[::]/",
            "http://[1:2:3:4:5:6:7:8]/",
            "http://[::ffff:1.2.3.4]/",
            "http://[1:2:3:4:5:6:1.2.3.4]/",
            "http://[::1.2.3.4]/",
            "http://[abcd:ef01:2345:6789:abcd:ef01:2345:6789]/",
            "http://[ABCD::EF01]/",
            "http://[0000::0001]/",
            "http://[1:2:3:4:5:6:7:038]/",
            "http://[::1]:8080/",
            "http://u@[::1]/",
            "http://user:pass@example.com/",
            "http://example.com@evil.com/",
            "http://example.com:65535/",
            "http://example.com:99999/",
            "http://example.com:0/",
            "http://example.com:00/",
            "http://example.com:00000/",
            "http://example.com./",
            "http://example.com.:8080/",
            "http://3com.com/",
            "http://a.bc/",
            "http://a-b.com/",
            "http://a--b.com/",
            "http://example.xn--p1ai/",
            "http://example.xn--123/",
            "http://example.xn--\u{17f}1/",
            "http://example.xn--\u{131}1/",
            "http://example.xn--\u{212a}1/",
            "http://example.xn--\u{130}1/",
            "http://stra\u{df}e.de/",
            "http://\u{4e2d}\u{6587}.com/",
            "http://M\u{dc}NCHEN.de/",
            "http://m\u{fc}nchen.de:8080/x",
            "http://user@m\u{fc}nchen.de/",
            "http://\u{df}.com/",
            "http://a\u{200d}b.com/",
            "http://\u{130}.com/",
            "http://\u{131}.com/",
            // IDN-retry successes (direct regex fails, punycode matches).
            "http://\u{20000}.com/",
            "http://\u{20000}.com:8080/x",
            "http://a.\u{20000}/",
            // …including with the tolerated trailing root dot (probed).
            "http://\u{20000}.com./",
            // Controls/symbols the path grammar admits (only \t\r\n are
            // unsafe, and \0 is caught by the field validator first).
            "https://example.com/\x7f",
            "https://example.com/\u{1}",
            "http://example.com/a%b",
            "http://example.com/a:b",
            "http://example.com/a;b",
            "http://example.com/a[b",
            "http://example.com/a]b",
            "http://example.com/a\"b",
            "http://example.com/a'b",
            "http://example.com/a<b",
            "http://example.com/a>b",
            "http://example.com/a\\b",
            "http://example.com/a^b",
            "http://example.com/a|b",
            "http://example.com/a`b",
            "http://example.com/a{b",
            "http://example.com/a@b",
        ] {
            url_case(url, Ok(()));
        }
    }

    #[test]
    fn url_format_vectors() {
        for url in [
            "notaurl",
            "http:/example.com",
            "mailto:a@b.com",
            "https:///example.com",
            ":///example.com",
            "ht tp://example.com/",
            "1http://example.com/",
            "http+unix://example.com/",
            "http",
            "https",
            "HTTP\u{17f}://example.com/",
            "https://example.com/a\tb",
            "https://example.com/a\rb",
            "https://exa mple.com/",
            "https://example.com/a b",
            "http://\u{1}example.com/",
            "http://a\u{1c}b.com/",
            "http://999.1.1.1/",
            "http://01.2.3.4/",
            "http://1.02.3.4/",
            "http://00.1.1.1/",
            "http://256.1.1.1/",
            "http://1.1.1/",
            "http://1.1.1.1.1/",
            "http://localhostx/",
            "http://intranet/",
            "http://com/",
            "http://localhost./",
            "http://a.b/",
            "http://example.c/",
            "http://example.123/",
            "http://example.xn--/",
            "http://-example.com/",
            "http://example-.com/",
            "http://under_score.com/",
            "http://exa%mple.com/",
            "http://a..b.com/",
            "http://example.com../",
            "http://[::zz]/",
            "http://[::1/",
            "http://[1:2:3:4:5:6:7:8:9]/",
            "http://[:1:2:3:4:5:6:7]/",
            "http://[1:2:3:4:5:6:7:]/",
            "http://[1::2::3]/",
            "http://[::ffff:01.02.03.04]/",
            "http://[1:2:3:4:5:1.2.3.4]/",
            "http://[1.2.3.4]/",
            "http://[12345::]/",
            "http://[1:2:3:4:5:6:99999]/",
            "http://[-1::2]/",
            "http://[fe80::1%25eth0]/",
            "http://[fe80::1%eth0]/",
            "http://[1:2:3:4:5:6:7:8%eth0]/",
            "http://[example]/",
            "http://[v1.fe]/",
            "http://u@[v1.fe]/",
            "http://u@[1:2:3:4:5:6:7:8:9]/",
            "http://[[::1]]/",
            "http://a[b]/",
            "http://[a]b/",
            "http://[@::1]/",
            "http://[u]@example.com/",
            "http://[u]@[::1]/",
            "http://[u]@1::2/",
            "http://[::1]:/",
            "http://[::1]abc/",
            "http://example.com:123456/",
            "http://example.com:000000/",
            "http://example.com:abc/",
            "http://example.com:/",
            "http://@example.com/",
            "http://:@example.com/",
            "http://us er@example.com/",
            "http://a@b@example.com/",
            "http://user@\u{20000}.com/",
            "http://\u{fc}ser@\u{20000}.com/",
            "http://host:\u{246f}/",
            "http://host:\u{ff18}\u{ff10}\u{ff18}\u{ff10}/",
            "http://localhos\u{212a}/",
        ] {
            url_case(url, Err(LinkUrlError::Format));
        }
        // Interior \s (the full set incl. \x1c-\x1f) fails the path/host.
        for ch in [
            '\u{1c}', '\u{1f}', '\u{85}', '\u{a0}', '\u{1680}', '\u{2000}', '\u{2028}', '\u{202f}',
            '\u{205f}', '\u{3000}', '\u{b}', '\u{c}',
        ] {
            url_case(
                &format!("https://example.com/a{ch}b"),
                Err(LinkUrlError::Format),
            );
        }
        // …while zero-width space / BOM are path-legal (not \s).
        for ch in ['\u{200b}', '\u{feff}'] {
            url_case(&format!("https://example.com/a{ch}b"), Ok(()));
        }
    }

    #[test]
    fn url_scheme_vectors() {
        // URLValidator passes, the case-sensitive prefix fails.
        for url in [
            "ftp://example.com/x",
            "ftps://example.com/x",
            "ftp://localhost/",
            "HTTP://example.com/",
            "Http://example.com/",
            "HTTPS://example.com/",
        ] {
            url_case(url, Err(LinkUrlError::Scheme));
        }
    }

    #[test]
    fn url_length_boundaries() {
        let base = "http://example.com/";
        for total in [2047, 2048] {
            let url = format!("{base}{}", "a".repeat(total - base.len()));
            assert_eq!(url.chars().count(), total);
            url_case(&url, Ok(()));
        }
        let url = format!("{base}{}", "a".repeat(2049 - base.len()));
        url_case(&url, Err(LinkUrlError::Format));
        // Length counts code points, not bytes.
        let url = format!("http://example.com/{}", "\u{e9}".repeat(2048 - base.len()));
        assert_eq!(url.chars().count(), 2048);
        url_case(&url, Ok(()));
    }

    #[test]
    fn url_hostname_boundaries() {
        for (total, want) in [
            (252, Ok(())),
            (253, Ok(())),
            (254, Err(LinkUrlError::Format)),
        ] {
            let tail = total - 63 * 3 - 3 - 4;
            let host = format!(
                "{}.{}.{}.{}.com",
                "a".repeat(63),
                "b".repeat(63),
                "c".repeat(63),
                "d".repeat(tail)
            );
            assert_eq!(host.chars().count(), total);
            url_case(&format!("http://{host}/"), want);
        }
        // The cap counts the LOWERCASED hostname: İ → i̇ (2 code points).
        for (raw, want) in [(252, Ok(())), (253, Err(LinkUrlError::Format))] {
            let tail = raw - 63 * 3 - 3 - 4;
            let host = format!(
                "{}.{}.{}.{}\u{130}.com",
                "a".repeat(63),
                "b".repeat(63),
                "c".repeat(63),
                "d".repeat(tail - 1)
            );
            assert_eq!(host.chars().count(), raw);
            url_case(&format!("http://{host}/"), want);
        }
        // Label/TLD length edges.
        url_case(&format!("http://{}.com/", "a".repeat(63)), Ok(()));
        url_case(
            &format!("http://{}.com/", "a".repeat(64)),
            Err(LinkUrlError::Format),
        );
        url_case(&format!("http://example.{}/", "a".repeat(63)), Ok(()));
        url_case(
            &format!("http://example.{}/", "a".repeat(64)),
            Err(LinkUrlError::Format),
        );
    }

    #[test]
    fn url_nfkc_dangerous_set() {
        // The exhaustive 19-char scan: each fails via urlsplit. The set is
        // pinned programmatically so implementation and scan cannot drift.
        let expected: Vec<char> = vec![
            '\u{2047}', '\u{2048}', '\u{2049}', '\u{2100}', '\u{2101}', '\u{2105}', '\u{2106}',
            '\u{2a74}', '\u{fe13}', '\u{fe16}', '\u{fe55}', '\u{fe56}', '\u{fe5f}', '\u{fe6b}',
            '\u{ff03}', '\u{ff0f}', '\u{ff1a}', '\u{ff1f}', '\u{ff20}',
        ];
        let actual: Vec<char> = (0..0x110000u32)
            .filter_map(char::from_u32)
            .filter(|ch| is_nfkc_dangerous(*ch))
            .collect();
        assert_eq!(actual, expected);
        for ch in expected {
            url_case(&format!("http://a{ch}b.com/"), Err(LinkUrlError::Format));
        }
        // Neighbors that NFKC leaves special-free validate.
        for ch in [
            "\u{ff08}", "\u{2215}", "\u{2044}", "\u{37e}", "\u{df}", "\u{17f}", "\u{212a}",
            "\u{ff21}", "\u{2160}", "\u{fb01}", "\u{ff71}", "\u{ad}", "\u{ff5e}", "\u{ff05}",
        ] {
            url_case(&format!("http://a{ch}b.com/"), Ok(()));
        }
    }

    #[test]
    fn url_ipv6_differential_vectors() {
        // (inner, overall verdict) — every row probed live against
        // `ipaddress.IPv6Address` + `URLValidator`.
        for (inner, want) in [
            ("::1", Ok(())),
            ("::", Ok(())),
            ("1:2:3:4:5:6:7:8", Ok(())),
            ("1:2:3:4:5:6:7:8:9", Err(LinkUrlError::Format)),
            (":1:2:3:4:5:6:7", Err(LinkUrlError::Format)),
            ("1:2:3:4:5:6:7:", Err(LinkUrlError::Format)),
            ("1::2::3", Err(LinkUrlError::Format)),
            ("::ffff:1.2.3.4", Ok(())),
            // Leading-zero embedded IPv4: both parsers reject.
            ("::ffff:01.02.03.04", Err(LinkUrlError::Format)),
            ("1:2:3:4:5:6:1.2.3.4", Ok(())),
            ("1:2:3:4:5:1.2.3.4", Err(LinkUrlError::Format)),
            ("::1.2.3.4", Ok(())),
            ("1.2.3.4", Err(LinkUrlError::Format)),
            ("abcd:ef01:2345:6789:abcd:ef01:2345:6789", Ok(())),
            ("ABCD::EF01", Ok(())),
            ("0000::0001", Ok(())),
            // Scoped inners pass `ipaddress` but die in the Django regex
            // (`%` outside `[0-9a-f:.]`); strict parse here is equivalent.
            ("1:2:3:4:5:6:7:8%eth0", Err(LinkUrlError::Format)),
            ("fe80::1%eth0", Err(LinkUrlError::Format)),
            ("%eth0", Err(LinkUrlError::Format)),
            ("", Err(LinkUrlError::Format)),
            ("g::1", Err(LinkUrlError::Format)),
            ("1:2:3:4:5:6:7:038", Ok(())),
            ("12345::", Err(LinkUrlError::Format)),
            ("1:2:3:4:5:6:99999", Err(LinkUrlError::Format)),
            ("-1::2", Err(LinkUrlError::Format)),
        ] {
            url_case(&format!("http://[{inner}]/"), want);
        }
    }

    #[test]
    fn url_idn_nameprep_agreement_vectors() {
        // nameprep-mapped labels agree through UTS-46 (probed Python
        // VALID): soft hyphen is dropped by both codecs (UTS-46 yields the
        // same `xn--j50i` as IDNA-2003 here) and astral encodes identically.
        for url in ["http://\u{ad}\u{20000}.com/", "http://a.\u{ad}\u{20000}/"] {
            url_case(url, Ok(()));
        }
        // …while a bare soft-hyphen label validates DIRECTLY (­ is inside
        // the `\u00a1-\uffff` host range — probed).
        url_case("http://a\u{ad}b.com/", Ok(()));
    }

    #[test]
    fn url_idn_retry_applies_length_cap() {
        // 50 astral labels (103 chars): direct fails everywhere, the retry
        // encodes and matches — and the 253-char cap passes (probed VALID).
        let host = format!("{}com", "\u{20000}.".repeat(50));
        assert_eq!(host.chars().count(), 103);
        url_case(&format!("http://{host}/"), Ok(()));
        // The cap counts the ORIGINAL hostname on the retry path too
        // (probed: 253 VALID, 254 FORMAT).
        for (extra, total, want) in [
            ("xxxxx", 253, Ok(())),
            ("xxxxxx", 254, Err(LinkUrlError::Format)),
        ] {
            let host = format!("{}{}.com", "a\u{20000}b.".repeat(61), extra);
            assert_eq!(host.chars().count(), total);
            url_case(&format!("http://{host}/"), want);
        }
    }

    // ---- F18-02 render replays ---------------------------------------------

    fn link_row_from<'a>(render: &'a Value, metadata: &'a Value) -> LinkRow<'a> {
        LinkRow {
            id: render["id"].as_str().expect("id"),
            created_at: render["created_at"].as_str().expect("created_at"),
            updated_at: render["updated_at"].as_str().expect("updated_at"),
            deleted_at: opt(render, "deleted_at"),
            title: opt(render, "title"),
            url: render["url"].as_str().expect("url"),
            metadata,
            created_by: opt(render, "created_by"),
            updated_by: opt(render, "updated_by"),
            project: render["project"].as_str().expect("project"),
            workspace: render["workspace"].as_str().expect("workspace"),
            issue: render["issue"].as_str().expect("issue"),
        }
    }

    #[test]
    fn link_render_replays_f18_02() {
        let fx = fixture(F18_02);
        let show = unit(&fx, "IssueLinkSerializer");
        assert_eq!(LINK_SHOW_FIELDS, str_list(&show["render_keys"]).as_slice());
        let render = &show["render"];
        let metadata: Value = serde_json::from_str(render["metadata"].as_str().expect("metadata"))
            .expect("metadata parses");
        let row = link_row_from(render, &metadata);
        let input = LinkShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        };
        let out = render_link(&input).expect("renders");
        assert_eq!(out_keys(&out), str_list(&show["render_keys"]));
        // Full value equality: strings, nulls and the metadata object.
        let mut want = Map::with_capacity(LINK_SHOW_FIELDS.len());
        for key in LINK_SHOW_FIELDS {
            let value = match *key {
                "metadata" => metadata.clone(),
                "deleted_at" | "created_by" | "updated_by" | "title" => opt_str(opt(render, key)),
                _ => Value::String(render[key].as_str().expect("string").to_string()),
            };
            want.insert(key.to_string(), value);
        }
        assert_eq!(out, want);
    }

    #[test]
    fn pr_render_replays_f18_02() {
        let fx = fixture(F18_02);
        let show = unit(&fx, "GithubPullRequestLinkSerializer");
        assert_eq!(PR_LINK_FIELDS, str_list(&show["render_keys"]).as_slice());
        let render = &show["render"];
        let boolean = |key: &str| match render[key].as_str().expect("bool") {
            "True" => true,
            "False" => false,
            other => panic!("unexpected bool {key}={other}"),
        };
        let row = PrLinkRow {
            id: render["id"].as_str().expect("id"),
            issue: render["issue"].as_str().expect("issue"),
            repo_owner: render["repo_owner"].as_str().expect("repo_owner"),
            repo_name: render["repo_name"].as_str().expect("repo_name"),
            pr_number: render["pr_number"]
                .as_str()
                .expect("pr_number")
                .parse()
                .expect("int"),
            url: render["url"].as_str().expect("url"),
            title: render["title"].as_str().expect("title"),
            state: render["state"].as_str().expect("state"),
            merged: boolean("merged"),
            draft: boolean("draft"),
            pr_updated_at: opt(render, "pr_updated_at"),
            created_at: render["created_at"].as_str().expect("created_at"),
            updated_at: render["updated_at"].as_str().expect("updated_at"),
            created_by: opt(render, "created_by"),
        };
        let input = PrLinkShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        };
        let out = render_pr_link(&input).expect("renders");
        assert_eq!(out_keys(&out), str_list(&show["render_keys"]));
        assert_eq!(out["pr_number"], json!(7));
        assert_eq!(out["merged"], Value::Bool(false));
        assert_eq!(out["draft"], Value::Bool(false));
        assert_eq!(out["pr_updated_at"], Value::Null);
        assert_eq!(out["created_by"], Value::Null);
        assert_eq!(out["title"], Value::String("Contract PR".to_string()));
    }

    #[test]
    fn review_render_replays_f18_02() {
        let fx = fixture(F18_02);
        let show = unit(&fx, "GitCodeReviewLinkSerializer");
        assert_eq!(
            REVIEW_LINK_FIELDS,
            str_list(&show["render_keys"]).as_slice()
        );
        let render = &show["render"];
        let boolean = |key: &str| match render[key].as_str().expect("bool") {
            "True" => true,
            "False" => false,
            other => panic!("unexpected bool {key}={other}"),
        };
        let metadata: Value = serde_json::from_str(render["metadata"].as_str().expect("metadata"))
            .expect("metadata parses");
        let row = ReviewLinkRow {
            id: render["id"].as_str().expect("id"),
            issue: render["issue"].as_str().expect("issue"),
            provider: render["provider"].as_str().expect("provider"),
            host_url: render["host_url"].as_str().expect("host_url"),
            namespace: render["namespace"].as_str().expect("namespace"),
            repo_name: render["repo_name"].as_str().expect("repo_name"),
            repo_external_id: render["repo_external_id"]
                .as_str()
                .expect("repo_external_id"),
            external_id: render["external_id"].as_str().expect("external_id"),
            external_iid: render["external_iid"].as_str().expect("external_iid"),
            url: render["url"].as_str().expect("url"),
            title: render["title"].as_str().expect("title"),
            state: render["state"].as_str().expect("state"),
            merged: boolean("merged"),
            draft: boolean("draft"),
            remote_updated_at: opt(render, "remote_updated_at"),
            metadata: &metadata,
            created_at: render["created_at"].as_str().expect("created_at"),
            updated_at: render["updated_at"].as_str().expect("updated_at"),
            created_by: opt(render, "created_by"),
        };
        let input = ReviewLinkShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        };
        let out = render_review_link(&input).expect("renders");
        assert_eq!(out_keys(&out), str_list(&show["render_keys"]));
        // `external_iid` is a CharField: the "7" stays a string.
        assert_eq!(out["external_iid"], Value::String("7".to_string()));
        assert_eq!(out["metadata"], json!({}));
        assert_eq!(out["remote_updated_at"], Value::Null);
        assert_eq!(out["repo_external_id"], Value::String(String::new()));
    }

    // ---- fields=/expand= parity ------------------------------------------------

    fn sample_link_row(metadata: &Value) -> LinkRow<'_> {
        LinkRow {
            id: UID,
            created_at: "2026-10-02T23:23:54.086909Z",
            updated_at: "2026-10-02T23:23:54.086931Z",
            deleted_at: None,
            title: Some("dup"),
            url: URL,
            metadata,
            created_by: None,
            updated_by: None,
            project: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            workspace: "92989e99-51c6-4725-8069-45784951694f",
            issue: "a7509d00-345f-47fb-bee3-6bcf7d3339e2",
        }
    }

    #[test]
    fn link_fields_filter_keeps_wire_order() {
        let metadata = json!({});
        let row = sample_link_row(&metadata);
        // Reversed request order still yields wire order; unknowns ignored.
        let specs = includes(&["issue", "id", "nope"]);
        let input = LinkShowInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            expansions: &[],
        };
        let out = render_link(&input).expect("renders");
        assert_eq!(out_keys(&out), vec!["id", "issue"]);
        // Nested dict entry raises (TypeError parity).
        let specs = vec![FieldSpec::Nested("issue".to_string(), vec![])];
        let input = LinkShowInput {
            row: &row,
            fields: Some(&specs),
            expand: &[],
            expansions: &[],
        };
        assert!(matches!(render_link(&input), Err(LinkShowError::Fields(_))));
    }

    #[test]
    fn link_expand_rules() {
        let metadata = json!({});
        let row = sample_link_row(&metadata);
        let issue_lite = json!({"id": "a7509d00-345f-47fb-bee3-6bcf7d3339e2"});
        // Map hit renders the caller value; null FK renders {}.
        let input = LinkShowInput {
            row: &row,
            fields: None,
            expand: &["issue", "created_by"],
            expansions: &[("issue", Some(issue_lite.clone())), ("created_by", None)],
        };
        let out = render_link(&input).expect("renders");
        assert_eq!(out["issue"], issue_lite);
        assert_eq!(out["created_by"], json!({}));
        // Non-map kept field renders null.
        let input = LinkShowInput {
            row: &row,
            fields: None,
            expand: &["url", "metadata"],
            expansions: &[],
        };
        let out = render_link(&input).expect("renders");
        assert_eq!(out["url"], Value::Null);
        assert_eq!(out["metadata"], Value::Null);
        // Expand outside the kept fields is skipped (no error even without
        // a caller value).
        let specs = includes(&["id"]);
        let input = LinkShowInput {
            row: &row,
            fields: Some(&specs),
            expand: &["issue"],
            expansions: &[],
        };
        let out = render_link(&input).expect("renders");
        assert_eq!(out_keys(&out), vec!["id"]);
        // Missing caller value for a kept map-hit errors.
        let input = LinkShowInput {
            row: &row,
            fields: None,
            expand: &["issue"],
            expansions: &[],
        };
        assert_eq!(
            render_link(&input),
            Err(LinkShowError::MissingExpansion("issue".to_string()))
        );
    }

    #[test]
    fn pr_and_review_fields_expand_smoke() {
        let fx = fixture(F18_02);
        let render = &unit(&fx, "GithubPullRequestLinkSerializer")["render"];
        let row = PrLinkRow {
            id: render["id"].as_str().expect("id"),
            issue: render["issue"].as_str().expect("issue"),
            repo_owner: "o",
            repo_name: "r",
            pr_number: 7,
            url: render["url"].as_str().expect("url"),
            title: "t",
            state: "open",
            merged: false,
            draft: false,
            pr_updated_at: None,
            created_at: "c",
            updated_at: "u",
            created_by: None,
        };
        let specs = includes(&["created_by", "id"]);
        let input = PrLinkShowInput {
            row: &row,
            fields: Some(&specs),
            expand: &["created_by"],
            expansions: &[("created_by", None)],
        };
        let out = render_pr_link(&input).expect("renders");
        assert_eq!(out_keys(&out), vec!["id", "created_by"]);
        assert_eq!(out["created_by"], json!({}));

        let render = &unit(&fx, "GitCodeReviewLinkSerializer")["render"];
        let metadata = json!({});
        let row = ReviewLinkRow {
            id: render["id"].as_str().expect("id"),
            issue: render["issue"].as_str().expect("issue"),
            provider: "github",
            host_url: "https://github.com",
            namespace: "n",
            repo_name: "r",
            repo_external_id: "",
            external_id: "e",
            external_iid: "7",
            url: render["url"].as_str().expect("url"),
            title: "t",
            state: "open",
            merged: true,
            draft: false,
            remote_updated_at: None,
            metadata: &metadata,
            created_at: "c",
            updated_at: "u",
            created_by: Some(UID),
        };
        // external_id is NOT a map name (no <name>_id attr) → null.
        let input = ReviewLinkShowInput {
            row: &row,
            fields: None,
            expand: &["external_id", "issue"],
            expansions: &[("issue", Some(json!({"id": 1})))],
        };
        let out = render_review_link(&input).expect("renders");
        assert_eq!(out["external_id"], Value::Null);
        assert_eq!(out["issue"], json!({"id": 1}));
    }

    #[test]
    fn link_render_wire_bytes_exact() {
        // Byte-identical wire: key order + escaper (ensure_ascii=False —
        // unicode literal, short escapes) in one exact string.
        let metadata = json!({"k": "v"});
        let row = LinkRow {
            id: UID,
            created_at: "2026-10-02T23:23:54.086909Z",
            updated_at: "2026-10-02T23:23:54.086931Z",
            deleted_at: None,
            title: Some("a\"b\\c\u{fc}d"),
            url: URL,
            metadata: &metadata,
            created_by: None,
            updated_by: Some(UID),
            project: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
            workspace: "92989e99-51c6-4725-8069-45784951694f",
            issue: "a7509d00-345f-47fb-bee3-6bcf7d3339e2",
        };
        let input = LinkShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        };
        let out = render_link(&input).expect("renders");
        let body = serde_json::to_string(&out).expect("serializes");
        assert_eq!(
            body,
            r#"{"id":"25ace52e-c64d-4043-a700-911b1e42bffc","created_at":"2026-10-02T23:23:54.086909Z","updated_at":"2026-10-02T23:23:54.086931Z","deleted_at":null,"title":"a\"b\\cüd","url":"https://example.com/spec","metadata":{"k":"v"},"created_by":null,"updated_by":"25ace52e-c64d-4043-a700-911b1e42bffc","project":"d715be3d-234f-46ef-89a3-97f0c7c04b7e","workspace":"92989e99-51c6-4725-8069-45784951694f","issue":"a7509d00-345f-47fb-bee3-6bcf7d3339e2"}"#
        );
    }
}
