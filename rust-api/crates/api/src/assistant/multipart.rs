//! Minimal `multipart/form-data` codec for the transcribe surface (D-06).
//!
//! Ports the two multipart touchpoints of
//! `apps/api/pi_dash/assistant/views/transcribe.py`:
//!
//! * inbound: `request.FILES.get("file")` plus the optional `language` /
//!   `response_format` text fields (`request.data`, `:98-103`). Django
//!   parses the framing; [`parse`] is the equivalent over the raw body.
//! * outbound: `httpx.post(url, files=..., data=...)` (`:117`) —
//!   [`encode`] frames the provider request the same way (field order
//!   `model`, `language?`, `response_format?`, then the `file` part).
//!
//! Hand-rolled because the workspace pins `axum`/`reqwest` without their
//! `multipart` features and the foundation `Cargo.toml` files are
//! read-only for port issues (PIDASHCONV-251 precedent). The codec covers
//! exactly what this surface needs: `Content-Disposition: form-data`
//! parts with `name`, optional `filename`, and an optional per-part
//! `Content-Type`. Anything fancier (nested multiparts, `filename*`
//! continuations) is out of scope and fails closed as `no_audio`, exactly
//! like a Django parse that yields no `file` part.
//!
//! Delimiter matching follows RFC 7578 §4.1: the boundary only counts at
//! a line edge (`\r\n--boundary` with a `--` / line terminator after it),
//! so binary audio that merely contains the boundary bytes cannot
//! false-split the body.

/// One parsed part: a text field or an uploaded file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// `Content-Disposition` `name` parameter (last part wins, like
    /// Django's `QueryDict.get`).
    pub name: String,
    /// `filename` parameter, when present (an empty string does not
    /// count: Django files a `filename=""` part under POST, not FILES).
    pub filename: Option<String>,
    /// Per-part `Content-Type` header, when present.
    pub content_type: Option<String>,
    /// Raw part bytes (transport padding stripped).
    pub body: Vec<u8>,
}

/// Whether the part is a file upload: `filename` was present and
/// non-empty. A `filename=""` part (the browser empty-file-input shape)
/// lands in Django's POST, never in FILES, so it must not count here —
/// otherwise an empty upload would forward silence to the provider
/// instead of answering `no_audio`.
pub fn is_file(part: &Part) -> bool {
    part.filename
        .as_deref()
        .is_some_and(|name| !name.is_empty())
}

/// Split a `multipart/form-data` body into its parts.
///
/// `content_type` is the request's `Content-Type` header value;
/// `body` the raw bytes. Returns an empty vec when the framing is absent
/// or unusable (no boundary, no parts) — the caller then answers
/// `no_audio`, mirroring a Django parse with no `file` entry.
pub fn parse(content_type: &str, body: &[u8]) -> Vec<Part> {
    let Some(boundary) = boundary_of(content_type) else {
        return Vec::new();
    };
    if boundary.is_empty() || boundary.len() > 70 {
        return Vec::new();
    }
    split_parts(body, boundary.as_bytes())
}

/// Frame an outbound transcription request: `model`, then the optional
/// `language` / `response_format` text fields, then the `file` part —
/// the `httpx.post(files=..., data=...)` order (`transcribe.py:97-114`).
///
/// Returns `(content_type, body)`. The boundary is fixed but
/// content-checked: when the audio already contains it, a numeric suffix
/// extends it until it is unique, so framing can never corrupt a payload.
pub fn encode(
    model: &str,
    language: Option<&str>,
    response_format: Option<&str>,
    filename: &str,
    file_content_type: &str,
    file_bytes: &[u8],
) -> (String, Vec<u8>) {
    let mut boundary = String::from("pidash-transcribe-7ma4yw3z3x8k9d2v");
    let delimiter = |b: &str| format!("--{b}");
    while contains_delimiter_line(file_bytes, delimiter(&boundary).as_bytes()) {
        boundary.push('0');
    }
    let mut out = Vec::new();
    let mut field = |name: &str, value: &str| {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        out.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    };
    field("model", model);
    if let Some(language) = language {
        field("language", language);
    }
    if let Some(format) = response_format {
        field("response_format", format);
    }
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    out.extend_from_slice(format!("Content-Type: {file_content_type}\r\n\r\n").as_bytes());
    out.extend_from_slice(file_bytes);
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), out)
}

