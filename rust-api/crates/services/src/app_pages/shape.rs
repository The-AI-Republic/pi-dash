//! Page serializer shapes (D-30, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/page.py:1-225`
//! (drift baseline `01a93e17`):
//! - `PageSerializer` (`:25-59`): the 19-field list, the read-only
//!   `is_favorite`, the write-only `labels` PK-related list, the
//!   `label_ids` / `project_ids` UUID lists, `read_only_fields`.
//! - `PageSerializer.create` (`:61-106`): workspace from the project,
//!   `description_*` from the serializer context, the `ProjectPage` row,
//!   and the bulk `PageLabel` write (`batch_size=10`).
//! - `PageSerializer.update` (`:108-126`): label wipe + bulk recreate,
//!   everything else via `super().update`.
//! - `PageDetailSerializer` (`:129-133`): base fields plus a bare
//!   `description_html` `CharField`.
//! - `PageVersionSerializer` (`:136-150`) and
//!   `PageVersionDetailSerializer` (`:153-170`): field lists with
//!   `read_only_fields = ["workspace", "page"]`.
//! - `PageBinaryUpdateSerializer` (`:173-225`): base64→bytes validation,
//!   HTML sanitize substitution, and partial field application.
//!
//! Fixture oracles: F30-01
//! (`rust-api/fixtures/app_pages/serializers/page_serializers.golden.json`),
//! F30-02 (`.../page_binary_update.golden.json`), F30-03
//! (`.../page_version_shapes.golden.json`); the unit tests below replay
//! those goldens byte-identically (field order, error strings, bodies).
//!
//! Out of scope: the actual `nh3.clean` HTML sanitization (a handler-layer
//! concern — this module ports the substitution rule and the error bodies,
//! not the cleaner), and the `~Q(projects__id=True)` `project_ids`
//! annotation no-op (owned by the queries layer; noted below).
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. `label_ids` / `project_ids` carry no `read_only` flag (`:33-34`): on
//!    read they render the annotated `Coalesce-ArrayAgg`; if supplied on
//!    create they land in `validated_data` and
//!    `Page.objects.create(**validated_data)` raises `TypeError`.
//! 2. `update()` pops `labels` then calls `super().update` with the
//!    remaining keys only (`:108-126`) — labels never reach the model.
//! 3. `create()` reads `description_*` from the serializer context, not
//!    from `validated_data` (`:63-67`).
//! 4. `PageDetailSerializer.description_html` is a bare required
//!    `CharField` with no sanitization (`:130`).

use base64::Engine as _;

/// `PageSerializer.Meta.fields`, in source order
/// (`app/serializers/page.py:38-58`).
pub const PAGE_SERIALIZER_FIELDS: &[&str] = &[
    "id",
    "name",
    "owned_by",
    "access",
    "color",
    "labels",
    "parent",
    "is_favorite",
    "is_locked",
    "archived_at",
    "workspace",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "view_props",
    "logo_props",
    "label_ids",
    "project_ids",
];

/// `PageSerializer.Meta.read_only_fields` (`page.py:59`).
pub const PAGE_READ_ONLY_FIELDS: &[&str] = &["workspace", "owned_by"];

/// `BaseSerializer` renders `id` as a read-only `PrimaryKeyRelatedField`
/// (`app/serializers/base.py:8-10`).
pub const ID_FIELD: &str = "id";

/// `is_favorite`: `BooleanField(read_only=True)` (`page.py:26`) —
/// annotated `Exists` in `get_queryset`, never written.
pub const IS_FAVORITE_FIELD: &str = "is_favorite";

/// `labels`: `ListField(PrimaryKeyRelatedField(queryset=Label.objects.all()))`,
/// `write_only`, `required=False` (`page.py:27-31`) — accepted PKs,
/// never rendered.
pub const LABELS_WRITE_ONLY_FIELD: &str = "labels";

/// `label_ids`: `ListField(UUIDField(), required=False)` (`page.py:33`) —
/// NO `read_only` flag (see bug 1 in the module docs).
pub const LABEL_IDS_FIELD: &str = "label_ids";

