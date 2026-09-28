//! HTML helpers for the space intake write paths (D-02, stage 4).
//!
//! Ports the two content functions `IssueCreateSerializer.validate`
//! applies to `description_html` (`space/serializer/issue.py:302-308` via
//! the app twin at `app/serializers/issue.py:326-332`, same body):
//!
//! * [`sanitize_html`] — `validate_html_content`
//!   (`utils/content_validator.py:211-247`): `nh3.clean` with the module's
//!   tags/attributes/schemes. `nh3` is Python bindings for the [`ammonia`]
//!   crate, so the same configuration renders the same bytes.
//! * [`strip_tags`] — Django's `django.utils.html.strip_tags`, which
//!   `Issue.save` uses for `description_stripped`
//!   (`db/models/issue.py:329-333,344-348`).
//!
//! `validate_binary_data` (`content_validator.py`) is unreachable on the
//! intake paths: the create/patch payloads never carry `description_binary`
//! (create ignores it, partial_update's 3-key subset drops it), so there is
//! nothing to port here.

use std::collections::{HashMap, HashSet};

/// 10MB input cap (`content_validator.py:16,220`).
pub const MAX_HTML_BYTES: usize = 10 * 1024 * 1024;

/// `description_html` fallback on intake create
/// (`views/intake.py:148`).
pub const DEFAULT_DESCRIPTION_HTML: &str = "<p></p>";

/// `description_json` fallback on intake create
/// (`views/intake.py:147`).
pub fn default_description_json() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

/// Outcome of [`sanitize_html`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sanitize {
    /// Clean HTML to store (`nh3.clean` output).
    Clean(String),
    /// Input exceeds [`MAX_HTML_BYTES`] or the cleaner failed: the
    /// serializer answers `{"error": ["html content is not valid"]}`.
    Invalid,
}

/// `validate_html_content` (`content_validator.py:211-247`).
///
/// Empty input never reaches here (the serializer only validates truthy
/// `description_html`); the `None` third-tuple arm is therefore unneeded.
/// The removals-diff log (`:231-239`, Sentry-only) has no wire effect.
pub fn sanitize_html(html: &str) -> Sanitize {
    if html.len() > MAX_HTML_BYTES {
        return Sanitize::Invalid;
    }
    let builder = ammonia_builder();
    // `nh3.clean` raises only on pathological input; map any failure to
    // the invalid arm exactly like the `except` at `:242-247`.
    let clean = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        builder.clean(html).to_string()
    }));
    match clean {
        Ok(clean) => Sanitize::Clean(clean),
        Err(_) => Sanitize::Invalid,
    }
}

/// The `nh3.clean(html, tags=ALLOWED_TAGS, attributes=ATTRIBUTES,
/// url_schemes=SAFE_PROTOCOLS)` configuration
/// (`content_validator.py:72-159,223-229`):
///
/// * tags — `nh3.ALLOWED_TAGS` (ammonia's default set, dumped from nh3
///   0.2.18) plus the four editor components.
/// * attributes — the module dict verbatim (`*` = generic attributes,
///   plus the per-tag sets).
/// * schemes — `{"http", "https", "mailto", "tel"}`.
/// * everything else — ammonia defaults, which are nh3's defaults too.
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

fn tag_set() -> HashSet<&'static str> {
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

fn generic_attributes() -> HashSet<&'static str> {
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

fn tag_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut map: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
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

fn url_schemes() -> HashSet<&'static str> {
    ["http", "https", "mailto", "tel"].into_iter().collect()
}

/// Django's `django.utils.html.strip_tags`.
///
/// Loop `_strip_once` while both `<` and `>` remain, stopping when an
/// iteration stops removing `<` (verbatim Django algorithm). `_strip_once`
/// feeds a lenient parser collecting text, `&name;`/`&#name;` references
/// verbatim, and skipping tags (quote-aware), comments and declarations.
pub fn strip_tags(value: &str) -> String {
    let mut current = value.to_owned();
    loop {
        if !(current.contains('<') && current.contains('>')) {
            break;
        }
        let next = strip_once(&current);
        if next.matches('<').count() == current.matches('<').count() {
            break;
        }
        current = next;
    }
    current
}

fn strip_once(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte != b'<' {
            // Copy one full char ('<' is ASCII, so byte detection never
            // splits a multi-byte sequence; entities pass through verbatim
            // like HTMLParser's handle_entityref/handle_charref arms).
            let ch = value[index..].chars().next().expect("char boundary");
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }
        // At '<': comments, declarations, and tags are skipped.
        if value[index..].starts_with("<!--") {
            if let Some(end) = value[index..].find("-->") {
                index += end + 3;
            } else {
                // Unterminated comment: HTMLParser bogus-comments to EOF.
                break;
            }
            continue;
        }
        if value[index..].starts_with("<!") || value[index..].starts_with("<?") {
            if let Some(end) = find_tag_end(&value[index..]) {
                index += end;
            } else {
                out.push('<');
                index += 1;
            }
            continue;
        }
        // A '<' followed by ASCII alpha, '/', '!', or '?' opens a tag;
        // anything else is literal text (HTMLParser parse-starttag rules).
        let rest = &value[index + 1..];
        let opener = rest.chars().next().unwrap_or('\0');
        if opener.is_ascii_alphabetic() || opener == '/' {
            if let Some(end) = find_tag_end(&value[index..]) {
                index += end;
                continue;
            }
        }
        out.push('<');
        index += 1;
    }
    out
}

/// Length in bytes of `<...>` starting at `text[0] == '<'`, respecting
/// single/double quotes; `None` when unterminated.
fn find_tag_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(q) = quote {
            if byte == q {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(index + 1);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_plain_editor_html() {
        for html in ["<p>why</p>", "<p>new</p>", "<p></p>", "plain"] {
            assert_eq!(
                sanitize_html(html),
                Sanitize::Clean(html.to_string()),
                "{html}"
            );
        }
    }

    #[test]
    fn sanitize_strips_script_and_event_handlers() {
        assert_eq!(
            sanitize_html("<p>x</p><script>alert(1)</script>"),
            Sanitize::Clean("<p>x</p>".to_string())
        );
        // `rel="noopener noreferrer"` is nh3/ammonia default `link_rel`
        // behavior (verified against live `nh3.clean`).
        assert_eq!(
            sanitize_html("<a href=\"javascript:evil()\">x</a>"),
            Sanitize::Clean("<a rel=\"noopener noreferrer\">x</a>".to_string())
        );
    }

    #[test]
    fn sanitize_rejects_oversize_input() {
        let big = "x".repeat(MAX_HTML_BYTES + 1);
        assert_eq!(sanitize_html(&big), Sanitize::Invalid);
    }

    #[test]
    fn strip_tags_matches_django_basics() {
        assert_eq!(strip_tags("<p>why</p>"), "why");
        assert_eq!(strip_tags("plain"), "plain");
        assert_eq!(strip_tags(""), "");
        assert_eq!(strip_tags("<p>a</p><p>b</p>"), "ab");
        // Entities pass through verbatim (handle_entityref arm).
        assert_eq!(strip_tags("a &amp; b"), "a &amp; b");
        // Bare '<' without '>' is literal; the loop never runs.
        assert_eq!(strip_tags("a < b"), "a < b");
        // Bare '<' with '>' elsewhere but no tag: kept literally.
        assert_eq!(strip_tags("a < b > c"), "a < b > c");
    }
}
