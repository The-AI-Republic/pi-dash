//! Django `ResponseHeaders` value encoding, shared by every response header.
//!
//! Ports `ResponseHeaders._convert_to_charset(value, "latin-1",
//! mime_encode=True)` + `__setitem__` (`django/http/response.py`): a value
//! that fits latin-1 goes on the wire as latin-1 bytes; anything else is
//! MIME-encoded via `email.header.Header(value, "utf-8",
//! maxlinelen=sys.maxsize).encode()` (RFC 2047). Values with `\r`/`\n`
//! raise `BadHeaderError` (a 500); values no header can carry (control
//! bytes, DEL) pass `ResponseHeaders` but die at Django's WSGI layer — a
//! 500 with Django's default page, where the port answers its own 500.
//!
//! The MIME path is `email.charset.SHORTEST` over utf-8 bytes: base64 wins
//! iff strictly shorter than header-quoted-printable (ties go to Q), one
//! encoded word, no length folding (`maxlinelen` is `sys.maxsize`).
//! `Header.encode` still splits on embedded `str.splitlines` boundaries
//! first: one line encodes to one word (a single trailing boundary is
//! dropped by `splitlines`); a leading boundary plus one more line
//! encodes to `' ' + word`; anything else folds with an embedded
//! newline, which Django's WSGI layer rejects (500).

use axum::http::HeaderValue;
use base64::Engine as _;

/// Encode `value` the way Django stores a response header value.
///
/// Returns `None` exactly when Django answers 500: a `\r`/`\n` in the
/// value (`BadHeaderError`), a latin-1 value with bytes no header can
/// carry (controls, DEL — rejected at Django's WSGI layer), or a MIME-path
/// value whose `splitlines` fold differently (see [`mime_header_str`]).
/// Callers map `None` to their module's own 500 response.
pub fn header_value(value: &str) -> Option<HeaderValue> {
    if value.contains('\r') || value.contains('\n') {
        return None;
    }
    if value.is_ascii() {
        return HeaderValue::from_str(value).ok();
    }
    if value.chars().all(|c| c as u32 <= 0xFF) {
        let bytes: Vec<u8> = value.chars().map(|c| c as u8).collect();
        return HeaderValue::from_bytes(&bytes).ok();
    }
    mime_header_str(value).and_then(|word| HeaderValue::from_str(&word).ok())
}

/// `django.utils.encoding.iri_to_uri` (`quote(iri,
/// safe="/#%[]=:;$&()+,!?*@'~")`, Django 4.2 `utils/encoding.py:110-138`):
/// `quote` always spares ASCII alphanumerics plus `-_.~`; the safe set
/// spares the RFC 3986 reserved marks and `%`. Every other byte — UTF-8
/// multibyte included — becomes uppercase `%XX`.
///
/// Django's `HttpResponseRedirect` applies this to the URL before storing
/// `Location` (`django/http/response.py:620`); every `HttpResponseRedirect`
/// emulation in this crate must call it before [`header_value`]. It must
/// NOT be applied to other headers: a hand-set `response["Location"]`
/// skips it in Django.
pub fn iri_to_uri(joined: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(joined.len());
    for byte in joined.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || matches!(
                byte,
                b'/' | b'#'
                    | b'%'
                    | b'['
                    | b']'
                    | b'='
                    | b':'
                    | b';'
                    | b'$'
                    | b'&'
                    | b'('
                    | b')'
                    | b'+'
                    | b','
                    | b'!'
                    | b'?'
                    | b'*'
                    | b'@'
                    | b'\''
            );
        if keep {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 15) as usize] as char);
        }
    }
    out
}

