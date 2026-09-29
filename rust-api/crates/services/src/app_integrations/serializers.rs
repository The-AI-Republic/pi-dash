//! [`WebhookSerializer`](https://github.com/The-AI-Republic/pi-dash/blob/rust-dev/apps/api/pi_dash/app/serializers/webhook.py)
//! validation chain plus `WebhookLogSerializer` metadata.
//!
//! Every message string below is byte-identical to the Python source it
//! cites. Field-level failures collect into `{"url": [...]}`; guard
//! failures render `{"url": "..."}` (verified against real DRF: a
//! `ValidationError({"url": msg})` raised in `create()` leaves the
//! exception handler as `{"url": "msg"}` with status 400).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Serializer metadata (`serializers/webhook.py:92-102`)
// ---------------------------------------------------------------------------

/// `WebhookSerializer.Meta.model` (`serializers/webhook.py:93`).
pub const WEBHOOK_MODEL: &str = "Webhook";

/// `WebhookSerializer.Meta.fields` (`serializers/webhook.py:94`).
pub const WEBHOOK_FIELDS: &str = "__all__";

/// `WebhookSerializer.Meta.read_only_fields`
/// (`serializers/webhook.py:95`).
pub const WEBHOOK_READ_ONLY_FIELDS: &[&str] = &["workspace", "secret_key", "deleted_at"];

/// `WebhookLogSerializer.Meta.model` (`serializers/webhook.py:100`).
pub const WEBHOOK_LOG_MODEL: &str = "WebhookLog";

/// `WebhookLogSerializer.Meta.fields` (`serializers/webhook.py:101`).
pub const WEBHOOK_LOG_FIELDS: &str = "__all__";

/// `WebhookLogSerializer.Meta.read_only_fields`
/// (`serializers/webhook.py:102`). The log serializer carries no custom
/// validation; these constants are its whole port.
pub const WEBHOOK_LOG_READ_ONLY_FIELDS: &[&str] = &["workspace", "webhook"];

/// `Webhook.url` column limit (`db/models/webhook.py:36`
/// `max_length=1024`), enforced by DRF's `CharField` before any URL check.
pub const MAX_URL_CHARS: usize = 1024;

/// Django `URLValidator.max_length` (`django/core/validators.py`);
/// unreachable through [`validate_url_field`] (1024 trips first) but part
/// of [`url_validator_ok`] when called directly.
pub const MAX_VALIDATOR_URL_CHARS: usize = 2048;

/// Django `URLValidator` hostname cap (RFC 1034 §3.1, 253 chars).
pub const MAX_HOSTNAME_CHARS: usize = 253;

/// Default disallowed list (`serializers/webhook.py:46,81`).
pub const DEFAULT_DISALLOWED_DOMAINS: &[&str] = &["airepublic.com"];

// ---------------------------------------------------------------------------
// Messages (byte-identical to Python)
// ---------------------------------------------------------------------------

/// Missing required field (DRF `CharField`, `required=True` default).
pub const MSG_REQUIRED: &str = "This field is required.";
/// Explicit JSON null (DRF `CharField`, `allow_null=False` default).
pub const MSG_NULL: &str = "This field may not be null.";
/// Empty value (DRF `CharField`, `allow_blank=False` default).
pub const MSG_BLANK: &str = "This field may not be blank.";
/// Over-`max_length` value (DRF `CharField`, `max_length=1024` from the model).
pub const MSG_MAX_LENGTH: &str = "Ensure this field has no more than 1024 characters.";
/// NUL content (DRF `ProhibitNullCharactersValidator`, pinned chain position
/// 4th: after `MaxLengthValidator`, before `URLValidator`).
pub const MSG_NULL_CHARS: &str = "Null characters are not allowed.";
/// Django `URLValidator.message`.
pub const MSG_INVALID_URL: &str = "Enter a valid URL.";
/// `validate_schema` (`db/models/webhook.py:24`).
pub const MSG_BAD_SCHEMA: &str = "Invalid schema. Only HTTP and HTTPS are allowed.";
/// `validate_domain` (`db/models/webhook.py:31`).
pub const MSG_LOCAL_URL: &str = "Local URLs are not allowed.";
/// `create()` / `update()` hostname branch
/// (`serializers/webhook.py:28,63`).
pub const MSG_NO_HOSTNAME: &str = "Invalid URL: No hostname found.";
/// `create()` / `update()` `gaierror` branch
/// (`serializers/webhook.py:34,69`).
pub const MSG_UNRESOLVABLE: &str = "Hostname could not be resolved.";
/// `create()` / `update()` empty-`getaddrinfo` branch
/// (`serializers/webhook.py:37,72`).
pub const MSG_NO_IPS: &str = "No IP addresses found for the hostname.";
/// `create()` / `update()` SSRF branch (`serializers/webhook.py:42,77`).
pub const MSG_BLOCKED_IP: &str = "URL resolves to a blocked IP address.";
/// `create()` / `update()` disallowed-domain branch
/// (`serializers/webhook.py:53,88`).
pub const MSG_DISALLOWED_DOMAIN: &str = "URL domain or its subdomain is not allowed.";

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// The two shapes a webhook URL failure takes on the wire.
///
/// `Field` mirrors `serializer.errors` (`{"url": [...]}`); `Guard` mirrors
/// the exception-handler rendering of a `ValidationError({"url": msg})`
/// raised in `create()` / `update()` (`{"url": "..."}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookUrlError {
    Field(Vec<String>),
    Guard(String),
}

impl WebhookUrlError {
    /// HTTP status for both shapes (DRF answers 400 for each).
    pub fn status_code(&self) -> u16 {
        400
    }

    /// The exact JSON body Django sends for this failure.
    pub fn body(&self) -> serde_json::Value {
        match self {
            WebhookUrlError::Field(messages) => serde_json::json!({"url": messages}),
            WebhookUrlError::Guard(message) => serde_json::json!({"url": message}),
        }
    }
}

// ---------------------------------------------------------------------------
// `urlparse` equivalent (scheme / netloc / hostname)
// ---------------------------------------------------------------------------

