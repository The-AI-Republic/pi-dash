//! Shared mail-send helper for the D-07 mail tasks (owned by T5, PIDASHCONV-216;
//! T6 reuses this module, never forks it).
//!
//! Port of the send block repeated verbatim in all four auth tasks
//! (`apps/api/pi_dash/bgtasks/magic_link_code_task.py:28-63`,
//! `forgot_password_task.py:28-71`,
//! `user_email_update_task.py:27-64,72-113`):
//!
//! * `get_email_configuration()` 7-tuple (`instance_value.py:57-74`) via the
//!   existing [`pidash_services::license::config`] composition (DB rows with
//!   call-time env/default fallback — no new resolver is written here);
//! * `get_connection(host, int(port), user, pass, use_tls == "1",
//!   use_ssl == "1")` — plain string compares, `int()` failure swallowed;
//! * `EmailMultiAlternatives(subject, text, from, [to])` + the HTML
//!   alternative, `.send()`, success log;
//! * ANY exception → `log_exception` + return `None` (swallow, never a
//!   retry signal). The worker-facing wrapper [`send_mail`] therefore always
//!   succeeds: the caller acknowledges unconditionally.
//!
//! Django-semantics notes (each is load-bearing for byte equality):
//!
//! * Templates are Django templates using only `{{ var }}` interpolation
//!   (no `{% %}` tags, no filters in any of the three files), so minijinja
//!   renders them identically. Django renders a missing variable as `""`
//!   (`string_if_invalid` default) — and every template here references
//!   `{{ current_site }}`, which no task context provides (see
//!   `F-AUTH.rendered.json` `context_keys`). A strict-undefined render
//!   would therefore swallow all four sends; the environment below keeps
//!   minijinja's lenient-missing (`""`) rendering to match Django byte for
//!   byte. Context values are pre-escaped with Django's exact escape map
//!   (`&<>"'` → `&amp;&lt;&gt;&quot;&#x27;`, `&` first) with autoescape off,
//!   which reproduces Django autoescape output for every input (minijinja's
//!   own escaping differs on `"`/`'`).
//! * `generate_plain_text_from_html` (`pi_dash/utils/email.py`) is ported as
//!   [`plain_text_from_html`], including Django `strip_tags` semantics
//!   (tags removed, `&...;` references kept verbatim).
//! * MIME shape mirrors `EmailMessage.message()` empirically: outer
//!   `multipart/alternative`, `text/plain` then `text/html`, each
//!   `charset="utf-8"` with `7bit` CTE for ASCII bodies and `8bit` for
//!   non-ASCII ones (raw UTF-8 either way), plus `Date`/`Message-ID`
//!   headers. Boundary/Date/Message-ID vary per send like Django's.
//! * SMTP wire behavior mirrors `smtplib` + Django's `EmailBackend`
//!   (`configuration.py` shape, same as the sibling client in
//!   `pidash-api`'s license handlers, which cannot be reused here without
//!   inverting the crate graph): greeting, EHLO→HELO fallback, STARTTLS
//!   with re-EHLO, AUTH (`CRAM-MD5` > `PLAIN` > `LOGIN` over advertised
//!   methods, only when username AND password are both set), `MAIL`/`RCPT`
//!   (250/251 accept), `DATA`, best-effort `QUIT`. An empty `EMAIL_HOST`
//!   connects to `localhost` (what `socket.create_connection(("", p))`
//!   does); a missing one falls back to Django's `EMAIL_HOST` default,
//!   also `localhost`. A missing `EMAIL_FROM` falls back to Django's
//!   `DEFAULT_FROM_EMAIL` (`webmaster@localhost`; the project overrides
//!   neither default in `settings/common.py`).
//! * `int(EMAIL_PORT)`: Python `int()` over the string form (whitespace +
//!   `+`/`-` + `_` separators); unparseable or out-of-`u16`-range ports
//!   fail the send exactly like Python's `ValueError`/`OverflowError` at
//!   connect — i.e. swallowed.
//! * Python `str()` over task args (`py_str`): `None` → `"None"`,
//!   `True`/`False`, numbers verbatim. f-string subjects and the reset
//!   link interpolate these, never Rust `Display` (`true` ≠ `True`).

use std::collections::HashMap;

use pidash_db::config::{ConfigRegistry, ConfigValue};

/// `emails/auth/magic_signin.html` — serves `magic_link` and
/// `send_email_update_magic_code` with different subjects.
pub const MAGIC_SIGNIN_TEMPLATE: &str =
    include_str!("../../../../../apps/api/templates/emails/auth/magic_signin.html");
/// `emails/auth/forgot_password.html` — serves `forgot_password`.
pub const FORGOT_PASSWORD_TEMPLATE: &str =
    include_str!("../../../../../apps/api/templates/emails/auth/forgot_password.html");
/// `emails/user/email_updated.html` — serves `send_email_update_confirmation`.
pub const EMAIL_UPDATED_TEMPLATE: &str =
    include_str!("../../../../../apps/api/templates/emails/user/email_updated.html");
/// `emails/exports/analytics.html` — serves `analytic_export_task`
/// (`analytic_plot_export.py:55`, rendered with `{}`).
pub const ANALYTICS_EXPORT_TEMPLATE: &str =
    include_str!("../../../../../apps/api/templates/emails/exports/analytics.html");

/// Django's `DEFAULT_FROM_EMAIL` global default: the project does not
/// override it, so a null `EMAIL_FROM` row falls back here
/// (`EmailMessage.from_email` → `settings.DEFAULT_FROM_EMAIL`).
pub const DEFAULT_FROM_EMAIL: &str = "webmaster@localhost";
/// Django's `EMAIL_HOST` global default (`global_settings.py`): used when
/// the resolved host is null or empty (see [`resolve_smtp`] — an empty host
/// string connects to localhost in `smtplib` too).
pub const DEFAULT_SMTP_HOST: &str = "localhost";
/// `EMAIL_FROM` default when no row and no env
/// (`instance_value.py:70-72`).
pub const DEFAULT_EMAIL_FROM: &str = "Team Pi Dash <team@airepublic.com>";

/// Python `str()` over a JSON task arg (`magic_link_code_task.py` f-string
/// subjects, `forgot_password_task.py:26-27` link building): `None` →
/// `"None"`, bools capitalized, numbers verbatim, strings as-is.
pub fn py_str(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "None".to_owned(),
        serde_json::Value::Bool(true) => "True".to_owned(),
        serde_json::Value::Bool(false) => "False".to_owned(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Django `django.utils.html.escape`: `&` first, then `<`, `>`, `"`, `'`.
/// Applied to every context value before rendering (autoescape stays off),
/// reproducing Django autoescape bytes exactly.
pub fn django_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
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

/// Django `strip_tags` (`django.utils.html`, as used by
/// `generate_plain_text_from_html`): tags removed, `&...;` references kept
/// verbatim (entities are NOT decoded), applied iteratively until no
/// `<...>` span changes.
pub fn strip_tags(html: &str) -> String {
    let mut value = html.to_owned();
    loop {
        let next = strip_once(&value);
        if next == value {
            return next;
        }
        value = next;
    }
}

/// End of a tag starting at the `<` of `rest`: comments close at the first
/// `-->`; declarations/processing instructions at the first `>`; open and
/// close tags at the first `>` outside single/double quotes (HTMLParser's
/// `parse_starttag` is quote-aware). `None` when unterminated.
fn tag_end(rest: &str) -> Option<usize> {
    if rest.starts_with("<!--") {
        return rest.find("-->").map(|end| end + 3);
    }
    let bytes = rest.as_bytes();
    let mut quote = 0u8;
    // Declarations (`<!DOCTYPE>`, `<![endif]>`) and PIs (`<?...?>`) never
    // carry quotes worth honoring; the quote scan is harmless for them.
    let mut i = if rest.starts_with("</") || rest.starts_with("<!") || rest.starts_with("<?") {
        2
    } else {
        1
    };
    while i < bytes.len() {
        let b = bytes[i];
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
        } else if b == b'\'' || b == b'"' {
            quote = b;
        } else if b == b'>' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn strip_once(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        let rest = &value[i..];
        if !rest.starts_with('<') {
            let ch = rest.chars().next().expect("non-empty");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        match tag_end(rest) {
            // Unterminated `<` rest: HTMLParser keeps it verbatim (the
            // incomplete-tag path returns -1 and the data is emitted), so
            // the outer loop sees no change and stops — except an
            // unterminated COMMENT, whose rest is comment data and dropped.
            None if rest.starts_with("<!--") => break,
            None => {
                out.push_str(rest);
                break;
            }
            Some(len) => i += len,
        }
    }
    out
}

/// `generate_plain_text_from_html` (`pi_dash/utils/email.py:19-44`): drop
/// `<style>` blocks (case-insensitive, dot-all, non-greedy), strip tags,
/// collapse 3+-newline runs (allowing whitespace-only lines between) to
/// `\n\n`, wrap in leading/trailing blank lines.
pub fn plain_text_from_html(html: &str) -> String {
    let style_re = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("static regex");
    let collapsed_re = regex::Regex::new(r"\n\s*\n\s*\n+").expect("static regex");
    let no_style = style_re.replace_all(html, "");
    let stripped = strip_tags(&no_style);
    let collapsed = collapsed_re.replace_all(&stripped, "\n\n");
    format!("\n\n{}\n\n", collapsed.trim())
}

/// Render one of the embedded templates with pre-escaped string values.
/// Missing names render as `""`, matching Django (`string_if_invalid`).
pub fn render_template(source: &str, context: &HashMap<String, String>) -> Result<String, String> {
    let mut env = minijinja::Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
    // Django keeps the template file's trailing newline; minijinja strips
    // it by default (Jinja2 `keep_trailing_newline=False`).
    env.set_keep_trailing_newline(true);
    env.add_template("mail.html", source)
        .map_err(|e| format!("bad template: {e}"))?;
    let template = env
        .get_template("mail.html")
        .map_err(|e| format!("bad template: {e}"))?;
    // Values arrive pre-escaped ([`django_escape`]); the map holds owned
    // strings either way, so rendering is a plain substitution.
    template
        .render(context)
        .map_err(|e| format!("render failed: {e}"))
}

/// Build the escaped render context from raw task values.
pub fn escape_context(raw: &[(&str, String)]) -> HashMap<String, String> {
    raw.iter()
        .map(|(k, v)| ((*k).to_owned(), django_escape(v)))
        .collect()
}

/// Resolved SMTP send inputs: `get_email_configuration()` 7-tuple mapped
/// exactly like `get_connection(...)` in every task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpSend {
    pub host: String,
    pub port: i64,
    pub username: Option<String>,
    pub password: Option<String>,
    pub use_tls: bool,
    pub use_ssl: bool,
    pub from_email: String,
}

/// Map the 7-tuple to send inputs (`EMAIL_HOST`, `EMAIL_HOST_USER`,
/// `EMAIL_HOST_PASSWORD`, `EMAIL_PORT`, `EMAIL_USE_TLS`, `EMAIL_USE_SSL`,
/// `EMAIL_FROM` in order). The `=="1"` compares are plain string compares;
/// the port keeps Python's unbounded `int()` — range-checked only at
/// connect, where an overflow fails the send like `OverflowError`.
pub fn resolve_smtp(values: &[ConfigValue]) -> Result<SmtpSend, String> {
    if values.len() != 7 {
        return Err(format!("expected 7 email values, got {}", values.len()));
    }
    let mut it = values.iter();
    let string_of = |v: &ConfigValue| match v {
        ConfigValue::Str(s) => Some(s.clone()),
        ConfigValue::Int(i) => Some(i.to_string()),
        ConfigValue::Float(f) => Some(f.to_string()),
        ConfigValue::Bool(true) => Some("True".to_owned()),
        ConfigValue::Bool(false) => Some("False".to_owned()),
        ConfigValue::Null => None,
    };
    let host_raw = it.next().and_then(string_of);
    let username = it.next().and_then(string_of);
    let password = it.next().and_then(string_of);
    let port_raw = it.next().and_then(string_of);
    let tls_raw = it.next().and_then(string_of);
    let ssl_raw = it.next().and_then(string_of);
    let from_raw = it.next().and_then(string_of);
    // `int(EMAIL_PORT)`: `None` (null row, no env) is `TypeError`; garbage
    // is `ValueError` — both fail the send.
    let port = match port_raw {
        Some(raw) => parse_py_int(&raw).ok_or_else(|| format!("bad EMAIL_PORT: {raw:?}"))?,
        None => return Err("missing EMAIL_PORT".to_owned()),
    };
    Ok(SmtpSend {
        host: match host_raw {
            Some(h) if !h.is_empty() => h,
            _ => DEFAULT_SMTP_HOST.to_owned(),
        },
        port,
        username,
        password,
        use_tls: tls_raw.as_deref() == Some("1"),
        use_ssl: ssl_raw.as_deref() == Some("1"),
        from_email: match from_raw {
            Some(f) if !f.is_empty() => f,
            _ => DEFAULT_FROM_EMAIL.to_owned(),
        },
    })
}

/// Python `int()` over a string: surrounding ASCII whitespace, one optional
/// `+`/`-`, single `_` separators between digits, arbitrary width
/// (saturating past `i64` — the range check happens at connect).
pub fn parse_py_int(text: &str) -> Option<i64> {
    let stripped = text.trim_matches(|c: char| c.is_ascii_whitespace());
    let (negative, digits) = match stripped.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, stripped.strip_prefix('+').unwrap_or(stripped)),
    };
    if digits.is_empty() {
        return None;
    }
    let mut magnitude: i64 = 0;
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        let digit = ch.to_digit(10)? as i64;
        prev_underscore = false;
        magnitude = magnitude.checked_mul(10)?.checked_add(digit)?;
    }
    if prev_underscore {
        return None;
    }
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

/// One rendered mail: the `EmailMultiAlternatives(subject, text, from,
/// [to]) + html alternative` payload.
pub struct OutgoingMail {
    pub to: String,
    pub subject: String,
    pub text_body: String,
    pub html_body: String,
}

/// True when any `\n`-separated line of `body` exceeds 998 UTF-8 bytes:
/// the SMTP line limit that flips Django to quoted-printable (probed
/// from `EmailMultiAlternatives`, PIDASHCONV-1104: ASCII 998 → `7bit`,
/// 999 → QP; `q`*997+é, 999 bytes → QP, `q`*990+é, 992 bytes → `8bit`).
/// A trailing `\r` is not measured: Django splits with `splitlines()`
/// (`SafeMIMEText.set_payload`), which strips the `\r` of `\r\n`.
fn body_needs_qp(body: &str) -> bool {
    body.split('\n')
        .any(|line| line.strip_suffix('\r').unwrap_or(line).len() > 998)
}

