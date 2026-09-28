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

/// Render the MIME bytes (`EmailMessage.message()` shape, verified against
/// the interpreter): `multipart/alternative` with `text/plain` then
/// `text/html` (`charset="utf-8"`, `7bit` CTE for ASCII parts, `8bit`
/// otherwise, raw UTF-8 either way), `From`/`To`/`Subject` verbatim,
/// `Date`/`Message-ID` synthesized like Django's.
pub fn message_bytes(
    from: &str,
    mail: &OutgoingMail,
    boundary: &str,
    date: &str,
    msg_id: &str,
) -> Vec<u8> {
    let part = |ctype: &str, body: &str| {
        let cte = if body.is_ascii() { "7bit" } else { "8bit" };
        format!(
            "--{boundary}\r\nContent-Type: {ctype}; charset=\"utf-8\"\r\nContent-Transfer-Encoding: {cte}\r\n\r\n{body}\r\n"
        )
    };
    let mut out = format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {date}\r\nMessage-ID: {msg_id}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n",
        to = mail.to,
        subject = mail.subject,
    );
    out.push_str(&part("text/plain", &mail.text_body));
    out.push_str(&part("text/html", &mail.html_body));
    out.push_str(&format!("--{boundary}--\r\n"));
    // `message.as_bytes(linesep="\r\n")` (what `EmailBackend.send`
    // transmits): normalize every line ending to CRLF first…
    let crlf = out.replace("\r\n", "\n").replace('\n', "\r\n");
    // …then dot-stuffing (`smtplib.data` quotes leading periods).
    let mut stuffed = String::with_capacity(crlf.len());
    for line in crlf.split_inclusive('\n') {
        if line.starts_with('.') {
            stuffed.push('.');
        }
        stuffed.push_str(line);
    }
    stuffed.into_bytes()
}

/// `smtplib.quoteaddr`: display name off, bare addr-spec in `<>`.
fn quote_address(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(start) = trimmed.rfind('<') {
        if let Some(end) = trimmed[start..].find('>') {
            let inner = trimmed[start + 1..start + end].trim();
            if !inner.is_empty() {
                return format!("<{inner}>");
            }
        }
    }
    if trimmed.starts_with('<') {
        return trimmed.to_owned();
    }
    format!("<{trimmed}>")
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
    if [
        mail.subject.as_str(),
        cfg.from_email.as_str(),
        mail.to.as_str(),
    ]
    .iter()
    .any(|h| h.contains(['\r', '\n']))
    {
        // `EmailMessage.message()` header validation (`BadHeaderError`).
        return Err("invalid email header".to_owned());
    }
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
    io.write_str(&format!("MAIL FROM:{}", quote_address(&cfg.from_email)))
        .await?;
    let (code, _) = io.read_reply().await?;
    if code != 250 {
        return Err("SMTP sender refused".to_owned());
    }
    io.write_str(&format!("RCPT TO:{}", quote_address(&mail.to)))
        .await?;
    let (code, _) = io.read_reply().await?;
    if code != 250 && code != 251 {
        return Err("SMTP recipient refused".to_owned());
    }
    io.write_str("DATA").await?;
    let (code, _) = io.read_reply().await?;
    if code != 354 {
        return Err("SMTP DATA rejected".to_owned());
    }
    let boundary = format!(
        "{:x}{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        std::process::id()
    );
    let date = chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S -0000")
        .to_string();
    let msg_id = format!("<{boundary}@pidash>");
    let content = message_bytes(&cfg.from_email, mail, &boundary, &date, &msg_id);
    io.write_bytes(&content).await?;
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
    fn quote_address_shapes_envelope() {
        assert_eq!(
            quote_address("Team Pi Dash <team@airepublic.com>"),
            "<team@airepublic.com>"
        );
        assert_eq!(quote_address("plain@example.com"), "<plain@example.com>");
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
        let bytes = message_bytes("f@x.io", &mail, "b", "date", "<id>");
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
}