/// Whether a URL scheme prefix is valid: ASCII alpha first, then alnum /
/// `+` / `-` / `.` (mirrors `urllib.parse` scheme parsing, which yields no
/// scheme for e.g. `"1http://x/"`).
fn valid_scheme_chars(raw_scheme: &str) -> bool {
    let mut bytes = raw_scheme.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_alphabetic() => (),
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// `(lowercased scheme, netloc)` mirroring `urlparse(value).scheme` and
/// `.netloc`: the netloc exists only after `//` and ends at the first `/`,
/// `?` or `#`. Returns an empty scheme when the value has none
/// (`"not-a-url"` → `("", "")`), and an empty netloc when there is no
/// authority (`"http:foo"` → `("http", "")`).
pub fn split_scheme_netloc(value: &str) -> (String, &str) {
    let Some(colon) = value.find(':') else {
        return (String::new(), "");
    };
    let (raw_scheme, rest) = value.split_at(colon);
    if !valid_scheme_chars(raw_scheme) {
        return (String::new(), "");
    }
    let Some(after) = rest[1..].strip_prefix("//") else {
        return (raw_scheme.to_ascii_lowercase(), "");
    };
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    (raw_scheme.to_ascii_lowercase(), &after[..end])
}

/// `urlparse(value).hostname`: the netloc's host lowercased, with userinfo
/// (after the last `@`), brackets and port stripped. `None` when no host
/// is present — empty netloc, bare port, or unbalanced `[` — which is the
/// falsy-`hostname` case raising "Invalid URL: No hostname found."
/// (`serializers/webhook.py:27-28,62-63`).
///
/// A port that Python would reject with `ValueError` on `.hostname`
/// access cannot occur here: the field gate only passes values whose
/// netloc matched the URL regex (or its IDNA fallback). Direct calls with
/// such input map to `None` so the serializer answers 400, never panics.
pub fn hostname_of(netloc: &str) -> Option<String> {
    // Userinfo ends at the last `@` (`urlparse("http://a@b@c/").hostname`
    // is `"c"`).
    let hostinfo = netloc.rsplit('@').next().unwrap_or("");
    if let Some(after_bracket) = hostinfo.strip_prefix('[') {
        // `urlsplit` itself raises `ValueError("Invalid IPv6 URL")` for an
        // unclosed bracket; that maps to `None` here.
        let end = after_bracket.find(']')?;
        let host = &after_bracket[..end];
        if host.is_empty() {
            return None;
        }
        return Some(host.to_lowercase());
    }
    // Non-bracketed hosts split the port at the first `:`:
    // `urlparse("http://1:2:3/x").hostname` is `"1"`, `"http://::1/"`
    // yields `None` (empty host).
    let host = hostinfo.split(':').next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

/// `str.strip()` with no arguments, as DRF's `CharField` applies it
/// (`trim_whitespace=True`): Unicode whitespace plus `\x1c`–`\x1f`, which
/// Rust's `trim()` leaves in place (probed on CPython 3.12 vs rustc:
/// DRF turns `"\x1chttps://example.com/hook\x1c"` into the bare URL).
fn python_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

// ---------------------------------------------------------------------------
// Model validators (`db/models/webhook.py:21-31`)
// ---------------------------------------------------------------------------

/// `validate_schema` (`db/models/webhook.py:21-24`): the `urlparse` scheme
/// must be `http` / `https`. (`urlparse` lowercases the scheme, so
/// `"HTTP://…"` passes, exactly as in Python.)
pub fn validate_schema(value: &str) -> Result<(), &'static str> {
    let (scheme, _) = split_scheme_netloc(value);
    if scheme == "http" || scheme == "https" {
        Ok(())
    } else {
        Err(MSG_BAD_SCHEMA)
    }
}

/// `validate_domain` (`db/models/webhook.py:27-31`): the `urlparse` netloc
/// must not be exactly `localhost` / `127.0.0.1`.
///
/// BUG B1, ported as-is: the comparison uses the netloc, which includes
/// any `:port`, so `"http://localhost:8000/hook"` passes. The unit tests
/// pin this (`fx-web-05-ssrf-guard.json`, `bug_b1`).
pub fn validate_domain(value: &str) -> Result<(), &'static str> {
    let (_, netloc) = split_scheme_netloc(value);
    if netloc == "localhost" || netloc == "127.0.0.1" {
        Err(MSG_LOCAL_URL)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Django `URLValidator` (`django/core/validators.py`, Django 4.2)
// ---------------------------------------------------------------------------

/// Assemble the `URLValidator` host expression. `ul` is the Unicode-letter
/// range `\u00a1-\uffff`; the `(?<!-)` / `(?!-)` guards have no equivalent
/// in the `regex` crate (see the Porting guide's semantic traps), so the
/// domain and TLD labels spell the no-leading/trailing-dash rule out with
/// explicit first/last classes — the same accept set.
fn url_pattern() -> String {
    let ul = "\\xa1-\\u{ffff}";
    let ipv4 = concat!(
        r"(?:0|25[0-5]|2[0-4][0-9]|1[0-9]?[0-9]?|[1-9][0-9]?)",
        r"(?:\.(?:0|25[0-5]|2[0-4][0-9]|1[0-9]?[0-9]?|[1-9][0-9]?)){3}",
    );
    let ipv6 = r"\[[0-9a-f:.]+\]";
    let label = format!(r"[a-z{ul}0-9](?:[a-z{ul}0-9-]{{0,61}}[a-z{ul}0-9])?");
    // `(?:\.(?!-)[…]{1,63}(?<!-))*` unrolled: first char dash-free, then an
    // optional dash-containing middle with a dash-free end (length 1-63).
    let domain = format!(r"(?:\.{label})*");
    // `\.(?!-)(?:[…]{2,63}|xn--[…]{1,59})(?<!-)\.?` unrolled per branch to
    // lengths 2-63 / `xn--` + 1-59 with dash-free ends.
    let tld = format!(r"\.(?:[a-z{ul}][a-z{ul}-]{{0,61}}[a-z{ul}]|xn--[a-z0-9]{{1,59}})\.?");
    format!(
        r"(?i)^(?:[a-z0-9.+\-]*)://(?:[^\s:@/]+(?::[^\s:@/]*)?@)?(?:{ipv4}|{ipv6}|({label}{domain}{tld}|localhost))(?::[0-9]{{1,5}})?(?:[/?#][^\s]*)?\z"
    )
}

static URL_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(&url_pattern()).expect("URLValidator pattern compiles"));

fn url_regex() -> &'static regex::Regex {
    &URL_RE
}

/// IDNA-encode the netloc's host for the validator's IDNA fallback, keeping
/// any `userinfo@` prefix and `:port` suffix byte-identical. Mirrors
/// Django's `punycode(netloc)` (`django/utils/encoding.py`) at verdict
/// level: Django applies IDNA 2003 to the whole netloc while this encodes
/// the host with the `idna` crate (UTS-46, hyphens allowed) and reattaches
/// the decorations — the same accept set on every probed input, including
/// ports, userinfo, trailing dots and uppercase names.
///
/// Returns `None` when the host cannot be encoded (Django's `UnicodeError`
/// path, e.g. empty labels or over-long labels).
fn idna_ascii_netloc(netloc: &str) -> Option<String> {
    let (userinfo, hostport) = match netloc.rsplit_once('@') {
        Some((user, rest)) => (format!("{user}@"), rest),
        None => (String::new(), netloc),
    };
    if let Some(after_bracket) = hostport.strip_prefix('[') {
        let end = after_bracket.find(']')?;
        let inner = &after_bracket[..end];
        let tail = &after_bracket[end + 1..];
        if inner.is_ascii() {
            return None;
        }
        let ascii = idna::domain_to_ascii(inner).ok()?;
        return Some(format!("{userinfo}[{ascii}]{tail}"));
    }
    let (host, port) = match hostport.find(':') {
        Some(i) => (&hostport[..i], &hostport[i..]),
        None => (hostport, ""),
    };
    if host.is_ascii() {
        return None;
    }
    // One trailing dot is legal (`tld_re` ends `\.?`); the encoder sees the
    // bare name and the dot is reattached, as Django's whole-netloc encode
    // produces for `"münchen.de."`.
    let (bare, dot) = host.strip_suffix('.').map_or((host, ""), |b| (b, "."));
    let ascii = idna::domain_to_ascii(bare).ok()?;
    Some(format!("{userinfo}{ascii}{dot}{port}"))
}

