//! Assistant markdown → sanitized HTML (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/runtime/markdown.py:1-38`: minimal
//! paragraph-per-blank-line rendering plus `nh3` sanitization, shared with
//! the GitHub sync importer. There is no Tiptap converter, so
//! `description_json` stays empty downstream.

/// Fallback HTML (`markdown.py:22,25,38`): empty input and sanitizer
/// failures both render an empty paragraph.
pub const EMPTY_HTML: &str = "<p></p>";
/// `validate_html_content` size cap (`content_validator.py:219-221`): inputs
/// over 10 MiB fail sanitization.
pub const MAX_HTML_BYTES: usize = 10 * 1024 * 1024;

/// HTML-escape paragraph text (`html.escape`, quote mode): `&`, `<`, `>`,
/// `"`, `'` — in that order, so the `&` of later entities is never
/// double-escaped.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Python `str.strip()` edge: `strip()` also trims `\x1c`-`\x1f`, which
/// Rust's Unicode `trim` leaves. Bodies carrying those controls are
/// vanishingly rare, but the strip is load-bearing for paragraph filtering,
/// so match it exactly.
fn strip_paragraph(text: &str) -> &str {
    text.trim_matches(|ch: char| ch.is_whitespace() || ('\x1c'..='\x1f').contains(&ch))
}

/// Render paragraphs (`markdown.py:20-26`): split on blank lines, strip,
/// drop empties, escape, fold single newlines to `<br/>`.
pub fn markdown_to_html(body: Option<&str>) -> String {
    let body = body.unwrap_or("");
    if body.is_empty() {
        return EMPTY_HTML.to_owned();
    }
    let mut html = String::new();
    for paragraph in body.split("\n\n") {
        let text = strip_paragraph(paragraph);
        if text.is_empty() {
            continue;
        }
        html.push_str("<p>");
        html.push_str(&escape_text(text).replace('\n', "<br/>"));
        html.push_str("</p>");
    }
    if html.is_empty() {
        return EMPTY_HTML.to_owned();
    }
    html
}

/// Decode the entity set `escape_text` emits, single left-to-right pass
/// (longest match, no recursive re-decode, mirroring one `html.escape`
/// inversion). Any other `&...;` shape passes through for the encoder below.
fn decode_entities(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        let rest = &text[index..];
        // (decoded char, entity source length); all ASCII.
        let entity = if rest.starts_with("&amp;") {
            Some(("&", 5))
        } else if rest.starts_with("&lt;") {
            Some(("<", 4))
        } else if rest.starts_with("&gt;") {
            Some((">", 4))
        } else if rest.starts_with("&quot;") {
            Some(("\"", 6))
        } else if rest.starts_with("&#x27;") {
            Some(("'", 6))
        } else {
            None
        };
        if let Some((decoded, len)) = entity {
            out.push_str(decoded);
            index += len;
        } else {
            let ch = rest.chars().next().expect("non-empty rest");
            out.push(ch);
            index += ch.len_utf8();
        }
    }
    out
}

/// Re-encode text the way `nh3` (html5ever serialization) emits it:
/// `&`, `<`, `>` escaped, non-breaking space as `&nbsp;`, quotes raw.
fn encode_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Sanitize `markdown_to_html` output the way `nh3.clean` does
/// (`content_validator.py:224-229`, default tags plus the project's custom
/// components). The input language is fixed — `<p>`/`</p>` wrappers, `<br/>`
/// separators, escaped text — so the observed `nh3` behavior is a small
/// deterministic transform, pinned vector-by-vector below: `<br/>` becomes
/// `<br>`; entities round-trip through parse/serialize (`&quot;`/`&#x27;`
/// emerge raw, `&amp;`/`&lt;`/`&gt;` stay escaped, `\u{a0}` emerges as
/// `&nbsp;`); carriage returns normalize to `\n`; NUL bytes are dropped.
/// Anything that is not a known tag is escaped as text (fail-safe; it is
/// unreachable from `markdown_to_html` output, whose `<` are all escaped).
pub fn sanitize_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    loop {
        match rest.find('<') {
            None => {
                out.push_str(&encode_text(&normalize_text(&decode_entities(rest))));
                break;
            }
            Some(tag_start) => {
                out.push_str(&encode_text(&normalize_text(&decode_entities(
                    &rest[..tag_start],
                ))));
                let tail = &rest[tag_start..];
                if let Some(after) = tail.strip_prefix("<p>") {
                    out.push_str("<p>");
                    rest = after;
                } else if let Some(after) = tail.strip_prefix("</p>") {
                    out.push_str("</p>");
                    rest = after;
                } else if let Some(after) = tail.strip_prefix("<br/>") {
                    out.push_str("<br>");
                    rest = after;
                } else {
                    // Unknown `<`: escape it as text and continue scanning.
                    out.push_str("&lt;");
                    rest = &tail[1..];
                }
            }
        }
    }
    out
}

