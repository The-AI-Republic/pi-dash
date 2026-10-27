//! The one shared HTML5 entity table and `html.unescape` (PIDASHCONV-1553).
//!
//! Tables live in [`super::html_entities_generated`] (emitted by
//! `rust-api/scripts/gen_html_entities.py` from live CPython); this module
//! holds the lookup and decode logic every crate shares. Former copies in
//! `api::v1_cycles_modules::sanitize_data`, `db::app_pages::entities`,
//! `db::v1_assets::entities`, `jobs::tasks_mail::bs_entities_generated`
//! and `services::tasks_webhooks::html_entities` are deleted; the inline
//! lxml copy in `api::v1_assets::intake` belongs to PIDASHCONV-1554.
//!
//! [`html_unescape`] is `html.unescape` (`CPython/Lib/html/__init__.py`):
//! the `&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\x0C <&#;]{1,32};?)`
//! scan with exact named lookup, longest-prefix legacy fallback
//! (`&notanentity;` → `¬anentity;`), numeric refs, and the
//! `_invalid_charrefs` / `_invalid_codepoints` rules. It is infallible:
//! a decimal ref past the 4300-digit `int()` cap saturates to U+FFFD
//! instead of raising, matching the strip/sticky/mail decoders it
//! replaced. [`html_unescape_strict`] keeps CPython 3.11+ behavior and
//! reports the cap as [`DigitCapError`] (the multipart filename path
//! maps it to its 500).
//!
//! The bs4-shaped lookups ([`lookup_bare_entity`], [`c1_remap`]) are thin
//! views over the same table, not a second copy: the bare-name fold is
//! conflict-free (asserted by the generator), and the C1 map is the
//! `0x80..=0x9F` slice of `_invalid_charrefs`.

use thiserror::Error as ThisError;

use super::html_entities_generated::{HTML5_ENTITIES, INVALID_CHARREFS, INVALID_CODEPOINT_RANGES};

/// A decimal character reference past CPython's `int()` digit cap
/// (`sys.get_int_max_str_digits()`, 4300 on 3.11+), which raises
/// `ValueError` there. Hex refs are unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ThisError)]
#[error("decimal character reference exceeds the 4300-digit cap")]
pub struct DigitCapError;

/// `html.unescape`, infallible: over-long decimal refs saturate to
/// U+FFFD (see the module docs). Single left-to-right pass, no rescan
/// (`&amp;amp;` → `&amp;`).
pub fn html_unescape(text: &str) -> String {
    unescape_with(text, false).unwrap_or_else(|_| unreachable!("cap disabled"))
}

/// `html.unescape` with CPython 3.11+ digit-cap errors.
pub fn html_unescape_strict(text: &str) -> Result<String, DigitCapError> {
    unescape_with(text, true)
}

/// Replacement text for a bare entity `name` (no `&`, no `;`), or `None`
/// when the reference stays literal. Tries `name`, then `name;` — the
/// bs4 `handle_entityref` lookup over the shared table.
pub fn lookup_bare_entity(name: &str) -> Option<&'static str> {
    if let Some(hit) = lookup_entity(name) {
        return Some(hit);
    }
    // `name;` without allocating: names are short ASCII.
    let bytes = name.as_bytes();
    let mut buf = [0u8; 64];
    if bytes.len() + 1 > buf.len() {
        return None;
    }
    buf[..bytes.len()].copy_from_slice(bytes);
    buf[bytes.len()] = b';';
    let key = std::str::from_utf8(&buf[..bytes.len() + 1]).ok()?;
    lookup_entity(key)
}

/// The C1 slice of `_invalid_charrefs` (`0x80..=0x9F` → Windows-1252)
/// for `html.parser`-style decoders. Every value is one char
/// (asserted by the generator and pinned below).
pub fn c1_remap(n: u32) -> char {
    debug_assert!((0x80..=0x9F).contains(&n));
    invalid_charref(n as u64)
        .and_then(|s| s.chars().next())
        .expect("C1 remaps are single chars")
}