/// Django `URLValidator.__call__` as a predicate: length cap, unsafe chars,
/// scheme allowlist (`http/https/ftp/ftps`), the host regex with its IDNA
/// fallback, strict IPv6 validation, and the 253-char hostname cap.
///
/// Two structural notes: after a successful IDNA fallback no IPv6 check
/// runs (Django's check sits in the direct-match branch only); the
/// hostname-length check always applies to the original URL's hostname.
pub fn url_validator_ok(value: &str) -> bool {
    if value.chars().count() > MAX_VALIDATOR_URL_CHARS {
        return false;
    }
    if value.contains(['\t', '\r', '\n']) {
        return false;
    }
    // `value.split("://")[0].lower()` must name a known scheme.
    let scheme = value.split("://").next().unwrap_or("").to_lowercase();
    if !["http", "https", "ftp", "ftps"].contains(&scheme.as_str()) {
        return false;
    }
    // `urlsplit(value)` raising `ValueError` (unbalanced `[`) is invalid.
    let (url_scheme, netloc, rest) = match split_url_parts(value) {
        Some(parts) => parts,
        None => return false,
    };
    if url_regex().is_match(value) {
        // Bracketed hosts are re-validated strictly (`validate_ipv6_address`
        // rejects e.g. `[1.2.3.4]`, which the loose `[0-9a-f:.]+` lets
        // through). A `userinfo@` prefix skips the check in both engines:
        // Django's `^\[…\]…$` search is anchored at the netloc start.
        let bracketed = netloc
            .strip_prefix('[')
            .and_then(|s| s.find(']').map(|end| &s[..end]));
        if let Some(inner) = bracketed {
            if inner.parse::<Ipv6Addr>().is_err() {
                return false;
            }
        }
    } else {
        if value.is_empty() {
            return false;
        }
        let Some(ascii_netloc) = idna_ascii_netloc(netloc) else {
            return false;
        };
        let rebuilt = format!("{url_scheme}://{ascii_netloc}{rest}");
        if !url_regex().is_match(&rebuilt) {
            return false;
        }
    }
    let Some(host) = hostname_of(netloc) else {
        return false;
    };
    if host.chars().count() > MAX_HOSTNAME_CHARS {
        return false;
    }
    true
}

/// `(scheme, netloc, rest)` triple for [`url_validator_ok`], where `rest`
/// is the path/query/fragment tail (possibly empty). `None` when
/// `urlsplit` would raise (`[` without a matching `]` in the authority).
fn split_url_parts(value: &str) -> Option<(String, &str, &str)> {
    let (scheme, netloc) = split_scheme_netloc(value);
    if netloc.contains('[') && !netloc.contains(']') {
        return None;
    }
    // The tail starts at the end of the netloc within the authority section.
    let authority_start = value.find("://").map(|i| i + 3).unwrap_or(0);
    let tail_start = authority_start + netloc.len();
    let rest = value.get(tail_start..).unwrap_or("");
    Some((scheme, netloc, rest))
}

// ---------------------------------------------------------------------------
// Field level (`URLField` + declared validators)
// ---------------------------------------------------------------------------

/// Full field validation for the `url` field: `URLField(validators=[
/// validate_schema, validate_domain])` (`serializers/webhook.py:20`) with
/// `max_length=1024` mapped from the model (`db/models/webhook.py:36`).
///
/// Order mirrors DRF `CharField.run_validation` + `run_validators`: missing
/// → required; [`python_strip`] (`trim_whitespace=True`); blank; then each
/// validator in list order — declared `[validate_schema, validate_domain]`,
/// `MaxLengthValidator` (chars, not bytes), `ProhibitNullCharactersValidator`,
/// `URLValidator` — collecting every failure (so `"not-a-url"` yields both
/// the schema and the URL message).
///
/// `ProhibitSurrogateCharactersValidator` sits between the null check and
/// `URLValidator` in DRF's list but needs no runtime check here: lone
/// surrogates cannot occur in a Rust `&str` (probed: DRF's message is
/// parameterized per codepoint, so there is no fixed string to port).
/// Returns the trimmed value that the guards then inspect.
///
/// Three input states mirror DRF: absent (`None`) → required; explicit JSON
/// null (`Some(None)`) → null; a string → the chain. Callers map the PATCH
/// body the same way (absent key → `None`, which [`validate_update_url`]
/// skips).
pub fn validate_url_field(raw: Option<Option<&str>>) -> Result<String, Vec<String>> {
    let Some(raw) = raw else {
        return Err(vec![MSG_REQUIRED.to_owned()]);
    };
    let Some(raw) = raw else {
        return Err(vec![MSG_NULL.to_owned()]);
    };
    let value = python_strip(raw).to_owned();
    if value.is_empty() {
        return Err(vec![MSG_BLANK.to_owned()]);
    }
    // Validator order mirrors the `self.validators` list DRF builds:
    // declared `[validate_schema, validate_domain]`, then the
    // `MaxLengthValidator` mapped from the model, then
    // `ProhibitNullCharactersValidator`, then `URLValidator`
    // (probed on live DRF: an over-long `ftp:` URL yields schema then
    // max-length; a NUL `https:` URL yields exactly the null message —
    // `URLValidator` itself passes NUL, so it contributes nothing there).
    let mut errors = Vec::new();
    if let Err(message) = validate_schema(&value) {
        errors.push(message.to_owned());
    }
    if let Err(message) = validate_domain(&value) {
        errors.push(message.to_owned());
    }
    if value.chars().count() > MAX_URL_CHARS {
        errors.push(MSG_MAX_LENGTH.to_owned());
    }
    // Lone surrogates (`ProhibitSurrogateCharactersValidator`, next in DRF's
    // list) cannot occur in a `&str`: no runtime check, comment only.
    if value.contains('\0') {
        errors.push(MSG_NULL_CHARS.to_owned());
    }
    if !url_validator_ok(&value) {
        errors.push(MSG_INVALID_URL.to_owned());
    }
    if errors.is_empty() {
        Ok(value)
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// SSRF flags (`ipaddress`, CPython 3.12 — the repo's runtime)
// ---------------------------------------------------------------------------

/// IPv4 networks in `IPv4Address._constants._private_networks`
/// (iana-ipv4-special-registry).
const V4_PRIVATE: [([u8; 4], u8); 14] = [
    ([0, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([127, 0, 0, 0], 8),
    ([169, 254, 0, 0], 16),
    ([172, 16, 0, 0], 12),
    ([192, 0, 0, 0], 24),
    ([192, 0, 0, 170], 31),
    ([192, 0, 2, 0], 24),
    ([192, 168, 0, 0], 16),
    ([198, 18, 0, 0], 15),
    ([198, 51, 100, 0], 24),
    ([203, 0, 113, 0], 24),
    ([240, 0, 0, 0], 4),
    ([255, 255, 255, 255], 32),
];

/// `IPv4Address._constants._private_networks_exceptions`.
const V4_PRIVATE_EXCEPTIONS: [([u8; 4], u8); 2] = [([192, 0, 0, 9], 32), ([192, 0, 0, 10], 32)];

/// `IPv4Address._constants._reserved_network` (240.0.0.0/4).
const V4_RESERVED: ([u8; 4], u8) = ([240, 0, 0, 0], 4);
/// `IPv4Address._constants._loopback_network` (127.0.0.0/8, RFC 3330).
const V4_LOOPBACK: ([u8; 4], u8) = ([127, 0, 0, 0], 8);
/// `IPv4Address._constants._linklocal_network` (169.254.0.0/16, RFC 3927).
const V4_LINK_LOCAL: ([u8; 4], u8) = ([169, 254, 0, 0], 16);

/// `IPv6Address._constants._private_networks` (iana-ipv6-special-registry).
const V6_PRIVATE: [(Ipv6Addr, u8); 11] = [
    (Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1), 128),
    (Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0), 128),
    (Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0, 0), 96),
    (Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0), 48),
    (Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 0), 64),
    (Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23),
    (Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16),
    (Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20),
    (Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7),
    (Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10),
];

