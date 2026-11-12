//! Shared `multipart/form-data` field reader (PIDASHCONV-1079).
//!
//! Django accepts multipart on the magic routes — the DRF
//! `MultiPartParser` on generate, `request.POST` on the plain sign
//! Views — so this is the fields-only equivalent: ordered pairs, last
//! value wins per field (Django `QueryDict.get`), file parts ignored.
//! `csrfmiddlewaretoken` surfaces as an ordinary field so the
//! PIDASHCONV-1077 gate can read it from the same pairs.
//!
//! Python: `django/http/multipartparser.py` (`MultiPartParser._parse`:
//! fields appendlist with `errors="replace"`, base64 transfer-decoding
//! with a raw fallback, parts without a name skipped, `filename=""`
//! parts filed under POST).
//!
//! Known approximations (all verified against Django 6.0.5; none is
//! reachable with the field names and values these routes read):
//! all-padding base64 quads (`====`) decode to empty in CPython but
//! fail strict decoding (raw kept); quoted `;` inside a field name is
//! mis-split; backslash escapes beyond `\\`/`\"` collapse; boundary
//! bytes inside a value split anywhere in Django but only at line
//! starts here (RFC 2046).

use base64::Engine as _;

/// `multipart/form-data`, ignoring parameters (`; boundary=…`).
pub(crate) fn is_multipart(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("multipart/form-data"))
}

/// Field parts as ordered `(name, value)` pairs, or `None` when the
/// body is not multipart or cannot be framed (missing/invalid
/// boundary, truncated framing). Callers fall back to the urlencoded
/// parse on `None` — the pre-multipart behavior.
pub(crate) fn parse_fields(content_type: &str, body: &[u8]) -> Option<Vec<(String, String)>> {
    if !is_multipart(content_type) {
        return None;
    }
    let boundary = boundary(content_type)?;
    let mut parts = Vec::new();
    for part in split_parts(body, &boundary)? {
        if let Some((name, value)) = parse_part(part) {
            parts.push((name, value));
        }
    }
    Some(parts)
}

/// The `boundary=` parameter (quoted or bare). Django rejects a
/// missing boundary (`MultiPartParserError`); here it is `None` so
/// the caller falls back to the urlencoded parse.
fn boundary(content_type: &str) -> Option<Vec<u8>> {
    for param in content_type.split(';').skip(1) {
        let param = param.trim();
        let Some(raw) = param
            .split_once('=')
            .filter(|(key, _)| key.trim().eq_ignore_ascii_case("boundary"))
            .map(|(_, value)| value)
        else {
            continue;
        };
        let raw = raw.trim();
        let value = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
            unescape(&raw[1..raw.len() - 1])
        } else {
            raw.to_owned()
        };
        if value.is_empty() || !value.is_ascii() {
            return None;
        }
        return Some(value.into_bytes());
    }
    None
}

/// `\\` escapes inside a quoted boundary (`\"` → `"`, `\\` → `\`).
fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(char) = chars.next() {
        if char == '\\' {
            out.push(chars.next().unwrap_or('\\'));
        } else {
            out.push(char);
        }
    }
    out
}

/// Split the body into raw part slices (headers + blank line +
/// content). Returns `None` when no closing delimiter is found
/// (truncated body). A line-start `--boundary` opens a part, a
/// line-start `--boundary--` closes; anything before the first
/// delimiter is preamble and the epilogue after the closer is
/// ignored, like Django's streaming parser.
fn split_parts<'a>(body: &'a [u8], boundary: &[u8]) -> Option<Vec<&'a [u8]>> {
    let mut delimiter = Vec::with_capacity(boundary.len() + 2);
    delimiter.extend_from_slice(b"--");
    delimiter.extend_from_slice(boundary);
    let mut parts = Vec::new();
    let mut cursor = first_delimiter(body, &delimiter)?;
    loop {
        let after = cursor + delimiter.len();
        if body.get(after..after + 2) == Some(b"--") {
            return Some(parts);
        }
        let start = line_break_len(&body[after..])?;
        let content_start = after + start;
        let (next, closed) = next_delimiter(body, &delimiter, content_start)?;
        parts.push(&body[content_start..next]);
        cursor = next;
        if closed {
            return Some(parts);
        }
    }
}