/// `project_ids`: `ListField(UUIDField(), required=False)` (`page.py:34`) —
/// same unguarded contract as `label_ids`. The `~Q(projects__id=True)`
/// UUID-vs-bool annotation no-op that feeds this field on read lives in
/// the queries layer (`app/views/page/base.py`), not here.
pub const PROJECT_IDS_FIELD: &str = "project_ids";

/// `PageDetailSerializer.Meta.fields`: base fields plus `description_html`
/// (`page.py:129-133`).
pub fn page_detail_fields() -> Vec<&'static str> {
    let mut fields = PAGE_SERIALIZER_FIELDS.to_vec();
    fields.push("description_html");
    fields
}

/// `PageVersionSerializer.Meta.fields`, in source order (`page.py:139-149`).
pub const PAGE_VERSION_FIELDS: &[&str] = &[
    "id",
    "workspace",
    "page",
    "last_saved_at",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
];

/// `PageVersionDetailSerializer.Meta.fields`, in source order
/// (`page.py:156-169`): the list shape plus `description_binary`,
/// `description_html`, `description_json` after `last_saved_at`.
pub const PAGE_VERSION_DETAIL_FIELDS: &[&str] = &[
    "id",
    "workspace",
    "page",
    "last_saved_at",
    "description_binary",
    "description_html",
    "description_json",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
];

/// `read_only_fields` shared by both version serializers
/// (`page.py:150,170`).
pub const PAGE_VERSION_READ_ONLY_FIELDS: &[&str] = &["workspace", "page"];

/// `PageBinaryUpdateSerializer` writable fields, in source order
/// (`page.py:176-178`): `description_binary` / `description_html` are
/// `CharField(required=False, allow_blank=True)`, `description_json` is
/// `JSONField(required=False, allow_null=True)`.
pub const BINARY_UPDATE_FIELDS: &[&str] =
    &["description_binary", "description_html", "description_json"];

/// `PageLabel.objects.bulk_create(..., batch_size=10)` on both the create
/// (`page.py:104`) and update (`page.py:123`) paths.
pub const PAGE_LABEL_BULK_BATCH_SIZE: usize = 10;

/// Serializer-context keys `PageSerializer.create` reads (`page.py:63-67`):
/// the workspace comes from the project, `owned_by_id` and the three
/// `description_*` values from the context — never from `validated_data`.
pub const PAGE_CREATE_CONTEXT_KEYS: &[&str] = &[
    "project_id",
    "owned_by_id",
    "description_json",
    "description_binary",
    "description_html",
];

/// Columns of the `Page` row created at `page.py:73-80`
/// (F30-01 `pages` table).
pub const PAGE_CREATE_COLUMNS: &[&str] = &[
    "access",
    "color",
    "created_by_id",
    "description_binary",
    "description_html",
    "description_json",
    "name",
    "owned_by_id",
    "parent_id",
    "updated_by_id",
    "workspace_id",
];

/// Columns of the `ProjectPage` row created at `page.py:83-89`
/// (F30-01 `project_pages` table; `created_by_id` / `updated_by_id` are
/// copied from the new page).
pub const PROJECT_PAGE_ROW_COLUMNS: &[&str] = &[
    "created_by_id",
    "page_id",
    "project_id",
    "updated_by_id",
    "workspace_id",
];

/// Columns of each `PageLabel` row bulk-created at `page.py:93-105`
/// (F30-01 `page_labels` table; `created_by_id` / `updated_by_id` are
/// copied from the page).
pub const PAGE_LABEL_ROW_COLUMNS: &[&str] = &["label_id", "page_id", "workspace_id"];

/// `validate_binary_data` size ceiling
/// (`pi_dash/utils/content_validator.py:16`): 10MB.
pub const BINARY_MAX_SIZE: usize = 10 * 1024 * 1024;

/// `SUSPICIOUS_BINARY_PATTERNS` (`content_validator.py:19-26`), matched
/// case-insensitively against the first 200 chars of the UTF-8-lossy
/// decode (`:61-64`).
pub const SUSPICIOUS_BINARY_PATTERNS: &[&str] = &[
    "<html",
    "<!doctype",
    "<script",
    "javascript:",
    "data:",
    "<iframe",
];