/// `quopri.body_encode` (CPython `email/quoprimime.py`, `maxlinelen=76`):
/// every byte outside tab / `0x20-0x3C` / `0x3E-0x7E` becomes `=XX`
/// (uppercase hex); lines wrap at 75 content chars + a `=` soft break,
/// never splitting an escape (a chunk ending on a literal `=` needs no
/// extra break); a trailing space/tab is encoded (or pushed past a soft
/// break when the line is full). Byte-oriented over the UTF-8 bytes,
/// exactly like the latin1 round-trip in `Charset.body_encode`. Joined
/// with `\n`; the caller's CRLF normalization runs after assembly.
/// NOTE the `room == 2` shape: CPython appends `line[-1] + soft_break`
/// (the break's own eol lives INSIDE the chunk), so the join newline
/// survives as a real line break — the break line is followed by an
/// empty line, never glue to the next chunk.
fn quoted_printable_encode(body: &str) -> String {
    let mut translated = String::with_capacity(body.len());
    for &byte in body.as_bytes() {
        if byte == b'\t'
            || (0x20..=0x3c).contains(&byte)
            || (0x3e..=0x7e).contains(&byte)
            || byte == b'\r'
            || byte == b'\n'
        {
            translated.push(byte as char);
        } else {
            translated.push_str(&format!("={byte:02X}"));
        }
    }
    if translated.is_empty() {
        return translated;
    }
    let mut encoded_lines: Vec<String> = Vec::new();
    for raw_line in translated.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let bytes = line.as_bytes();
        let mut start = 0usize;
        // Wrap while the remainder exceeds 76 chars (CPython: `while
        // start <= len - 77`).
        while bytes.len() - start >= 77 {
            let stop = start + 75;
            if bytes[stop - 2] == b'=' {
                encoded_lines.push(line[start..stop - 1].to_owned());
                start = stop - 2;
            } else if bytes[stop - 1] == b'=' {
                encoded_lines.push(line[start..stop].to_owned());
                start = stop - 1;
            } else {
                encoded_lines.push(format!("{}=", &line[start..stop]));
                start = stop;
            }
        }
        let rest = &line[start..];
        match rest.as_bytes().last() {
            Some(last) if *last == b' ' || *last == b'\t' => {
                let room = 77 - rest.len();
                let quoted = format!("={last:02X}");
                if room >= 3 {
                    encoded_lines.push(format!("{}{quoted}", &rest[..rest.len() - 1]));
                } else if room == 2 {
                    // `line[-1] + soft_break`, then the real break:
                    // CPython's chunk ends with the break's own eol, so
                    // the join newline survives (`...X=\n\n<next>`).
                    encoded_lines.push(format!("{}{}=", &rest[..rest.len() - 1], *last as char));
                    encoded_lines.push(String::new());
                } else {
                    encoded_lines.push(format!("{}=", &rest[..rest.len() - 1]));
                    encoded_lines.push(quoted);
                }
            }
            _ => encoded_lines.push(rest.to_owned()),
        }
    }
    encoded_lines.join("\n")
}

/// Render the MIME bytes (`EmailMessage.message()` shape, verified
/// against the interpreter): `multipart/alternative` with `text/plain`
/// then `text/html` (`charset="utf-8"`, CTE `7bit` for short ASCII
/// parts, `8bit` for short non-ASCII parts, `quoted-printable` when
/// any body line exceeds 998 bytes — the probed Django selection).
/// `From`, `To` and `Subject` go through the two-stage header render
/// (RFC 2047 plus IDNA plus the generator refold); `Date` and
/// `Message-ID` are synthesized like Django's. CR/LF headers and
/// unparsable addresses fail, like the errors Django raises out of
/// `message()`.
pub fn message_bytes(
    from: &str,
    mail: &OutgoingMail,
    boundary: &str,
    date: &str,
    msg_id: &str,
) -> Result<Vec<u8>, String> {
    let part = |ctype: &str, body: &str| {
        let (cte, encoded) = if body_needs_qp(body) {
            ("quoted-printable", quoted_printable_encode(body))
        } else if body.is_ascii() {
            ("7bit", body.to_owned())
        } else {
            ("8bit", body.to_owned())
        };
        format!(
            "--{boundary}\r\nContent-Type: {ctype}; charset=\"utf-8\"\r\nContent-Transfer-Encoding: {cte}\r\n\r\n{encoded}\r\n"
        )
    };
    let from = address_header("From", from)?;
    let to = address_header("To", &mail.to)?;
    let subject = subject_header(&mail.subject)?;
    let mut out = format!(
        "{from}\r\n{to}\r\n{subject}\r\nDate: {date}\r\nMessage-ID: {msg_id}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n",
    );
    out.push_str(&part("text/plain", &mail.text_body));
    out.push_str(&part("text/html", &mail.html_body));
    out.push_str(&format!("--{boundary}--\r\n"));
    Ok(crlf_dot_stuff(out))
}

/// `message.as_bytes(linesep="\r\n")` (what `EmailBackend.send`
/// transmits): normalize every line ending to CRLF first, then
/// dot-stuffing (`smtplib.data` quotes leading periods). Shared by the
/// `alternative` and `mixed` builders (same bytes either way).
fn crlf_dot_stuff(out: String) -> Vec<u8> {
    let crlf = out.replace("\r\n", "\n").replace('\n', "\r\n");
    let mut stuffed = String::with_capacity(crlf.len());
    for line in crlf.split_inclusive('\n') {
        if line.starts_with('.') {
            stuffed.push('.');
        }
        stuffed.push_str(line);
    }
    stuffed.into_bytes()
}

/// One file attached to an outgoing mail: `msg.attach(filename,
/// content)` (`analytic_plot_export.py:86` attaches the CSV text).
/// Content is always `str` on this path (never bytes).
pub struct MailAttachment<'a> {
    pub filename: String,
    pub content: &'a str,
}

/// The attachment MIME type the way `EmailMessage.attach` guesses it:
/// `mimetypes.guess_type(filename)[0]`. The export task only ever
/// attaches `{slug}-analytics.csv` (`text/csv`, in CPython's builtin
/// table on every platform); anything else is the `None.split`
/// `AttributeError` path, which fails the send like Python.
pub fn attachment_mimetype(filename: &str) -> Result<&'static str, String> {
    if filename.to_ascii_lowercase().ends_with(".csv") {
        return Ok("text/csv");
    }
    Err(format!("cannot guess mimetype for {filename:?}"))
}

/// `Content-Disposition` filename parameter: quoted ASCII verbatim, or
/// RFC 2231 (`filename*=utf-8''...`) when the name is non-ASCII (what
/// `Message.add_header` emits). Slugs are ASCII in practice; the
/// fallback only makes the builder total.
fn disposition_filename(filename: &str) -> String {
    if filename.is_ascii() {
        return format!("filename=\"{filename}\"");
    }
    let mut encoded = String::with_capacity(filename.len());
    for byte in filename.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("filename*=utf-8''{encoded}")
}

/// Render the MIME bytes for a body + attachments mail with no HTML
/// alternative (`EmailMessage.message()` shape, probed against Django
/// 4.2): outer `multipart/mixed`, the `text/plain` body first, then one
/// part per attachment (`{mimetype}; charset="utf-8"`,
/// `Content-Disposition: attachment`). Each part carries its own
/// `MIME-Version: 1.0`; CTE is `quoted-printable` when any line of the
/// part exceeds 998 bytes, else `7bit` for ASCII payloads and `8bit`
/// otherwise (raw UTF-8 either way — Django never base64s `str`
/// attachments). Header order is Django's (`Content-Type`,
/// `MIME-Version`, `Subject`, `From`, `To`, `Date`, `Message-ID`).
#[allow(clippy::too_many_arguments)]
pub fn message_bytes_mixed(
    from: &str,
    to: &str,
    subject: &str,
    text_body: &str,
    attachments: &[MailAttachment<'_>],
    boundary: &str,
    date: &str,
    msg_id: &str,
) -> Result<Vec<u8>, String> {
    // Same selection as `message_bytes`: `SafeMIMEText.set_payload`
    // (`django/core/mail/message.py:168-175`) QP-encodes the body and
    // every `str` attachment whose lines exceed 998 bytes alike.
    let part = |body: &str| {
        if body_needs_qp(body) {
            ("quoted-printable", quoted_printable_encode(body))
        } else if body.is_ascii() {
            ("7bit", body.to_owned())
        } else {
            ("8bit", body.to_owned())
        }
    };
    let subject = subject_header(subject)?;
    let from = address_header("From", from)?;
    let to = address_header("To", to)?;
    let mut out = format!(
        "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\
         MIME-Version: 1.0\r\n\
         {subject}\r\n\
         {from}\r\n\
         {to}\r\n\
         Date: {date}\r\n\
         Message-ID: {msg_id}\r\n\
         \r\n"
    );
    let (body_cte, encoded_body) = part(text_body);
    out.push_str(&format!(
        "--{boundary}\r\n\
         Content-Type: text/plain; charset=\"utf-8\"\r\n\
         MIME-Version: 1.0\r\n\
         Content-Transfer-Encoding: {body_cte}\r\n\
         \r\n\
         {encoded_body}\r\n",
    ));
    for attachment in attachments {
        let mimetype = attachment_mimetype(&attachment.filename)?;
        let (part_cte, encoded) = part(attachment.content);
        out.push_str(&format!(
            "--{boundary}\r\n\
             Content-Type: {mimetype}; charset=\"utf-8\"\r\n\
             MIME-Version: 1.0\r\n\
             Content-Transfer-Encoding: {part_cte}\r\n\
             Content-Disposition: attachment; {disposition}\r\n\
             \r\n\
             {encoded}\r\n",
            disposition = disposition_filename(&attachment.filename),
        ));
    }
    out.push_str(&format!("--{boundary}--\r\n"));
    Ok(crlf_dot_stuff(out))
}

/// `email.header.MAXLINELEN` (`header.py:28`): the fold width both
/// header stages wrap at.
const HEADER_MAX_LINE: usize = 78;
/// RFC 2047 chrome around one `utf-8` encoded word (`charset.py:318`:
/// `len(charset) + RFC2047_CHROME_LEN` = 5 + 7): `=?`, the `?q?`/`?b?`
/// marker, `?=`.
const ENCODED_WORD_CHROME: usize = 12;

// ---- RFC 2047 header encoding (PIDASHCONV-1286) ----
//
// Django renders `From`/`To`/`Subject` in two stages, and the wire
// bytes need both. Stage 1 runs at `msg[name] = value` time through
// `forbid_multi_line_headers` (`message.py:55-76`): CR/LF is rejected
// (`BadHeaderError`); `Subject` folds ASCII via `Header(val).encode()`
// and encodes non-ASCII via `Header(val, 'utf-8').encode()`; address
// headers pass ASCII through verbatim and sanitize non-ASCII piece by
// piece over `getaddresses`. Stage 2 is the compat32 generator refold
// (`_policybase.py:348-377`): `Header(stored, header_name).encode()`
// at `maxlinelen` 78 under the real `Name: ` prefix — a single long
// encoded word lands on its own continuation line
// (`Subject: \r\n =?utf-8?q?...?=`), it is never split mid-word.

/// Safe header bytes under `email.quoprimime._QUOPRI_HEADER_MAP`
/// (`quoprimime.py:62-66`): `-!*+/`, ASCII letters and digits pass
/// through; space becomes `_`; every other octet is `=XX`.
fn is_qp_header_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'!' | b'*' | b'+' | b'/')
}

/// `email.quoprimime.header_length` (`quoprimime.py:84-94`): the
/// Q-encoded width of raw bytes, chrome excluded.
fn qp_header_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .map(|&b| {
            if b == b' ' || is_qp_header_safe(b) {
                1
            } else {
                3
            }
        })
        .sum()
}

/// `email.quoprimime.header_encode` with the `utf-8` charset
/// (`quoprimime.py:127-145`): empty input encodes empty (no chrome).
fn qp_header_encode(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut out = String::from("=?utf-8?q?");
    for &byte in bytes {
        if byte == b' ' {
            out.push('_');
        } else if is_qp_header_safe(byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("={byte:02X}"));
        }
    }
    out.push_str("?=");
    out
}

/// `email.base64mime.header_length` (`base64mime.py:49-56`): 4 chars
/// per 3-byte group (or nonzero fraction thereof).
fn b64_header_len(nbytes: usize) -> usize {
    nbytes.div_ceil(3) * 4
}

/// `email.base64mime.header_encode` with the `utf-8` charset
/// (`base64mime.py:59-70`): standard padded base64; empty stays empty.
fn b64_header_encode(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    format!("=?utf-8?b?{}?=", STANDARD.encode(bytes))
}

/// `Header(val, 'utf-8').encode()` for the single-chunk values Django
/// passes (`header.py:309-387` + `charset.py:293-352`): the SHORTEST
/// encoder (B vs Q over the WHOLE value, ties to Q —
/// `_get_encoder`, `charset.py:354-367`) is fixed once, then the value
/// splits greedily per character so each encoded word fits its line
/// budget (first `78 - 12`, continuations `78 - 1 - 12`: one
/// continuation space plus the chrome), and the words join with
/// `\n ` (the `feed` assembly, `header.py:454-473`, emits first,
/// middles, last in order).
fn utf8_header_encode(value: &str) -> String {
    let bytes = value.as_bytes();
    let use_base64 = b64_header_len(bytes.len()) < qp_header_len(bytes);
    let encode = |chunk: &str| {
        if use_base64 {
            b64_header_encode(chunk.as_bytes())
        } else {
            qp_header_encode(chunk.as_bytes())
        }
    };
    let chunk_len = |chunk: &str| {
        if use_base64 {
            b64_header_len(chunk.len())
        } else {
            qp_header_len(chunk.as_bytes())
        }
    };
    // Greedy per-character split (`charset.py:330-348`).
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut budget = HEADER_MAX_LINE - ENCODED_WORD_CHROME;
    for ch in value.chars() {
        current.push(ch);
        if chunk_len(&current) > budget {
            current.pop();
            if words.is_empty() && current.is_empty() {
                // Python appends None (nothing fits the first line);
                // unreachable for utf-8 (one char encodes to at most
                // 12, below every budget) — emit the lone char.
                words.push(encode(&ch.to_string()));
            } else {
                words.push(encode(&current));
                current.clear();
                current.push(ch);
            }
            budget = HEADER_MAX_LINE - 1 - ENCODED_WORD_CHROME;
        }
    }
    words.push(encode(&current));
    let (first, rest) = words.split_first().expect("final push always runs");
    let mut out = first.clone();
    for word in rest {
        out.push('\n');
        out.push(' ');
        out.push_str(word);
    }
    out
}