/// `multipart/form-data; boundary=...` → the boundary token (dequoted).
fn boundary_of(content_type: &str) -> Option<String> {
    let (media, params) = content_type.split_once(';')?;
    if !media.trim().eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    for param in params.split(';') {
        let (key, value) = param.split_once('=')?;
        if key.trim().eq_ignore_ascii_case("boundary") {
            let token = value.trim();
            let unquoted = token
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(token);
            return Some(unquoted.to_string());
        }
    }
    None
}

/// Split on line-anchored `--boundary` delimiters (RFC 7578 §4.1).
fn split_parts(body: &[u8], boundary: &[u8]) -> Vec<Part> {
    let mut delimiter = Vec::with_capacity(boundary.len() + 2);
    delimiter.extend_from_slice(b"--");
    delimiter.extend_from_slice(boundary);
    // Offsets where a delimiter starts a line (body start counts).
    let mut marks: Vec<usize> = Vec::new();
    let mut i = 0;
    while i + delimiter.len() <= body.len() {
        if body[i..].starts_with(&delimiter)
            && (i == 0 || &body[i - 2..i] == b"\r\n")
            && is_terminated(&body[i + delimiter.len()..])
        {
            marks.push(if i == 0 { 0 } else { i - 2 });
            i += delimiter.len();
        } else {
            i += 1;
        }
    }
    let mut parts = Vec::new();
    for window in marks.windows(2) {
        let chunk = &body[window[0]..window[1]];
        if let Some(part) = parse_chunk(chunk, &delimiter) {
            parts.push(part);
        }
    }
    parts
}

/// Whether the bytes after a delimiter start a part (`\r\n`), close the
/// body (`--`), or are padding before one of those.
fn is_terminated(after: &[u8]) -> bool {
    if after.starts_with(b"\r\n") || after.starts_with(b"--") {
        return true;
    }
    // Transport padding (spaces) before the line break or close.
    let mut j = 0;
    while j < after.len() && after[j] == b' ' {
        j += 1;
    }
    after[j..].starts_with(b"\r\n") || after[j..].starts_with(b"--")
}

/// Parse one delimiter-to-delimiter chunk; `None` for the close or for a
/// preamble-only chunk.
fn parse_chunk(chunk: &[u8], delimiter: &[u8]) -> Option<Part> {
    // Find the opening delimiter (a leading preamble is skipped).
    let mut start = None;
    let mut i = 0;
    while i + delimiter.len() <= chunk.len() {
        if chunk[i..].starts_with(delimiter)
            && (i == 0 || (i >= 2 && &chunk[i - 2..i] == b"\r\n"))
            && is_terminated(&chunk[i + delimiter.len()..])
        {
            start = Some(i);
            break;
        }
        i += 1;
    }
    let mut body = &chunk[start? + delimiter.len()..];
    body = body.strip_prefix(b"\r\n").unwrap_or(body);
    if body.starts_with(b"--") {
        return None; // close delimiter (possibly with padding/epilogue)
    }
    // No trailing strip: the chunk already ends where the next
    // delimiter's line break begins (marks exclude it), so a payload
    // that itself ends in CRLF keeps its bytes — like Django.
    let sep = find_header_end(body)?;
    let (raw_headers, payload) = body.split_at(sep);
    let payload = &payload[4..];
    let mut name: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
    for line in split_lines(raw_headers) {
        let line = String::from_utf8_lossy(line);
        if let Some(rest) = header_value(&line, "content-disposition") {
            for (key, value) in disposition_params(rest) {
                match key.to_ascii_lowercase().as_str() {
                    "name" => name = Some(value),
                    "filename" => filename = Some(value),
                    _ => {}
                }
            }
        } else if let Some(rest) = header_value(&line, "content-type") {
            content_type = Some(rest.trim().to_string());
        }
    }
    Some(Part {
        name: name?,
        filename,
        content_type,
        body: payload.to_vec(),
    })
}