/// `IPv6Address._constants._private_networks_exceptions`.
const V6_PRIVATE_EXCEPTIONS: [(Ipv6Addr, u8); 6] = [
    (Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 1), 128),
    (Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 2), 128),
    (Ipv6Addr::new(0x2001, 3, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2001, 4, 0x112, 0, 0, 0, 0, 0), 48),
    (Ipv6Addr::new(0x2001, 0x20, 0, 0, 0, 0, 0, 0), 28),
    (Ipv6Addr::new(0x2001, 0x30, 0, 0, 0, 0, 0, 0), 28),
];

/// `IPv6Address._constants._reserved_networks`.
const V6_RESERVED: [(Ipv6Addr, u8); 15] = [
    (Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0), 8),
    (Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 0), 8),
    (Ipv6Addr::new(0x200, 0, 0, 0, 0, 0, 0, 0), 7),
    (Ipv6Addr::new(0x400, 0, 0, 0, 0, 0, 0, 0), 6),
    (Ipv6Addr::new(0x800, 0, 0, 0, 0, 0, 0, 0), 5),
    (Ipv6Addr::new(0x1000, 0, 0, 0, 0, 0, 0, 0), 4),
    (Ipv6Addr::new(0x4000, 0, 0, 0, 0, 0, 0, 0), 3),
    (Ipv6Addr::new(0x6000, 0, 0, 0, 0, 0, 0, 0), 3),
    (Ipv6Addr::new(0x8000, 0, 0, 0, 0, 0, 0, 0), 3),
    (Ipv6Addr::new(0xa000, 0, 0, 0, 0, 0, 0, 0), 3),
    (Ipv6Addr::new(0xc000, 0, 0, 0, 0, 0, 0, 0), 3),
    (Ipv6Addr::new(0xe000, 0, 0, 0, 0, 0, 0, 0), 4),
    (Ipv6Addr::new(0xf000, 0, 0, 0, 0, 0, 0, 0), 5),
    (Ipv6Addr::new(0xf800, 0, 0, 0, 0, 0, 0, 0), 6),
    (Ipv6Addr::new(0xfe00, 0, 0, 0, 0, 0, 0, 0), 9),
];

/// `IPv6Address._constants._linklocal_network` (fe80::/10, RFC 4291).
const V6_LINK_LOCAL: (Ipv6Addr, u8) = (Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10);

fn in_v4_net(ip: &Ipv4Addr, base: [u8; 4], prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let (addr, net) = (u32::from(*ip), u32::from(Ipv4Addr::from(base)));
    let shift = 32 - prefix;
    addr >> shift == net >> shift
}

fn in_v6_net(ip: &Ipv6Addr, base: &Ipv6Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let shift = 128 - prefix;
    u128::from(*ip) >> shift == u128::from(*base) >> shift
}

/// `ip.is_private`, mirroring `ipaddress` on CPython 3.12: registry
/// membership minus the exceptions list, with IPv4-mapped IPv6 addresses
/// delegating to the embedded IPv4 address (`address.is_private ==
/// address.ipv4_mapped.is_private`).
pub fn ip_is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            V4_PRIVATE
                .iter()
                .any(|(base, prefix)| in_v4_net(v4, *base, *prefix))
                && !V4_PRIVATE_EXCEPTIONS
                    .iter()
                    .any(|(base, prefix)| in_v4_net(v4, *base, *prefix))
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return ip_is_private(&IpAddr::V4(mapped));
            }
            V6_PRIVATE
                .iter()
                .any(|(base, prefix)| in_v6_net(v6, base, *prefix))
                && !V6_PRIVATE_EXCEPTIONS
                    .iter()
                    .any(|(base, prefix)| in_v6_net(v6, base, *prefix))
        }
    }
}

/// `ip.is_loopback` (`ipaddress.py`, CPython 3.12): 127.0.0.0/8 for IPv4;
/// exactly `::1` for IPv6. Only `is_private` delegates mapped addresses to
/// the embedded IPv4; loopback does not (`::ffff:127.0.0.1` is not
/// loopback on 3.12 — probed live).
pub fn ip_is_loopback(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => in_v4_net(v4, V4_LOOPBACK.0, V4_LOOPBACK.1),
        IpAddr::V6(v6) => *v6 == Ipv6Addr::LOCALHOST,
    }
}

/// `ip.is_link_local` (`ipaddress.py`, CPython 3.12): 169.254.0.0/16 for
/// IPv4; fe80::/10 for IPv6. No mapped delegation (`::ffff:169.254.1.1`
/// is not link-local on 3.12 — probed live).
pub fn ip_is_link_local(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => in_v4_net(v4, V4_LINK_LOCAL.0, V4_LINK_LOCAL.1),
        IpAddr::V6(v6) => in_v6_net(v6, &V6_LINK_LOCAL.0, V6_LINK_LOCAL.1),
    }
}

/// `ip.is_reserved` (`ipaddress.py`, CPython 3.12): 240.0.0.0/4 for IPv4;
/// the IETF-reserved ranges for IPv6, with no mapped delegation — every
/// `::ffff:0:0/96` address is reserved via `::/8`
/// (`::ffff:8.8.8.8` is reserved, hence blocked, on 3.12 — probed live).
pub fn ip_is_reserved(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => in_v4_net(v4, V4_RESERVED.0, V4_RESERVED.1),
        IpAddr::V6(v6) => V6_RESERVED
            .iter()
            .any(|(base, prefix)| in_v6_net(v6, base, *prefix)),
    }
}

/// The SSRF guard expression itself
/// (`serializers/webhook.py:41,76`): `ip.is_private or ip.is_loopback or
/// ip.is_reserved or ip.is_link_local`.
pub fn ip_is_blocked(ip: &IpAddr) -> bool {
    ip_is_private(ip) || ip_is_loopback(ip) || ip_is_reserved(ip) || ip_is_link_local(ip)
}

// ---------------------------------------------------------------------------
// DNS resolution
// ---------------------------------------------------------------------------

/// The two `getaddrinfo` outcomes the guards distinguish: lookup failure
/// (`socket.gaierror`) versus an empty answer list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsError {
    Unresolvable,
    Empty,
}

/// Hostname resolution behind the guards. `socket.getaddrinfo(hostname,
/// None)` is blocking in Python too; the system implementation below is
/// likewise blocking — handlers must call it off the async executor
/// (e.g. `spawn_blocking`). Tests inject canned answers through this
/// trait, including the DNS-mocked unresolvable host the issue requires.
pub trait ResolveHost {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, DnsError>;
}

/// Live DNS via the platform resolver (`getaddrinfo` semantics: numeric
/// names resolve without a query, unknown names fail).
pub struct SystemResolver;