/// One `(fws, part)` pair of the accumulator below.
struct AccPart {
    fws: String,
    part: String,
}

/// Port of `email.header._Accumulator` (`header.py:536-573`): the
/// current line as (folding whitespace, text) pairs plus the pending
/// header-name prefix length.
struct Accumulator {
    parts: Vec<AccPart>,
    initial_size: usize,
}

impl Accumulator {
    fn new(initial_size: usize) -> Self {
        Self {
            parts: Vec::new(),
            initial_size,
        }
    }

    fn push(&mut self, fws: &str, part: &str) {
        self.parts.push(AccPart {
            fws: fws.to_owned(),
            part: part.to_owned(),
        });
    }

    fn pop_from(&mut self, index: usize) -> Vec<AccPart> {
        self.parts.split_off(index)
    }

    fn pop(&mut self) -> (String, String) {
        self.parts
            .pop()
            .map(|pair| (pair.fws, pair.part))
            .unwrap_or_default()
    }

    /// `__len__` (`:555-557`) is the two-arg `sum` form: text width
    /// plus the pending prefix.
    fn len(&self) -> usize {
        self.parts
            .iter()
            .map(|pair| pair.fws.len() + pair.part.len())
            .sum::<usize>()
            + self.initial_size
    }

    fn part_count(&self) -> usize {
        self.parts.len()
    }

    fn to_line(&self) -> String {
        self.parts
            .iter()
            .map(|pair| format!("{}{}", pair.fws, pair.part))
            .collect()
    }

    fn reset(&mut self, start: Vec<AccPart>) {
        self.parts = start;
        self.initial_size = 0;
    }

    /// `is_onlyws` (`:569-570`): note `''.isspace()` is false, so a
    /// nonempty all-empty pair list is NOT whitespace-only.
    fn is_onlyws(&self) -> bool {
        if self.initial_size != 0 {
            return false;
        }
        if self.parts.is_empty() {
            return true;
        }
        let line = self.to_line();
        !line.is_empty() && line.chars().all(|ch| ch.is_whitespace())
    }
}

/// Port of `email.header._ValueFormatter` for the ASCII path
/// (`header.py:408-533`): `maxlinelen` 78, `splitchars` `;, \t`,
/// one-space continuation.
struct AsciiFolder {
    lines: Vec<String>,
    current: Accumulator,
}

impl AsciiFolder {
    fn new(header_len: usize) -> Self {
        Self {
            lines: Vec::new(),
            current: Accumulator::new(header_len),
        }
    }

    /// `newline` (`header.py:424-433`): a whitespace-only tail glues
    /// onto the previous line instead of standing alone.
    fn newline(&mut self) {
        let end = self.current.pop();
        if end != (" ".to_owned(), String::new()) {
            self.current.push(&end.0, &end.1);
        }
        if self.current.len() > 0 {
            if self.current.is_onlyws() && !self.lines.is_empty() {
                let tail = self.current.to_line();
                self.lines
                    .last_mut()
                    .expect("guarded by is_empty check")
                    .push_str(&tail);
            } else {
                self.lines.push(self.current.to_line());
            }
        }
        self.current.reset(Vec::new());
    }

    /// `add_transition` (`header.py:435-436`).
    fn add_transition(&mut self) {
        self.current.push(" ", "");
    }

    /// `_append_chunk` (`header.py:503-533`): past 78 chars the best
    /// split point wins scanning backward, `;` first, then `,`, then
    /// whitespace. With no split point and a pending header prefix,
    /// the prefix stands alone and the chunk moves to a continuation
    /// line; without a prefix the overlong line stands unfolded.
    fn append_chunk(&mut self, fws: &str, part: &str) {
        self.current.push(fws, part);
        if self.current.len() <= HEADER_MAX_LINE {
            return;
        }
        let mut found = None;
        for ch in [';', ',', ' ', '\t'] {
            for index in (1..self.current.part_count()).rev() {
                if ch == ' ' || ch == '\t' {
                    let pair_fws = &self.current.parts[index].fws;
                    if !pair_fws.is_empty() && pair_fws.starts_with(ch) {
                        found = Some(index);
                        break;
                    }
                }
                let prev = &self.current.parts[index - 1].part;
                if !prev.is_empty() && prev.ends_with(ch) {
                    found = Some(index);
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        if let Some(index) = found {
            let remainder = self.current.pop_from(index);
            self.lines.push(self.current.to_line());
            self.current.reset(remainder);
        } else {
            let (fws, part) = self.current.pop();
            if self.current.initial_size > 0 {
                self.newline();
                // "We don't use continuation_ws here because the
                // whitespace after a header should always be a space."
                let fws = if fws.is_empty() { " ".to_owned() } else { fws };
                self.current.push(&fws, &part);
            } else {
                self.current.push(&fws, &part);
            }
        }
    }

    /// `_ascii_split` (`header.py:481-501`): split on space/tab runs,
    /// keeping the separators, then feed `(fws, part)` pairs.
    fn ascii_split(&mut self, fws: &str, string: &str) {
        let combined = format!("{fws}{string}");
        let mut atoms = split_fws_runs(&combined);
        if atoms.first().is_some_and(|first| !first.is_empty()) {
            atoms.insert(0, "");
        } else {
            atoms.remove(0);
        }
        let (pairs, _) = atoms.as_chunks::<2>();
        for pair in pairs {
            self.append_chunk(pair[0], pair[1]);
        }
    }

    fn finish(mut self, linesep: &str) -> String {
        self.newline();
        self.lines.join(linesep)
    }
}

/// `re.split("([ \\t]+)", value)`: split on space/tab runs, keeping
/// the separators (edge elements stay, possibly empty).
fn split_fws_runs(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b' ' || bytes[index] == b'\t' {
            let mut end = index;
            while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
                end += 1;
            }
            out.push(&value[start..index]);
            out.push(&value[index..end]);
            start = end;
            index = end;
        } else {
            index += 1;
        }
    }
    out.push(&value[start..]);
    out
}

/// Source line split for the folder: `\r\n`, `\r` or `\n` (Python's
/// `splitlines` also breaks on vertical tab, form feed and Unicode
/// line boundaries — absurd in a header value, kept glued here).
fn split_header_lines(value: &str) -> Vec<&str> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' {
            out.push(&value[start..index]);
            index += usize::from(bytes.get(index + 1) == Some(&b'\n')) + 1;
            start = index;
        } else if bytes[index] == b'\n' {
            out.push(&value[start..index]);
            index += 1;
            start = index;
        } else {
            index += 1;
        }
    }
    if start < bytes.len() {
        out.push(&value[start..]);
    }
    out
}

/// `Header(val).encode()` for a pure-ASCII `val` (`header.py:309-387`
/// with one `us-ascii` chunk): feed the first source line, then each
/// further line after a forced break (leading space/tab kept as the
/// fold point, `:376-378`), a trailing transition (`:381-382`), and
/// join with `linesep`. `header_len` is the `Name: ` prefix width (0
/// for the stage-1 fold; the generator refold passes the real name).
/// The `_embedded_header` refusal (`:384-386`) cannot trigger here:
/// stage-1 inputs carry no newlines (rejected upstream) and every
/// fold the stages emit starts with a space.
fn ascii_header_encode(value: &str, header_len: usize, linesep: &str) -> String {
    let mut folder = AsciiFolder::new(header_len);
    let mut raw_lines = split_header_lines(value);
    if raw_lines.is_empty() {
        folder.ascii_split("", "");
    } else {
        let first = raw_lines.remove(0);
        folder.ascii_split("", first);
        for line in raw_lines {
            folder.newline();
            let stripped = line.trim_start_matches([' ', '\t']);
            let fws = &line[..line.len() - stripped.len()];
            folder.ascii_split(fws, stripped);
        }
    }
    folder.add_transition();
    folder.finish(linesep)
}

/// Collapse each space/tab run to one space and trim (the mailbox
/// parser folds CFWS this way: `José\tBoss` and `José  Boss` both
/// read as one-spaced).
fn collapse_ws(value: &str) -> String {
    value
        .split([' ', '\t'])
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Strip `(comment)` groups, innermost-first so nesting resolves;
/// quotes (backslash-aware) protect parens, and backslashes pass
/// through verbatim (display escapes are not unescaped — absurd
/// input, kept literal). Unbalanced parens or quotes fail: the stdlib
/// parser consumes the rest as a runaway comment, which we refuse to
/// reinterpret.
fn strip_comments(value: &str) -> Result<String, String> {
    let mut out = String::with_capacity(value.len());
    let mut depth = 0;
    let mut in_quotes = false;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            if depth == 0 {
                out.push('\\');
                out.push(ch);
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                in_quotes = !in_quotes;
                if depth == 0 {
                    out.push(ch);
                }
            }
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => {
                if depth == 0 {
                    return Err("invalid email address".to_owned());
                }
                depth -= 1;
            }
            _ => {
                if depth == 0 {
                    out.push(ch);
                }
            }
        }
    }
    if escaped && depth == 0 {
        out.push('\\');
    }
    if depth > 0 || in_quotes {
        return Err("invalid email address".to_owned());
    }
    Ok(out)
}

/// Split a quoted local part: `"..."` plus what follows it. The inner
/// text unescapes quoted-pairs (`\"` → `"`, `\\` → `\`, generally
/// backslash + any char → the char); spacing inside is preserved
/// verbatim. Returns `None` when the addr-spec is not quoted.
fn parse_quoted_local(addr: &str) -> Result<Option<(String, &str)>, String> {
    if !addr.starts_with('"') {
        return Ok(None);
    }
    let mut inner = String::new();
    let mut chars = addr[1..].char_indices();
    while let Some((index, ch)) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some((_, escaped)) => inner.push(escaped),
                None => return Err("invalid email address".to_owned()),
            }
        } else if ch == '"' {
            return Ok(Some((inner, &addr[1 + index + 1..])));
        } else {
            inner.push(ch);
        }
    }
    Err("invalid email address".to_owned())
}

/// Split one addr-spec into local part and domain. A quoted local
/// part must be followed by exactly `@domain`; a bare one needs
/// exactly one `@`, except the envelope's local-only shape (no `@`
/// at all — accepted when `require_at` is false). Bare `,;:` and
/// stray quotes fail (the mailbox parser stops there); whitespace in
/// the domain fails; bare local-part whitespace collapses.
fn split_addr_spec(addr: &str, require_at: bool) -> Result<(String, String), String> {
    let invalid = || "invalid email address".to_owned();
    if let Some((inner, rest)) = parse_quoted_local(addr)? {
        let domain = rest.strip_prefix('@').ok_or_else(invalid)?;
        if domain.is_empty() || domain.contains('@') {
            return Err(invalid());
        }
        return Ok((inner, domain.to_owned()));
    }
    if addr.contains('"') {
        return Err(invalid());
    }
    let mut pieces = addr.split('@');
    let local = pieces.next().ok_or_else(invalid)?;
    let Some(domain) = pieces.next() else {
        if require_at {
            return Err(invalid());
        }
        let local = collapse_ws(local);
        if local.is_empty() {
            return Err(invalid());
        }
        return Ok((local, String::new()));
    };
    if pieces.next().is_some() {
        return Err(invalid());
    }
    let local = collapse_ws(local);
    if local.is_empty() || domain.is_empty() {
        return Err(invalid());
    }
    if local.contains([',', ';', ':']) || domain.contains([',', ';', ':', '"']) {
        return Err(invalid());
    }
    if domain.contains([' ', '\t']) {
        return Err(invalid());
    }
    Ok((local, domain.to_owned()))
}

/// Clean the display-name half of `display <addr>`: comments off,
/// then unquoted phrase-enders fail (`@,;:` and `<>` outside quotes
/// stop the mailbox parser), quotes come off, whitespace collapses.
fn clean_display(raw: &str) -> Result<String, String> {
    let uncommented = strip_comments(raw)?;
    let mut in_quotes = false;
    for ch in uncommented.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes && matches!(ch, '@' | ',' | ';' | ':' | '<' | '>') {
            return Err("invalid email address".to_owned());
        }
    }
    Ok(collapse_ws(&uncommented.replace('"', "")))
}

/// The `get_mailbox` half of `sanitize_address` (`message.py:82-104`):
/// split `display <addr>` (last `<`, first `>` after it, only
/// whitespace past it) or a bare addr-spec (comments off, trimmed),
/// then local/domain. `require_at` selects the header tuple rule
/// (the addr-spec must carry `@` — `rsplit` fails otherwise) vs the
/// envelope string rule (a bare local part without domain is
/// accepted and renders bare).
fn parse_address(value: &str, require_at: bool) -> Result<(String, String, String), String> {
    let invalid = || "invalid email address".to_owned();
    let (display_raw, addr_raw) = if let Some(open) = value.rfind('<') {
        let rest = &value[open + 1..];
        let close = rest.find('>').ok_or_else(invalid)?;
        if !rest[close + 1..].chars().all(|ch| ch == ' ' || ch == '\t') {
            return Err(invalid());
        }
        (
            value[..open].to_owned(),
            rest[..close].trim_matches([' ', '\t']).to_owned(),
        )
    } else {
        if value.contains('>') {
            return Err(invalid());
        }
        (String::new(), strip_comments(value)?.trim().to_owned())
    };
    if addr_raw.is_empty() {
        return Err(invalid());
    }
    // The bracket split took the last `<`, so the addr-spec holds no
    // `<`; a comment may still trail inside the brackets.
    let addr = strip_comments(&addr_raw)?.trim().to_owned();
    let display = clean_display(&display_raw)?;
    let (local, domain) = split_addr_spec(&addr, require_at)?;
    // Newlines in the parts fail (`message.py:102-104`) — checked
    // post-parse, so a newline smuggled inside a stripped comment
    // does not fail the send.
    if display.contains(['\r', '\n'])
        || local.contains(['\r', '\n'])
        || domain.contains(['\r', '\n'])
    {
        return Err("invalid email address".to_owned());
    }
    Ok((display, local, domain))
}

