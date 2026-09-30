//! Sticky serializer `validate()` kernel plus content-validator transcription
//! (D-21 serializers, PIDASHCONV-392).
//!
//! Ports `apps/api/pi_dash/api/serializers/sticky.py:12-34`
//! (`StickySerializer`: `Meta.fields = "__all__"`, read-only
//! `workspace`/`owner`, `name` not required, and `validate()`), transcribing
//! the exact accept/reject semantics of `validate_html_content` and
//! `validate_binary_data` from `apps/api/pi_dash/utils/content_validator.py`.
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-ser-sticky.json` (`fx-ser-sticky`).
//!
//! DRF envelope note: `validate()` raises
//! `ValidationError({"error": "..."})` with a bare-string value, but
//! `Serializer.run_validation` funnels it through `as_serializer_error`
//! (DRF 3.15.2 `serializers.py`), which wraps non-list dict values in lists.
//! The wire bodies are therefore `{"error": ["html content is not valid"]}`
//! and `{"description_binary": ["Invalid binary data"]}` — verified by
//! executing the live serializer.
//!
//! Ported bugs / divergences (translate, don't redesign; listed for the PR):
//!
//! * DEAD-CODE binary branch (`sticky.py:29-32`): `description_binary` is a
//!   `BinaryField`, which DRF maps to `ModelField(read_only=True)` — it is
//!   not writable, so validated data never carries the key and the binary
//!   check never runs. The utility itself ([`validate_binary_b64`] /
//!   [`validate_binary_bytes`]) is transcribed exactly and tested directly.
//! * DIVERGENCE read-only writes: `fx-ser-sticky` records writing `workspace`
//!   as 400 `"This field is read-only."`. Live DRF silently drops read-only
//!   input (`to_internal_value` iterates `_writable_fields`), so writes are
//!   ignored and validation succeeds. [`strip_ignored_sticky_keys`] models
//!   that projection.
//! * DIVERGENCE error envelopes: the fixture records bare-string bodies;
//!   live DRF list-wraps them (see above). This kernel emits the live shape.
//!
//! The ammonia tag/attribute/scheme tables below duplicate
//! `rust-api/crates/api/src/space/sanitize.rs` (same Python source lines):
//! the crate graph (`types → … → api`) forbids sharing the helper, and the
//! configuration is data, not logic.

use serde_json::{Map, Value};

/// `StickySerializer.Meta.read_only_fields` (`serializers/sticky.py:16`).
pub const STICKY_READ_ONLY_FIELDS: &[&str] = &["workspace", "owner"];

/// `StickySerializer.Meta.extra_kwargs` (`serializers/sticky.py:17`): `name`
/// is not required (absent key validates; the model default/null applies on
/// save, which the models layer owns).
pub const STICKY_NAME_REQUIRED: bool = false;

/// Every serializer field ignored on write, as observed live: the declared
/// read-only pair plus `id` / `created_at` / `updated_at` (DRF read-only
/// auto-fields) plus `description_binary` (`BinaryField` →
/// `ModelField(read_only=True)`).
pub const STICKY_INPUT_IGNORED_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "description_binary",
    "workspace",
    "owner",
];

/// `validate()` HTML rejection message (`serializers/sticky.py:24`).
pub const HTML_INVALID_MESSAGE: &str = "html content is not valid";
/// `validate()` binary rejection message (`serializers/sticky.py:32`; the
/// validator's own message is discarded and replaced with this literal).
pub const BINARY_INVALID_MESSAGE: &str = "Invalid binary data";

/// Live wire body for rejected HTML (list-wrapped by `as_serializer_error`).
pub fn html_invalid_body() -> Value {
    serde_json::json!({"error": [HTML_INVALID_MESSAGE]})
}

/// Live wire body for rejected binary (list-wrapped by `as_serializer_error`).
pub fn binary_invalid_body() -> Value {
    serde_json::json!({"description_binary": [BINARY_INVALID_MESSAGE]})
}

/// 10MB input cap (`content_validator.py:16,220`; `str::len` is byte length,
/// exactly like `len(html.encode("utf-8"))`).
pub const MAX_HTML_BYTES: usize = 10 * 1024 * 1024;

/// 10MB cap for binary data (`content_validator.py:16`).
pub const MAX_BINARY_BYTES: usize = 10 * 1024 * 1024;

/// Minimum decoded length (`content_validator.py:61-62`).
pub const MIN_BINARY_BYTES: usize = 4;

/// Suspicious text patterns (`content_validator.py:19-26`).
pub const SUSPICIOUS_BINARY_PATTERNS: &[&str] = &[
    "<html",
    "<!doctype",
    "<script",
    "javascript:",
    "data:",
    "<iframe",
];

/// Outcome of [`sanitize_html`], mirroring `validate_html_content`'s
/// `(is_valid, _, clean_html)` triple for truthy input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SanitizeOutcome {
    /// Clean HTML to store (`nh3.clean` output, replaces the input).
    Clean(String),
    /// Input exceeds [`MAX_HTML_BYTES`] or the cleaner failed: the serializer
    /// answers `{"error": ["html content is not valid"]}`.
    Invalid,
}

/// Rejection reason from [`validate_binary_bytes`] / [`validate_binary_b64`],
/// mirroring `validate_binary_data`'s messages verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryError {
    /// `b64decode` raised (`"Invalid base64 encoding"`).
    Base64,
    /// Decoded length exceeds 10MB.
    TooLarge,
    /// Decoded length below 4 bytes.
    TooShort,
    /// First 200 chars decode as text holding a suspicious pattern.
    Suspicious,
}

impl BinaryError {
    /// The exact `validate_binary_data` message for this reason.
    pub fn message(self) -> &'static str {
        match self {
            BinaryError::Base64 => "Invalid base64 encoding",
            BinaryError::TooLarge => "Binary data exceeds maximum size limit (10MB)",
            BinaryError::TooShort => "Binary data too short to be valid document format",
            BinaryError::Suspicious => "Binary data contains suspicious content patterns",
        }
    }
}

/// `validate_html_content` (`content_validator.py:211-247`) for truthy input.
/// Falsy input never reaches here (the serializer guards on truthiness; the
/// `(True, None, None)` arm is therefore unneeded). The removals-diff log
/// (`:231-239`) is Sentry-only observability with no wire effect.
///
/// `nh3` is Python bindings for the [`ammonia`] crate, so the same
/// configuration renders the same bytes; `nh3.clean` raising on pathological
/// input maps to [`SanitizeOutcome::Invalid`], like the `except` at `:242-247`.
pub fn sanitize_html(html: &str) -> SanitizeOutcome {
    if html.len() > MAX_HTML_BYTES {
        return SanitizeOutcome::Invalid;
    }
    let builder = ammonia_builder();
    let clean = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        builder.clean(html).to_string()
    }));
    match clean {
        Ok(clean) => SanitizeOutcome::Clean(clean),
        Err(_) => SanitizeOutcome::Invalid,
    }
}

/// Decode like `bytes.decode("utf-8", errors="ignore")`: invalid sequences are
/// skipped (not replaced with U+FFFD, unlike `String::from_utf8_lossy`).
fn decode_ignore_invalid(data: &[u8]) -> String {
    let mut out = String::new();
    let mut rest = data;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                out.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if let Ok(prefix) = std::str::from_utf8(&rest[..valid_up_to]) {
                    out.push_str(prefix);
                }
                let skip = error.error_len().unwrap_or(rest.len() - valid_up_to);
                rest = &rest[valid_up_to + skip..];
            }
        }
    }
    out
}

/// `validate_binary_data` over already-decoded bytes
/// (`content_validator.py:33-76`): size caps, 4-byte minimum, then the
/// suspicious-pattern scan over the first 200 characters (Python slicing
/// counts code points) of the lossy-decoded prefix, lowercased. Empty input
/// is valid (`if not data`, `:44`).
pub fn validate_binary_bytes(data: &[u8]) -> Result<(), BinaryError> {
    if data.is_empty() {
        return Ok(());
    }
    check_decoded_bytes(data)
}

/// Size and pattern checks over decoded bytes. Unlike
/// [`validate_binary_bytes`], empty input is checked, not short-circuited:
/// Python's falsy early return applies to the *input* (`if not data`), so a
/// truthy base64 string decoding to zero bytes still hits the 4-byte minimum
/// (verified live: `"!!!!"` → `"Binary data too short …"`).
fn check_decoded_bytes(data: &[u8]) -> Result<(), BinaryError> {
    if data.len() > MAX_BINARY_BYTES {
        return Err(BinaryError::TooLarge);
    }
    if data.len() < MIN_BINARY_BYTES {
        return Err(BinaryError::TooShort);
    }
    let prefix: String = decode_ignore_invalid(data).chars().take(200).collect();
    let lowered = prefix.to_lowercase();
    if SUSPICIOUS_BINARY_PATTERNS
        .iter()
        .any(|pattern| lowered.contains(pattern))
    {
        return Err(BinaryError::Suspicious);
    }
    Ok(())
}

/// `validate_binary_data` over a base64 string (`content_validator.py:47-52`):
/// Python's `b64decode` with `validate=False` discards non-alphabet
/// characters before the padding check, so the input is filtered to the
/// standard alphabet first; any decode failure is `"Invalid base64 encoding"`.
/// Empty input is valid (the falsy early return).
pub fn validate_binary_b64(data: &str) -> Result<(), BinaryError> {
    if data.is_empty() {
        return Ok(());
    }
    use base64::Engine as _;
    let filtered: String = data
        .bytes()
        .filter(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/' || *b == b'=')
        .map(|b| b as char)
        .collect();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(filtered)
        .map_err(|_| BinaryError::Base64)?;
    check_decoded_bytes(&decoded)
}

/// `StickySerializer.validate()` (`serializers/sticky.py:19-34`) over
/// post-field-validation values: `None` means the key is absent (or was
/// falsy, which the truthiness guards skip), `Some` carries the value.
/// Sanitized HTML replaces the input when cleaning succeeds.
///
/// The `binary` arm transcribes `:29-32` exactly, but it is unreachable
/// through the serializer (see the module docs): callers pass `None` because
/// the read-only `ModelField` drops the key before `validate()` runs.
pub fn validate_sticky_descriptions(
    html: Option<&str>,
    binary: Option<&str>,
) -> Result<Option<String>, StickyValidateError> {
    let mut sanitized: Option<String> = None;
    if let Some(text) = html {
        if !text.is_empty() {
            match sanitize_html(text) {
                SanitizeOutcome::Clean(clean) => sanitized = Some(clean),
                SanitizeOutcome::Invalid => return Err(StickyValidateError::HtmlInvalid),
            }
        }
    }
    if let Some(blob) = binary {
        if !blob.is_empty() && validate_binary_b64(blob).is_err() {
            return Err(StickyValidateError::BinaryInvalid);
        }
    }
    Ok(sanitized.or_else(|| html.map(str::to_owned)))
}

/// Which `validate()` arm failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickyValidateError {
    /// `{"error": ["html content is not valid"]}`.
    HtmlInvalid,
    /// `{"description_binary": ["Invalid binary data"]}`.
    BinaryInvalid,
}

impl StickyValidateError {
    /// The live wire body for this failure.
    pub fn body(self) -> Value {
        match self {
            StickyValidateError::HtmlInvalid => html_invalid_body(),
            StickyValidateError::BinaryInvalid => binary_invalid_body(),
        }
    }
}

/// The writable-fields projection of `to_internal_value`: read-only keys are
/// silently dropped (verified live — no `"This field is read-only."` error
/// exists in DRF 3.15 for input); every other key passes through untouched.
/// Full per-field validation of the surviving keys belongs to the handler
/// layer; [`validate_sticky_descriptions`] applies the `validate()` step.
pub fn strip_ignored_sticky_keys(input: &Map<String, Value>) -> Map<String, Value> {
    input
        .iter()
        .filter(|(key, _)| !STICKY_INPUT_IGNORED_FIELDS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// The `nh3.clean(html, tags=ALLOWED_TAGS, attributes=ATTRIBUTES,
/// url_schemes=SAFE_PROTOCOLS)` configuration
/// (`content_validator.py:72-159,223-229`): tags are `nh3.ALLOWED_TAGS`
/// (ammonia's default set, dumped from nh3 0.2.18) plus the four editor
/// components; attributes are the module dict verbatim (`*` = generic);
/// schemes are `{"http", "https", "mailto", "tel"}`; everything else is the
/// ammonia default, which is nh3's default too.
fn ammonia_builder() -> ammonia::Builder<'static> {
    let mut builder = ammonia::Builder::default();
    builder.tags(tag_set());
    builder.generic_attributes(generic_attributes());
    for (tag, attrs) in tag_attributes() {
        builder.add_tag_attributes(tag, attrs);
    }
    builder.url_schemes(url_schemes());
    builder
}