/// `validate_binary_data` failure messages (`content_validator.py:48-64`).
pub const BINARY_BASE64_MESSAGE: &str = "Invalid base64 encoding";
pub const BINARY_TOO_LARGE_MESSAGE: &str = "Binary data exceeds maximum size limit (10MB)";
pub const BINARY_TOO_SHORT_MESSAGE: &str = "Binary data too short to be valid document format";
pub const BINARY_SUSPICIOUS_MESSAGE: &str = "Binary data contains suspicious content patterns";

/// `PageBinaryUpdateSerializer` error strings (`page.py:192,198`).
pub const INVALID_BINARY_PREFIX: &str = "Invalid binary data: ";
pub const DECODE_FAILED_MESSAGE: &str = "Failed to decode base64 data";

/// `validate_html_content` failure messages (`content_validator.py:221,243`).
pub const HTML_TOO_LARGE_MESSAGE: &str = "HTML content exceeds maximum size limit (10MB)";
pub const HTML_SANITIZE_FAILED_MESSAGE: &str = "Failed to sanitize HTML";

/// Mirrors Python's `if not value` guard on the `CharField` inputs
/// (`page.py:182,202`): the validators only ever see `str` (an absent key
/// skips validation, `None` is rejected by the field itself), so falsy
/// means the empty string, which passes through untouched.
pub fn is_blank(value: &str) -> bool {
    value.is_empty()
}

/// Mirrors `validate_binary_data` (`content_validator.py:29-68`) over
/// already-decoded bytes.
///
/// Returns `Ok(())` for empty input (`:40-41`); otherwise enforces the
/// 10MB ceiling (`:53-54`), the 4-byte format floor (`:57-58`), and the
/// case-insensitive suspicious-pattern scan over the first 200 chars of
/// the UTF-8-lossy decode (`:61-64`; `errors="ignore"` drops undecodable
/// bytes, `from_utf8_lossy` replaces them — both only affect whether a
/// suspicious ASCII pattern survives, and the patterns are pure ASCII).
pub fn validate_binary_data(data: &[u8]) -> Result<(), &'static str> {
    if data.is_empty() {
        return Ok(());
    }
    if data.len() > BINARY_MAX_SIZE {
        return Err(BINARY_TOO_LARGE_MESSAGE);
    }
    if data.len() < 4 {
        return Err(BINARY_TOO_SHORT_MESSAGE);
    }
    let head: String = String::from_utf8_lossy(data)
        .chars()
        .take(200)
        .collect::<String>()
        .to_lowercase();
    if SUSPICIOUS_BINARY_PATTERNS
        .iter()
        .any(|pattern| head.contains(pattern))
    {
        return Err(BINARY_SUSPICIOUS_MESSAGE);
    }
    Ok(())
}

/// Mirrors `validate_binary_data` called with a `str`
/// (`content_validator.py:44-50`, falling through to the shared checks at
/// `:53-68`): a base64-encoded string is decoded first — undecodable input
/// reports `"Invalid base64 encoding"` (`:48`) — then the decoded bytes run
/// the same size/floor/pattern checks as the bytes path.
/// (Unreachable via the serializer — there the value is already decoded
/// bytes — but kept for direct-call parity.)
pub fn validate_binary_text(text: &str) -> Result<Vec<u8>, &'static str> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let binary_data = decode_base64_lenient(text).map_err(|_| BINARY_BASE64_MESSAGE)?;
    validate_binary_data(&binary_data)?;
    Ok(binary_data)
}

/// Decodes with CPython `base64.b64decode(value)` semantics
/// (`page.py:187`): `validate=False`, so bytes outside the standard
/// alphabet are discarded before decoding rather than rejected. The
/// strict `STANDARD` engine runs over the filtered text, so lengths and
/// padding fail exactly where `binascii` fails.
pub fn decode_base64_lenient(value: &str) -> Result<Vec<u8>, String> {
    let filtered: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(filtered.as_bytes())
        .map_err(|err| err.to_string())
}