/// `django.utils.encoding.punycode` (`encoding.py:211-213`) with
/// `encodings/idna.py:60-71` label rules: ASCII labels pass through
/// byte-identical (case kept — `ToASCII` step 1 returns early);
/// non-ASCII labels encode per label through UTS-46
/// (`idna::domain_to_ascii`, the locked 1.x the services/api crates
/// use) with Python's `0 < len < 64` encoded-length refusal; a
/// trailing root dot survives, any other empty label refuses, and
/// `''` stays empty. Residual: deviation mappings (ß→ss, ς→σ,
/// ZWJ/ZWNJ) — IDNA2003 maps them, UTS-46 keeps them (same residual
/// as the EmailValidator port).
fn idna_ascii_domain(domain: &str) -> Result<String, String> {
    let invalid = || "invalid email address".to_owned();
    if domain.is_empty() {
        return Ok(String::new());
    }
    let (body, dot) = match domain.strip_suffix('.') {
        Some(stripped) => (stripped, "."),
        None => (domain, ""),
    };
    let mut out = Vec::new();
    for label in body.split('.') {
        if label.is_empty() {
            return Err(invalid());
        }
        if label.is_ascii() {
            if label.len() >= 64 {
                return Err(invalid());
            }
            out.push(label.to_owned());
        } else {
            let encoded = idna::domain_to_ascii(label).map_err(|_| invalid())?;
            if encoded.is_empty() || encoded.len() >= 64 {
                return Err(invalid());
            }
            out.push(encoded);
        }
    }
    Ok(out.join(".") + dot)
}

/// `email.utils.specialsre` (`utils.py:48`): display names carrying
/// one of these quote under `formataddr`.
fn is_formataddr_special(byte: u8) -> bool {
    matches!(
        byte,
        b']' | b'[' | b'\\' | b'(' | b')' | b'<' | b'>' | b'@' | b',' | b':' | b';' | b'"' | b'.'
    )
}

/// `email._header_value_parser.DOT_ATOM_ENDS` (`ATOM_ENDS` minus `.`):
/// a local part carrying one of these quotes under `Address`.
fn is_dot_atom_end(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
        || matches!(
            byte,
            b'(' | b')' | b'<' | b'>' | b'@' | b',' | b':' | b';' | b'"' | b'\\' | b'[' | b']'
        )
}

/// `Address.addr_spec` (`headerregistry.py`) without CTE: quote the
/// local part when it carries a `DOT_ATOM_ENDS` char (escaping `\`
/// then `"` — `quote_string`), join `local@domain`, or the bare
/// local when the domain is empty.
fn addr_spec_join(local: &str, domain: &str) -> String {
    let quoted = if local.bytes().any(is_dot_atom_end) {
        format!("\"{}\"", local.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        local.to_owned()
    };
    if domain.is_empty() {
        quoted
    } else {
        format!("{quoted}@{domain}")
    }
}

/// One `getaddresses` piece through `sanitize_address`
/// (`message.py:78-119`): display and local-part Header-encoding,
/// `punycode` domain, `formataddr` join. An ASCII display quotes per
/// `specialsre` (after `escapesre`); an encoded display never quotes
/// (its alphabet is disjoint from the specials). `require_at`
/// selects the header tuple rule vs the envelope string rule (see
/// [`parse_address`]).
fn sanitize_address_piece(piece: &str, require_at: bool) -> Result<String, String> {
    let (display, local, domain) = parse_address(piece, require_at)?;
    let display = if display.is_empty() {
        String::new()
    } else if display.is_ascii() {
        let folded = ascii_header_encode(&display, 0, "\n");
        if folded.bytes().any(is_formataddr_special) {
            format!("\"{}\"", folded.replace('\\', "\\\\").replace('"', "\\\""))
        } else {
            folded
        }
    } else {
        utf8_header_encode(&display)
    };
    let local = if local.is_ascii() {
        local
    } else {
        utf8_header_encode(&local)
    };
    let domain = idna_ascii_domain(&domain)?;
    let addr_spec = addr_spec_join(&local, &domain);
    if display.is_empty() {
        Ok(addr_spec)
    } else {
        Ok(format!("{display} <{addr_spec}>"))
    }
}

/// `getaddresses` splitting for the header path: commas at depth 0
/// only (inside `<>`, `""` or `()` they belong to the address).
/// Empty pieces fail (the tuple unpack fails on them).
fn split_top_level_commas(value: &str) -> Result<Vec<String>, String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut angle: u32 = 0;
    let mut paren: u32 = 0;
    let mut in_quotes = false;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push('\\');
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            '<' if !in_quotes => {
                angle += 1;
                current.push(ch);
            }
            '>' if !in_quotes => {
                angle = angle.saturating_sub(1);
                current.push(ch);
            }
            '(' if !in_quotes => {
                paren += 1;
                current.push(ch);
            }
            ')' if !in_quotes => {
                paren = paren.saturating_sub(1);
                current.push(ch);
            }
            ',' if !in_quotes && angle == 0 && paren == 0 => {
                if current.trim().is_empty() {
                    return Err("invalid email address".to_owned());
                }
                pieces.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if escaped {
        current.push('\\');
    }
    if current.trim().is_empty() {
        return Err("invalid email address".to_owned());
    }
    pieces.push(current.trim().to_owned());
    Ok(pieces)
}

/// `forbid_multi_line_headers` (`message.py:55-76`) + the compat32
/// generator refold (`_policybase.py:348-377`) for `Subject`: CR/LF
/// is rejected, ASCII folds via `Header(val).encode()`, non-ASCII
/// encodes via `Header(val, 'utf-8').encode()`, and the stored value
/// refolds under the `Subject: ` prefix at emit time. Returns the
/// full header (folds use CRLF).
pub fn subject_header(subject: &str) -> Result<String, String> {
    if subject.contains(['\r', '\n']) {
        return Err("invalid email header".to_owned());
    }
    let stage1 = if subject.is_ascii() {
        ascii_header_encode(subject, 0, "\n")
    } else {
        utf8_header_encode(subject)
    };
    Ok(format!(
        "Subject: {}",
        ascii_header_encode(&stage1, "Subject".len() + 2, "\r\n")
    ))
}

/// Same two stages for `From`/`To` (`message.py:55-76` address
/// branch): ASCII values pass through verbatim (only `subject` gets
/// the `Header()` wrap), non-ASCII values split over `getaddresses`
/// and sanitize piece by piece (`, ` join), then the generator
/// refolds under the real header name. Returns the full header.
pub fn address_header(name: &str, value: &str) -> Result<String, String> {
    if value.contains(['\r', '\n']) {
        return Err("invalid email header".to_owned());
    }
    let stage1 = if value.is_ascii() {
        value.to_owned()
    } else {
        let mut pieces = Vec::new();
        for piece in split_top_level_commas(value)? {
            pieces.push(sanitize_address_piece(&piece, true)?);
        }
        pieces.join(", ")
    };
    Ok(format!(
        "{name}: {}",
        ascii_header_encode(&stage1, name.len() + 2, "\r\n")
    ))
}

/// The `smtp.py:_send` envelope (`:141-159`): `sanitize_address` over
/// the raw string (no comma split — each recipient sanitizes alone),
/// then `smtplib.sendmail`'s `quoteaddr` (bare addr-spec in `<>`).
/// The display name validates but never reaches the wire; unparsable
/// input fails the send instead of mailing garbage.
pub fn envelope_address(value: &str) -> Result<String, String> {
    let (_display, local, domain) = parse_address(value, false)?;
    let local = if local.is_ascii() {
        local
    } else {
        utf8_header_encode(&local)
    };
    let domain = idna_ascii_domain(&domain)?;
    Ok(format!("<{}>", addr_spec_join(&local, &domain)))
}

fn tls_config() -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

async fn upgrade_tls(
    stream: tokio::net::TcpStream,
    host: &str,
) -> Result<tokio::io::BufReader<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>, String> {
    use std::str::FromStr;
    let name = if let Ok(addr) = std::net::IpAddr::from_str(host) {
        rustls::pki_types::ServerName::IpAddress(addr.into())
    } else {
        rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| format!("bad TLS host: {host:?}"))?
    };
    let config = std::sync::Arc::new(tls_config()?);
    let connector = tokio_rustls::TlsConnector::from(config);
    let tls = connector
        .connect(name, stream)
        .await
        .map_err(|e| format!("TLS handshake failed: {e}"))?;
    Ok(tokio::io::BufReader::new(tls))
}

enum Io {
    Plain(tokio::io::BufReader<tokio::net::TcpStream>),
    Tls(Box<tokio::io::BufReader<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>>),
}