fn tag_set() -> std::collections::HashSet<&'static str> {
    [
        // nh3.ALLOWED_TAGS (nh3 0.2.18; ammonia's default set).
        "a",
        "abbr",
        "acronym",
        "area",
        "article",
        "aside",
        "b",
        "bdi",
        "bdo",
        "blockquote",
        "br",
        "caption",
        "center",
        "cite",
        "code",
        "col",
        "colgroup",
        "data",
        "dd",
        "del",
        "details",
        "dfn",
        "div",
        "dl",
        "dt",
        "em",
        "figcaption",
        "figure",
        "footer",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "header",
        "hgroup",
        "hr",
        "i",
        "img",
        "ins",
        "kbd",
        "li",
        "map",
        "mark",
        "nav",
        "ol",
        "p",
        "pre",
        "q",
        "rp",
        "rt",
        "rtc",
        "ruby",
        "s",
        "samp",
        "small",
        "span",
        "strike",
        "strong",
        "sub",
        "summary",
        "sup",
        "table",
        "tbody",
        "td",
        "th",
        "thead",
        "time",
        "tr",
        "tt",
        "u",
        "ul",
        "var",
        "wbr",
        // CUSTOM_TAGS (content_validator.py:72-78).
        "mention-component",
        "label",
        "input",
        "image-component",
    ]
    .into_iter()
    .collect()
}