/// Outcome of `validate_description_binary` (`page.py:180-198`):
/// a falsy value passes through untouched (`:182-183`), otherwise the
/// caller receives the *decoded bytes* (`:194`), not the input string.
#[derive(Debug, PartialEq, Eq)]
pub enum BinaryFieldOutcome {
    Passthrough,
    Decoded(Vec<u8>),
}

/// Mirrors `validate_description_binary` (`page.py:180-198`).
///
/// `Err` carries the exact `ValidationError` message Django renders:
/// `"Invalid binary data: {validator message}"` (`:192`) for validator
/// failures, `"Failed to decode base64 data"` (`:198`) for anything else.
/// A `ValidationError` raised by the validator is re-raised as-is
/// (`:196-197`) — which is what the `map_err` below does by keeping the
/// validator's message under the prefix instead of replacing it.
pub fn validate_description_binary(value: &str) -> Result<BinaryFieldOutcome, String> {
    if is_blank(value) {
        return Ok(BinaryFieldOutcome::Passthrough);
    }
    let binary_data = decode_base64_lenient(value).map_err(|_| DECODE_FAILED_MESSAGE.to_owned())?;
    validate_binary_data(&binary_data)
        .map(|()| BinaryFieldOutcome::Decoded(binary_data))
        .map_err(|message| format!("{INVALID_BINARY_PREFIX}{message}"))
}

/// Mirrors the `validate_description_html` return rule (`page.py:200-211`):
/// after `validate_html_content` passes, the *sanitized* HTML replaces the
/// input when the cleaner produced one, otherwise the original is kept
/// (`:211`). The cleaner itself (`nh3.clean` over `ALLOWED_TAGS` /
/// `ATTRIBUTES` / `SAFE_PROTOCOLS`, `content_validator.py:224-240`) runs
/// in the handler layer; `sanitized` is its output (`None` when the input
/// was empty, per `:217`).
pub fn substitute_description_html<'a>(original: &'a str, sanitized: Option<&'a str>) -> &'a str {
    sanitized.unwrap_or(original)
}

/// Mirrors `PageBinaryUpdateSerializer.update` (`page.py:213-225`): each
/// key is applied independently iff present in `validated_data` — absent
/// keys are never touched, present keys overwrite even with empty values —
/// then `instance.save()` (which recomputes `description_stripped`; the
/// save itself belongs to the models layer).
///
/// Returns the applied field names in wire order.
pub fn applied_binary_fields(
    has_binary: bool,
    has_html: bool,
    has_json: bool,
) -> Vec<&'static str> {
    let mut applied = Vec::with_capacity(3);
    if has_binary {
        applied.push("description_binary");
    }
    if has_html {
        applied.push("description_html");
    }
    if has_json {
        applied.push("description_json");
    }
    applied
}

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

/// Renders one DRF field-`ValidationError` body byte-identically:
/// `{"<field>": ["<message>"]}` with status 400. Covers both
/// `validate_description_binary` messages (`page.py:192,198`) and the
/// `validate_description_html` message (`page.py:208`).
pub fn field_error_body(field: &str, message: &str) -> String {
    format!(
        "{{\"{}\":[\"{}\"]}}",
        escape_json_string(field),
        escape_json_string(message)
    )
}

/// The `description_binary` 400 body for `message` (`page.py:192,198`).
pub fn binary_error_body(message: &str) -> String {
    field_error_body("description_binary", message)
}