impl Io {
    async fn read_reply(&mut self) -> Result<(i32, String), String> {
        use tokio::io::AsyncBufReadExt;
        let mut text = String::new();
        let mut code = -1;
        loop {
            let mut line = String::new();
            let n = match self {
                Io::Plain(buf) => buf.read_line(&mut line).await,
                Io::Tls(buf) => buf.read_line(&mut line).await,
            }
            .map_err(|e| format!("SMTP read failed: {e}"))?;
            if n == 0 {
                return Err("SMTP server disconnected".to_owned());
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(prefix) = trimmed.get(..3) {
                if let Ok(parsed) = prefix.parse::<i32>() {
                    code = parsed;
                }
                text.push_str(trimmed.get(4..).unwrap_or(""));
            }
            if !(trimmed.len() > 3 && trimmed.as_bytes()[3] == b'-') {
                break;
            }
            text.push('\n');
        }
        Ok((code, text))
    }

    async fn write_str(&mut self, command: &str) -> Result<(), String> {
        let bytes = format!("{command}\r\n");
        self.write_bytes(bytes.as_bytes()).await
    }

    async fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;
        let result = match self {
            Io::Plain(buf) => {
                let stream = buf.get_mut();
                stream.write_all(bytes).await.and(stream.flush().await)
            }
            Io::Tls(buf) => {
                let stream = buf.get_mut();
                stream.write_all(bytes).await.and(stream.flush().await)
            }
        };
        result.map_err(|e| format!("SMTP write failed: {e}"))
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(bytes)
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(text.trim())
        .map_err(|e| format!("bad base64 challenge: {e}"))
}

/// One AUTH challenge/response round (`smtplib.auth`, `_MAXCHALLENGE=5`):
/// 235/503 accept, 334 answered, anything else refused, too many
/// challenges aborts.
async fn auth_attempt<F>(
    io: &mut Io,
    mechanism: &str,
    initial: Option<String>,
    respond: F,
) -> Result<AuthOutcome, String>
where
    F: Fn(&[u8]) -> String + Send,
{
    const MAX_CHALLENGE: u32 = 5;
    let command = match &initial {
        Some(response) => format!("AUTH {mechanism} {response}"),
        None => format!("AUTH {mechanism}"),
    };
    io.write_str(&command).await?;
    let mut challenges = u32::from(initial.is_some());
    loop {
        let (code, text) = io.read_reply().await?;
        if code == 235 || code == 503 {
            return Ok(AuthOutcome::Accepted);
        }
        if code != 334 {
            return Ok(AuthOutcome::Refused);
        }
        challenges += 1;
        if challenges > MAX_CHALLENGE {
            return Err("too many SMTP auth challenges".to_owned());
        }
        let first = text.lines().next().unwrap_or("");
        let challenge = base64_decode(first)?;
        io.write_str(&respond(&challenge)).await?;
    }
}

#[derive(PartialEq, Eq)]
enum AuthOutcome {
    Accepted,
    Refused,
}

/// `smtplib.login`: advertised methods tried in `CRAM-MD5`, `PLAIN`,
/// `LOGIN` order; a refusal falls through to the next method and only the
/// last refusal propagates — every other error aborts immediately.
async fn smtp_login(
    io: &mut Io,
    methods: &[String],
    username: &str,
    password: &str,
) -> Result<(), String> {
    use hmac::Mac;
    let upper: Vec<String> = methods.iter().map(|m| m.to_uppercase()).collect();
    let mut order: Vec<&str> = Vec::new();
    for method in ["CRAM-MD5", "PLAIN", "LOGIN"] {
        if upper.iter().any(|m| m == method) {
            order.push(method);
        }
    }
    if order.is_empty() {
        return Err("SMTP AUTH extension not supported by server".to_owned());
    }
    let plain_initial = base64_encode(format!("\0{username}\0{password}").as_bytes());
    let login_user = base64_encode(username.as_bytes());
    let login_pass = base64_encode(password.as_bytes());
    let mut refused = false;
    for method in order {
        let outcome = match method {
            "CRAM-MD5" => {
                auth_attempt(io, method, None, |challenge| {
                    let mut mac = hmac::Hmac::<md5::Md5>::new_from_slice(password.as_bytes())
                        .expect("HMAC accepts any key length");
                    mac.update(challenge);
                    let digest = mac.finalize().into_bytes();
                    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
                    base64_encode(format!("{username} {hex}").as_bytes())
                })
                .await?
            }
            "PLAIN" => {
                auth_attempt(io, method, Some(plain_initial.clone()), |_| {
                    plain_initial.clone()
                })
                .await?
            }
            _ => auth_attempt(io, method, Some(login_user.clone()), |_| login_pass.clone()).await?,
        };
        match outcome {
            AuthOutcome::Accepted => return Ok(()),
            AuthOutcome::Refused => refused = true,
        }
    }
    if refused {
        return Err("SMTP authentication failed".to_owned());
    }
    Ok(())
}

/// EHLO with HELO fallback (`ehlo_or_helo_if_needed`); returns the EHLO
/// extension lines on success.
async fn ehlo(io: &mut Io) -> Result<Vec<String>, String> {
    io.write_str("EHLO localhost").await?;
    let (code, text) = io.read_reply().await?;
    if code == 250 {
        return Ok(text.lines().map(str::to_owned).collect());
    }
    io.write_str("HELO localhost").await?;
    let (code, _) = io.read_reply().await?;
    if code == 250 {
        return Ok(Vec::new());
    }
    Err("SMTP greeting rejected".to_owned())
}

/// The `EmailBackend.open/send` conversation for one already-resolved
/// config: connect (+implicit TLS), greeting, EHLO, STARTTLS, AUTH, `MAIL`,
/// `RCPT`, `DATA`, best-effort `QUIT`. Any failure is `Err` — the caller
/// swallows it.
pub async fn smtp_send(cfg: &SmtpSend, mail: &OutgoingMail) -> Result<(), String> {
    // `port=int(...)` already ran; a range failure is `OverflowError` at
    // connect — inside the `try`, so it fails the send.
    let port: u16 = u16::try_from(cfg.port).map_err(|_| format!("bad SMTP port: {}", cfg.port))?;
    let boundary = make_boundary();
    let date = make_date();
    let msg_id = format!("<{boundary}@pidash>");
    // Header validation (`BadHeaderError`) and address parsing live in
    // the builder now; both fail the send like Django's raise.
    let content = message_bytes(&cfg.from_email, mail, &boundary, &date, &msg_id)?;
    smtp_deliver(cfg, port, &mail.to, &content).await
}

/// Per-send MIME boundary: nanos-since-epoch + pid in hex (varies per
/// send like Django's, which mixes time + pid + random).
fn make_boundary() -> String {
    format!(
        "{:x}{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        std::process::id()
    )
}

/// `Date` header: `email.utils.formatdate(localtime=True)` shape with a
/// UTC `-0000` offset (the worker runs on UTC; Django renders the same
/// shape with the local offset).
fn make_date() -> String {
    chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S -0000")
        .to_string()
}

/// The `EmailBackend.open/send` conversation for prebuilt `content`:
/// connect (+implicit TLS), greeting, EHLO, STARTTLS, AUTH, `MAIL`,
/// `RCPT`, `DATA`, best-effort `QUIT`. Shared by [`smtp_send`] and the
/// attachment variant (same wire, never a fork). Any failure is `Err` —
/// the caller swallows it.
async fn smtp_deliver(cfg: &SmtpSend, port: u16, to: &str, content: &[u8]) -> Result<(), String> {
    // The envelope sanitizes before connecting (`smtp.py:_send`
    // sanitizes, then `sendmail` dials): unparsable addresses fail
    // without any network traffic.
    let mail_from = envelope_address(&cfg.from_email)?;
    let rcpt_to = envelope_address(to)?;
    let stream = tokio::net::TcpStream::connect((cfg.host.as_str(), port))
        .await
        .map_err(|e| format!("SMTP connect failed: {e}"))?;
    let mut io = if cfg.use_ssl {
        Io::Tls(Box::new(upgrade_tls(stream, &cfg.host).await?))
    } else {
        Io::Plain(tokio::io::BufReader::new(stream))
    };
    let (code, _) = io.read_reply().await?;
    if code != 220 {
        return Err("SMTP greeting rejected".to_owned());
    }
    let mut lines = ehlo(&mut io).await?;
    if !cfg.use_ssl && cfg.use_tls {
        let advertised = lines.iter().any(|l| {
            l.split_whitespace()
                .next()
                .is_some_and(|w| w.eq_ignore_ascii_case("STARTTLS"))
        });
        if !advertised {
            return Err("STARTTLS not advertised".to_owned());
        }
        io.write_str("STARTTLS").await?;
        let (code, _) = io.read_reply().await?;
        if code != 220 {
            return Err("STARTTLS rejected".to_owned());
        }
        let Io::Plain(buffered) = io else {
            return Err("STARTTLS failed".to_owned());
        };
        io = Io::Tls(Box::new(
            upgrade_tls(buffered.into_inner(), &cfg.host).await?,
        ));
        lines = ehlo(&mut io).await?;
    }
    // `login()` when both are set (`if self.username and self.password`).
    let has_creds = cfg.username.as_deref().is_some_and(|u| !u.is_empty())
        && cfg.password.as_deref().is_some_and(|p| !p.is_empty());
    if has_creds {
        let mut methods: Vec<String> = Vec::new();
        for line in &lines {
            let mut parts = line.split_whitespace();
            if parts.next().is_some_and(|w| w.eq_ignore_ascii_case("AUTH")) {
                methods.extend(parts.map(str::to_owned));
            }
        }
        if methods.is_empty() {
            return Err("SMTP AUTH extension not supported by server".to_owned());
        }
        smtp_login(
            &mut io,
            &methods,
            cfg.username.as_deref().unwrap_or(""),
            cfg.password.as_deref().unwrap_or(""),
        )
        .await?;
    }
    io.write_str(&format!("MAIL FROM:{mail_from}")).await?;
    let (code, _) = io.read_reply().await?;
    if code != 250 {
        return Err("SMTP sender refused".to_owned());
    }
    io.write_str(&format!("RCPT TO:{rcpt_to}")).await?;
    let (code, _) = io.read_reply().await?;
    if code != 250 && code != 251 {
        return Err("SMTP recipient refused".to_owned());
    }
    io.write_str("DATA").await?;
    let (code, _) = io.read_reply().await?;
    if code != 354 {
        return Err("SMTP DATA rejected".to_owned());
    }
    io.write_bytes(content).await?;
    io.write_bytes(b"\r\n.\r\n").await?;
    let (code, _) = io.read_reply().await?;
    if code != 250 {
        return Err("SMTP send rejected".to_owned());
    }
    // `close()` after a successful send: `quit()` errors are swallowed
    // once the send itself succeeded.
    let _ = io.write_str("QUIT").await;
    Ok(())
}

/// [`smtp_send`] for a body + attachments mail with no HTML alternative
/// (`send_export_email`, `analytic_plot_export.py:52-88`): same header
/// validation, same conversation, `multipart/mixed` content.
#[allow(clippy::too_many_arguments)]
pub async fn smtp_send_mixed(
    cfg: &SmtpSend,
    to: &str,
    subject: &str,
    text_body: &str,
    attachments: &[MailAttachment<'_>],
) -> Result<(), String> {
    let port: u16 = u16::try_from(cfg.port).map_err(|_| format!("bad SMTP port: {}", cfg.port))?;
    let boundary = make_boundary();
    let date = make_date();
    let msg_id = format!("<{boundary}@pidash>");
    let content = message_bytes_mixed(
        &cfg.from_email,
        to,
        subject,
        text_body,
        attachments,
        &boundary,
        &date,
        &msg_id,
    )?;
    smtp_deliver(cfg, port, to, &content).await
}

/// Full pipeline for one mail: resolve config → render → plain text →
/// send. Mirrors one task body: success logs `success_log`
/// (`pi_dash.worker`), ANY failure logs the error (`pi_dash.exception`,
/// the `log_exception` port) and returns normally — swallow, never retry.
pub async fn send_mail(
    pool: &sqlx::PgPool,
    template_source: &str,
    subject: &str,
    context: &HashMap<String, String>,
    to: &str,
    success_log: &str,
) {
    match try_send_mail(pool, template_source, subject, context, to).await {
        Ok(()) => tracing::info!(target: "pi_dash.worker", "{success_log}"),
        Err(error) => tracing::error!(target: "pi_dash.exception", "{error}"),
    }
}

async fn try_send_mail(
    pool: &sqlx::PgPool,
    template_source: &str,
    subject: &str,
    context: &HashMap<String, String>,
    to: &str,
) -> Result<(), String> {
    // `get_email_configuration()` runs INSIDE the `try` in all four tasks:
    // a store failure is swallowed like any other error.
    let store = pidash_db::config::PgConfigStore::new(pool.clone());
    let keyring = pidash_db::config::encryption::Keyring::from_env();
    let registry: &ConfigRegistry = pidash_db::config::registry::global();
    let values =
        pidash_services::license::config::get_email_configuration(&store, &keyring, registry)
            .await
            .map_err(|e| format!("email configuration failed: {e}"))?;
    let cfg = resolve_smtp(&values)?;
    let html = render_template(template_source, context)?;
    let text = plain_text_from_html(&html);
    smtp_send(
        &cfg,
        &OutgoingMail {
            to: to.to_owned(),
            subject: subject.to_owned(),
            text_body: text,
            html_body: html,
        },
    )
    .await
}

/// [`send_mail`] for a body + attachments mail with no HTML alternative
/// (`send_export_email`, `analytic_plot_export.py:52-88`): resolve config
/// → render → plain text → attach → send. Success logs `success_log`
/// (`pi_dash.worker`), ANY failure logs the error (`pi_dash.exception`)
/// and returns normally — swallow, never retry.
#[allow(clippy::too_many_arguments)]
pub async fn send_mail_with_attachment(
    pool: &sqlx::PgPool,
    template_source: &str,
    subject: &str,
    context: &HashMap<String, String>,
    to: &str,
    attachment_name: &str,
    attachment_content: &str,
    success_log: &str,
) {
    match try_send_mail_with_attachment(
        pool,
        template_source,
        subject,
        context,
        to,
        attachment_name,
        attachment_content,
    )
    .await
    {
        Ok(()) => tracing::info!(target: "pi_dash.worker", "{success_log}"),
        Err(error) => tracing::error!(target: "pi_dash.exception", "{error}"),
    }
}

async fn try_send_mail_with_attachment(
    pool: &sqlx::PgPool,
    template_source: &str,
    subject: &str,
    context: &HashMap<String, String>,
    to: &str,
    attachment_name: &str,
    attachment_content: &str,
) -> Result<(), String> {
    // `get_email_configuration()` runs INSIDE the `try` (`:60-68`): a
    // store failure is swallowed like any other error.
    let store = pidash_db::config::PgConfigStore::new(pool.clone());
    let keyring = pidash_db::config::encryption::Keyring::from_env();
    let registry: &ConfigRegistry = pidash_db::config::registry::global();
    let values =
        pidash_services::license::config::get_email_configuration(&store, &keyring, registry)
            .await
            .map_err(|e| format!("email configuration failed: {e}"))?;
    let cfg = resolve_smtp(&values)?;
    let html = render_template(template_source, context)?;
    let text = plain_text_from_html(&html);
    smtp_send_mixed(
        &cfg,
        to,
        subject,
        &text,
        &[MailAttachment {
            filename: attachment_name.to_owned(),
            content: attachment_content,
        }],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_values(items: &[(&str, &str)]) -> Vec<ConfigValue> {
        items
            .iter()
            .map(|(k, v)| {
                let _ = k;
                ConfigValue::Str((*v).to_owned())
            })
            .collect()
    }

    #[test]
    fn smtp_mapping_matches_fixture() {
        // `F-COMMON.email_configuration.json` smtp_mapping: string
        // `=="1"` compares, `from_email=EMAIL_FROM`.
        let cfg = resolve_smtp(&str_values(&[
            ("h", "smtp.test"),
            ("u", "user"),
            ("p", "pass"),
            ("port", "587"),
            ("tls", "1"),
            ("ssl", "0"),
            ("from", "Team Pi Dash <team@airepublic.com>"),
        ]))
        .expect("valid config");
        assert_eq!(cfg.host, "smtp.test");
        assert_eq!(cfg.port, 587);
        assert!(cfg.use_tls);
        assert!(!cfg.use_ssl);
        assert_eq!(cfg.from_email, "Team Pi Dash <team@airepublic.com>");
        // `"True"`/`"true"`/`1` are NOT `"1"` — plain string compare.
        let cfg = resolve_smtp(&str_values(&[
            ("h", "smtp.test"),
            ("u", "user"),
            ("p", "pass"),
            ("port", "587"),
            ("tls", "true"),
            ("ssl", "True"),
            ("from", "f"),
        ]))
        .expect("valid config");
        assert!(!cfg.use_tls);
        assert!(!cfg.use_ssl);
    }

    #[test]
    fn bad_port_fails_resolution() {
        // `int(EMAIL_PORT)` failure fails the send (swallowed by the
        // caller, never a retry).
        assert!(resolve_smtp(&str_values(&[
            ("h", "h"),
            ("u", "u"),
            ("p", "p"),
            ("port", "not-a-port"),
            ("tls", "1"),
            ("ssl", "0"),
            ("from", "f"),
        ]))
        .is_err());
        assert!(resolve_smtp(&str_values(&[
            ("h", "h"),
            ("u", "u"),
            ("p", "p"),
            ("port", ""),
            ("tls", "1"),
            ("ssl", "0"),
            ("from", "f"),
        ]))
        .is_err());
    }

    #[test]
    fn empty_host_and_from_take_django_defaults() {
        let cfg = resolve_smtp(&str_values(&[
            ("h", ""),
            ("u", ""),
            ("p", ""),
            ("port", "25"),
            ("tls", "0"),
            ("ssl", "0"),
            ("from", ""),
        ]))
        .expect("valid config");
        assert_eq!(cfg.host, DEFAULT_SMTP_HOST);
        assert_eq!(cfg.from_email, DEFAULT_FROM_EMAIL);
    }

    #[test]
    fn py_int_matches_python() {
        assert_eq!(parse_py_int("587"), Some(587));
        assert_eq!(parse_py_int("  587  "), Some(587));
        assert_eq!(parse_py_int("+587"), Some(587));
        assert_eq!(parse_py_int("1_000"), Some(1000));
        assert_eq!(parse_py_int("-1"), Some(-1));
        assert_eq!(parse_py_int(""), None);
        assert_eq!(parse_py_int("   "), None);
        assert_eq!(parse_py_int("587.0"), None);
        assert_eq!(parse_py_int("abc"), None);
        assert_eq!(parse_py_int("1__0"), None);
        assert_eq!(parse_py_int("_1"), None);
        assert_eq!(parse_py_int("1_"), None);
    }

    #[test]
    fn py_str_matches_python_str() {
        use serde_json::json;
        assert_eq!(py_str(&serde_json::Value::Null), "None");
        assert_eq!(py_str(&json!(true)), "True");
        assert_eq!(py_str(&json!(false)), "False");
        assert_eq!(py_str(&json!(587)), "587");
        assert_eq!(py_str(&json!(5.0)), "5.0");
        assert_eq!(py_str(&json!("x")), "x");
    }

    #[test]
    fn escape_matches_django() {
        assert_eq!(
            django_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&#x27;f"
        );
        assert_eq!(django_escape("plain"), "plain");
    }

    #[test]
    fn plain_text_matches_golden_bytes() {
        // `F-AUTH.rendered.json` texts were produced by the real
        // `generate_plain_text_from_html` over the Django-rendered HTML:
        // byte-exact agreement pins the port (incl. verbatim `&amp;`).
        static FIXTURE: &str =
            include_str!("../../../../fixtures/tasks_mail/auth/F-AUTH.rendered.json");
        let golden: serde_json::Value = serde_json::from_str(FIXTURE).expect("valid fixture");
        let renders = golden["renders"].as_object().expect("renders map");
        assert_eq!(renders.len(), 3);
        for (name, render) in renders {
            let html = render["html"].as_str().expect("html");
            let expected = render["text"].as_str().expect("text");
            assert_eq!(plain_text_from_html(html), expected, "{name}");
        }
    }

    #[test]
    fn renders_match_django_goldens() {
        // Full-pipeline render check against the Django-produced goldens:
        // the probe's task-shaped contexts plus its `current_site`
        // sentinel (absent from real task contexts, where Django renders
        // `""`). Byte equality here proves template fidelity; the missing-
        // variable case is pinned by `missing_var_renders_empty`.
        static FIXTURE: &str =
            include_str!("../../../../fixtures/tasks_mail/auth/F-AUTH.rendered.json");
        let golden: serde_json::Value = serde_json::from_str(FIXTURE).expect("valid fixture");
        let renders = &golden["renders"];
        // (fixture key, template source, probe context).
        type RenderCase<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);
        let cases: &[RenderCase] = &[
            (
                "emails/auth/magic_signin.html",
                MAGIC_SIGNIN_TEMPLATE,
                &[
                    ("code", "842103"),
                    ("email", "contract@example.com"),
                    ("current_site", "INVALID-current_site"),
                ],
            ),
            (
                "emails/auth/forgot_password.html",
                FORGOT_PASSWORD_TEMPLATE,
                &[
                    ("first_name", "Contract"),
                    (
                        "forgot_password_url",
                        "example.com/accounts/reset-password/?uidb64=uidb64&token=token&email=contract@example.com",
                    ),
                    ("email", "contract@example.com"),
                    ("current_site", "INVALID-current_site"),
                ],
            ),
            (
                "emails/user/email_updated.html",
                EMAIL_UPDATED_TEMPLATE,
                &[
                    ("email", "contract@example.com"),
                    ("current_site", "INVALID-current_site"),
                ],
            ),
        ];
        for (name, source, raw) in cases {
            let escaped: HashMap<String, String> = raw
                .iter()
                .map(|(k, v)| ((*k).to_owned(), django_escape(v)))
                .collect();
            let rendered = render_template(source, &escaped).expect("renders");
            assert_eq!(
                rendered,
                renders[name]["html"].as_str().expect("html"),
                "{name}"
            );
        }
    }

    #[test]
    fn missing_var_renders_empty_like_django() {
        // The production shape: no `current_site` in context (no task
        // passes it) — Django renders `src="/static/..."`, verified live
        // (`MISSING-VAR-IMG` probe). Strict-undefined would fail here.
        let mut ctx = HashMap::new();
        ctx.insert("code".to_owned(), "842103".to_owned());
        ctx.insert("email".to_owned(), "contract@example.com".to_owned());
        let rendered = render_template(MAGIC_SIGNIN_TEMPLATE, &ctx).expect("renders");
        assert!(rendered.contains("src=\"/static/logos/Logo.png\""));
    }

    #[test]
    fn envelope_address_shapes_envelope() {
        assert_eq!(
            envelope_address("Team Pi Dash <team@airepublic.com>").expect("valid"),
            "<team@airepublic.com>"
        );
        assert_eq!(
            envelope_address("plain@example.com").expect("valid"),
            "<plain@example.com>"
        );
    }

    /// Scripted fake SMTP server: greeting + one reply per command,
    /// captures the DATA payload. Verifies the client's conversation
    /// (EHLO, MAIL/RCPT envelope with display-name stripping, DATA + end
    /// marker, QUIT) without any external process.
    async fn run_fake_smtp(replies: Vec<&'static str>) -> (u16, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (reader, mut writer) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(reader);
            let mut data = String::new();
            writer
                .write_all(b"220 fake ESMTP\r\n")
                .await
                .expect("greet");
            let mut replies = replies.into_iter();
            let mut line = String::new();
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await.expect("read");
                if n == 0 {
                    break;
                }
                if line.starts_with("DATA") {
                    writer.write_all(b"354 go\r\n").await.expect("354");
                    // DATA content until the end marker.
                    let mut content = String::new();
                    loop {
                        let mut chunk = String::new();
                        reader.read_line(&mut chunk).await.expect("data");
                        if chunk == ".\r\n" {
                            break;
                        }
                        // Undo dot-stuffing for the assertion.
                        if let Some(rest) = chunk.strip_prefix("..") {
                            chunk = format!(".{rest}");
                        }
                        content.push_str(&chunk);
                    }
                    data.push_str(&content);
                    writer.write_all(b"250 ok\r\n").await.expect("250");
                    continue;
                }
                if line.starts_with("QUIT") {
                    writer.write_all(b"221 bye\r\n").await.expect("221");
                    break;
                }
                let reply = replies.next().unwrap_or("250 ok\r\n");
                writer.write_all(reply.as_bytes()).await.expect("reply");
            }
            data
        });
        (port, handle)
    }

    /// Fake server variant that also scripts AUTH: maps each `AUTH <mech>`
    /// line to a canned reply sequence, records the mechanism lines seen.
    async fn run_fake_smtp_auth(
        auth_script: Vec<(&'static str, Vec<&'static str>)>,
    ) -> (u16, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (reader, mut writer) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(reader);
            let mut seen: Vec<String> = Vec::new();
            writer
                .write_all(b"220 fake ESMTP\r\n")
                .await
                .expect("greet");
            let mut line = String::new();
            // EHLO with AUTH advertised.
            loop {
                line.clear();
                reader.read_line(&mut line).await.expect("read");
                if line.starts_with("EHLO") {
                    writer
                        .write_all(b"250-fake\r\n250 AUTH PLAIN LOGIN\r\n")
                        .await
                        .expect("ehlo");
                    break;
                }
            }
            let mut script = auth_script.into_iter();
            let mut pending: Vec<&'static str> = Vec::new();
            let mut in_data = false;
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await.expect("read");
                if n == 0 {
                    break;
                }
                let trimmed = line.trim_end().to_owned();
                // DATA content runs to the end marker with no replies.
                if in_data {
                    if trimmed == "." {
                        in_data = false;
                        writer.write_all(b"250 ok\r\n").await.expect("250");
                    }
                    continue;
                }
                if trimmed.starts_with("AUTH ") {
                    seen.push(trimmed.clone());
                    let mech = trimmed.split_whitespace().nth(1).unwrap_or("").to_owned();
                    pending = script
                        .find(|(m, _)| *m == mech)
                        .map(|(_, r)| r)
                        .unwrap_or_default();
                }
                if trimmed.starts_with("QUIT") {
                    writer.write_all(b"221 bye\r\n").await.expect("221");
                    break;
                }
                if trimmed == "DATA" {
                    writer.write_all(b"354 go\r\n").await.expect("354");
                    in_data = true;
                    continue;
                }
                if trimmed.starts_with("MAIL ") || trimmed.starts_with("RCPT ") {
                    writer.write_all(b"250 ok\r\n").await.expect("250");
                    continue;
                }
                // AUTH challenge/response round.
                if !pending.is_empty() {
                    let reply = pending.remove(0).to_owned();
                    writer.write_all(reply.as_bytes()).await.expect("auth");
                } else {
                    writer.write_all(b"250 ok\r\n").await.expect("250");
                }
            }
            seen
        });
        (port, handle)
    }

    #[tokio::test]
    async fn smtp_auth_plain_accepted() {
        // `AUTH PLAIN <b64>` → 235: single-round accept.
        let (port, handle) = run_fake_smtp_auth(vec![("PLAIN", vec!["235 ok\r\n"])]).await;
        let cfg = SmtpSend {
            host: "127.0.0.1".to_owned(),
            port: i64::from(port),
            username: Some("user".to_owned()),
            password: Some("pass".to_owned()),
            use_tls: false,
            use_ssl: false,
            from_email: "f@x.io".to_owned(),
        };
        let mail = OutgoingMail {
            to: "t@x.io".to_owned(),
            subject: "s".to_owned(),
            text_body: "b".to_owned(),
            html_body: "<p>b</p>".to_owned(),
        };
        smtp_send(&cfg, &mail).await.expect("send");
        let seen = handle.await.expect("server");
        assert_eq!(seen.len(), 1);
        assert!(seen[0].starts_with("AUTH PLAIN "));
        // `auth_plain` initial response: base64("\0user\0pass").
        assert!(seen[0].ends_with("AHVzZXIAcGFzcw=="));
    }

    #[tokio::test]
    async fn smtp_auth_falls_through_to_login() {
        // PLAIN refused (535) → LOGIN challenge round (334 user, 334 pass)
        // → accepted. Mirrors `smtplib.login` fallthrough.
        let (port, handle) = run_fake_smtp_auth(vec![
            ("PLAIN", vec!["535 refused\r\n"]),
            (
                "LOGIN",
                vec!["334 VXNlcm5hbWU6\r\n", "334 UGFzc3dvcmQ6\r\n", "235 ok\r\n"],
            ),
        ])
        .await;
        let cfg = SmtpSend {
            host: "127.0.0.1".to_owned(),
            port: i64::from(port),
            username: Some("user".to_owned()),
            password: Some("pass".to_owned()),
            use_tls: false,
            use_ssl: false,
            from_email: "f@x.io".to_owned(),
        };
        let mail = OutgoingMail {
            to: "t@x.io".to_owned(),
            subject: "s".to_owned(),
            text_body: "b".to_owned(),
            html_body: "<p>b</p>".to_owned(),
        };
        smtp_send(&cfg, &mail).await.expect("send");
        let seen = handle.await.expect("server");
        assert_eq!(seen.len(), 2);
        assert!(seen[0].starts_with("AUTH PLAIN "));
        assert!(seen[1].starts_with("AUTH LOGIN "));
    }

    #[tokio::test]
    async fn smtp_auth_all_refused_fails_send() {
        // Every method refused → the send fails (swallowed upstream).
        let (port, handle) = run_fake_smtp_auth(vec![
            ("PLAIN", vec!["535 refused\r\n"]),
            ("LOGIN", vec!["535 refused\r\n"]),
        ])
        .await;
        let cfg = SmtpSend {
            host: "127.0.0.1".to_owned(),
            port: i64::from(port),
            username: Some("user".to_owned()),
            password: Some("pass".to_owned()),
            use_tls: false,
            use_ssl: false,
            from_email: "f@x.io".to_owned(),
        };
        let mail = OutgoingMail {
            to: "t@x.io".to_owned(),
            subject: "s".to_owned(),
            text_body: "b".to_owned(),
            html_body: "<p>b</p>".to_owned(),
        };
        assert!(smtp_send(&cfg, &mail).await.is_err());
        let seen = handle.await.expect("server");
        assert_eq!(seen.len(), 2);
    }

    #[tokio::test]
    async fn smtp_conversation_delivers_multipart() {
        let (port, handle) = run_fake_smtp(vec![
            "250-fake\r\n250 AUTH PLAIN LOGIN\r\n", // EHLO extensions
            "250 ok\r\n",                           // MAIL
            "250 ok\r\n",                           // RCPT
        ])
        .await;
        let cfg = SmtpSend {
            host: "127.0.0.1".to_owned(),
            port: i64::from(port),
            username: None,
            password: None,
            use_tls: false,
            use_ssl: false,
            from_email: "Team Pi Dash <team@airepublic.com>".to_owned(),
        };
        let mail = OutgoingMail {
            to: "contract@example.com".to_owned(),
            subject: "Your unique Pi Dash login code is 842103".to_owned(),
            text_body: "plain".to_owned(),
            html_body: "<p>hi</p>".to_owned(),
        };
        smtp_send(&cfg, &mail).await.expect("send");
        let data = handle.await.expect("server");
        assert!(data.contains("From: Team Pi Dash <team@airepublic.com>\r\n"));
        assert!(data.contains("To: contract@example.com\r\n"));
        assert!(data.contains("Subject: Your unique Pi Dash login code is 842103\r\n"));
        assert!(data.contains("Content-Type: multipart/alternative;"));
        assert!(data.contains("Content-Type: text/plain; charset=\"utf-8\""));
        assert!(data.contains("Content-Type: text/html; charset=\"utf-8\""));
        assert!(data.contains("\r\nplain\r\n"));
        assert!(data.contains("\r\n<p>hi</p>\r\n"));
    }

    #[test]
    fn message_shape_matches_django() {
        let mail = OutgoingMail {
            to: "contract@example.com".to_owned(),
            subject: "s".to_owned(),
            text_body: "plain".to_owned(),
            html_body: "<p>hi ’</p>".to_owned(),
        };
        let bytes = message_bytes("f@x.io", &mail, "b", "date", "<id>").expect("builds");
        let text = String::from_utf8(bytes).expect("utf-8");
        // `as_bytes(linesep="\r\n")`: no bare-LF line ending survives
        // (a strict server reads the whole body as one overlong line).
        assert!(!text.replace("\r\n", "").contains('\n'));
        assert!(text.contains("Content-Type: multipart/alternative; boundary=\"b\""));
        assert!(text.contains("Content-Type: text/plain; charset=\"utf-8\""));
        // ASCII part 7bit, non-ASCII part 8bit (probed Django behavior).
        assert!(text.contains("Content-Transfer-Encoding: 7bit\r\n\r\nplain"));
        assert!(text.contains("Content-Transfer-Encoding: 8bit"));
        assert!(text.ends_with("--b--\r\n"));
    }

    #[test]
    fn cte_selection_matches_django_boundary() {
        // Probed from `EmailMultiAlternatives` (PIDASHCONV-1104): a part
        // flips to quoted-printable exactly when a UTF-8 body line
        // exceeds 998 bytes.
        assert!(!body_needs_qp(&"q".repeat(998)));
        assert!(body_needs_qp(&"q".repeat(999)));
        assert!(!body_needs_qp(&format!("{}é", "q".repeat(990))));
        assert!(body_needs_qp(&format!("{}é", "q".repeat(997))));
        assert!(!body_needs_qp("short\nlines\nok"));
        assert!(!body_needs_qp("short\nok\n"));
        let mail = OutgoingMail {
            to: "t@e.st".to_owned(),
            subject: "s".to_owned(),
            text_body: "q".repeat(999),
            html_body: "<p>é</p>".to_owned(),
        };
        let text =
            String::from_utf8(message_bytes("f@x.io", &mail, "b", "date", "<id>").expect("builds"))
                .expect("utf-8");
        assert!(text.contains("Content-Transfer-Encoding: quoted-printable"));
        // The short non-ASCII part stays 8bit.
        assert!(text.contains("Content-Transfer-Encoding: 8bit"));
    }

    /// Minimal quoted-printable decoder for the roundtrip pin below
    /// (soft breaks + `=XX`; the encoder never emits lowercase hex).
    fn qp_decode(encoded: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let bytes = encoded.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'=' {
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 2;
                    continue;
                }
                if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                    i += 3;
                    continue;
                }
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).expect("hex pair");
                out.push(u8::from_str_radix(hex, 16).expect("hex value"));
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        out
    }

    #[test]
    fn quoted_printable_matches_quopri_shape() {
        // Wrapping: 75 content chars + soft `=`, all lines short.
        let encoded = quoted_printable_encode(&"q".repeat(200));
        for line in encoded.split('\n') {
            assert!(line.len() <= 76, "{line:?}");
        }
        assert!(encoded.contains('='));
        assert_eq!(qp_decode(&encoded), vec![b'q'; 200]);
        // Non-ASCII becomes uppercase `=XX`; escapes never split.
        let encoded = quoted_printable_encode(&format!("é{}", "q".repeat(200)));
        assert!(encoded.starts_with("=C3=A9"));
        assert_eq!(
            qp_decode(&encoded),
            format!("é{}", "q".repeat(200)).into_bytes()
        );
        for line in encoded.split('\n') {
            assert!(line.len() <= 76, "{line:?}");
            assert!(!line.ends_with('=') || line.len() == 76);
        }
        // `=` itself is encoded; trailing spaces/tabs never survive raw.
        assert_eq!(quoted_printable_encode("a=b"), "a=3Db");
        assert_eq!(quoted_printable_encode("a "), "a=20");
        assert_eq!(quoted_printable_encode("a\t"), "a=09");
        assert_eq!(qp_decode(&quoted_printable_encode("a ")), b"a ");
        // Empty and short bodies pass through the translate-only path.
        assert_eq!(quoted_printable_encode(""), "");
        assert_eq!(quoted_printable_encode("hi"), "hi");
    }

    #[test]
    fn quoted_printable_matches_quopri_goldens() {
        // Byte-for-byte against CPython `quopri.body_encode`
        // (`/tmp/probe1104_qp.py`): escapes never split at wrap points,
        // full trailing-whitespace lines encode.
        let break_case = format!("{}é{}", "q".repeat(73), "q".repeat(100));
        let encoded = quoted_printable_encode(&break_case);
        let lines: Vec<&str> = encoded.split('\n').collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], format!("{}=", "q".repeat(73)));
        assert!(lines[1].starts_with("=C3=A9"));
        assert_eq!(lines[1].len(), 76);
        assert_eq!(lines[2], "q".repeat(31));
        assert_eq!(qp_decode(&encoded), break_case.into_bytes());
        let room1 = format!("{} ", "q".repeat(75));
        assert_eq!(
            quoted_printable_encode(&room1),
            format!("{}=\n=20", "q".repeat(75))
        );
        assert_eq!(
            quoted_printable_encode("café ’ =\t\nline2 "),
            "caf=C3=A9 =E2=80=99 =3D=09\nline2=20"
        );
        // `room == 2` (75-char line, trailing whitespace): the soft
        // break keeps its own eol, so the real line break survives
        // (review fuzz vs `quoprimime.body_encode`: `...X=\n\n<next>`).
        assert_eq!(
            quoted_printable_encode(&format!("{} ", "q".repeat(74))),
            format!("{} =\n", "q".repeat(74))
        );
        assert_eq!(
            quoted_printable_encode(&format!("{} \nx", "q".repeat(74))),
            format!("{} =\n\nx", "q".repeat(74))
        );
        assert_eq!(
            quoted_printable_encode(&format!("{}\t", "q".repeat(74))),
            format!("{}\t=\n", "q".repeat(74))
        );
    }

    #[test]
    fn mixed_shape_matches_probed_django() {
        // Pinned to the Django 4.2 probe: body + one CSV attach, no
        // HTML alternative (`send_export_email` shape).
        let csv = "\"X-Axis\",\"Y-Axis\"\r\n\"a\",\"1\"\r\n";
        let bytes = message_bytes_mixed(
            "Team Pi Dash <team@airepublic.com>",
            "contract@example.com",
            "Your Export is ready",
            "plain ascii body",
            &[MailAttachment {
                filename: "acme-analytics.csv".to_owned(),
                content: csv,
            }],
            "b",
            "date",
            "<id>",
        )
        .expect("builds");
        let text = String::from_utf8(bytes).expect("utf-8");
        assert!(!text.replace("\r\n", "").contains('\n'));
        // Header order is Django's.
        let head = text.split("\r\n\r\n").next().expect("headers");
        let order: Vec<&str> = head
            .split("\r\n")
            .map(|l| l.split(':').next().unwrap())
            .collect();
        assert_eq!(
            order,
            [
                "Content-Type",
                "MIME-Version",
                "Subject",
                "From",
                "To",
                "Date",
                "Message-ID"
            ]
        );
        assert!(text.contains("Content-Type: multipart/mixed; boundary=\"b\""));
        assert!(text.contains(
            "--b\r\nContent-Type: text/plain; charset=\"utf-8\"\r\nMIME-Version: 1.0\r\n\
             Content-Transfer-Encoding: 7bit\r\n\r\nplain ascii body\r\n"
        ));
        assert!(text.contains(
            "--b\r\nContent-Type: text/csv; charset=\"utf-8\"\r\nMIME-Version: 1.0\r\n\
             Content-Transfer-Encoding: 7bit\r\n\
             Content-Disposition: attachment; filename=\"acme-analytics.csv\"\r\n\
             \r\n\"X-Axis\",\"Y-Axis\"\r\n\"a\",\"1\"\r\n"
        ));
        assert!(text.ends_with("--b--\r\n"));
    }

    #[test]
    fn mixed_non_ascii_part_is_8bit_raw() {
        // Probed: non-ASCII bodies/attachments stay raw UTF-8 with
        // `8bit` (Django never base64s `str` parts).
        let bytes = message_bytes_mixed(
            "f@x.io",
            "t@x.io",
            "s",
            "café",
            &[MailAttachment {
                filename: "s-analytics.csv".to_owned(),
                content: "\"café\"\r\n",
            }],
            "b",
            "date",
            "<id>",
        )
        .expect("builds");
        let text = String::from_utf8(bytes).expect("utf-8");
        assert!(text.contains("Content-Transfer-Encoding: 8bit\r\n\r\ncafé\r\n"));
        assert!(
            text.contains("Content-Transfer-Encoding: 8bit\r\nContent-Disposition: attachment;")
        );
        assert!(text.contains("\"café\"\r\n\r\n--b--\r\n"));
    }

    #[test]
    fn mixed_long_attachment_line_is_quoted_printable() {
        // PIDASHCONV-1691: Django's `SafeMIMEText.set_payload`
        // (`django/core/mail/message.py:168-175`, Django 4.2.30) switches
        // any part with a line over 998 bytes to quoted-printable — the
        // body and every `str` attachment alike (`_create_mime_attachment`
        // builds attachments as `SafeMIMEText`). The analytic CSV export
        // (`send_export_email`, `analytic_plot_export.py:52-88`) attaches
        // its CSV this way, so a segmented header over 998 bytes must go
        // QP with no wire line over 76, exactly like `message_bytes`.
        let labels: Vec<String> = (0..60).map(|i| format!("seg-label-{i:05}")).collect();
        let mut header = String::from("\"X-Axis\",\"Issue Count\"");
        for name in &labels {
            header.push_str(&format!(",\"{name}\""));
        }
        assert_eq!(header.len(), 1102);
        let mut row = String::from("\"Todo\",\"60\"");
        for _ in 0..60 {
            row.push_str(",\"1\"");
        }
        let csv = format!("{header}\r\n{row}\r\n");
        let bytes = message_bytes_mixed(
            "Team Pi Dash <team@airepublic.com>",
            "contract@example.com",
            "Your Export is ready",
            "plain ascii body",
            &[MailAttachment {
                filename: "acme-analytics.csv".to_owned(),
                content: &csv,
            }],
            "b",
            "date",
            "<id>",
        )
        .expect("builds");
        let text = String::from_utf8(bytes).expect("utf-8");
        assert!(text.contains(
            "--b\r\nContent-Type: text/csv; charset=\"utf-8\"\r\nMIME-Version: 1.0\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             Content-Disposition: attachment; filename=\"acme-analytics.csv\"\r\n"
        ));
        // Short ASCII body still goes raw.
        assert!(text.contains("Content-Transfer-Encoding: 7bit\r\n\r\nplain ascii body\r\n"));
        // No wire line survives overlong (QP wraps payload at 76).
        assert!(
            text.split("\r\n").all(|line| line.len() <= 78),
            "longest: {}",
            text.split("\r\n").map(str::len).max().unwrap_or(0)
        );
        // First wrapped line, byte-exact against the Django 4.2.30 probe.
        let payload = attachment_payload(&text, "b");
        let first = payload.split("\r\n").next().expect("payload");
        assert_eq!(
            first,
            "\"X-Axis\",\"Issue Count\",\"seg-label-00000\",\"seg-label-00001\",\"seg-label-00002="
        );
        // Round-trip: soft breaks unfold back to the original CSV
        // (the part's terminating CRLF is framing, not content).
        let payload = payload.strip_suffix("\r\n").unwrap_or(payload);
        let mut unfolded = String::new();
        for line in payload.split("\r\n") {
            match line.strip_suffix('=') {
                Some(chunk) => unfolded.push_str(chunk),
                None => {
                    unfolded.push_str(line);
                    unfolded.push('\n');
                }
            }
        }
        assert_eq!(unfolded, csv.replace("\r\n", "\n"));
    }

    /// Payload bytes of the attachment part (between its blank line and
    /// the closing boundary), CRLF-joined.
    fn attachment_payload<'a>(text: &'a str, boundary: &str) -> &'a str {
        let marker = "Content-Disposition: attachment;";
        let start = text.find(marker).expect("attachment part");
        let after_headers = text[start..]
            .find("\r\n\r\n")
            .map(|i| start + i + 4)
            .expect("blank line");
        let end_marker = format!("\r\n--{boundary}--");
        let end = text[after_headers..]
            .find(&end_marker)
            .map(|i| after_headers + i)
            .expect("closing boundary");
        &text[after_headers..end]
    }

    #[test]
    fn mixed_long_body_line_is_quoted_printable() {
        // Same `SafeMIMEText` selection for the mixed text body: a
        // 999-byte line flips to QP, 998 stays `7bit` (the probed
        // `message_bytes` boundary, PIDASHCONV-1104).
        for (len, cte) in [(998, "7bit"), (999, "quoted-printable")] {
            let body = "q".repeat(len);
            let bytes = message_bytes_mixed(
                "f@x.io",
                "t@x.io",
                "s",
                &body,
                &[MailAttachment {
                    filename: "s-analytics.csv".to_owned(),
                    content: "\"a\"\r\n",
                }],
                "b",
                "date",
                "<id>",
            )
            .expect("builds");
            let text = String::from_utf8(bytes).expect("utf-8");
            let marker = format!(
                "--b\r\nContent-Type: text/plain; charset=\"utf-8\"\r\nMIME-Version: 1.0\r\n\
                 Content-Transfer-Encoding: {cte}\r\n"
            );
            assert!(text.contains(&marker), "len {len} -> {cte}");
        }
    }

    #[test]
    fn rfc2047_subject_vectors_match_python() {
        // Stage-1 `Header(val, 'utf-8').encode()` pins (Django 4.2.30
        // venv probes): single words, Q/B SHORTEST picks, ties to Q,
        // multi-word folds.
        let cases = [
            (
                "José has invited you to join them in Café on Pi Dash",
                "=?utf-8?q?Jos=C3=A9_has_invited_you_to_join_them_in_Caf=C3=A9_on_Pi_Dash?=",
            ),
            (
                "José has invited you to join them in Café on Pi Dash with extra words éàü",
                "=?utf-8?q?Jos=C3=A9_has_invited_you_to_join_them_in_Caf=C3=A9_on_Pi_Dash_wit?=\n \
                 =?utf-8?q?h_extra_words_=C3=A9=C3=A0=C3=BC?=",
            ),
            ("josé", "=?utf-8?b?am9zw6k=?="),
            ("Équipe Pi Dash", "=?utf-8?q?=C3=89quipe_Pi_Dash?="),
            ("Jürgen", "=?utf-8?q?J=C3=BCrgen?="),
            ("é", "=?utf-8?b?w6k=?="),
            ("éab", "=?utf-8?q?=C3=A9ab?="),
            ("aéb", "=?utf-8?q?a=C3=A9b?="),
            ("", ""),
            ("   ", "=?utf-8?q?___?="),
            (
                "日本語の長い表示名テスト追加分です",
                "=?utf-8?b?5pel5pys6Kqe44Gu6ZW344GE6KGo56S65ZCN44OG44K544OI6L+95Yqg5YiG44Gn?=\n \
                 =?utf-8?b?44GZ?=",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(utf8_header_encode(input), expected, "{input:?}");
        }
        // Full wire headers (stage 1 + generator refold under
        // `Subject: `): the encoded word is unbreakable, so over-long
        // subjects fold right after the prefix.
        assert_eq!(
            subject_header("José has invited you to join them in Café on Pi Dash")
                .expect("encodes"),
            "Subject: \r\n \
             =?utf-8?q?Jos=C3=A9_has_invited_you_to_join_them_in_Caf=C3=A9_on_Pi_Dash?=",
        );
        assert_eq!(
            subject_header("Hello Pi Dash").expect("encodes"),
            "Subject: Hello Pi Dash",
        );
        assert_eq!(
            subject_header(
                "A new password to your Pi Dash account has been requested \
                 with some extra padding words here"
            )
            .expect("encodes"),
            "Subject: A new password to your Pi Dash account has been requested with some\r\n \
             extra\r\n \
             padding words here",
        );
        assert_eq!(
            subject_header(
                "José has invited you to join them in Café on Pi Dash with extra words éàü"
            )
            .expect("encodes"),
            "Subject: \r\n \
             =?utf-8?q?Jos=C3=A9_has_invited_you_to_join_them_in_Caf=C3=A9_on_Pi_Dash_wit?=\r\n \
             =?utf-8?q?h_extra_words_=C3=A9=C3=A0=C3=BC?=",
        );
        assert_eq!(
            subject_header("a\nb").expect_err("newline rejected"),
            "invalid email header",
        );
        assert_eq!(
            subject_header("a\rb").expect_err("CR rejected"),
            "invalid email header",
        );
    }

    #[test]
    fn ascii_fold_engine_vectors_match_python() {
        // `Header(val).encode()` pins: unbreakable lines stand (no
        // prefix) or move whole (with prefix), splits prefer `;` over
        // `,` over whitespace, FWS runs ride along.
        assert_eq!(
            ascii_header_encode(&"s".repeat(100), 0, "\n"),
            "s".repeat(100)
        );
        assert_eq!(
            ascii_header_encode(&"s".repeat(100), "Subject".len() + 2, "\n"),
            format!("\n {}", "s".repeat(100)),
        );
        assert_eq!(
            ascii_header_encode(&format!("{} b", "a".repeat(70)), 0, "\n"),
            format!("{} b", "a".repeat(70)),
        );
        assert_eq!(
            ascii_header_encode(&format!("{} b", "a".repeat(77)), 0, "\n"),
            format!("{}\n b", "a".repeat(77)),
        );
        assert_eq!(
            ascii_header_encode(&format!("{}; {}", "a".repeat(70), "b".repeat(20)), 0, "\n"),
            format!("{};\n {}", "a".repeat(70), "b".repeat(20)),
        );
        assert_eq!(
            ascii_header_encode(&format!("{}, {}", "a".repeat(70), "b".repeat(20)), 0, "\n"),
            format!("{},\n {}", "a".repeat(70), "b".repeat(20)),
        );
        assert_eq!(
            ascii_header_encode(&format!("{}  {}", "a".repeat(70), "b".repeat(20)), 0, "\n"),
            format!("{}\n  {}", "a".repeat(70), "b".repeat(20)),
        );
        assert_eq!(
            ascii_header_encode(
                &format!(
                    "{}; {} , {}",
                    "a".repeat(60),
                    "b".repeat(10),
                    "c".repeat(20)
                ),
                0,
                "\n",
            ),
            format!(
                "{};\n {} , {}",
                "a".repeat(60),
                "b".repeat(10),
                "c".repeat(20)
            ),
        );
        assert_eq!(
            ascii_header_encode(&format!("{}\t{}", "a".repeat(70), "b".repeat(20)), 0, "\n"),
            format!("{}\n\t{}", "a".repeat(70), "b".repeat(20)),
        );
    }

    #[test]
    fn sanitize_address_vectors_match_django() {
        // `sanitize_address` pins (tuple/header semantics: `@`
        // required): display encode + quote, local encode + quote,
        // IDNA domain, comment/whitespace handling.
        let cases = [
            ("contract@example.com", "contract@example.com"),
            ("user@münchen.de", "user@xn--mnchen-3ya.de"),
            ("josé@example.com", "=?utf-8?b?am9zw6k=?=@example.com"),
            ("Team <t@e.com>", "Team <t@e.com>"),
            (
                "Équipe Pi Dash <team@x.com>",
                "=?utf-8?q?=C3=89quipe_Pi_Dash?= <team@x.com>",
            ),
            ("\"Doe, John\" <j@x.com>", "\"Doe, John\" <j@x.com>"),
            ("\"a@b\" <x@y.de>", "\"a@b\" <x@y.de>"),
            ("\"á@b\" <x@y.de>", "=?utf-8?b?w6FAYg==?= <x@y.de>"),
            ("José (boss) <x@y.de>", "=?utf-8?b?Sm9zw6k=?= <x@y.de>"),
            ("José (a (b) c) <x@y.de>", "=?utf-8?b?Sm9zw6k=?= <x@y.de>"),
            ("(boss) <x@y.de>", "x@y.de"),
            ("x@y.de (José)", "x@y.de"),
            ("  x@y.de  ", "x@y.de"),
            ("<team@x.com>", "team@x.com"),
            ("a b@c.de", "\"a b\"@c.de"),
            ("á b@c.de", "=?utf-8?q?=C3=A1_b?=@c.de"),
            ("a  b@c.de", "\"a b\"@c.de"),
            ("\"a b\"@x.de", "\"a b\"@x.de"),
            ("\"áb\"@x.de", "=?utf-8?b?w6Fi?=@x.de"),
            ("\"a\\\"b\"@x.de", "\"a\\\"b\"@x.de"),
            ("a.b@x.de", "a.b@x.de"),
            (".a@x.de", ".a@x.de"),
            ("a..b@x.de", "a..b@x.de"),
            ("\"ab\"@x.de", "ab@x.de"),
            ("\"a.b\"@x.de", "a.b@x.de"),
            ("\"\"@b.de", "@b.de"),
            ("Mr. José <a@b.de>", "=?utf-8?b?TXIuIEpvc8Op?= <a@b.de>"),
            ("José\tBoss <x@y.de>", "=?utf-8?q?Jos=C3=A9_Boss?= <x@y.de>"),
            ("José  Boss <x@y.de>", "=?utf-8?q?Jos=C3=A9_Boss?= <x@y.de>"),
            ("Team <téam@x.com>", "Team <=?utf-8?b?dMOpYW0=?=@x.com>"),
            // The parts check runs post-parse: a newline smuggled
            // inside a stripped comment does not fail the send.
            ("(a\nb) <x@y.de>", "x@y.de"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                sanitize_address_piece(input, true).expect("valid"),
                expected,
                "{input:?}",
            );
        }
        for input in [
            "a@b@c.de",
            "<>",
            "Jürgen <>",
            "",
            "@b.de",
            "a@",
            "@",
            "fóo",
            "fóo bar",
            "José: boss <x@y.de>",
            "José; boss <x@y.de>",
            "José@home <x@y.de>",
            "a<b <x@y.de>",
            "Doe, José <x@y.de>",
            "a\nb@c.de",
        ] {
            assert!(
                sanitize_address_piece(input, true).is_err(),
                "{input:?} must fail",
            );
        }
    }

    #[test]
    fn idna_domain_vectors_match_python() {
        // `punycode` pins: ASCII labels verbatim (case kept), per-label
        // encode, trailing root dot kept, empty/overlong refused.
        let cases = [
            ("example.com", "example.com"),
            ("EXAMPLE.COM", "EXAMPLE.COM"),
            ("münchen.de", "xn--mnchen-3ya.de"),
            ("MÜNCHEN.DE", "xn--mnchen-3ya.DE"),
            ("exämple.com", "xn--exmple-cua.com"),
            ("mün_chen.de", "xn--mn_chen-n2a.de"),
            ("münchen.de.", "xn--mnchen-3ya.de."),
            ("example.com.", "example.com."),
            ("", ""),
            ("münchen", "xn--mnchen-3ya"),
            ("例え.jp", "xn--r8jz45g.jp"),
            ("-x.de", "-x.de"),
            ("x-.de", "x-.de"),
            ("_dmarc.x.de", "_dmarc.x.de"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                idna_ascii_domain(input).expect("valid"),
                expected,
                "{input:?}",
            );
        }
        let long_label = "l".repeat(63) + ".de";
        assert_eq!(
            idna_ascii_domain(&long_label).expect("63 chars fit"),
            long_label,
        );
        // Residual deviation mapping (UTS-46 keeps ß; IDNA2003 maps it
        // to `ss`, so Python answers `strasse.de`): same residual as
        // the EmailValidator port.
        assert_eq!(
            idna_ascii_domain("straße.de").expect("valid"),
            "xn--strae-oqa.de",
        );
        for input in [".", "a..de"] {
            assert!(idna_ascii_domain(input).is_err(), "{input:?} must fail");
        }
        assert!(idna_ascii_domain(&("l".repeat(64) + ".de")).is_err());
        assert!(idna_ascii_domain(&format!("{}.de", "ü".repeat(63))).is_err());
    }

    #[test]
    fn address_header_wire_vectors_match_django() {
        // Full `From:`/`To:` wire headers: ASCII verbatim, non-ASCII
        // sanitized piece by piece, refolded under the name.
        let cases = [
            (
                "From",
                "Pi Dash Contract <contract@example.com>",
                "From: Pi Dash Contract <contract@example.com>",
            ),
            (
                "From",
                "Équipe Pi Dash <team@x.com>",
                "From: =?utf-8?q?=C3=89quipe_Pi_Dash?= <team@x.com>",
            ),
            ("To", "user@münchen.de", "To: user@xn--mnchen-3ya.de"),
            (
                "To",
                "josé@example.com",
                "To: =?utf-8?b?am9zw6k=?=@example.com",
            ),
            (
                "To",
                "Jürgen <j@exämple.com>",
                "To: =?utf-8?q?J=C3=BCrgen?= <j@xn--exmple-cua.com>",
            ),
            ("To", "a@x.de, b@y.de", "To: a@x.de, b@y.de"),
            (
                "To",
                "a@x.de, Jürgen <j@y.de>",
                "To: a@x.de, =?utf-8?q?J=C3=BCrgen?= <j@y.de>",
            ),
            (
                "To",
                "\"Doe, John\" <j@x.com>",
                "To: \"Doe, John\" <j@x.com>",
            ),
        ];
        for (name, input, expected) in cases {
            assert_eq!(
                address_header(name, input).expect("valid"),
                expected,
                "{name}: {input:?}",
            );
        }
        assert!(address_header("To", "á@b@c").is_err());
        assert!(address_header("To", "fóo").is_err());
        assert!(address_header("From", "a\nb@c.de").is_err());
    }

    #[test]
    fn envelope_address_vectors_match_smtplib() {
        // `sanitize` + `quoteaddr`: display validated but dropped,
        // addr-spec IDNA-encoded, local-only accepted, garbage fails.
        let cases = [
            (
                "Team Pi Dash <team@airepublic.com>",
                "<team@airepublic.com>",
            ),
            ("plain@example.com", "<plain@example.com>"),
            ("Équipe Pi Dash <team@x.com>", "<team@x.com>"),
            ("user@münchen.de", "<user@xn--mnchen-3ya.de>"),
            ("josé@example.com", "<=?utf-8?b?am9zw6k=?=@example.com>"),
            ("fóo", "<=?utf-8?q?f=C3=B3o?=>"),
            ("foo", "<foo>"),
            ("a b@c.de", "<\"a b\"@c.de>"),
            ("<team@x.com>", "<team@x.com>"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                envelope_address(input).expect("valid"),
                expected,
                "{input:?}"
            );
        }
        for input in ["a@b@c.de", "", "a@", "@b", "<>", "Jürgen <>", "a\nb@c.de"] {
            assert!(envelope_address(input).is_err(), "{input:?} must fail");
        }
    }

    #[test]
    fn header_path_residuals_vs_old_parser_getaddresses() {
        // Known divergences from Django's HEADER path (probed on Django
        // 4.2.30 / CPython 3.12 in the PIDASHCONV-1286 review): the header
        // path feeds `getaddresses` TUPLES through `sanitize_address`
        // (`message.py:63-69`), while this port sanitizes each comma
        // piece with the string-path (`get_mailbox`) rules. The two agree
        // on every realistic input (bare and `Display <addr>` addresses,
        // quoted `,`/`;` in displays, multi-recipient lists — see the
        // vectors above); they differ only on malformed or exotic inputs
        // that app validation (`validate_email` on invites) and the
        // single-recipient `to=[email]` shape keep out of reach. The
        // envelope is unaffected throughout (the display never reaches
        // the wire there).
        // An unquoted `:` reads as a route marker to the old parser, so
        // Django SENDS `To: boss <x@y.de>`; refused here, like the new
        // parser refuses it.
        assert!(address_header("To", "José: boss <x@y.de>").is_err());
        // The old parser keeps `(comment)` text in the display name and
        // encodes it (`To: =?utf-8?b?Sm9zw6kgKGJvc3Mp?= <x@y.de>`);
        // stripped here.
        assert_eq!(
            address_header("To", "José (boss) <x@y.de>").expect("builds"),
            "To: =?utf-8?b?Sm9zw6k=?= <x@y.de>",
        );
        // A trailing comment becomes the display name to the old parser
        // (`To: =?utf-8?b?Sm9zw6k=?= <x@y.de>`); stripped here.
        assert_eq!(
            address_header("To", "x@y.de (José)").expect("builds"),
            "To: x@y.de",
        );
        // Quoted local parts keep their quotes through `getaddresses`
        // and `Address` re-quotes them (`To: "\"ab\""@x.de`, Django's own
        // garbage); unquoted here.
        assert_eq!(
            address_header("To", "\"ab\"@x.de, é@y.de").expect("builds"),
            "To: ab@x.de, =?utf-8?b?w6k=?=@y.de",
        );
        // Group syntax (`Friends: a@x.de, é@y.de;`) delivers to the
        // members upstream; refused here (the `:` fails the piece).
        assert!(address_header("To", "Friends: a@x.de, é@y.de;").is_err());
        // `""` parses to an empty mailbox upstream (`Address('','')`
        // renders `<>`, so Django sends `MAIL FROM:<>`); refused here.
        assert!(envelope_address("\"\"").is_err());
    }

    #[test]
    fn message_bytes_encodes_headers_like_django() {
        // The issue case end to end: non-ASCII From/To/Subject render
        // as Django's wire bytes (folded encoded words, punycode).
        let mail = OutgoingMail {
            to: "invite@münchen.de".to_owned(),
            subject: "José has invited you to join them in Café on Pi Dash".to_owned(),
            text_body: "plain".to_owned(),
            html_body: "<p>hi</p>".to_owned(),
        };
        let text = String::from_utf8(
            message_bytes("Équipe Pi Dash <team@x.com>", &mail, "b", "date", "<id>")
                .expect("builds"),
        )
        .expect("utf-8");
        let head = text.split("\r\n\r\n").next().expect("headers");
        assert!(
            head.contains(
                "Subject: \r\n \
                 =?utf-8?q?Jos=C3=A9_has_invited_you_to_join_them_in_Caf=C3=A9_on_Pi_Dash?="
            ),
            "{head}",
        );
        assert!(
            head.contains("From: =?utf-8?q?=C3=89quipe_Pi_Dash?= <team@x.com>"),
            "{head}",
        );
        assert!(head.contains("To: invite@xn--mnchen-3ya.de"), "{head}");
        // Short ASCII headers stay byte-identical to before.
        let mail = OutgoingMail {
            to: "contract@example.com".to_owned(),
            subject: "Your unique Pi Dash login code is 842103".to_owned(),
            text_body: "plain".to_owned(),
            html_body: "<p>hi</p>".to_owned(),
        };
        let text = String::from_utf8(
            message_bytes(
                "Team Pi Dash <team@airepublic.com>",
                &mail,
                "b",
                "date",
                "<id>",
            )
            .expect("builds"),
        )
        .expect("utf-8");
        assert!(text.contains("From: Team Pi Dash <team@airepublic.com>\r\n"));
        assert!(text.contains("To: contract@example.com\r\n"));
        assert!(text.contains("Subject: Your unique Pi Dash login code is 842103\r\n"));
        // CR/LF still fails the build (was the smtp_send pre-check).
        let mail = OutgoingMail {
            to: "t@e.st".to_owned(),
            subject: "a\nb".to_owned(),
            text_body: "b".to_owned(),
            html_body: "h".to_owned(),
        };
        assert_eq!(
            message_bytes("f@x.io", &mail, "b", "date", "<id>").expect_err("rejected"),
            "invalid email header",
        );
    }

    #[test]
    fn mixed_unknown_extension_fails_send() {
        // `mimetypes.guess_type(...)[0]` is `None` → `.split` raises in
        // Python; here the builder errors and the send fails the same way.
        let result = message_bytes_mixed(
            "f@x.io",
            "t@x.io",
            "s",
            "b",
            &[MailAttachment {
                filename: "file.unknownext".to_owned(),
                content: "x",
            }],
            "b",
            "date",
            "<id>",
        );
        assert!(result.is_err());
    }
}