/// HTML-tokenizer text normalization `nh3` applies: CR/CRLF become LF, NUL
/// is dropped.
fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\0' => {}
            _ => out.push(ch),
        }
    }
    out
}

/// Sanitized HTML for an assistant body (`markdown.py:29-38`): render, run
/// `validate_html_content`, fall back to [`EMPTY_HTML`] when sanitization
/// fails or yields nothing. The only failure this port can produce is the
/// 10 MiB cap (sanitization itself is total); oversized input falls back
/// exactly like Python's `(False, …, None)` branch.
pub fn to_safe_html(body: Option<&str>) -> String {
    let html = markdown_to_html(body);
    if html.len() > MAX_HTML_BYTES {
        return EMPTY_HTML.to_owned();
    }
    let clean = sanitize_html(&html);
    if clean.is_empty() {
        return EMPTY_HTML.to_owned();
    }
    clean
}

#[cfg(test)]
mod tests {
    use super::*;

    // (body, markdown_to_html, to_safe_html): the middle column pins the
    // render step, the last column pins live `nh3.clean` output (nh3 0.2.18,
    // probed against the real `markdown_to_html`).
    const VECTORS: &[(&str, &str, &str)] = &[
        ("hi", "<p>hi</p>", "<p>hi</p>"),
        ("a\nb", "<p>a<br/>b</p>", "<p>a<br>b</p>"),
        ("a\n\nb", "<p>a</p><p>b</p>", "<p>a</p><p>b</p>"),
        (
            "<script>alert(1)</script>",
            "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>",
            "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>",
        ),
        ("a & b", "<p>a &amp; b</p>", "<p>a &amp; b</p>"),
        (
            "\"quoted\"",
            "<p>&quot;quoted&quot;</p>",
            "<p>\"quoted\"</p>",
        ),
        ("a'b", "<p>a&#x27;b</p>", "<p>a'b</p>"),
        ("a&amp;b", "<p>a&amp;amp;b</p>", "<p>a&amp;amp;b</p>"),
        ("<br/>", "<p>&lt;br/&gt;</p>", "<p>&lt;br/&gt;</p>"),
        ("é☃ unicode", "<p>é☃ unicode</p>", "<p>é☃ unicode</p>"),
        ("a\u{a0}b", "<p>a\u{a0}b</p>", "<p>a&nbsp;b</p>"), // U+00A0 renders as &nbsp;
        ("a\rb", "<p>a\rb</p>", "<p>a\nb</p>"),
        ("a\0b", "<p>a\0b</p>", "<p>ab</p>"),
        (
            " emerging <b>bold</b> ",
            "<p>emerging &lt;b&gt;bold&lt;/b&gt;</p>",
            "<p>emerging &lt;b&gt;bold&lt;/b&gt;</p>",
        ),
        (
            "line1\nline2\n\npara2 with <tag>",
            "<p>line1<br/>line2</p><p>para2 with &lt;tag&gt;</p>",
            "<p>line1<br>line2</p><p>para2 with &lt;tag&gt;</p>",
        ),
    ];

    #[test]
    fn render_and_sanitize_match_probed_vectors() {
        for (body, html, clean) in VECTORS {
            assert_eq!(&markdown_to_html(Some(body)), html, "render {body:?}");
            assert_eq!(&to_safe_html(Some(body)), clean, "sanitize {body:?}");
        }
    }

    #[test]
    fn empty_and_blank_bodies_fall_back() {
        assert_eq!(markdown_to_html(None), "<p></p>");
        assert_eq!(markdown_to_html(Some("")), "<p></p>");
        assert_eq!(markdown_to_html(Some("  \n\n  ")), "<p></p>");
        assert_eq!(to_safe_html(None), "<p></p>");
        assert_eq!(to_safe_html(Some("")), "<p></p>");
    }

    #[test]
    fn carriage_return_pairs_normalize_once() {
        // `markdown_to_html` folds only `\n` to `<br/>`, so the surviving
        // `\r` normalizes to `\n` in place (verified against live nh3).
        assert_eq!(to_safe_html(Some("a\r\nb")), "<p>a\n<br>b</p>");
    }

    #[test]
    fn oversized_input_falls_back() {
        let big = "x".repeat(MAX_HTML_BYTES);
        assert_eq!(to_safe_html(Some(&big)), "<p></p>");
    }
}