fn generic_attributes() -> std::collections::HashSet<&'static str> {
    // ATTRIBUTES["*"] (content_validator.py:83-107).
    [
        "class",
        "id",
        "title",
        "role",
        "aria-label",
        "aria-hidden",
        "style",
        "start",
        "type",
        "xmlns",
        "data-tight",
        "data-node-type",
        "data-type",
        "data-checked",
        "data-background-color",
        "data-text-color",
        "data-name",
        "data-id",
        "data-icon-name",
        "data-icon-color",
        "data-background",
        "data-emoji-unicode",
        "data-emoji-url",
        "data-logo-in-use",
        "data-block-type",
    ]
    .into_iter()
    .collect()
}

fn tag_attributes(
) -> std::collections::HashMap<&'static str, std::collections::HashSet<&'static str>> {
    let mut map: std::collections::HashMap<&'static str, std::collections::HashSet<&'static str>> =
        std::collections::HashMap::new();
    let mut add = |tag: &'static str, attrs: &[&'static str]| {
        map.insert(tag, attrs.iter().copied().collect());
    };
    add("a", &["href", "target"]);
    add(
        "image-component",
        &[
            "id",
            "width",
            "height",
            "aspectRatio",
            "aspectratio",
            "src",
            "alignment",
            "status",
        ],
    );
    add(
        "img",
        &[
            "width",
            "height",
            "aspectRatio",
            "aspectratio",
            "alignment",
            "src",
            "alt",
            "title",
        ],
    );
    add(
        "mention-component",
        &["id", "entity_identifier", "entity_name"],
    );
    add(
        "th",
        &["colspan", "rowspan", "colwidth", "background", "style"],
    );
    add(
        "td",
        &[
            "colspan",
            "rowspan",
            "colwidth",
            "background",
            "textColor",
            "textcolor",
            "style",
        ],
    );
    add("tr", &["background", "textColor", "textcolor", "style"]);
    add("pre", &["language"]);
    add("code", &["language", "spellcheck"]);
    add("input", &["type", "checked"]);
    map
}