/// Offset of the first line-start delimiter, if any.
fn first_delimiter(body: &[u8], delimiter: &[u8]) -> Option<usize> {
    let (offset, _) = next_delimiter(body, delimiter, 0)?;
    Some(offset)
}

/// Offset of the next line-start delimiter at or after `from`, plus
/// whether it is the closing (`--`-suffixed) one. A `--boundary`
/// occurrence is only a delimiter when it starts a line and is
/// followed by `--`, CRLF, or the end of the body.
fn next_delimiter(body: &[u8], delimiter: &[u8], from: usize) -> Option<(usize, bool)> {
    let mut search = from;
    while let Some(relative) = find_subslice(&body[search..], delimiter) {
        let offset = search + relative;
        let at_line_start = offset == 0 || body[offset - 1] == b'\n';
        let after = offset + delimiter.len();
        let rest = &body[after..];
        let valid = at_line_start
            && (rest.starts_with(b"--") || line_break_len(rest).is_some() || rest.is_empty());
        if valid {
            return Some((offset, rest.starts_with(b"--")));
        }
        search = offset + 1;
    }
    None
}

/// Length of the `\r\n` line break at the slice start. Django splits
/// framing only on CRLF (`multipartparser.py`: `\r\n\r\n` header end,
/// `\r\n` header lines), so a bare LF never opens, continues, or
/// closes a part — such bodies fall back to the urlencoded parse.
fn line_break_len(slice: &[u8]) -> Option<usize> {
    if slice.starts_with(b"\r\n") {
        Some(2)
    } else {
        None
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// One part → `(name, value)`, or `None` for file parts (a
/// non-empty `filename`/`filename*` — Django checks
/// `params.get("filename")` truthiness, so `filename=""` lands in
/// POST: `multipartparser.py:737`) and parts without a `name`. Names
/// and values decode lossy (`force_str(…, errors="replace")`).
fn parse_part(part: &[u8]) -> Option<(String, String)> {
    let (raw_headers, value) = split_headers(part)?;
    let mut name: Option<String> = None;
    let mut is_file = false;
    let mut base64 = false;
    for line in raw_headers.split(|b| *b == b'\n') {
        let line = strip_cr(line);
        // Django `line.decode()`s every header line strictly: a
        // non-UTF-8 line is skipped, never lossy-read.
        if std::str::from_utf8(line).is_err() {
            continue;
        }
        if line.len() > "content-disposition:".len()
            && line[..="content-disposition".len()].eq_ignore_ascii_case(b"content-disposition:")
        {
            let (field, file) = disposition_params(&line["content-disposition:".len()..]);
            if field.is_some() {
                name = field;
            }
            is_file |= file;
        } else if line.len() > "content-transfer-encoding:".len()
            && line[..="content-transfer-encoding".len()]
                .eq_ignore_ascii_case(b"content-transfer-encoding:")
        {
            base64 |= strip_cr(&line["content-transfer-encoding:".len()..])
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect::<Vec<u8>>()
                .eq_ignore_ascii_case(b"base64");
        }
    }
    if is_file {
        return None;
    }
    let name = name?;
    // The line break before the next delimiter belongs to the framing
    // (RFC 2046), never to the value — Django's parser strips it too.
    let value = value.strip_suffix(b"\n").unwrap_or(value);
    let value = value.strip_suffix(b"\r").unwrap_or(value);
    if base64 {
        let decoded = decode_base64_field(value);
        return Some((name, String::from_utf8_lossy(&decoded).into_owned()));
    }
    Some((name, String::from_utf8_lossy(value).into_owned()))
}

/// Split part bytes into headers and content at the first blank line
/// (`\r\n\r\n` only, like Django — a bare-LF part has no headers and
/// is skipped). The content keeps its bytes minus the line break that
/// precedes the next delimiter (that break belongs to the framing,
/// not the value).
fn split_headers(part: &[u8]) -> Option<(&[u8], &[u8])> {
    let index = find_subslice(part, b"\r\n\r\n")?;
    Some((&part[..index], &part[index + 4..]))
}

fn strip_cr(slice: &[u8]) -> &[u8] {
    slice.strip_suffix(b"\r").unwrap_or(slice)
}

/// `Content-Disposition` params: the field `name` (if any) and whether
/// a non-empty `filename`/`filename*` value marks a file part
/// (`filename=""` is a field — Django `params.get` truthiness).
fn disposition_params(value: &[u8]) -> (Option<String>, bool) {
    let mut name = None;
    let mut is_file = false;
    for param in value.split(|b| *b == b';').skip(1) {
        let param = param
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .collect::<Vec<u8>>();
        if let Some(raw) = strip_param(&param, b"name=") {
            name = Some(String::from_utf8_lossy(&param_value(raw)).into_owned());
        } else if strip_param(&param, b"filename=").is_some_and(|raw| !param_value(raw).is_empty())
            || strip_param(&param, b"filename*=").is_some_and(|raw| !param_value(raw).is_empty())
        {
            is_file = true;
        }
    }
    let name = name.map(|n| n.trim_matches(|c: char| c.is_ascii_whitespace()).to_owned());
    (name.filter(|n| !n.is_empty()), is_file)
}

/// Case-insensitive `prefix` strip over the raw param bytes.
fn strip_param<'a>(param: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if param.len() >= prefix.len() && param[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&param[prefix.len()..])
    } else {
        None
    }
}