impl ResolveHost for SystemResolver {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, DnsError> {
        match (host, 0).to_socket_addrs() {
            Err(_) => Err(DnsError::Unresolvable),
            Ok(addrs) => {
                let ips: Vec<IpAddr> = addrs.map(|addr| addr.ip()).collect();
                if ips.is_empty() {
                    Err(DnsError::Empty)
                } else {
                    Ok(ips)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Disallowed domains + guards (`create` / `update`)
// ---------------------------------------------------------------------------

/// `any(hostname == domain or hostname.endswith("." + domain) for domain
/// in disallowed_domains)` (`serializers/webhook.py:52,87`), probed in
/// FX-WEB-05: `"notairepublic.com"` and `"airepublic.com.evil.com"` pass,
/// `"sub.airepublic.com"` does not.
pub fn hostname_is_disallowed(hostname: &str, disallowed: &[&str]) -> bool {
    disallowed
        .iter()
        .any(|domain| hostname == *domain || hostname.ends_with(&format!(".{domain}")))
}

/// Shared guard chain behind `create()` (`serializers/webhook.py:26-53`)
/// and `update()` (`:61-88`) once field validation has passed: hostname
/// extraction, DNS resolution with per-IP SSRF screening, then the
/// disallowed-domain check over `["airepublic.com"]` plus the request host
/// (when the caller supplies one).
///
/// `request_host` is the already-resolved `request.get_host().split(":")[0]`
/// value — i.e. `self.context.get("request")` in Python. On create the POST
/// view passes the real request (`context={"request": request}`); on
/// update the PATCH view's `context={request: request}` never resolves, so
/// the handler port passes `None` (BUG B2, ported as-is).
fn run_guards(
    value: &str,
    request_host: Option<&str>,
    resolver: &dyn ResolveHost,
) -> Result<(), WebhookUrlError> {
    let (_, netloc) = split_scheme_netloc(value);
    let Some(hostname) = hostname_of(netloc) else {
        return Err(WebhookUrlError::Guard(MSG_NO_HOSTNAME.to_owned()));
    };
    let ips = match resolver.resolve(&hostname) {
        Err(DnsError::Unresolvable) => {
            return Err(WebhookUrlError::Guard(MSG_UNRESOLVABLE.to_owned()));
        }
        Err(DnsError::Empty) => {
            return Err(WebhookUrlError::Guard(MSG_NO_IPS.to_owned()));
        }
        Ok(ips) => ips,
    };
    // Every answer is screened (`for addr in ip_addresses`), not just the
    // first: one blocked address fails the URL.
    if ips.iter().any(ip_is_blocked) {
        return Err(WebhookUrlError::Guard(MSG_BLOCKED_IP.to_owned()));
    }
    let mut disallowed: Vec<&str> = DEFAULT_DISALLOWED_DOMAINS.to_vec();
    if let Some(host) = request_host {
        disallowed.push(host);
    }
    if hostname_is_disallowed(&hostname, &disallowed) {
        return Err(WebhookUrlError::Guard(MSG_DISALLOWED_DOMAIN.to_owned()));
    }
    Ok(())
}

/// `WebhookSerializer.create()` validation (`serializers/webhook.py:22-55`):
/// field checks, then the guard chain with the request host appended.
/// Returns the trimmed URL the row is created with.
pub fn validate_create_url(
    raw: Option<Option<&str>>,
    request_host: Option<&str>,
    resolver: &dyn ResolveHost,
) -> Result<String, WebhookUrlError> {
    let value = validate_url_field(raw).map_err(WebhookUrlError::Field)?;
    run_guards(&value, request_host, resolver)?;
    Ok(value)
}

/// `WebhookSerializer.update()` validation (`serializers/webhook.py:57-90`):
/// the guard chain runs only when `url` is present in the PATCH body
/// (`if url:`); otherwise the update proceeds untouched (`super().update`).
/// An explicit JSON null still fails (`allow_null=False`), exactly as
/// `is_valid()` rejects it before `update()` is ever reached.
/// `request_host` is almost always `None` here — see BUG B2 above.
pub fn validate_update_url(
    raw: Option<Option<&str>>,
    request_host: Option<&str>,
    resolver: &dyn ResolveHost,
) -> Result<Option<String>, WebhookUrlError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value = validate_url_field(Some(raw)).map_err(WebhookUrlError::Field)?;
    run_guards(&value, request_host, resolver)?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::str::FromStr;

    // -- harness -----------------------------------------------------------

    /// Canned DNS: exact hostnames map to answers; anything unlisted is
    /// unresolvable (like a name with no record).
    #[derive(Default)]
    struct MockResolver {
        table: HashMap<String, Result<Vec<IpAddr>, DnsError>>,
    }

    impl MockResolver {
        fn answer(mut self, host: &str, ips: &[&str]) -> Self {
            self.table.insert(
                host.to_owned(),
                Ok(ips.iter().map(|s| IpAddr::from_str(s).unwrap()).collect()),
            );
            self
        }

        fn failing(mut self, host: &str, error: DnsError) -> Self {
            self.table.insert(host.to_owned(), Err(error));
            self
        }
    }

    impl ResolveHost for MockResolver {
        fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, DnsError> {
            self.table
                .get(host)
                .cloned()
                .unwrap_or(Err(DnsError::Unresolvable))
        }
    }

    /// Public test host resolving to a public IP (fixture label: 93.184.216.34).
    fn public_dns() -> MockResolver {
        MockResolver::default()
            .answer("example.com", &["93.184.216.34"])
            .answer("hooks.example.com", &["93.184.216.34"])
            .answer("sub.airepublic.com", &["93.184.216.34"])
            .answer("airepublic.com", &["93.184.216.34"])
            .answer("notairepublic.com", &["93.184.216.34"])
            .answer("airepublic.com.evil.com", &["93.184.216.34"])
            .answer("münchen.de", &["93.184.216.34"])
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/app_integrations/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn field_body(messages: &[&str]) -> serde_json::Value {
        serde_json::json!({"url": messages})
    }

    fn guard_body(message: &str) -> serde_json::Value {
        serde_json::json!({"url": message})
    }

    // -- FX-WEB-04: golden valid --------------------------------------------

    #[test]
    fn fx04_fixture_names_this_module() {
        let gold = fixture("fx-web-04-webhook-serializer.json");
        assert!(gold.get("golden_valid").is_some());
        assert!(gold.get("golden_invalid").is_some());
        assert!(gold.get("create_flow").is_some());
        assert_eq!(gold["meta"]["read_only_fields"][1], "secret_key");
    }

    #[test]
    fn fx04_valid_url_passes_create() {
        let gold = fixture("fx-web-04-webhook-serializer.json");
        let url = gold["golden_valid"]["request"]["url"].as_str().unwrap();
        let body =
            validate_create_url(Some(Some(url)), None, &public_dns()).expect("valid golden passes");
        assert_eq!(body, url);
    }

    #[test]
    fn fx04_secret_and_workspace_are_read_only() {
        // `Meta.read_only_fields` (`serializers/webhook.py:95`): client values
        // for these are ignored; the view forces `workspace`, the model
        // defaults `secret_key` via `generate_token`.
        assert_eq!(
            WEBHOOK_READ_ONLY_FIELDS,
            &["workspace", "secret_key", "deleted_at"]
        );
        assert_eq!(WEBHOOK_MODEL, "Webhook");
        assert_eq!(WEBHOOK_LOG_MODEL, "WebhookLog");
        assert_eq!(WEBHOOK_LOG_READ_ONLY_FIELDS, &["workspace", "webhook"]);
    }

    // -- FX-WEB-04: every ValidationError string -----------------------------

    #[test]
    fn fx04_each_field_error_string() {
        // `None` is a missing required field (DRF default `required=True`).
        assert_eq!(
            validate_create_url(None, None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["This field is required."])
        );
        // Explicit null is a different failure (`allow_null=False`).
        assert_eq!(
            validate_create_url(Some(None), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["This field may not be null."])
        );
        assert_eq!(
            validate_update_url(Some(None), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["This field may not be null."])
        );
        assert_eq!(
            validate_create_url(Some(Some("")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["This field may not be blank."])
        );
        assert_eq!(
            validate_create_url(Some(Some("   ")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["This field may not be blank."])
        );
        // Contract-pinned order: schema, then URL (`test_create_rejects_non_http_scheme`).
        assert_eq!(
            validate_create_url(Some(Some("not-a-url")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&[
                "Invalid schema. Only HTTP and HTTPS are allowed.",
                "Enter a valid URL."
            ])
        );
        assert_eq!(
            validate_create_url(Some(Some("ftp://example.com/hook")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["Invalid schema. Only HTTP and HTTPS are allowed."])
        );
        assert_eq!(
            validate_create_url(Some(Some("http://127.0.0.1/hook")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["Local URLs are not allowed."])
        );
        // `https:///no-host` fails at the field gate with the URL message
        // (probed against real DRF); the "No hostname found." string below
        // covers the transcribed guard branch.
        assert_eq!(
            validate_create_url(Some(Some("https:///no-host")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["Enter a valid URL."])
        );
        let long = format!("https://example.com/{}", "a".repeat(2000));
        assert_eq!(
            validate_create_url(Some(Some(&long)), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&["Ensure this field has no more than 1024 characters."])
        );
        // Over-long *and* mis-schemed: both messages, schema first (probed
        // against real DRF with `max_length=1024` + the declared validators).
        let long_ftp = format!("ftp://example.com/{}", "a".repeat(2000));
        assert_eq!(
            validate_create_url(Some(Some(&long_ftp)), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&[
                "Invalid schema. Only HTTP and HTTPS are allowed.",
                "Ensure this field has no more than 1024 characters."
            ])
        );
        // NUL content yields exactly the null message: `URLValidator`
        // itself passes NUL (probed on live DRF 3.16.1 / Django 4.2.30).
        assert_eq!(
            validate_create_url(
                Some(Some("https://example.com/\0hook")),
                None,
                &public_dns()
            )
            .unwrap_err()
            .body(),
            field_body(&["Null characters are not allowed."])
        );
        // Combined order: schema, then null — no URL message (probed).
        assert_eq!(
            validate_create_url(Some(Some("ftp://example.com/\0hook")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&[
                "Invalid schema. Only HTTP and HTTPS are allowed.",
                "Null characters are not allowed."
            ])
        );
        // NUL without a scheme collects all three, in chain order (probed).
        assert_eq!(
            validate_create_url(Some(Some("not-a-url\0")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&[
                "Invalid schema. Only HTTP and HTTPS are allowed.",
                "Null characters are not allowed.",
                "Enter a valid URL."
            ])
        );
        // Over-long NUL URL: schema, max-length, null, in that order (probed).
        let long_nul = format!("ftp://example.com/{}\0", "a".repeat(2000));
        assert_eq!(
            validate_create_url(Some(Some(&long_nul)), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&[
                "Invalid schema. Only HTTP and HTTPS are allowed.",
                "Ensure this field has no more than 1024 characters.",
                "Null characters are not allowed."
            ])
        );
        // Every field failure answers 400 like Django.
        assert_eq!(
            validate_create_url(Some(Some("not-a-url")), None, &public_dns())
                .unwrap_err()
                .status_code(),
            400
        );
    }

    #[test]
    fn fx04_each_guard_error_string() {
        // Hostname branch (transcribed in the fixture: unreachable past the
        // field gate for this exact input, exercised at the guard layer).
        let dns = MockResolver::default();
        assert_eq!(
            validate_create_url_despite_field("https:///no-host", &dns)
                .unwrap_err()
                .body(),
            guard_body("Invalid URL: No hostname found.")
        );
        // DNS-mocked unresolvable host (the issue's explicit Done-when case).
        let dns = MockResolver::default().failing("gone.invalid", DnsError::Unresolvable);
        assert_eq!(
            validate_create_url(Some(Some("https://gone.invalid/hook")), None, &dns)
                .unwrap_err()
                .body(),
            guard_body("Hostname could not be resolved.")
        );
        let dns = MockResolver::default().failing("empty.invalid", DnsError::Empty);
        assert_eq!(
            validate_create_url(Some(Some("https://empty.invalid/hook")), None, &dns)
                .unwrap_err()
                .body(),
            guard_body("No IP addresses found for the hostname.")
        );
        // One blocked answer among public ones still fails (`for addr in …`).
        let dns = MockResolver::default().answer("mix.example.com", &["93.184.216.34", "10.0.0.5"]);
        assert_eq!(
            validate_create_url(Some(Some("https://mix.example.com/hook")), None, &dns)
                .unwrap_err()
                .body(),
            guard_body("URL resolves to a blocked IP address.")
        );
        assert_eq!(
            validate_create_url(
                Some(Some("https://sub.airepublic.com/hook")),
                None,
                &public_dns()
            )
            .unwrap_err()
            .body(),
            guard_body("URL domain or its subdomain is not allowed.")
        );
    }

    /// Guard chain without the field gate, for the transcribed hostname
    /// branch only.
    fn validate_create_url_despite_field(
        raw: &str,
        resolver: &dyn ResolveHost,
    ) -> Result<String, WebhookUrlError> {
        run_guards(python_strip(raw), None, resolver)?;
        Ok(python_strip(raw).to_owned())
    }

    // -- URLValidator differential table (all verdicts probed on Django 4.2)
    // ------------------------------------------------------------------------

    /// Every verdict below was read off Django 4.2's `URLValidator`
    /// (repo pin) before porting; the Rust port must agree byte for byte.
    #[test]
    fn url_validator_matches_django_on_corpus() {
        let ok = [
            "https://example.com/hook",
            "http://example.com/hook",
            "ftp://example.com/hook",
            "ftps://example.com/hook",
            "HTTP://EXAMPLE.COM/hook",
            "http://localhost/",
            "http://localhost:8000/hook",
            "http://user:pass@example.com/hook",
            "http://user@example.com/hook",
            "https://example.com:8443/hook?a=b#c",
            "http://[::1]/hook",
            "http://[::1]:8080/x",
            "http://127.0.0.1/hook",
            "http://127.0.0.1:8000/hook",
            "https://example.com./x",
            "https://xn--mnchen-3ya.de/hook",
            "https://münchen.de/hook",
            "https://münchen.de./hook",
            "https://münchen.de:8080/hook",
            "http://user@münchen.de/hook",
            "https://MÜNCHEN.DE/hook",
            "https://müch@münchen.de/",
            "https://-münchen.de/hook",
            "https://münchen-.de/hook",
            "https://ß.de/hook",
            "https://a.b-cd.ef/",
            "https://example.com/",
            "https://sub.airepublic.com/hook",
            "http://10.0.0.5/hook",
            "http://[2001:db8::1]/hook",
            "http://[2001:db8::1]:8080/hook",
            "http://[::ffff:1.2.3.4]/",
            "http://[::]/",
        ];
        for url in ok {
            assert!(url_validator_ok(url), "Django accepts {url:?}");
        }
        let bad = [
            "not-a-url",
            "",
            "https:///no-host",
            "http://?x",
            "http://#f",
            "http://:8080/p",
            "http://a/",
            "https://x",
            "http://foo_bar/",
            "http://[::1",
            "http://[gggg::1]/",
            "http://[fe80::1%25eth0]/",
            "http://[1.2.3.4]/",
            "http://[2001:db8:::1]/",
            "http://256.1.1.1/",
            "http://01.2.3.4/",
            "http://1.2.3/",
            "http://1.2.3.4.5/",
            "https://-bad.com/",
            "https://bad-.com/",
            "https://example.123/",
            "https://example.c/",
            "https://example.com/white space",
            "https://example.com/tab\there",
            "https://example.com/nl\nhere",
            "https://example.com/cr\rhere",
            "gopher://example.com/",
            "mailto:foo@bar.com",
            "http:/single-slash",
            "//example.com/noscheme",
            "https://example.com:999999/",
            "https://example.com:abc/",
            "https://example.com:/",
            "https://a..münchen.de/hook",
            "https://münchen_de.de/hook",
            "https://münchen.de-.com/hook",
        ];
        for url in bad {
            assert!(!url_validator_ok(url), "Django rejects {url:?}");
        }
        // 64-char label, over-long TLD-absent names and boundary hostnames.
        assert!(!url_validator_ok(&format!(
            "https://{}.com/",
            "a".repeat(64)
        )));
        assert!(!url_validator_ok(&format!(
            "https://{}.de/",
            "ü".repeat(64)
        )));
        let host253 = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(host253.len(), 253);
        assert!(url_validator_ok(&format!("https://{host253}/")));
        assert!(!url_validator_ok(&format!("https://{host253}e/")));
        // Over the 2048-char validator cap.
        assert!(!url_validator_ok(&format!(
            "https://example.com/{}",
            "a".repeat(2040)
        )));
    }

    // -- FX-WEB-05: SSRF matrix ----------------------------------------------

    #[test]
    fn fx05_fixture_ssrf_matrix() {
        let gold = fixture("fx-web-05-ssrf-guard.json");
        let matrix = gold["ssrf_matrix"].as_array().unwrap();
        assert!(matrix.len() >= 9, "fixture carries the full matrix");
        for row in matrix {
            let ip: IpAddr = row["ip"].as_str().unwrap().parse().unwrap();
            let expect = row["blocked"].as_bool().unwrap();
            assert_eq!(ip_is_blocked(&ip), expect, "SSRF verdict for {}", row["ip"]);
            assert_eq!(
                ip_is_private(&ip),
                row["flags"]["is_private"].as_bool().unwrap()
            );
            assert_eq!(
                ip_is_loopback(&ip),
                row["flags"]["is_loopback"].as_bool().unwrap()
            );
            assert_eq!(
                ip_is_reserved(&ip),
                row["flags"]["is_reserved"].as_bool().unwrap()
            );
            assert_eq!(
                ip_is_link_local(&ip),
                row["flags"]["is_link_local"].as_bool().unwrap()
            );
        }
    }

    /// Per-flag parity with CPython 3.12's `ipaddress` past the fixture's
    /// nine rows: `(private, loopback, reserved, link_local, blocked)`.
    #[test]
    fn ssrf_flags_match_cpython312() {
        let cases: &[(&str, bool, bool, bool, bool, bool)] = &[
            ("10.0.0.5", true, false, false, false, true),
            ("172.16.0.9", true, false, false, false, true),
            ("192.168.1.1", true, false, false, false, true),
            ("127.0.0.1", true, true, false, false, true),
            ("::1", true, true, true, false, true),
            ("169.254.169.254", true, false, false, true, true),
            ("240.0.0.1", true, false, true, false, true),
            ("255.255.255.255", true, false, true, false, true),
            ("0.0.0.0", true, false, false, false, true),
            ("8.8.8.8", false, false, false, false, false),
            ("93.184.216.34", false, false, false, false, false),
            ("100.64.0.1", false, false, false, false, false),
            ("192.0.2.1", true, false, false, false, true),
            ("198.51.100.1", true, false, false, false, true),
            ("203.0.113.1", true, false, false, false, true),
            ("198.18.0.1", true, false, false, false, true),
            ("192.0.0.170", true, false, false, false, true),
            ("192.0.0.9", false, false, false, false, false),
            ("192.0.0.10", false, false, false, false, false),
            ("192.31.196.0", false, false, false, false, false),
            ("::", true, false, true, false, true),
            // Mapped addresses delegate to the embedded IPv4 for
            // `is_private` only (3.12 docstring guarantee); loopback,
            // reserved and link-local use the v6 tables — every mapped
            // address is reserved via `::/8`, none is loopback or
            // link-local (all probed live on CPython 3.12).
            ("::ffff:127.0.0.1", true, false, true, false, true),
            ("::ffff:0:0", true, false, true, false, true),
            ("::ffff:8.8.8.8", false, false, true, false, true),
            ("::ffff:100.64.0.1", false, false, true, false, true),
            ("::ffff:192.0.0.9", false, false, true, false, true),
            ("::ffff:224.0.0.1", false, false, true, false, true),
            ("::ffff:169.254.1.1", true, false, true, false, true),
            ("fe80::1", true, false, false, true, true),
            ("fc00::1", true, false, false, false, true),
            ("2001:db8::1", true, false, false, false, true),
            ("ff02::1", false, false, false, false, false),
            ("64:ff9b::808:808", false, false, true, false, true),
            ("64:ff9b:1::1", true, false, true, false, true),
            ("100::1", true, false, true, false, true),
            ("2001::1", true, false, false, false, true),
            ("2001:1::1", false, false, false, false, false),
        ];
        for (text, private, loopback, reserved, link_local, blocked) in cases {
            let ip: IpAddr = text.parse().unwrap();
            assert_eq!(ip_is_private(&ip), *private, "{text} private");
            assert_eq!(ip_is_loopback(&ip), *loopback, "{text} loopback");
            assert_eq!(ip_is_reserved(&ip), *reserved, "{text} reserved");
            assert_eq!(ip_is_link_local(&ip), *link_local, "{text} link-local");
            assert_eq!(ip_is_blocked(&ip), *blocked, "{text} blocked");
        }
        // 224.0.0.0/4 multicast is globally reachable in 3.12: all four
        // flags false, guard passes.
        let multicast: IpAddr = "224.0.0.1".parse().unwrap();
        assert!(!ip_is_private(&multicast));
        assert!(!ip_is_loopback(&multicast));
        assert!(!ip_is_reserved(&multicast));
        assert!(!ip_is_link_local(&multicast));
        assert!(!ip_is_blocked(&multicast));
    }

    // -- FX-WEB-05: schema / domain (incl BUG B1) ------------------------------

    #[test]
    fn fx05_validate_schema_cases() {
        let gold = fixture("fx-web-05-ssrf-guard.json");
        for row in gold["validate_schema"]["cases"].as_array().unwrap() {
            let input = row["input"].as_str().unwrap();
            let pass = row["result"].as_str().unwrap() == "pass";
            assert_eq!(validate_schema(input).is_ok(), pass, "schema {input:?}");
        }
        assert_eq!(
            validate_schema("not-a-url").unwrap_err(),
            "Invalid schema. Only HTTP and HTTPS are allowed."
        );
        // The scheme check lowercases like `urlparse` (`HTTP://…` passes).
        assert!(validate_schema("HTTP://example.com/hook").is_ok());
    }

    #[test]
    fn fx05_validate_domain_cases_with_bug_b1() {
        let gold = fixture("fx-web-05-ssrf-guard.json");
        assert!(gold["validate_domain"]["bug_b1"]
            .as_str()
            .unwrap()
            .starts_with("BUG:"));
        assert_eq!(validate_domain("https://example.com/hook"), Ok(()));
        assert_eq!(
            validate_domain("http://localhost/hook").unwrap_err(),
            "Local URLs are not allowed."
        );
        assert_eq!(
            validate_domain("http://127.0.0.1/hook").unwrap_err(),
            "Local URLs are not allowed."
        );
        // BUG B1: the netloc carries the port, so these pass.
        assert_eq!(validate_domain("http://localhost:8000/hook"), Ok(()));
        assert_eq!(validate_domain("http://127.0.0.1:8000/hook"), Ok(()));
        // Exact match only: subdomains of localhost-adjacent names pass.
        assert_eq!(validate_domain("http://localhost.evil.com/hook"), Ok(()));
    }

    // -- FX-WEB-05: disallowed domains ------------------------------------------

    #[test]
    fn fx05_disallowed_domain_rule() {
        let gold = fixture("fx-web-05-ssrf-guard.json");
        assert_eq!(
            gold["disallowed_domains"]["default_list"][0],
            "airepublic.com"
        );
        let only_default = DEFAULT_DISALLOWED_DOMAINS;
        assert!(hostname_is_disallowed("airepublic.com", only_default));
        assert!(hostname_is_disallowed("sub.airepublic.com", only_default));
        assert!(hostname_is_disallowed("a.b.airepublic.com", only_default));
        assert!(!hostname_is_disallowed("notairepublic.com", only_default));
        assert!(!hostname_is_disallowed(
            "airepublic.com.evil.com",
            only_default
        ));
        assert!(!hostname_is_disallowed("example.com", only_default));
        // The request host appends per-request on create
        // (`serializers/webhook.py:47-49`).
        assert!(hostname_is_disallowed(
            "hooks.example.com",
            &["airepublic.com", "hooks.example.com"]
        ));
        assert!(!hostname_is_disallowed(
            "other.example.com",
            &["airepublic.com", "hooks.example.com"]
        ));
    }

    #[test]
    fn fx05_request_host_guards_create_but_bug_b2_skips_it_on_update() {
        let url = "https://hooks.example.com/hook";
        // Create appends the request host: blocked.
        assert_eq!(
            validate_create_url(Some(Some(url)), Some("hooks.example.com"), &public_dns())
                .unwrap_err()
                .body(),
            guard_body("URL domain or its subdomain is not allowed.")
        );
        // Update never sees the request host (BUG B2: the PATCH view builds
        // `context={request: request}`, so `.get("request")` is `None`):
        // the same URL passes.
        assert_eq!(
            validate_update_url(Some(Some(url)), None, &public_dns()).unwrap(),
            Some(url.to_owned())
        );
        // …while the serializer itself honors a host when one is present,
        // proving the bug lives in the view's context, not here.
        assert_eq!(
            validate_update_url(Some(Some(url)), Some("hooks.example.com"), &public_dns())
                .unwrap_err()
                .body(),
            guard_body("URL domain or its subdomain is not allowed.")
        );
    }

    // -- update flow ------------------------------------------------------------

    #[test]
    fn update_without_url_skips_guards() {
        // `if url:` is falsy → `super().update` untouched. Even a resolver
        // that fails everything is never consulted.
        let dns = MockResolver::default();
        assert_eq!(validate_update_url(None, None, &dns).unwrap(), None);
    }

    #[test]
    fn update_with_url_runs_full_chain() {
        assert_eq!(
            validate_update_url(Some(Some("not-a-url")), None, &public_dns())
                .unwrap_err()
                .body(),
            field_body(&[
                "Invalid schema. Only HTTP and HTTPS are allowed.",
                "Enter a valid URL."
            ])
        );
        let dns = MockResolver::default().answer("example.com", &["10.0.0.5"]);
        assert_eq!(
            validate_update_url(Some(Some("https://example.com/hook")), None, &dns)
                .unwrap_err()
                .body(),
            guard_body("URL resolves to a blocked IP address.")
        );
    }

    // -- urlparse-equivalent units ------------------------------------------------

    #[test]
    fn split_scheme_netloc_mirrors_urlparse() {
        assert_eq!(
            split_scheme_netloc("https://example.com:8443/a?b#c"),
            ("https".into(), "example.com:8443")
        );
        assert_eq!(split_scheme_netloc("not-a-url"), (String::new(), ""));
        assert_eq!(split_scheme_netloc("HTTP://X/"), ("http".into(), "X"));
        assert_eq!(split_scheme_netloc("http:foo"), ("http".into(), ""));
        assert_eq!(split_scheme_netloc("http:///x?q"), ("http".into(), ""));
        assert_eq!(split_scheme_netloc("1http://x/"), (String::new(), ""));
        assert_eq!(split_scheme_netloc("a+b-c.d://x/"), ("a+b-c.d".into(), "x"));
        assert_eq!(split_scheme_netloc("ftp://u@/"), ("ftp".into(), "u@"));
    }

    #[test]
    fn hostname_of_mirrors_urlparse_hostname() {
        assert_eq!(hostname_of("example.com:8443"), Some("example.com".into()));
        assert_eq!(
            hostname_of("user:pass@Example.COM:8080"),
            Some("example.com".into())
        );
        assert_eq!(hostname_of("a@b@c"), Some("c".into()));
        assert_eq!(hostname_of("[::1]:8080"), Some("::1".into()));
        assert_eq!(hostname_of("[::1]"), Some("::1".into()));
        assert_eq!(hostname_of("example.com."), Some("example.com.".into()));
        assert_eq!(hostname_of(""), None);
        assert_eq!(hostname_of(":8080"), None);
        assert_eq!(hostname_of("@"), None);
        assert_eq!(hostname_of("::1"), None);
        assert_eq!(hostname_of("[::1"), None);
    }

    // -- strip parity ---------------------------------------------------------------

    #[test]
    fn strip_matches_python_including_fs_chars() {
        assert_eq!(
            python_strip("  https://example.com/hook \n"),
            "https://example.com/hook"
        );
        // `\x1c`–`\x1f`: stripped by Python, kept by Rust `trim` (probed).
        assert_eq!(
            python_strip("\u{1c}https://example.com/hook\u{1c}"),
            "https://example.com/hook"
        );
        assert_eq!(
            validate_create_url(
                Some(Some("\u{1c}https://example.com/hook\u{1c}")),
                None,
                &public_dns()
            )
            .unwrap(),
            "https://example.com/hook"
        );
    }

    // -- system resolver smoke (offline-safe: numeric + invalid only) -------------

    #[test]
    fn system_resolver_handles_numeric_and_invalid_without_dns() {
        let dns = SystemResolver;
        assert_eq!(
            dns.resolve("127.0.0.1").unwrap(),
            vec![IpAddr::from_str("127.0.0.1").unwrap()]
        );
        assert_eq!(dns.resolve("not a host!!"), Err(DnsError::Unresolvable));
    }
}