/// `str.splitlines` boundaries other than `\r`/`\n` (rejected before the
/// MIME path): `Header.encode` splits on these even with a huge
/// `maxlinelen`.
fn is_split_boundary(c: char) -> bool {
    matches!(
        c,
        '\x0B' | '\x0C' | '\x1C' | '\x1D' | '\x1E' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// The MIME-path header string for a value past latin-1.
///
/// Ports `Header(value, "utf-8", maxlinelen=sys.maxsize).encode()`
/// including its `splitlines` pre-split (CPython `email/header.py`):
/// one line encodes to one word; a leading boundary plus one more line
/// encodes to `' ' + word(' ' + line.lstrip())`; anything else folds
/// with an embedded newline, which Django's WSGI layer rejects with
/// `ValueError` (500), so this returns `None` there.
fn mime_header_str(value: &str) -> Option<String> {
    if !value.contains(is_split_boundary) {
        return Some(mime_encode(value));
    }
    // `splitlines` semantics over the boundary set: split everywhere,
    // dropping one trailing empty when the value ends on a boundary.
    let mut lines: Vec<&str> = value.split(is_split_boundary).collect();
    if value.ends_with(is_split_boundary) {
        lines.pop();
    }
    match lines.as_slice() {
        [single] => Some(mime_encode(single)),
        ["", second] => {
            let mut cont = String::with_capacity(second.len() + 1);
            cont.push(' ');
            // Python `str.lstrip`: `char::is_whitespace` plus `\x1C`-`\x1F`
            // (the only `str.isspace` chars outside Unicode White_Space).
            cont.push_str(second.trim_start_matches(|c: char| {
                c.is_whitespace() || matches!(c, '\u{1C}'..='\u{1F}')
            }));
            Some(format!(" {}", mime_encode(&cont)))
        }
        _ => None,
    }
}

/// Port of `email.header.Header(value, "utf-8",
/// maxlinelen=sys.maxsize).encode()` for single-line values: the whole
/// value becomes one `=?utf-8?q?...?=` or `=?utf-8?b?...?=` word, Q or B
/// by `SHORTEST` (base64 iff strictly shorter, ties to Q).
///
/// Proven byte for byte against CPython 3.12's `email.header` over the
/// 831-vector corpus in `tests/data/headers_corpus.json`.
pub fn mime_encode(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let bytes = value.as_bytes();
    // `base64mime.header_length`: 4 out for each 3 (or fraction) in.
    let base64_len = bytes.len().div_ceil(3) * 4;
    // `quoprimime.header_length`: 1 per safe/space byte, 3 per `=XX`.
    let qp_len: usize = bytes.iter().map(|b| qp_encoded_len(*b)).sum();
    if base64_len < qp_len {
        format!(
            "=?utf-8?b?{}?=",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    } else {
        let mut out = String::with_capacity(qp_len + "=?utf-8?q??=".len());
        out.push_str("=?utf-8?q?");
        for b in bytes {
            push_qp_byte(&mut out, *b);
        }
        out.push_str("?=");
        out
    }
}

/// `quoprimime._QUOPRI_HEADER_MAP` safe set: `-!*+/` plus letters and
/// digits stay literal; space becomes `_`; anything else is `=XX`.
fn qp_safe(byte: u8) -> bool {
    matches!(
        byte,
        b'-' | b'!' | b'*' | b'+' | b'/' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
    )
}

fn qp_encoded_len(byte: u8) -> usize {
    if qp_safe(byte) || byte == b' ' {
        1
    } else {
        3
    }
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

fn push_qp_byte(out: &mut String, byte: u8) {
    if qp_safe(byte) {
        out.push(byte as char);
    } else if byte == b' ' {
        out.push('_');
    } else {
        out.push('=');
        out.push(HEX_UPPER[(byte >> 4) as usize] as char);
        out.push(HEX_UPPER[(byte & 0x0F) as usize] as char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_examples_encode_like_django() {
        // Observed live from Django (PIDASHCONV-880): the shorter URL
        // picks B, the longer one ties and picks Q.
        assert_eq!(
            mime_encode("https://example.com/\u{1f600}"),
            "=?utf-8?b?aHR0cHM6Ly9leGFtcGxlLmNvbS/wn5iA?="
        );
        assert_eq!(
            mime_encode("https://example.com/django9/\u{1f600}"),
            "=?utf-8?q?https=3A//example=2Ecom/django9/=F0=9F=98=80?="
        );
    }

    #[test]
    fn header_value_matches_django_bytes() {
        // Wire bytes recorded live from Django runserver (PIDASHCONV-873):
        // ASCII passes through, latin-1 goes out as latin-1 (ü = 0xFC,
        // not UTF-8), control bytes fail (Django 500s at its server layer).
        let ascii = header_value("https://example.com/specs").expect("ascii");
        assert_eq!(ascii.as_bytes(), b"https://example.com/specs");
        let latin1 = header_value("https://example.com/\u{fc}ser").expect("latin-1");
        assert_eq!(latin1.as_bytes(), b"https://example.com/\xfcser");
        assert!(header_value("https://example.com/\x01").is_none());
        assert!(header_value("https://example.com/\x7f").is_none());
    }

    #[test]
    fn newlines_always_fail_even_with_mime_chars() {
        // Django checks for newlines before the MIME path, so a value
        // with both still raises `BadHeaderError` (500).
        assert!(header_value("a\nb").is_none());
        assert!(header_value("a\rb").is_none());
        assert!(header_value("a\nb\u{1f600}").is_none());
        assert!(header_value("a\rb\u{1f600}").is_none());
    }

    #[test]
    fn tab_passes_raw_but_encodes_past_latin1() {
        // TAB is legal in a header value on both sides.
        let tab = header_value("a\tb").expect("tab");
        assert_eq!(tab.as_bytes(), b"a\tb");
        // Past latin-1 the whole value (TAB included) is MIME-encoded.
        assert_eq!(mime_encode("\t\u{1f600}"), "=?utf-8?b?CfCfmIA=?=");
    }

    #[test]
    fn controls_with_mime_chars_encode_instead_of_failing() {
        // A control byte plus a non-latin-1 char takes the MIME path on
        // Django (only \r\n raise first), so the control is `=XX`.
        let value = header_value("u\x01\u{1f600}").expect("mime path");
        assert_eq!(value.to_str().expect("ascii"), "=?utf-8?b?dQHwn5iA?=");
        let del = header_value("u\x7f\u{1f600}").expect("mime path");
        assert_eq!(del.to_str().expect("ascii"), "=?utf-8?b?dX/wn5iA?=");
    }

    #[test]
    fn short_values_pick_base64() {
        assert_eq!(mime_encode("a b\u{1f600}"), "=?utf-8?b?YSBi8J+YgA==?=");
        assert_eq!(mime_encode("_\u{1f600}"), "=?utf-8?b?X/CfmIA=?=");
    }

    #[test]
    fn q_escapes_unsafe_punct_and_space() {
        // A long safe run tips SHORTEST to Q: space becomes `_`, `_`
        // and `~` become `=XX`, safe bytes stay literal.
        assert_eq!(
            mime_encode("abcdefghijklmnopqrstuvwxyz0123456789 _~\u{1f600}"),
            "=?utf-8?q?abcdefghijklmnopqrstuvwxyz0123456789_=5F=7E=F0=9F=98=80?="
        );
    }

    #[test]
    fn empty_and_edge_values() {
        assert_eq!(mime_encode(""), "");
        assert!(header_value("").expect("empty").as_bytes().is_empty());
    }

    #[test]
    fn iri_to_uri_matches_django_vectors() {
        // Oracle: `django.utils.encoding.iri_to_uri` on the venv Django
        // (moved from `app_assets::handlers_v1`; CR/LF rows added for
        // PIDASHCONV-1356: `HttpResponseRedirect` percent-encodes them,
        // so they stay a 302 instead of `BadHeaderError` 500s).
        for (raw, expected) in [
            ("http://h/p/a b", "http://h/p/a%20b"),
            ("http://h/p/caf\u{e9}", "http://h/p/caf%C3%A9"),
            ("http://h/p/100%25x", "http://h/p/100%25x"),
            ("http://h/p/a?b#c", "http://h/p/a?b#c"),
            ("http://h/p/a+b", "http://h/p/a+b"),
            ("http://h/p/a\"b<c>d", "http://h/p/a%22b%3Cc%3Ed"),
            (
                "http://h/p/\u{4e2d}\u{6587}",
                "http://h/p/%E4%B8%AD%E6%96%87",
            ),
            ("http://h/p/a\nb", "http://h/p/a%0Ab"),
            ("http://h/p/a\rb", "http://h/p/a%0Db"),
        ] {
            assert_eq!(iri_to_uri(raw), expected, "iri_to_uri({raw:?})");
        }
    }

    #[test]
    fn split_boundaries_fold_to_500() {
        // Interior `splitlines` boundaries fold the MIME output with an
        // embedded newline, which Django's WSGI layer rejects (500).
        // Expected shapes recorded from CPython's `email.header`.
        assert!(header_value("a\x0b\u{1f600}").is_none());
        assert!(header_value("a\u{2028}b\u{1f600}").is_none());
        assert!(header_value("\u{2028}a\u{2028}\u{1f600}").is_none());
        assert!(header_value("a\u{1f600}\u{2028}\u{2029}").is_none());
    }

    #[test]
    fn trailing_boundary_is_dropped_like_splitlines() {
        // One trailing boundary leaves a single line: the word covers
        // the stripped value. Recorded from CPython's `email.header`.
        let vt = header_value("a\u{1f600}\u{2028}").expect("single line");
        assert_eq!(vt.to_str().expect("ascii"), "=?utf-8?b?YfCfmIA=?=");
        let nel = header_value("a\u{1f600}\u{85}").expect("single line");
        assert_eq!(nel.to_str().expect("ascii"), "=?utf-8?b?YfCfmIA=?=");
        let lone = header_value("\u{2028}").expect("empty line");
        assert!(lone.as_bytes().is_empty());
    }

    #[test]
    fn leading_boundary_continuation_matches_django() {
        // Leading boundary plus one more line: `' ' + word`, with the
        // continuation lstripped (Python strips `\x1f` too, which Rust's
        // `trim_start` would keep). Recorded from CPython's `email.header`.
        let lead = header_value("\u{85}\u{1f600}").expect("continuation");
        assert_eq!(lead.to_str().expect("ascii"), " =?utf-8?b?IPCfmIA=?=");
        let two = header_value("\u{2028}\u{2029}").expect("continuation");
        assert_eq!(two.to_str().expect("ascii"), " =?utf-8?q?_?=");
        let ws = header_value("\u{2028} \t\u{1f600}").expect("continuation");
        assert_eq!(ws.to_str().expect("ascii"), " =?utf-8?b?IPCfmIA=?=");
        let us = header_value("\u{2028}\x1f\u{1f600}").expect("continuation");
        assert_eq!(us.to_str().expect("ascii"), " =?utf-8?b?IPCfmIA=?=");
    }
}