/// A param value: surrounding quotes stripped, `\\` escapes resolved.
fn param_value(raw: &[u8]) -> Vec<u8> {
    let mut value = raw.to_vec();
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value.pop();
    }
    if value.len() >= 2 && value.starts_with(b"\"") && value.ends_with(b"\"") {
        value = value[1..value.len() - 1].to_vec();
    }
    let mut out = Vec::with_capacity(value.len());
    let mut bytes = value.iter();
    while let Some(byte) = bytes.next() {
        if *byte == b'\\' {
            out.push(bytes.next().copied().unwrap_or(b'\\'));
        } else {
            out.push(*byte);
        }
    }
    out
}

/// Base64 field decoding (`multipartparser.py`: `b64decode` with
/// non-alphabet bytes ignored, raw fallback on error).
fn decode_base64_field(value: &[u8]) -> Vec<u8> {
    let filtered: Vec<u8> = value
        .iter()
        .copied()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(&filtered)
        .unwrap_or_else(|_| value.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CT: &str = "multipart/form-data; boundary=abc123";

    fn part(name: &str, value: &str) -> Vec<u8> {
        format!("--abc123\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
            .into_bytes()
    }

    fn frame(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut body = parts.concat();
        body.extend_from_slice(b"--abc123--\r\n");
        body
    }

    #[test]
    fn fields_come_back_in_order() {
        let framed = frame(&[part("email", "a@x.com"), part("code", "123456")]);
        assert_eq!(
            parse_fields(CT, &framed),
            Some(vec![
                ("email".to_owned(), "a@x.com".to_owned()),
                ("code".to_owned(), "123456".to_owned()),
            ])
        );
    }

    #[test]
    fn duplicate_names_keep_both_so_last_wins() {
        let framed = frame(&[part("email", "first@x.com"), part("email", "second@x.com")]);
        let pairs = parse_fields(CT, &framed).expect("parses");
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs.last(),
            Some(&("email".to_owned(), "second@x.com".to_owned()))
        );
    }

    #[test]
    fn file_parts_are_ignored() {
        let file = b"--abc123\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"a.bin\"\r\nContent-Type: application/octet-stream\r\n\r\nxxx\r\n".to_vec();
        let rfc5987 =
            b"--abc123\r\nContent-Disposition: form-data; name=\"doc\"; filename*=utf-8''a.bin\r\n\r\nzzz\r\n"
                .to_vec();
        let framed = frame(&[part("email", "a@x.com"), file, rfc5987]);
        assert_eq!(
            parse_fields(CT, &framed),
            Some(vec![("email".to_owned(), "a@x.com".to_owned())])
        );
    }

    #[test]
    fn empty_filename_is_a_field() {
        // `filename=""` (the browser empty-file-input shape): Django
        // checks `params.get("filename")` truthiness, so the part
        // lands in POST — the email still reads.
        let empty =
            b"--abc123\r\nContent-Disposition: form-data; name=\"email\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\na@x.com\r\n"
                .to_vec();
        let framed = frame(&[empty]);
        assert_eq!(
            parse_fields(CT, &framed),
            Some(vec![("email".to_owned(), "a@x.com".to_owned())])
        );
    }

    #[test]
    fn bare_lf_framing_yields_no_fields() {
        // Django splits framing only on CRLF: a bare-LF body has no
        // headers per part, so POST stays empty.
        let body =
            b"--abc123\nContent-Disposition: form-data; name=\"email\"\n\nU@x.com\n--abc123--\n"
                .to_vec();
        assert_eq!(parse_fields(CT, &body), Some(vec![]));
    }

    #[test]
    fn non_utf8_header_lines_are_skipped() {
        // Django `line.decode()`s strictly: a non-UTF-8 disposition
        // line drops the part, a non-UTF-8 encoding line drops the
        // transfer decoding (value stays raw).
        let bad_disposition =
            b"--abc123\r\nContent-Disposition: form-data; name=\"caf\xe9\"\r\n\r\nv\r\n".to_vec();
        assert_eq!(parse_fields(CT, &frame(&[bad_disposition])), Some(vec![]));
        let bad_encoding = b"--abc123\r\nContent-Disposition: form-data; name=\"code\"\r\nContent-Transfer-Encoding: base64\xe9\r\n\r\nMTIzNDU2\r\n".to_vec();
        assert_eq!(
            parse_fields(CT, &frame(&[bad_encoding])),
            Some(vec![("code".to_owned(), "MTIzNDU2".to_owned())])
        );
    }

    #[test]
    fn csrf_token_field_surfaces_like_any_field() {
        let framed = frame(&[part("csrfmiddlewaretoken", "secret"), part("code", "1")]);
        let pairs = parse_fields(CT, &framed).expect("parses");
        assert!(pairs.contains(&("csrfmiddlewaretoken".to_owned(), "secret".to_owned())));
    }

    #[test]
    fn quoted_boundary_parses() {
        let content_type = "multipart/form-data; boundary=\"abc123\"";
        let framed = frame(&[part("email", "a@x.com")]);
        assert_eq!(
            parse_fields(content_type, &framed),
            Some(vec![("email".to_owned(), "a@x.com".to_owned())])
        );
    }

    #[test]
    fn non_multipart_and_missing_boundary_are_none() {
        let framed = frame(&[part("email", "a@x.com")]);
        assert_eq!(
            parse_fields("application/x-www-form-urlencoded", &framed),
            None
        );
        assert_eq!(parse_fields("multipart/form-data", &framed), None);
        assert_eq!(
            parse_fields("multipart/form-data; boundary=", &framed),
            None
        );
    }

    #[test]
    fn truncated_body_is_none() {
        let framed = part("email", "a@x.com");
        assert_eq!(parse_fields(CT, &framed), None);
    }

    #[test]
    fn base64_transfer_encoding_decodes_with_raw_fallback() {
        let encoded = b"--abc123\r\nContent-Disposition: form-data; name=\"code\"\r\nContent-Transfer-Encoding: base64\r\n\r\nMTIzNDU2\r\n".to_vec();
        let framed = frame(&[encoded]);
        assert_eq!(
            parse_fields(CT, &framed),
            Some(vec![("code".to_owned(), "123456".to_owned())])
        );
        // Bad padding raises in Python too, so the raw bytes survive.
        let broken = b"--abc123\r\nContent-Disposition: form-data; name=\"code\"\r\nContent-Transfer-Encoding: base64\r\n\r\nabc\r\n".to_vec();
        let framed = frame(&[broken]);
        assert_eq!(
            parse_fields(CT, &framed),
            Some(vec![("code".to_owned(), "abc".to_owned())])
        );
        // Non-alphabet bytes are discarded before decoding (`b64decode`
        // without `validate`), so `!!!` decodes to empty — not raw.
        let junk = b"--abc123\r\nContent-Disposition: form-data; name=\"code\"\r\nContent-Transfer-Encoding: base64\r\n\r\n!!!\r\n".to_vec();
        let framed = frame(&[junk]);
        assert_eq!(
            parse_fields(CT, &framed),
            Some(vec![("code".to_owned(), String::new())])
        );
    }
}