/// Offset of the blank line ending the part headers (`\r\n\r\n`).
fn find_header_end(body: &[u8]) -> Option<usize> {
    body.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Split header bytes on CRLF.
fn split_lines(headers: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + 1 < headers.len() {
        if headers[i] == b'\r' && headers[i + 1] == b'\n' {
            lines.push(&headers[start..i]);
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    lines.push(&headers[start..]);
    lines
}

/// `Header-Name: value` (case-insensitive name), trimmed.
fn header_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (key, value) = line.split_once(':')?;
    if key.trim().eq_ignore_ascii_case(name) {
        Some(value.trim())
    } else {
        None
    }
}

/// `form-data; name="x"; filename="y"` → `[(name, x), (filename, y)]`.
/// Quoted values dequote; backslash escapes collapse (`\"` → `"`).
fn disposition_params(rest: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for param in rest.split(';').skip(1) {
        if let Some((key, value)) = param.split_once('=') {
            out.push((key.trim().to_string(), dequote(value.trim())));
        }
    }
    out
}

fn dequote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Whether `needle` already opens a line in `haystack` (the boundary
/// uniqueness check for [`encode`]).
fn contains_delimiter_line(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    for i in 0..=haystack.len() - needle.len() {
        if &haystack[i..i + needle.len()] == needle
            && (i == 0
                || (i >= 2 && &haystack[i - 2..i] == b"\r\n")
                || (i >= 1 && haystack[i - 1] == b'\n'))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn httpx_like_body() -> (String, Vec<u8>) {
        // Framing in the shape httpx emits for `files={"file": AUDIO}`.
        let boundary = "aBcDeF123456";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.webm\"\r\nContent-Type: audio/webm\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(b"RIFFxxxxWAVE");
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    #[test]
    fn parses_httpx_file_upload() {
        let (content_type, body) = httpx_like_body();
        let parts = parse(&content_type, &body);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "file");
        assert_eq!(parts[0].filename.as_deref(), Some("clip.webm"));
        assert_eq!(parts[0].content_type.as_deref(), Some("audio/webm"));
        assert_eq!(parts[0].body, b"RIFFxxxxWAVE");
        assert!(is_file(&parts[0]));
    }

    #[test]
    fn parses_text_fields_alongside_the_file() {
        let boundary = "b2";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nen\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a\"\r\nContent-Type: application/octet-stream\r\n\r\nBYTES\r\n\
             --{boundary}--\r\n"
        );
        let parts = parse(
            &format!("multipart/form-data; boundary={boundary}"),
            body.as_bytes(),
        );
        assert_eq!(parts.len(), 3);
        assert!(!is_file(&parts[0]));
        assert_eq!(parts[0].body, b"whisper-1");
        assert_eq!(parts[1].name, "language");
        assert_eq!(parts[2].filename.as_deref(), Some("a"));
    }

    #[test]
    fn non_multipart_body_yields_no_parts() {
        assert!(parse("application/json", b"{}").is_empty());
        assert!(parse("text/plain", b"hi").is_empty());
    }

    #[test]
    fn boundary_inside_audio_does_not_split() {
        let boundary = "ZZZ";
        let mut audio = b"RIFF".to_vec();
        audio.extend_from_slice(b"--ZZZ-not-a-delimiter");
        audio.extend_from_slice(b"\r\n--ZZZfake");
        let (content_type, body) = (
            format!("multipart/form-data; boundary={boundary}"),
            [
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a\"\r\n\r\n")
                    .into_bytes(),
                audio.clone(),
                format!("\r\n--{boundary}--\r\n").into_bytes(),
            ]
            .concat(),
        );
        let parts = parse(&content_type, &body);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].body, audio);
    }

    #[test]
    fn encode_round_trips_through_parse() {
        let audio = b"\x00\x01binary\xffpayload".to_vec();
        let (content_type, body) = encode(
            "whisper-1",
            Some("en"),
            None,
            "clip.webm",
            "audio/webm",
            &audio,
        );
        let parts = parse(&content_type, &body);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].name, "model");
        assert_eq!(parts[0].body, b"whisper-1");
        assert_eq!(parts[1].name, "language");
        assert_eq!(parts[1].body, b"en");
        assert_eq!(parts[2].name, "file");
        assert_eq!(parts[2].filename.as_deref(), Some("clip.webm"));
        assert_eq!(parts[2].content_type.as_deref(), Some("audio/webm"));
        assert_eq!(parts[2].body, audio);
    }

    #[test]
    fn empty_filename_is_not_a_file() {
        // The browser empty-file-input shape: Django files it under
        // POST, so `FILES.get("file")` misses and the view answers
        // `no_audio`.
        let boundary = "e1";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n--{boundary}--\r\n"
        );
        let parts = parse(
            &format!("multipart/form-data; boundary={boundary}"),
            body.as_bytes(),
        );
        assert_eq!(parts.len(), 1);
        assert!(!is_file(&parts[0]));
    }

    #[test]
    fn encode_extends_a_colliding_boundary() {
        let audio = b"--pidash-transcribe-7ma4yw3z3x8k9d2v\r\n".to_vec();
        let (content_type, body) = encode("m", None, None, "a", "application/octet-stream", &audio);
        let parts = parse(&content_type, &body);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].body, audio);
        assert!(content_type.contains("pidash-transcribe-7ma4yw3z3x8k9d2v0"));
    }
}