fn url_schemes() -> std::collections::HashSet<&'static str> {
    ["http", "https", "mailto", "tel"].into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static FIXTURE: &str = include_str!("../../../../fixtures/v1_assets/fx-ser-sticky.json");

    #[test]
    fn fixture_meta_matches_serializer() {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let meta = &fixture["meta"];
        assert_eq!(meta["fields"], json!("__all__"));
        assert_eq!(meta["model"], json!("Sticky"));
        let read_only: Vec<&str> = meta["read_only_fields"]
            .as_array()
            .expect("read_only_fields")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        assert_eq!(read_only, STICKY_READ_ONLY_FIELDS);
        assert_eq!(meta["extra_kwargs"]["name"]["required"], json!(false));
        assert_eq!(
            STICKY_NAME_REQUIRED,
            meta["extra_kwargs"]["name"]["required"]
                .as_bool()
                .expect("bool")
        );
    }

    #[test]
    fn html_goldens_sanitize_and_pass_through() {
        // `{"description_html": "<p>hello</p>"}` validates with the input
        // carried through (nh3 normalization is identity here).
        assert_eq!(
            validate_sticky_descriptions(Some("<p>hello</p>"), None),
            Ok(Some("<p>hello</p>".to_owned()))
        );
        // Embedded script is stripped and the sanitized value replaces the input.
        assert_eq!(
            validate_sticky_descriptions(Some("<p>hello</p><script>evil()</script>"), None),
            Ok(Some("<p>hello</p>".to_owned()))
        );
        // Falsy inputs skip validation and pass through unchanged.
        assert_eq!(
            validate_sticky_descriptions(Some(""), None),
            Ok(Some(String::new()))
        );
        assert_eq!(validate_sticky_descriptions(None, None), Ok(None));
    }

    #[test]
    fn sanitize_vectors_match_live_nh3() {
        // Each pair is `(input, clean_html)` captured by executing
        // `validate_html_content` (nh3 0.2.18) under `pi_dash.settings.test`.
        let vectors = [
            ("<p>hello</p>", "<p>hello</p>"),
            ("<p>hello</p><script>evil()</script>", "<p>hello</p>"),
            ("<p onclick=\"x()\">t</p>", "<p>t</p>"),
            (
                "<a href=\"javascript:alert(1)\">x</a>",
                "<a rel=\"noopener noreferrer\">x</a>",
            ),
            (
                "<a href=\"https://e.com\" target=\"_blank\">x</a>",
                "<a href=\"https://e.com\" target=\"_blank\" rel=\"noopener noreferrer\">x</a>",
            ),
            (
                "<mention-component id=\"1\">@a</mention-component>",
                "<mention-component id=\"1\">@a</mention-component>",
            ),
            ("<p>x</p><p>x</p><p>x</p>", "<p>x</p><p>x</p><p>x</p>"),
        ];
        for (input, expected) in vectors {
            assert_eq!(
                sanitize_html(input),
                SanitizeOutcome::Clean(expected.to_owned()),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn html_too_large_is_invalid_with_live_body() {
        let big = "x".repeat(MAX_HTML_BYTES + 1);
        assert_eq!(sanitize_html(&big), SanitizeOutcome::Invalid);
        assert_eq!(
            validate_sticky_descriptions(Some(&big), None),
            Err(StickyValidateError::HtmlInvalid)
        );
        assert_eq!(
            StickyValidateError::HtmlInvalid.body(),
            json!({"error": ["html content is not valid"]})
        );
        assert_eq!(
            serde_json::to_string(&StickyValidateError::HtmlInvalid.body()).unwrap(),
            r#"{"error":["html content is not valid"]}"#
        );
    }

    #[test]
    fn binary_vectors_match_live_validator() {
        // `(input, expected_message)` captured by executing
        // `validate_binary_data` live; `Ok` entries are `(input, None)`.
        assert!(validate_binary_b64("").is_ok());
        assert!(validate_binary_b64("aGVsbG8=").is_ok());
        assert!(validate_binary_b64("aGVsbG8gd29ybGQh").is_ok());
        assert_eq!(
            validate_binary_b64("not-base64!!!___")
                .unwrap_err()
                .message(),
            "Invalid base64 encoding"
        );
        assert_eq!(
            validate_binary_b64("aGk=").unwrap_err().message(),
            "Binary data too short to be valid document format"
        );
        assert_eq!(
            validate_binary_b64("!!!!").unwrap_err().message(),
            "Binary data too short to be valid document format"
        );
        assert_eq!(
            validate_binary_b64("PGh0bWw+").unwrap_err().message(),
            "Binary data contains suspicious content patterns"
        );
        assert_eq!(validate_binary_bytes(&[]), Ok(()));
        assert_eq!(
            validate_binary_bytes(&vec![b'A'; MAX_BINARY_BYTES + 1])
                .unwrap_err()
                .message(),
            "Binary data exceeds maximum size limit (10MB)"
        );
        // The serializer discards the validator message for this literal.
        assert_eq!(
            StickyValidateError::BinaryInvalid.body(),
            json!({"description_binary": ["Invalid binary data"]})
        );
    }

    #[test]
    fn read_only_and_binary_keys_are_dropped_live_behavior() {
        // `fx-ser-sticky` records writing `workspace` as a 400 and an invalid
        // `description_binary` as a 400. Live DRF drops both keys (read-only
        // fields are not writable) and validation succeeds; the projection
        // below pins that behavior. Fixture correction filed.
        let input = Map::from_iter([
            (
                "workspace".to_owned(),
                json!("11111111-1111-1111-1111-111111111111"),
            ),
            ("owner".to_owned(), json!(1)),
            ("description_binary".to_owned(), json!("not-base64!!!___")),
            ("description_html".to_owned(), json!("<p>hello</p>")),
        ]);
        let projected = strip_ignored_sticky_keys(&input);
        assert_eq!(
            projected,
            Map::from_iter([("description_html".to_owned(), json!("<p>hello</p>"))])
        );
        // End to end over the projected input: valid, sanitized value kept.
        assert_eq!(
            validate_sticky_descriptions(
                projected.get("description_html").and_then(Value::as_str),
                projected.get("description_binary").and_then(Value::as_str),
            ),
            Ok(Some("<p>hello</p>".to_owned()))
        );
    }
}