/// The `description_html` 400 body for `message` (`page.py:208`).
pub fn html_error_body(message: &str) -> String {
    field_error_body("description_html", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const F30_01: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/serializers/page_serializers.golden.json"
    );
    const F30_02: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/serializers/page_binary_update.golden.json"
    );
    const F30_03: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/serializers/page_version_shapes.golden.json"
    );

    fn golden(path: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(path).expect("fixture golden exists");
        serde_json::from_str(&raw).expect("fixture golden is valid JSON")
    }

    fn str_list(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_array()
            .expect("golden carries a string list")
            .iter()
            .map(|v| v.as_str().expect("field names are strings"))
            .collect()
    }

    #[test]
    fn page_serializer_fields_match_f30_01_in_order() {
        let parsed = golden(F30_01);
        let expected = str_list(&parsed["field_list"]);
        assert_eq!(PAGE_SERIALIZER_FIELDS.len(), 19);
        assert_eq!(PAGE_SERIALIZER_FIELDS, expected.as_slice());
    }

    #[test]
    fn read_only_guards_and_detail_shape_match_f30_01() {
        let parsed = golden(F30_01);
        let guards = str_list(&parsed["field_policy"]["read_only_fields"]);
        assert_eq!(PAGE_READ_ONLY_FIELDS, guards.as_slice());
        let mut expected = str_list(&parsed["field_list"]);
        expected.push("description_html");
        assert_eq!(page_detail_fields(), expected);
        assert_eq!(page_detail_fields().len(), 20);
    }

    #[test]
    fn create_row_contract_matches_f30_01() {
        let parsed = golden(F30_01);
        let rows = parsed["cases"][0]["create_db_rows"]
            .as_array()
            .expect("golden carries create_db_rows");
        let columns = |table: &str| {
            rows.iter()
                .find(|row| row["table"] == table)
                .map(|row| str_list(&row["columns"]))
                .expect("golden carries the table")
        };
        assert_eq!(PAGE_LABEL_BULK_BATCH_SIZE, 10);
        assert_eq!(PAGE_LABEL_ROW_COLUMNS, columns("page_labels").as_slice());
        assert_eq!(
            PROJECT_PAGE_ROW_COLUMNS,
            columns("project_pages").as_slice()
        );
        assert_eq!(PAGE_CREATE_COLUMNS, columns("pages").as_slice());
        assert_eq!(PAGE_CREATE_CONTEXT_KEYS.len(), 5);
        assert!(PAGE_CREATE_CONTEXT_KEYS.contains(&"project_id"));
    }

    #[test]
    fn version_shapes_match_f30_03() {
        let parsed = golden(F30_03);
        let cases = parsed["cases"].as_array().expect("golden carries cases");
        let list = str_list(&cases[0]["fields"]);
        let detail = str_list(&cases[1]["fields"]);
        assert_eq!(PAGE_VERSION_FIELDS, list.as_slice());
        assert_eq!(PAGE_VERSION_DETAIL_FIELDS, detail.as_slice());
        assert_eq!(
            PAGE_VERSION_DETAIL_FIELDS.len(),
            PAGE_VERSION_FIELDS.len() + 3
        );
        for case in cases {
            let guards = str_list(&case["read_only_fields"]);
            assert_eq!(PAGE_VERSION_READ_ONLY_FIELDS, guards.as_slice());
        }
    }

    #[test]
    fn binary_decode_ok_returns_bytes_not_string() {
        // F30-02 "base64 decode ok + binary validator pass": validate()
        // returns the DECODED bytes (page.py:194), not the string.
        let parsed = golden(F30_02);
        let input = parsed["cases"][0]["input"]["description_binary"]
            .as_str()
            .expect("golden carries the base64 input");
        match validate_description_binary(input).expect("valid input passes") {
            BinaryFieldOutcome::Decoded(bytes) => {
                assert_eq!(bytes, b"\x89PNG\r\n\x1a\n");
            }
            BinaryFieldOutcome::Passthrough => panic!("non-empty input must decode"),
        }
    }

    #[test]
    fn binary_error_strings_and_bodies_match_f30_02() {
        let parsed = golden(F30_02);
        let cases = parsed["cases"].as_array().expect("golden carries cases");
        // "base64 decode fail" (page.py:195-198: broad except).
        let fail_input = cases[1]["input"]["description_binary"]
            .as_str()
            .expect("golden carries the bad input");
        let fail_err = validate_description_binary(fail_input).expect_err("must fail");
        assert_eq!(fail_err, DECODE_FAILED_MESSAGE);
        assert_eq!(
            serde_json::to_string(&cases[1]["output"]["errors"]).expect("serializable"),
            binary_error_body(&fail_err)
        );
        assert_eq!(
            binary_error_body(&fail_err),
            r#"{"description_binary":["Failed to decode base64 data"]}"#
        );
        // "binary validator fail (suspicious content)" (page.py:190-192).
        let suspect_input = cases[2]["input"]["description_binary"]
            .as_str()
            .expect("golden carries the suspicious input");
        let suspect_err =
            validate_description_binary(suspect_input).expect_err("must fail validation");
        assert_eq!(
            suspect_err,
            "Invalid binary data: Binary data contains suspicious content patterns"
        );
        assert_eq!(
            serde_json::to_string(&cases[2]["output"]["errors"]).expect("serializable"),
            binary_error_body(&suspect_err)
        );
        // "short-but-suspicious binary fails validator".
        let short_input = cases[4]["input"]["description_binary"]
            .as_str()
            .expect("golden carries the short input");
        assert_eq!(
            validate_description_binary(short_input).expect_err("must fail"),
            suspect_err
        );
    }

    #[test]
    fn blank_inputs_pass_through_untouched() {
        // page.py:182-183 and :202-203 — falsy values skip validation.
        assert!(matches!(
            validate_description_binary(""),
            Ok(BinaryFieldOutcome::Passthrough)
        ));
        assert!(is_blank(""));
        assert!(!is_blank("x"));
    }

    #[test]
    fn html_substitution_rule_matches_f30_02() {
        // F30-02 "HTML sanitize substitution": the sanitized HTML replaces
        // the input (page.py:211); without one the original is kept.
        let parsed = golden(F30_02);
        let cases = parsed["cases"].as_array().expect("golden carries cases");
        let html_input = cases[5]["input"]["description_html"]
            .as_str()
            .expect("golden carries the html input");
        let sanitized = cases[5]["output"]["description_html_applied"]
            .as_str()
            .expect("golden carries the sanitized output");
        assert_eq!(
            substitute_description_html(html_input, Some(sanitized)),
            sanitized
        );
        assert_eq!(substitute_description_html(html_input, None), html_input);
        // Empty HTML passes through (golden renders it as "''").
        assert_eq!(cases[6]["output"]["description_html_applied"], "''");
    }

    #[test]
    fn partial_update_applies_present_keys_only() {
        // F30-02 "partial-update field application" (page.py:213-225):
        // absent keys are NOT touched; present keys overwrite.
        let parsed = golden(F30_02);
        let cases = parsed["cases"].as_array().expect("golden carries cases");
        let applied: Vec<&str> = cases[7]["output"]["applied"]
            .as_array()
            .expect("golden carries the applied list")
            .iter()
            .map(|v| v.as_str().expect("applied names are strings"))
            .collect();
        assert_eq!(applied_binary_fields(true, true, true), applied);
        assert_eq!(
            applied_binary_fields(true, true, true),
            BINARY_UPDATE_FIELDS
        );
        assert_eq!(
            applied_binary_fields(true, false, false),
            ["description_binary"]
        );
        assert_eq!(
            applied_binary_fields(false, false, false),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn validator_units_match_content_validator() {
        assert_eq!(validate_binary_data(&[]), Ok(()));
        assert_eq!(
            validate_binary_data(&[1, 2, 3]),
            Err(BINARY_TOO_SHORT_MESSAGE)
        );
        assert_eq!(
            validate_binary_data(b"<script>alert(1)</script>"),
            Err(BINARY_SUSPICIOUS_MESSAGE)
        );
        assert_eq!(validate_binary_data(b"\x89PNG\r\n\x1a\n"), Ok(()));
        // Case-insensitive scan: uppercase pattern still matches.
        assert_eq!(
            validate_binary_data(b"AB<JAVASCRIPT:xxxx>CD"),
            Err(BINARY_SUSPICIOUS_MESSAGE)
        );
        assert_eq!(
            validate_binary_text("!!!not-base64!!!"),
            Err(BINARY_BASE64_MESSAGE)
        );
        assert_eq!(
            validate_binary_text("iVBORw0KGgo="),
            Ok(b"\x89PNG\r\n\x1a\n".to_vec())
        );
        // str input falls through to the shared checks (content_validator.py:53-68).
        assert_eq!(validate_binary_text("YWI="), Err(BINARY_TOO_SHORT_MESSAGE));
    }
}