fn unescape_with(text: &str, strict_digits: bool) -> Result<String, DigitCapError> {
    if !text.contains('&') {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        match match_charref(rest, strict_digits)? {
            Some((replacement, consumed)) => {
                out.push_str(&replacement);
                rest = &rest[consumed..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// One `_charref` match at `&...`: `Some((replacement, bytes))` when the
/// regex matches (even when the replacement is the literal text — CPython
/// consumes the group either way), `None` when it does not match.
fn match_charref(
    text: &str,
    strict_digits: bool,
) -> Result<Option<(String, usize)>, DigitCapError> {
    debug_assert!(text.starts_with('&'));
    let after = &text[1..];
    if let Some(body) = after.strip_prefix('#') {
        return match_numeric_ref(body, strict_digits);
    }
    // Named ref: 1..=32 class chars (counted in chars, like the `re`
    // engine), then one optional `;`. Every slice below cuts at ASCII
    // matches, so byte offsets are always char boundaries.
    let mut end = 1;
    let mut count = 0;
    for (offset, c) in after.char_indices() {
        if count == 32 || !is_charref_char(c) {
            break;
        }
        count += 1;
        end = 1 + offset + c.len_utf8();
    }
    if count == 0 {
        return Ok(None);
    }
    let mut group = &text[1..end];
    let mut consumed = end;
    if text[end..].starts_with(';') {
        group = &text[1..end + 1];
        consumed = end + 1;
    }
    Ok(Some((replace_named_ref(group), consumed)))
}

/// The named-ref class `[^\t\n\f <&#;]` (note: `\r` and non-ASCII pass).
fn is_charref_char(c: char) -> bool {
    !matches!(c, '\t' | '\n' | '\x0C' | ' ' | '<' | '&' | '#' | ';')
}

/// `_replace_charref` named arm: the full key, then the longest `len >= 2`
/// prefix in the table (the remainder stays literal), else the literal
/// text. The prefix fallback also applies to `;`-terminated groups
/// (`&notanentity;` → `¬anentity;`).
fn replace_named_ref(group: &str) -> String {
    if let Some(hit) = lookup_entity(group) {
        return hit.to_owned();
    }
    let chars: Vec<char> = group.chars().collect();
    for end in (2..chars.len()).rev() {
        let prefix: String = chars[..end].iter().collect();
        if let Some(hit) = lookup_entity(&prefix) {
            let rest: String = chars[end..].iter().collect();
            return format!("{hit}{rest}");
        }
    }
    format!("&{group}")
}

fn lookup_entity(key: &str) -> Option<&'static str> {
    HTML5_ENTITIES
        .binary_search_by(|probe| probe.0.cmp(key))
        .ok()
        .map(|index| HTML5_ENTITIES[index].1)
}

/// The numeric alternatives `#[0-9]+;?` / `#[xX][0-9a-fA-F]+;?`; `body` is
/// the text after `&#`. All-ASCII, so byte offsets are char boundaries.
fn match_numeric_ref(
    body: &str,
    strict_digits: bool,
) -> Result<Option<(String, usize)>, DigitCapError> {
    let (hex, digits) = match body.strip_prefix(['x', 'X']) {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let digit_len = digits
        .bytes()
        .take_while(|b| {
            if hex {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        })
        .count();
    if digit_len == 0 {
        return Ok(None);
    }
    let mut consumed = 2 + digit_len + usize::from(hex);
    if digits[digit_len..].starts_with(';') {
        consumed += 1;
    }
    let digits = &digits[..digit_len];
    // CPython converts the full digit run with `int()`: more than 4300
    // decimal digits raises `ValueError` (the count, not the value —
    // 5000 zeros raise too). Hex parsing is unbounded.
    if !hex && digits.len() > 4300 {
        if strict_digits {
            return Err(DigitCapError);
        }
        return Ok(Some(("\u{FFFD}".to_owned(), consumed)));
    }
    let radix = if hex { 16 } else { 10 };
    let value = u64::from_str_radix(digits, radix).unwrap_or(u64::MAX);
    Ok(Some((replace_numeric_ref(value), consumed)))
}

/// `_replace_charref` numeric arm: the remap table first (it shadows the
/// codepoint set on 0x80-0x9F), then surrogates/out-of-range → U+FFFD,
/// then invalid codepoints → the empty string, else the char.
fn replace_numeric_ref(value: u64) -> String {
    if let Some(mapped) = invalid_charref(value) {
        return mapped.to_owned();
    }
    if value > 0x10FFFF || (0xD800..0xE000).contains(&value) {
        return "\u{FFFD}".to_owned();
    }
    if in_ranges(value as u32, INVALID_CODEPOINT_RANGES) {
        return String::new();
    }
    char::from_u32(value as u32).map_or_else(|| "\u{FFFD}".to_owned(), |c| c.to_string())
}

fn invalid_charref(value: u64) -> Option<&'static str> {
    if value > u32::MAX as u64 {
        return None;
    }
    INVALID_CHARREFS
        .binary_search_by(|probe| (probe.0 as u64).cmp(&value))
        .ok()
        .map(|index| INVALID_CHARREFS[index].1)
}

/// Point-in-sorted-disjoint-ranges.
fn in_ranges(value: u32, ranges: &[(u32, u32)]) -> bool {
    let index = ranges.partition_point(|&(lo, _)| lo <= value);
    index > 0 && value <= ranges[index - 1].1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The committed CPython record (`fixtures/html_entities/`): every
    /// `&key` input with its live-`html.unescape` output, plus both
    /// invalid tables.
    const DUMP: &str = include_str!("../../../fixtures/html_entities/cpython_unescape.json");

    fn dump() -> Value {
        serde_json::from_str(DUMP).expect("dump parses")
    }

    #[test]
    fn tables_match_committed_cpython_dump() {
        let dump = dump();
        assert_eq!(dump["cpython"].as_str(), Some("3.12.3"));
        // Table integrity: pinned counts, sorted, and equal to the dump.
        assert_eq!(HTML5_ENTITIES.len(), 2231);
        assert!(HTML5_ENTITIES.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(INVALID_CHARREFS.len(), 34);
        assert!(INVALID_CHARREFS.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(INVALID_CODEPOINT_RANGES.len(), 22);
        let dump_charrefs = dump["invalid_charrefs"].as_object().expect("charrefs");
        assert_eq!(dump_charrefs.len(), INVALID_CHARREFS.len());
        for (num, text) in INVALID_CHARREFS {
            let got = dump_charrefs
                .get(num.to_string().as_str())
                .and_then(Value::as_str);
            assert_eq!(got, Some(*text), "{num}");
        }
        let dump_points: Vec<u32> = dump["invalid_codepoints"]
            .as_array()
            .expect("codepoints")
            .iter()
            .map(|v| v.as_u64().expect("u64") as u32)
            .collect();
        assert_eq!(dump_points.len(), 126);
        assert!(dump_points.windows(2).all(|w| w[0] < w[1]));
        for point in 0..=0x10FFFFu32 {
            assert_eq!(
                in_ranges(point, INVALID_CODEPOINT_RANGES),
                dump_points.binary_search(&point).is_ok(),
                "{point:#X}"
            );
        }
        // Every entity through the shared function vs the dump.
        let cases = dump["cases"].as_object().expect("cases");
        assert_eq!(cases.len(), HTML5_ENTITIES.len());
        for (input, expected) in cases {
            assert_eq!(
                &html_unescape(input),
                expected.as_str().expect("str"),
                "{input}"
            );
        }
    }

    #[test]
    fn legacy_and_prefix_fallback() {
        for (entity, expected) in [
            ("&amp", "&"),
            ("&lt", "<"),
            ("&gt", ">"),
            ("&quot", "\""),
            ("&nbsp", "\u{A0}"),
            ("&copy", "©"),
            ("&reg", "®"),
            ("&not", "¬"),
            ("&AMP", "&"),
            ("&apos", "&apos"),
            ("&hellip", "&hellip"),
            ("&mdash", "&mdash"),
            ("&sol", "&sol"),
            ("&ampx", "&x"),
            ("&copyx;", "©x;"),
            ("&notanentity;", "¬anentity;"),
            ("&notit;", "¬it;"),
            ("&notit x", "¬it x"),
            ("&not in", "¬ in"),
            ("&notin; y", "∉ y"),
            ("&foobar;", "&foobar;"),
            ("&unknown;", "&unknown;"),
            ("&", "&"),
            ("&;", "&;"),
            ("&&amp;", "&&"),
            ("&am\rp;", "&am\rp;"),
            ("& Razor", "& Razor"),
            ("&bogusx y", "&bogusx y"),
            ("&\rb", "&\rb"),
            ("&amp;amp;", "&amp;"),
            ("&amp &amp;", "& &"),
            ("a&ampb c", "a&b c"),
            ("&ampb c", "&b c"),
            ("&#38;#38;", "&#38;"),
            ("&#x26;#38;", "&#38;"),
        ] {
            assert_eq!(&html_unescape(entity), expected, "{entity}");
            assert_eq!(
                &html_unescape_strict(entity).expect("strict"),
                expected,
                "{entity}"
            );
        }
        // Past the 32-char greedy window the match cannot reach `;`.
        let long = format!("&{};b", "a".repeat(40));
        assert_eq!(&html_unescape(&long), &long);
    }

    #[test]
    fn numeric_refs_and_invalids() {
        for (entity, expected) in [
            ("&#65", "A"),
            ("&#65;", "A"),
            ("&#00065;", "A"),
            ("&#X41;", "A"),
            ("&#x41;", "A"),
            ("&#13;", "\r"),
            ("&#0;", "\u{FFFD}"),
            ("&#xD800;", "\u{FFFD}"),
            ("&#xDFFF;", "\u{FFFD}"),
            ("&#x110000;", "\u{FFFD}"),
            ("&#99999999999999999999999999;", "\u{FFFD}"),
            ("&#1;", ""),
            ("&#8;", ""),
            ("&#127;", ""),
            ("&#64976;", ""),
            ("&#173;", "\u{AD}"),
            ("&#x1;", ""),
            ("&#xB;", ""),
            ("&#xC;", "\u{C}"),
            ("&#x1F;", ""),
            ("&#x7F;", ""),
            ("&#xFDD0;", ""),
            ("&#xFFFE;", ""),
            ("&#x10FFFF;", ""),
            ("&#1114111;", ""),
            ("&#1114112;", "\u{FFFD}"),
            ("&#x1F600;", "😀"),
            ("&#12x;", "\u{C}x;"),
            ("&#x4z;", "z;"),
            ("&#;", "&#;"),
            ("&#x;", "&#x;"),
            ("&#xg;", "&#xg;"),
        ] {
            assert_eq!(&html_unescape(entity), expected, "{entity}");
            assert_eq!(
                &html_unescape_strict(entity).expect("strict"),
                expected,
                "{entity}"
            );
        }
        // The full WHATWG remap table, decimal and hex.
        for (value, expected) in INVALID_CHARREFS.iter() {
            for entity in [format!("&#{value};"), format!("&#x{value:X};")] {
                assert_eq!(&html_unescape(&entity), *expected, "{entity}");
            }
        }
    }

    #[test]
    fn decimal_digit_cap() {
        // At the cap both modes agree (huge value → U+FFFD).
        let at_cap = format!("&#{};", "9".repeat(4300));
        assert_eq!(&html_unescape(&at_cap), "\u{FFFD}");
        assert_eq!(&html_unescape_strict(&at_cap).expect("strict"), "\u{FFFD}");
        // Past it the strict mode raises (the count, not the value —
        // 5000 zeros raise too) while the infallible mode saturates.
        for digits in ["9".repeat(4301), "0".repeat(5000)] {
            let entity = format!("&#{digits};");
            assert_eq!(&html_unescape(&entity), "\u{FFFD}", "{digits:.10}...");
            assert_eq!(
                html_unescape_strict(&entity).unwrap_err(),
                DigitCapError,
                "{digits:.10}..."
            );
        }
        // Hex parsing is unbounded in both modes.
        let hex = format!("&#x{};", "F".repeat(5000));
        assert_eq!(&html_unescape(&hex), "\u{FFFD}");
        assert_eq!(&html_unescape_strict(&hex).expect("strict"), "\u{FFFD}");
    }

    #[test]
    fn bare_lookup_matches_folded_table() {
        for (name, expected) in [
            ("mdash", "\u{2014}"),
            ("nbsp", "\u{A0}"),
            ("apos", "\u{27}"),
            ("AMP", "&"),
            ("amp", "&"),
            ("frac12", "\u{BD}"),
            ("CounterClockwiseContourIntegral", "\u{2233}"),
            ("acE", "\u{223E}\u{333}"),
            ("notin", "\u{2209}"),
            ("not", "¬"),
            ("fjlig", "fj"),
        ] {
            assert_eq!(lookup_bare_entity(name), Some(expected), "{name}");
        }
        for name in ["unknown", "aMp", "", &"a".repeat(70)] {
            assert_eq!(lookup_bare_entity(name), None, "{name}");
        }
        // Every bare name the fold produces resolves identically.
        for (key, value) in HTML5_ENTITIES.iter() {
            let bare = key.strip_suffix(';').unwrap_or(key);
            assert_eq!(lookup_bare_entity(bare), Some(*value), "{bare}");
        }
    }

    #[test]
    fn c1_remaps_are_single_chars() {
        for n in 0x80..=0x9Fu32 {
            let c = c1_remap(n);
            assert_eq!(
                Some(c.to_string()),
                invalid_charref(n as u64).map(str::to_owned),
                "{n:#X}"
            );
        }
    }
}
