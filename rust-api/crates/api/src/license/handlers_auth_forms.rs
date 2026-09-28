//! Admin sign-up + sign-in form-POST redirect flows (D-01, PIDASHCONV-122).
//!
//! Python: `apps/api/pi_dash/license/api/views/admin.py:89-358`
//! (`InstanceAdminSignUpEndpoint`, `InstanceAdminSignInEndpoint`); routes
//! `POST api/instances/admins/sign-up/` and `POST admins/sign-in/`
//! (`license/urls.py`). Fixture:
//! `rust-api/fixtures/license/handlers/signup_signin.golden.json`.
//!
//! Both endpoints are plain Django `View`s reading `request.POST` (not
//! DRF), answering every outcome with `HttpResponseRedirect` (302): errors
//! redirect to `urljoin(base_host(admin), '?' + urlencode({error_code,
//! error_message, **payload}))`, success to `urljoin(base_host(admin),
//! 'general/')`. There is no JSON body anywhere on these paths.
//!
//! This module is the pure kernel of those flows: error codes, `quote_plus`
//! query encoding, the admin `base_host` port, Django's `EmailValidator`
//! (ASCII matchers plus an RFC 3492 punycode path for IDN), form-falsy
//! parsing, the branch evaluators, and the 302 `Location` builders. Row
//! fetching, row writes, zxcvbn scoring, salt generation, and session-row
//! issuance stay the wiring layer's job (route registration is a separate
//! wiring issue; until it lands these paths proxy to Django, which also
//! preserves the token-less POST contract — plain `View`s sit behind
//! `CsrfViewMiddleware`, so a POST without a token never reaches the view
//! and answers the 200 CSRF-failure page `test_auth_forms.py` pins).
//!
//! Calling convention (same as `validate_slug` taking its `slug__iexact`
//! existence bit): every database- or secret-derived input arrives as a
//! caller-supplied value — `SignupSnapshot` / `SigninSnapshot` for row
//! state, `password_score: u8` for `zxcvbn(password)["score"]`, explicit
//! `salt`/`iterations` for `make_password`, explicit session inputs for
//! `user_login`. Nothing here performs I/O, mints randomness, or reads
//! settings; F-05 primitives (`pidash_auth::password`, `session`,
//! `signing`) are used read-only at the wiring layer.
//!
//! Ported quirks (kept, listed in the PR):
//!
//! * `is_telemetry_enabled` is stored raw from the POST (missing field
//!   arrives as boolean `True`, present fields as strings) and
//!   `BooleanField.get_prep_value` coerces case-sensitively: only
//!   `True/False/1/0/'t'/'True'/'1'/'f'/'False'/'0'` convert — lowercase
//!   `'true'`/`'false'`, `''`, and anything else raise `ValidationError`
//!   on `instance.save()`, i.e. an uncaught 500 *after* the User, Profile,
//!   and InstanceAdmin rows have committed (no `transaction.atomic`
//!   anywhere in `admin.py`).
//! * `ADMIN_ALREADY_EXIST` fires when *any* `InstanceAdmin` row exists
//!   (`first()`, `admin.py:108`), regardless of instance.
//! * `ADMIN_USER_DEACTIVATED` carries no payload; every other sign-in error
//!   echoes `{"email": ...}`.
//! * A valid non-admin user's sign-in fails with
//!   `ADMIN_AUTHENTICATION_FAILED`, not 403.
//! * `ADMIN_BASE_URL` unset falls back to `WEB_URL or APP_BASE_URL`, and
//!   `ADMIN_BASE_PATH` defaults to `/god-mode/` with slash normalization
//!   (`host.py:16-39`).
//! * `last_login_uagent`/`last_login_ip` are `None` when the request
//!   carries no User-Agent header / IP, and both columns are NOT NULL, so
//!   the stamp `save()` raises `IntegrityError` (500) on such requests.
//! * `get_client_ip` takes the first `X-Forwarded-For` entry unstripped,
//!   else `REMOTE_ADDR` (which may itself be `None`).
//! * The email allowlist check is case-sensitive: `a@LOCALHOST` fails while
//!   `a@localhost` passes.
//! * All-numeric dotted domains pass when the TLD is 2+ chars
//!   (`email@123.123.123.123` is valid); a TLD may start with `-`
//!   (`example.-com` passes) but not end with one.
#![forbid(unsafe_code)]

// ---------------------------------------------------------------------------
// §1 Error codes (authentication/adapter/error.py:60-70)
// ---------------------------------------------------------------------------

/// No `Instance` row exists yet.
pub const INSTANCE_NOT_CONFIGURED: i64 = 5000;
/// `zxcvbn(password)["score"] < 3` on sign-up.
pub const PASSWORD_TOO_WEAK: i64 = 5021;
/// An `InstanceAdmin` row already exists (sign-up).
pub const ADMIN_ALREADY_EXIST: i64 = 5150;
/// Sign-up with email, password, or first name missing.
pub const REQUIRED_ADMIN_EMAIL_PASSWORD_FIRST_NAME: i64 = 5155;
/// `validate_email` rejected the stripped, lowered email.
pub const INVALID_ADMIN_EMAIL: i64 = 5160;
/// Sign-in with email or password missing.
pub const REQUIRED_ADMIN_EMAIL_PASSWORD: i64 = 5170;
/// Wrong password, or a valid non-admin user (sign-in).
pub const ADMIN_AUTHENTICATION_FAILED: i64 = 5175;
/// A `User` with the email already exists (sign-up).
pub const ADMIN_USER_ALREADY_EXIST: i64 = 5180;
/// No `User` with the email exists (sign-in).
pub const ADMIN_USER_DOES_NOT_EXIST: i64 = 5185;
/// The `User` row is inactive (sign-in; carries no payload).
pub const ADMIN_USER_DEACTIVATED: i64 = 5190;

/// `HttpResponseRedirect.status_code`.
pub const REDIRECT_STATUS: u16 = 302;
/// Success target appended to the admin base (`admin.py:238,357`).
pub const GENERAL_SUFFIX: &str = "general/";
/// `zxcvbn` acceptance threshold (`results["score"] < 3` rejects).
pub const PASSWORD_MIN_SCORE: u8 = 3;
/// `EmailValidator`: maximum email length in characters (RFC 3696 §3).
pub const MAX_EMAIL_CHARS: usize = 320;

// ---------------------------------------------------------------------------
// §2 Query encoding (urllib.parse.urlencode, exc.get_error_dict)
// ---------------------------------------------------------------------------

/// One query value. Rendering matches Python `str()`: integers as decimal,
/// booleans as `True`/`False` — the payload echo of a missing form field is
/// boolean `False` (`request.POST.get("email", False)`), and a missing
/// `is_telemetry_enabled` is boolean `True`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryValue {
    Int(i64),
    Bool(bool),
    Text(String),
}

/// Render a value exactly like `urlencode` stringifies it.
pub fn render_query_value(value: &QueryValue) -> String {
    match value {
        QueryValue::Int(n) => n.to_string(),
        QueryValue::Bool(true) => "True".to_owned(),
        QueryValue::Bool(false) => "False".to_owned(),
        QueryValue::Text(s) => s.clone(),
    }
}

/// True for bytes `urlencode` (via `quote_plus`, `safe=''`) never escapes:
/// ASCII letters and digits plus `_.-~`. Space is handled separately (it
/// becomes `+`); every other byte is percent-encoded from its UTF-8 form
/// with uppercase hex.
fn quote_plus_unreserved(byte: u8) -> bool {
    matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~')
}

/// Percent-encode exactly like `urllib.parse.quote_plus(s, safe='')`.
pub fn quote_plus(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for byte in value.bytes() {
        if byte == b' ' {
            out.push('+');
        } else if quote_plus_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    out
}

/// `urlencode(pairs)`: `name=quote_plus(value)` joined with `&`, in order.
/// Callers pass `error_code`, `error_message` first, then the payload keys
/// in dict-insertion order — that order is part of the redirect bytes.
pub fn encode_query(pairs: &[(&str, QueryValue)]) -> String {
    pairs
        .iter()
        .map(|(name, value)| {
            format!(
                "{}={}",
                quote_plus(name),
                quote_plus(&render_query_value(value))
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

// ---------------------------------------------------------------------------
// §3 Admin base + redirect targets (authentication/utils/host.py:16-39)
// ---------------------------------------------------------------------------

/// Normalize `ADMIN_BASE_PATH` exactly like `base_host`: non-strings fall
/// back to `/god-mode/` (the caller passes `None` for those), a missing
/// leading `/` is added, a missing trailing `/` is added.
pub fn normalize_admin_base_path(path: Option<&str>) -> String {
    let mut p = path.unwrap_or("/god-mode/").to_owned();
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    if !p.ends_with('/') {
        p.push('/');
    }
    p
}

/// `base_host(request, is_admin=True)`: `ADMIN_BASE_URL + path` when the
/// admin URL is configured, else `(WEB_URL or APP_BASE_URL) + path`.
/// `origin` is the already-resolved `WEB_URL or APP_BASE_URL` (both unset
/// is a deployment misconfiguration that raises `TypeError` in Python, so
/// the wiring must guarantee a non-empty origin).
pub fn admin_base_url(
    origin: &str,
    admin_base_url: Option<&str>,
    admin_base_path: Option<&str>,
) -> String {
    let path = normalize_admin_base_path(admin_base_path);
    match admin_base_url {
        Some(base) => format!("{base}{path}"),
        None => format!("{origin}{path}"),
    }
}

/// Error target: `urljoin(base, '?' + urlencode(...))`. The base always ends
/// with `/`, so `urljoin` is plain concatenation (`admin.py:101-104` etc.).
pub fn error_location(
    base: &str,
    code: i64,
    message: &str,
    payload: &[(&str, QueryValue)],
) -> String {
    let mut pairs: Vec<(&str, QueryValue)> = vec![
        ("error_code", QueryValue::Int(code)),
        ("error_message", QueryValue::Text(message.to_owned())),
    ];
    pairs.extend(payload.iter().cloned());
    format!("{}?{}", base, encode_query(&pairs))
}

/// Success target: `urljoin(base, "general/")` (`admin.py:238,357`).
pub fn success_location(base: &str) -> String {
    format!("{base}{GENERAL_SUFFIX}")
}

// ---------------------------------------------------------------------------
// §4 Email normalization + validation (admin.py:150,278; Django EmailValidator)
// ---------------------------------------------------------------------------

/// `email.strip().lower()` (`admin.py:150,278`). Python `str.strip`
/// removes Unicode whitespace and `lower` is simple (not full) case
/// folding; `trim` + `to_lowercase` is the same operation. Runs only after
/// the presence check, so missing/empty values never reach it.
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Django `validate_email` verdict (`django/core/validators.py`,
/// `EmailValidator`, verified against Django 4.2.30): length cap, `rsplit`
/// at the last `@`, ASCII dot-atom/quoted-string user match, then the
/// domain gate — `localhost` allowlist (case-sensitive), ASCII label/TLD
/// match, `[IP]` literal, else the IDN punycode round-trip.
pub fn email_is_valid(value: &str) -> bool {
    if value.is_empty() || !value.contains('@') || value.chars().count() > MAX_EMAIL_CHARS {
        return false;
    }
    let (user_part, domain_part) = match value.rsplit_once('@') {
        Some(pair) => pair,
        None => return false,
    };
    if !user_part_is_valid(user_part) {
        return false;
    }
    // Case-sensitive allowlist check (`domain_part not in [...]`).
    if domain_part == "localhost" {
        return true;
    }
    if domain_part_is_valid_ascii(domain_part) {
        return true;
    }
    // Possible IDN: `punycode(domain_part)` (`__call__` try/except
    // `UnicodeError`) then revalidate; anything non-ASCII that cannot be
    // encoded, or whose encoding fails the gate, is invalid.
    match idna_to_ascii(domain_part) {
        Some(ascii) => domain_part_is_valid_ascii(&ascii),
        None => false,
    }
}

/// One half of `EmailValidator.user_regex` (case-insensitive): dot-atom
/// `[-!#$%&'*+/=?^_`{}|~0-9A-Z]+(\.same)*`, or a quoted string. Both halves
/// are ASCII-only; any non-ASCII byte fails.
fn user_part_is_valid(user: &str) -> bool {
    if !user.is_ascii() {
        return false;
    }
    dot_atom_is_valid(user) || quoted_string_is_valid(user)
}

fn dot_atom_char(byte: u8) -> bool {
    matches!(
        byte,
        b'-' | b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+'
            | b'/' | b'=' | b'?' | b'^' | b'_' | b'`' | b'{' | b'}' | b'|'
            | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
    )
}

/// `^atom(\.atom)*\Z`: every dot-separated atom is non-empty and made of
/// dot-atom chars.
fn dot_atom_is_valid(user: &str) -> bool {
    if user.is_empty() {
        return false;
    }
    user.split('.')
        .all(|atom| !atom.is_empty() && atom.bytes().all(dot_atom_char))
}

/// The quoted-string half:
/// `"([\001-\010\013\014\016-\037!#-\[\]-\177]|\\[\001-\011\013\014\016-\177])*"`.
/// `!` is `\041`; the `#-\[` range stops at `\133` and `\]-\177` resumes at
/// `\135`, so `"` (`\042`) and `\` (`\134`) are only legal escaped. Space
/// (`\040`) is illegal everywhere — `"quoted string"@x` fails Django.
fn quoted_string_is_valid(user: &str) -> bool {
    let bytes = user.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return false;
    }
    let inner = &bytes[1..bytes.len() - 1];
    let mut i = 0;
    while i < inner.len() {
        let b = inner[i];
        if b == b'\\' {
            i += 1;
            if i >= inner.len() || !is_escaped_char(inner[i]) {
                return false;
            }
        } else if !is_quoted_char(b) {
            return false;
        }
        i += 1;
    }
    true
}

fn is_quoted_char(b: u8) -> bool {
    matches!(b, 0x01..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F)
        || b == b'!'
        || (0x23..=0x5B).contains(&b)
        || (0x5D..=0x7F).contains(&b)
}

fn is_escaped_char(b: u8) -> bool {
    matches!(b, 0x01..=0x09 | 0x0B | 0x0C | 0x0E..=0x7F)
}

/// `validate_domain_part`: the label/TLD match or the `[IP]` literal.
fn domain_part_is_valid_ascii(domain: &str) -> bool {
    domain_labels_match(domain) || ip_literal_is_valid(domain)
}

/// `domain_regex` (case-insensitive):
/// `((?:[A-Z0-9](?:[A-Z0-9-]{0,61}[A-Z0-9])?\.)+)(?:[A-Z0-9-]{2,63}(?<!-))\Z`.
/// At least one dotted label plus a 2–63 char TLD that may start with `-`
/// but not end with one (`example.-com` passes, `example.com-` fails).
fn domain_labels_match(domain: &str) -> bool {
    if !domain.is_ascii() {
        return false;
    }
    let parts: Vec<&str> = domain.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    for label in &parts[..parts.len() - 1] {
        if !domain_label_is_valid(label) {
            return false;
        }
    }
    let tld = parts[parts.len() - 1];
    if !(2..=63).contains(&tld.len()) || tld.ends_with('-') {
        return false;
    }
    tld.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// One dotted label: 1–63 chars, alnum ends, alnum/hyphen interior.
fn domain_label_is_valid(label: &str) -> bool {
    if !(1..=63).contains(&label.len()) {
        return false;
    }
    let bytes = label.as_bytes();
    let len = label.len();
    if !bytes[0].is_ascii_alphanumeric() || !bytes[len - 1].is_ascii_alphanumeric() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

/// `literal_regex` + `validate_ipv46_address`: `[` hex-digits/colons/dots
/// `]`, then a real IPv4 or IPv6 address. Django's IPv4 leg additionally
/// rejects leading zeros (`[001.002.003.004]` fails); `std` parsing agrees
/// on every probed vector, so it is used directly. (`[IPv6:...]`-style zone
/// prefixes fail the charset first, exactly like Django.)
fn ip_literal_is_valid(domain: &str) -> bool {
    let inner = match domain.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(inner) if !inner.is_empty() => inner,
        _ => return false,
    };
    if !inner
        .bytes()
        .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
    {
        return false;
    }
    if inner.contains(':') {
        inner.parse::<std::net::Ipv6Addr>().is_ok()
    } else {
        inner.parse::<std::net::Ipv4Addr>().is_ok()
    }
}

/// `django.utils.encoding.punycode` (`domain.encode("idna")`): per-label
/// ToASCII. Pure-ASCII labels pass through unchanged (case preserved —
/// `MÜNCHEN.DE` keeps `DE`); labels with non-ASCII are lowercased and
/// `xn--`-encoded. Empty labels, over-long encodings, and encode failures
/// are `None` (the `except UnicodeError` leg).
fn idna_to_ascii(domain: &str) -> Option<String> {
    if domain.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for label in domain.split('.') {
        if label.is_empty() {
            return None;
        }
        if label.is_ascii() {
            out.push(label.to_owned());
        } else {
            let lowered: String = label.chars().flat_map(|c| c.to_lowercase()).collect();
            if lowered.is_ascii() {
                // Case folding already produced ASCII (ToASCII passes it
                // through: `K\u{212A}` encodes to `kk`, verified).
                out.push(lowered);
            } else {
                let encoded = punycode_encode(&lowered)?;
                let ascii = format!("xn--{encoded}");
                // Python's encoder raises once the label no longer fits.
                if ascii.len() > 63 {
                    return None;
                }
                out.push(ascii);
            }
        }
    }
    Some(out.join("."))
}

/// RFC 3492 punycode encode, lowercase output. Returns `None` on overflow
/// (mirroring the codec raising `UnicodeError`). Verified against
/// `str.encode("punycode")`: `münchen→mnchen-3ya`, `☃→n3h`, `ü→tda`,
/// `bücher→bcher-kva`, `-münchen→-mnchen-o2a`, `münchen-→mnchen--n2a`.
///
/// Residual delta: Python runs full IDNA2003 nameprep first (NFKC +
/// case folding + mappings such as `ß→ss`, `ﬁ→fi`, plus prohibitions), so
/// exotic labels can encode to different ASCII and, in corner cases, reach
/// a different gate verdict. The probed battery agrees everywhere.
fn punycode_encode(label: &str) -> Option<String> {
    const BASE: u32 = 36;
    const TMIN: u32 = 1;
    const TMAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;
    const INITIAL_BIAS: u32 = 72;
    const INITIAL_N: u32 = 128;

    fn encode_digit(d: u32) -> Option<char> {
        if d < 26 {
            Some((b'a' + d as u8) as char)
        } else if d < 36 {
            Some((b'0' + (d - 26) as u8) as char)
        } else {
            None
        }
    }
    fn adapt(mut delta: u32, numpoints: u32, first: bool) -> Option<u32> {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta = delta.checked_add(delta / numpoints.max(1))?;
        let mut k = 0u32;
        while delta > ((BASE - TMIN) * TMAX) / 2 {
            delta /= BASE - TMIN;
            k = k.checked_add(BASE)?;
        }
        k.checked_add((BASE - TMIN + 1).checked_mul(delta)? / (delta.checked_add(SKEW)?))
    }

    let codepoints: Vec<u32> = label.chars().map(|c| c as u32).collect();
    let mut out = String::new();
    for c in label.chars() {
        if (c as u32) < 0x80 {
            out.push(c);
        }
    }
    let basic_len = codepoints.iter().filter(|c| **c < 0x80).count();
    if basic_len > 0 {
        out.push('-');
    }
    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let mut handled = basic_len;
    while handled < codepoints.len() {
        let m = *codepoints.iter().filter(|c| **c >= n).min()?;
        let increment = m
            .checked_sub(n)?
            .checked_mul((handled as u32).checked_add(1)?)?;
        delta = delta.checked_add(increment)?;
        n = m;
        for c in &codepoints {
            if *c < n {
                delta = delta.checked_add(1)?;
            } else if *c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    // `k - bias` goes negative on the first rounds; the
                    // clamp to `[TMIN, TMAX]` is what the spec means there.
                    let t = k.saturating_sub(bias).clamp(TMIN, TMAX);
                    if q < t {
                        break;
                    }
                    out.push(encode_digit(t + (q - t) % (BASE - t))?);
                    q = (q - t) / (BASE - t);
                    k = k.checked_add(BASE)?;
                }
                out.push(encode_digit(q)?);
                bias = adapt(
                    delta,
                    (handled as u32).checked_add(1)?,
                    handled == basic_len,
                )?;
                delta = 0;
                handled += 1;
            }
        }
        delta = delta.checked_add(1)?;
        n = n.checked_add(1)?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// §5 Form parsing (request.POST.get defaults; admin.py:120-125,261-262)
// ---------------------------------------------------------------------------

/// Python falsiness for a form field: missing (`False` default) and `""`
/// are both falsy; any non-empty string is truthy. `request.POST` values
/// are always strings when present — only absence yields the `False`
/// default — but both spell the same branch.
pub fn form_field_is_present(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|s| !s.is_empty())
}

/// Sign-up form fields with their `request.POST.get` defaults
/// (`admin.py:120-125`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupForm {
    /// `.get("email", False)` — echoed raw (or `False`) in error payloads.
    pub email: Option<String>,
    /// `.get("password", False)` — never echoed.
    pub password: Option<String>,
    /// `.get("first_name", False)`.
    pub first_name: Option<String>,
    /// `.get("last_name", "")`.
    pub last_name: String,
    /// `.get("company_name", "")`.
    pub company_name: String,
    /// `.get("is_telemetry_enabled", True)`: missing arrives as boolean
    /// `True`, present fields as strings. Stored raw (see
    /// [`coerce_telemetry`]).
    pub telemetry: TelemetryRaw,
}

/// The raw `is_telemetry_enabled` POST value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryRaw {
    Missing,
    Text(String),
}

/// What `instance.is_telemetry_enabled = <raw>` becomes on `save()`.
/// `BooleanField.get_prep_value` coerces case-sensitively
/// (`django/db/models/fields/__init__.py`, verified against Django
/// 4.2.30): only `True`/`False`/`1`/`0` and the exact strings
/// `'t'`/`'True'`/`'1'`/`'f'`/`'False'`/`'0'` convert. Anything else —
/// including lowercase `'true'`/`'false'` and `''` — raises
/// `ValidationError`, uncaught, i.e. a 500 *after* the User, Profile, and
/// InstanceAdmin rows have committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryStored {
    Bool(bool),
    /// The save raises; the redirect never happens.
    Invalid,
}

pub fn coerce_telemetry(raw: &TelemetryRaw) -> TelemetryStored {
    match raw {
        TelemetryRaw::Missing => TelemetryStored::Bool(true),
        TelemetryRaw::Text(s) => match s.as_str() {
            "t" | "True" | "1" => TelemetryStored::Bool(true),
            "f" | "False" | "0" => TelemetryStored::Bool(false),
            _ => TelemetryStored::Invalid,
        },
    }
}

/// Sign-in form fields (`admin.py:261-262`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigninForm {
    pub email: Option<String>,
    pub password: Option<String>,
}

// ---------------------------------------------------------------------------
// §6 Branch evaluators (admin.py:93-239,246-358)
// ---------------------------------------------------------------------------

/// Row state the sign-up branches read: `Instance.objects.first()` is
/// `None`, `InstanceAdmin.objects.first()`, `User.objects.filter(email)`
/// existence, the `validate_email` verdict on the normalized email, and
/// `zxcvbn(password)["score"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupSnapshot {
    pub instance_exists: bool,
    pub admin_exists: bool,
    pub user_exists: bool,
    pub email_valid: bool,
    pub password_score: u8,
}

/// Row state the sign-in branches read, plus the two computed checks.
/// `user` is `User.objects.filter(email).first()`; `password_ok` is
/// `user.check_password(password)`; `is_instance_admin` is
/// `InstanceAdmin.objects.filter(instance, user)` existence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigninUserState {
    pub is_active: bool,
    pub password_ok: bool,
    pub is_instance_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigninSnapshot {
    pub instance_exists: bool,
    pub email_valid: bool,
    pub user: Option<SigninUserState>,
}

/// One redirect outcome: the 302 `Location` plus the structured error for
/// tests (`None` on success). Payload pairs render into the query in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthOutcome {
    pub location: String,
    pub code: Option<i64>,
    pub message: Option<&'static str>,
}

/// `InstanceAdminSignUpEndpoint.post` (`admin.py:93-239`). `base` is the
/// admin base URL (§3). The success arm returns the redirect target; the
/// row writes it implies are specified in §7 (the wiring executes them,
/// then issues the session and redirects).
pub fn decide_signup(base: &str, form: &SignupForm, snap: &SignupSnapshot) -> AuthOutcome {
    let error = |code: i64, message: &'static str, payload: Vec<(&str, QueryValue)>| AuthOutcome {
        location: error_location(base, code, message, &payload),
        code: Some(code),
        message: Some(message),
    };
    if !snap.instance_exists {
        return error(INSTANCE_NOT_CONFIGURED, "INSTANCE_NOT_CONFIGURED", vec![]);
    }
    if snap.admin_exists {
        return error(ADMIN_ALREADY_EXIST, "ADMIN_ALREADY_EXIST", vec![]);
    }
    if !form_field_is_present(&form.email)
        || !form_field_is_present(&form.password)
        || !form_field_is_present(&form.first_name)
    {
        return error(
            REQUIRED_ADMIN_EMAIL_PASSWORD_FIRST_NAME,
            "REQUIRED_ADMIN_EMAIL_PASSWORD_FIRST_NAME",
            vec![
                ("email", missing_or_text(&form.email)),
                ("first_name", missing_or_text(&form.first_name)),
                ("last_name", QueryValue::Text(form.last_name.clone())),
                ("company_name", QueryValue::Text(form.company_name.clone())),
                (
                    "is_telemetry_enabled",
                    telemetry_query_value(&form.telemetry),
                ),
            ],
        );
    }
    // Presence is proven, so these are non-empty strings.
    let email = normalize_email(form.email.as_deref().unwrap_or(""));
    if !snap.email_valid {
        return error(
            INVALID_ADMIN_EMAIL,
            "INVALID_ADMIN_EMAIL",
            signup_echo(form, &email),
        );
    }
    if snap.user_exists {
        return error(
            ADMIN_USER_ALREADY_EXIST,
            "ADMIN_USER_ALREADY_EXIST",
            signup_echo(form, &email),
        );
    }
    if snap.password_score < PASSWORD_MIN_SCORE {
        return error(
            PASSWORD_TOO_WEAK,
            "PASSWORD_TOO_WEAK",
            signup_echo(form, &email),
        );
    }
    AuthOutcome {
        location: success_location(base),
        code: None,
        message: None,
    }
}

/// `InstanceAdminSignInEndpoint.post` (`admin.py:246-358`).
pub fn decide_signin(base: &str, form: &SigninForm, snap: &SigninSnapshot) -> AuthOutcome {
    let error = |code: i64, message: &'static str, payload: Vec<(&str, QueryValue)>| AuthOutcome {
        location: error_location(base, code, message, &payload),
        code: Some(code),
        message: Some(message),
    };
    if !snap.instance_exists {
        return error(INSTANCE_NOT_CONFIGURED, "INSTANCE_NOT_CONFIGURED", vec![]);
    }
    if !form_field_is_present(&form.email) || !form_field_is_present(&form.password) {
        return error(
            REQUIRED_ADMIN_EMAIL_PASSWORD,
            "REQUIRED_ADMIN_EMAIL_PASSWORD",
            vec![("email", missing_or_text(&form.email))],
        );
    }
    let email = normalize_email(form.email.as_deref().unwrap_or(""));
    if !snap.email_valid {
        return error(
            INVALID_ADMIN_EMAIL,
            "INVALID_ADMIN_EMAIL",
            vec![("email", QueryValue::Text(email.clone()))],
        );
    }
    let user = match &snap.user {
        Some(user) => user,
        None => {
            return error(
                ADMIN_USER_DOES_NOT_EXIST,
                "ADMIN_USER_DOES_NOT_EXIST",
                vec![("email", QueryValue::Text(email.clone()))],
            );
        }
    };
    if !user.is_active {
        // No payload on this branch (`admin.py:310-319`).
        return error(ADMIN_USER_DEACTIVATED, "ADMIN_USER_DEACTIVATED", vec![]);
    }
    if !user.password_ok {
        return error(
            ADMIN_AUTHENTICATION_FAILED,
            "ADMIN_AUTHENTICATION_FAILED",
            vec![("email", QueryValue::Text(email.clone()))],
        );
    }
    if !user.is_instance_admin {
        return error(
            ADMIN_AUTHENTICATION_FAILED,
            "ADMIN_AUTHENTICATION_FAILED",
            vec![("email", QueryValue::Text(email))],
        );
    }
    AuthOutcome {
        location: success_location(base),
        code: None,
        message: None,
    }
}

/// The payload echo for the normalized-email sign-up errors
/// (`INVALID_ADMIN_EMAIL`, `ADMIN_USER_ALREADY_EXIST`, `PASSWORD_TOO_WEAK`):
/// normalized email plus the raw companion fields, in dict order.
fn signup_echo<'f>(form: &'f SignupForm, email: &'f str) -> Vec<(&'f str, QueryValue)> {
    vec![
        ("email", QueryValue::Text(email.to_owned())),
        (
            "first_name",
            QueryValue::Text(form.first_name.clone().unwrap_or_default()),
        ),
        ("last_name", QueryValue::Text(form.last_name.clone())),
        ("company_name", QueryValue::Text(form.company_name.clone())),
        (
            "is_telemetry_enabled",
            telemetry_query_value(&form.telemetry),
        ),
    ]
}

/// Missing field → `Bool(false)` (the `request.POST.get(..., False)`
/// default); present field → its text, even when empty.
fn missing_or_text(value: &Option<String>) -> QueryValue {
    match value {
        Some(s) => QueryValue::Text(s.clone()),
        None => QueryValue::Bool(false),
    }
}

fn telemetry_query_value(raw: &TelemetryRaw) -> QueryValue {
    match raw {
        TelemetryRaw::Missing => QueryValue::Bool(true),
        TelemetryRaw::Text(s) => QueryValue::Text(s.clone()),
    }
}

// ---------------------------------------------------------------------------
// §7 Success writes + session issuance (admin.py:210-239,346-358)
// ---------------------------------------------------------------------------

/// Backend string `django.contrib.auth.login` resolves for `user_login`:
/// the project configures a single `AUTHENTICATION_BACKENDS` entry
/// (`settings/common.py:125`), so `login()` takes it without a
/// `user.backend` attribute.
pub const LOGIN_BACKEND: &str = "django.contrib.auth.backends.ModelBackend";

/// Cookie `SessionMiddleware` reads on `*instances*` paths, and the expiry
/// override `user_login` applies (`ADMIN_SESSION_COOKIE_AGE`, default 3600;
/// `settings/common.py:606-607`). Same values as F-05
/// (`pidash_auth::session::ADMIN_SESSION_COOKIE_NAME`); the wiring issues
/// the row through that module read-only.
pub const ADMIN_SESSION_COOKIE_NAME: &str = "admin-session-id";
pub const ADMIN_SESSION_DEFAULT_AGE_SECS: i64 = 3600;

/// `make_password(password)` (`admin.py:215`): PBKDF2 with a fresh
/// 22-alphanumeric salt (`BasePasswordHasher.salt()` for 128 bits of
/// entropy) at the project's iteration count (600000 under the pinned
/// Django 4.2.30; the hasher writes the count into the string, so
/// verification never depends on it). Salt generation stays the caller's —
/// the wiring draws it from the Django alphabet — and this wrapper keeps
/// the encoding in one place, read-only over F-05.
pub fn encode_password(password: &str, salt: &str, iterations: u32) -> String {
    pidash_auth::password::hash_password(password, salt, iterations)
}

/// `user.check_password(password)` (`admin.py:322`): `False` on mismatch
/// *and* on unparsable/foreign-algorithm hashes (Django's `check_password`
/// returns `False` when `identify_hasher` raises instead of propagating).
pub fn check_password(candidate: &str, encoded: &str) -> bool {
    pidash_auth::password::verify_password(candidate, encoded).unwrap_or(false)
}

/// First `X-Forwarded-For` entry, unstripped, else `REMOTE_ADDR`
/// (`utils/ip_address.py:get_client_ip`). Either leg may be absent, in
/// which case the stamp columns receive NULL (see below).
pub fn client_ip(x_forwarded_for: Option<&str>, remote_addr: Option<&str>) -> Option<String> {
    if let Some(forwarded) = x_forwarded_for {
        if !forwarded.is_empty() {
            return Some(forwarded.split(',').next().unwrap_or("").to_owned());
        }
    }
    remote_addr.map(str::to_owned)
}

/// `device_info` dict `user_login` stores in the session
/// (`authentication/utils/login.py:21-25`). The user-agent default is `""`
/// when the header is absent; a missing IP stays JSON null.
pub fn device_info(user_agent: &str, ip: Option<&str>, domain: &str) -> serde_json::Value {
    serde_json::json!({
        "user_agent": user_agent,
        "ip_address": ip,
        "domain": domain,
    })
}

/// Session payload `login()` + `user_login` persist: `_auth_user_id` is the
/// stringified user PK, `_auth_user_hash` the session-auth HMAC of the
/// password field, `_auth_user_backend` ([`LOGIN_BACKEND`]), plus the
/// `device_info` dict. The wiring signs it with F-05
/// (`pidash_auth::signing`, `SESSION_SIGNING_SALT`), inserts the `sessions`
/// row (`session_key` fresh 128 `[a-z0-9]`, `expire_date = now +
/// ADMIN_SESSION_COOKIE_AGE`, mirrored `user_id`/`device_info` columns per
/// `db/models/session.py`), and sets the `admin-session-id` cookie.
pub fn admin_session_payload(
    user_id: &str,
    session_hash: &str,
    info: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "_auth_user_id": user_id,
        "_auth_user_backend": LOGIN_BACKEND,
        "_auth_user_hash": session_hash,
        "device_info": info,
    })
}

/// The success-path writes, in order, with the exact values `admin.py`
/// assigns. The wiring executes them (each ORM call is its own autocommit
/// statement — there is no `transaction.atomic`), then calls `user_login`
/// and redirects to [`success_location`].
///
/// Sign-up (`admin.py:210-237`):
/// * `users`: `first_name`, `last_name`, `email` (normalized),
///   `username = uuid4().hex`, `password = make_password(password)`,
///   `is_password_autoset = False`; then `is_active = True`,
///   `last_active = last_login_time = token_updated_at = now`,
///   `last_login_ip = get_client_ip(request)`
///   ([`client_ip`], NULL when absent),
///   `last_login_uagent = request.META.get("HTTP_USER_AGENT")` (NULL when
///   the header is absent — both columns are NOT NULL, so such a request
///   raises `IntegrityError`, i.e. a 500, on this second `save()`).
/// * `profiles`: `user`, `company_name` as posted (may be `""`).
/// * `instance_admins`: `user`, `instance` (role defaults to 20,
///   `is_verified` to `False`).
/// * `instances`: `is_setup_done = True`, `instance_name = company_name`
///   verbatim (may be `""`), `is_telemetry_enabled` per
///   [`coerce_telemetry`] ([`TelemetryStored::Invalid`] raises on save).
///
/// Sign-in (`admin.py:346-353`): the same stamp set on the existing user
/// (including the NULL `last_login_ip`/`last_login_uagent` hazard), then
/// `user_login`.
///
/// Both arms run `invalidate_cache(path="/api/instances/", user=False)`
/// *before* the view body (decorator order), on every branch including
/// errors — the wiring replays it as a post-commit cache delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStamps {
    /// `timezone.now()` once per request (Python calls it per assignment;
    /// the values are equal up to clock granularity).
    pub now: String,
    /// `None` renders NULL (NOT NULL columns → save raises).
    pub ip: Option<String>,
    /// `None` renders NULL (NOT NULL column → save raises).
    pub user_agent: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://localhost:8000/god-mode/";

    fn full_signup_form() -> SignupForm {
        SignupForm {
            email: Some("Admin@Example.COM ".to_owned()),
            password: Some("Correct Horse Battery Staple 99!x".to_owned()),
            first_name: Some("Ada".to_owned()),
            last_name: "Lovelace".to_owned(),
            company_name: "Acme".to_owned(),
            telemetry: TelemetryRaw::Missing,
        }
    }

    fn open_signup_snapshot() -> SignupSnapshot {
        SignupSnapshot {
            instance_exists: true,
            admin_exists: false,
            user_exists: false,
            email_valid: true,
            password_score: 4,
        }
    }

    // -- §1 codes ---------------------------------------------------------

    #[test]
    fn error_code_values_match_adapter() {
        // authentication/adapter/error.py:60-70 + the golden fixture's
        // error_codes map.
        assert_eq!(INSTANCE_NOT_CONFIGURED, 5000);
        assert_eq!(PASSWORD_TOO_WEAK, 5021);
        assert_eq!(ADMIN_ALREADY_EXIST, 5150);
        assert_eq!(REQUIRED_ADMIN_EMAIL_PASSWORD_FIRST_NAME, 5155);
        assert_eq!(INVALID_ADMIN_EMAIL, 5160);
        assert_eq!(REQUIRED_ADMIN_EMAIL_PASSWORD, 5170);
        assert_eq!(ADMIN_AUTHENTICATION_FAILED, 5175);
        assert_eq!(ADMIN_USER_ALREADY_EXIST, 5180);
        assert_eq!(ADMIN_USER_DOES_NOT_EXIST, 5185);
        assert_eq!(ADMIN_USER_DEACTIVATED, 5190);
        assert_eq!(REDIRECT_STATUS, 302);
    }

    // -- §2 query encoding --------------------------------------------------

    #[test]
    fn query_encoding_matches_urlencode() {
        // Rendered with Django 4.2.30's urlencode (verified vectors).
        assert_eq!(quote_plus("admin@example.com"), "admin%40example.com");
        assert_eq!(quote_plus("München GmbH"), "M%C3%BCnchen+GmbH");
        assert_eq!(quote_plus("a+b c/d~e_f.g-h"), "a%2Bb+c%2Fd~e_f.g-h");
        assert_eq!(quote_plus(""), "");
        assert_eq!(render_query_value(&QueryValue::Bool(false)), "False");
        assert_eq!(render_query_value(&QueryValue::Bool(true)), "True");
        assert_eq!(render_query_value(&QueryValue::Int(5155)), "5155");
        let q = encode_query(&[
            ("error_code", QueryValue::Int(5170)),
            (
                "error_message",
                QueryValue::Text("REQUIRED_ADMIN_EMAIL_PASSWORD".to_owned()),
            ),
            ("email", QueryValue::Bool(false)),
        ]);
        assert_eq!(
            q,
            "error_code=5170&error_message=REQUIRED_ADMIN_EMAIL_PASSWORD&email=False"
        );
    }

    // -- §3 base + locations -------------------------------------------------

    #[test]
    fn admin_base_matches_host_py() {
        // Slash normalization (host.py:27-34).
        assert_eq!(normalize_admin_base_path(None), "/god-mode/");
        assert_eq!(normalize_admin_base_path(Some("god-mode")), "/god-mode/");
        assert_eq!(normalize_admin_base_path(Some("/god-mode")), "/god-mode/");
        assert_eq!(normalize_admin_base_path(Some("god-mode/")), "/god-mode/");
        // ADMIN_BASE_URL wins; else WEB_URL/APP_BASE_URL origin.
        assert_eq!(
            admin_base_url(
                "http://localhost:3000",
                Some("https://admin.example.com"),
                None
            ),
            "https://admin.example.com/god-mode/"
        );
        assert_eq!(
            admin_base_url("http://localhost:3000", None, None),
            "http://localhost:3000/god-mode/"
        );
        assert_eq!(
            admin_base_url("http://localhost:3000", None, Some("console")),
            "http://localhost:3000/console/"
        );
    }

    #[test]
    fn redirect_locations_match_python() {
        // Byte-exact against Django 4.2.30 urlencode/urljoin vectors.
        assert_eq!(
            error_location(
                BASE,
                5160,
                "INVALID_ADMIN_EMAIL",
                &[
                    ("email", QueryValue::Text("admin@example.com".to_owned())),
                    ("first_name", QueryValue::Text("Ada".to_owned())),
                    ("last_name", QueryValue::Text("L".to_owned())),
                    ("company_name", QueryValue::Text("München GmbH".to_owned())),
                    ("is_telemetry_enabled", QueryValue::Text("false".to_owned())),
                ],
            ),
            "http://localhost:8000/god-mode/?error_code=5160&error_message=INVALID_ADMIN_EMAIL\
             &email=admin%40example.com&first_name=Ada&last_name=L\
             &company_name=M%C3%BCnchen+GmbH&is_telemetry_enabled=false"
        );
        assert_eq!(
            error_location(BASE, 5190, "ADMIN_USER_DEACTIVATED", &[],),
            "http://localhost:8000/god-mode/?error_code=5190&error_message=ADMIN_USER_DEACTIVATED"
        );
        assert_eq!(
            success_location(BASE),
            "http://localhost:8000/god-mode/general/"
        );
    }

    // -- §4 email --------------------------------------------------------------

    #[test]
    fn normalize_email_strips_and_lowers() {
        assert_eq!(
            normalize_email("  Admin@Example.COM  "),
            "admin@example.com"
        );
        assert_eq!(normalize_email("a@b.com"), "a@b.com");
    }

    #[test]
    fn punycode_vectors_match_python_codec() {
        // `s.encode("punycode")` on Django 4.2.30's interpreter.
        assert_eq!(punycode_encode("münchen").as_deref(), Some("mnchen-3ya"));
        assert_eq!(punycode_encode("☃").as_deref(), Some("n3h"));
        assert_eq!(punycode_encode("ü").as_deref(), Some("tda"));
        assert_eq!(punycode_encode("bücher").as_deref(), Some("bcher-kva"));
        assert_eq!(punycode_encode("-münchen").as_deref(), Some("-mnchen-o2a"));
        assert_eq!(punycode_encode("münchen-").as_deref(), Some("mnchen--n2a"));
        assert_eq!(punycode_encode("ﬁle").as_deref(), Some("le-1b1n"));
        assert_eq!(punycode_encode("faß").as_deref(), Some("fa-hia"));
    }

    #[test]
    fn email_corpus_matches_django() {
        // Every verdict below was read off Django 4.2.30's validate_email.
        let label63 = "l".repeat(63);
        let tld63 = "t".repeat(63);
        let long_label = format!("a@{label63}.com");
        let long_tld = format!("a@example.{tld63}");
        let valid: Vec<&str> = vec![
            "admin@example.com",
            "Admin@Example.COM",
            "a@b.com",
            "a@localhost",
            "user@mail.example.co.uk",
            "first.last+tag@sub.domain.example",
            "postmaster@[127.0.0.1]",
            "email@123.123.123.123",
            "email@[123.123.123.123]",
            "UPPER@UPPERCASE.ORG",
            "o'brien@example.com",
            "_underscore@start.com",
            "dash-ok@my-domain.io",
            "\"foo\"@example.com",
            "\"foo@bar\"@example.com",
            "\"a\\\"b\"@example.com",
            "user@xn--mnchen-3ya.de",
            "test@münchen.de",
            "test@MÜNCHEN.de",
            "a@münchen.münchen.de",
            "a@☃.com",
            "a@-münchen.de",
            "a@münchen-.de",
            "a@faß.de",
            "a@ﬁle.com",
            "a@ü.com",
            "a@b.co",
            "a@x-y.com",
            "a@x--y.com",
            "a@example.-com",
            "user@[1.2.3.4]",
            "user@[::1]",
            "user@[2001:db8::1]",
            "user@[::ffff:1.2.3.4]",
            "user@[2001:DB8::1]",
            "user@[fe80::]",
            "user@[::]",
            "user@[1:2:3:4:5:6:7:8]",
            "a@KK.com",
            "a@K.com",
            &long_label,
            &long_tld,
        ];
        for email in valid {
            assert!(email_is_valid(email), "should accept {email:?}");
        }
        let label64 = "l".repeat(64);
        let tld64 = "t".repeat(64);
        let bad_label = format!("a@{label64}.com");
        let bad_tld = format!("a@example.{tld64}");
        let idn60 = format!("a@{}.de", "ü".repeat(60));
        let overlong = format!("{}@example.com", "a".repeat(310));
        let overlong_noshape = "a".repeat(321);
        let invalid: Vec<&str> = vec![
            "  spaced@example.com  ",
            "\"quoted string\"@example.com",
            "\"much.more unusual\"@example.com",
            "user@[IPv6:2001:db8::1]",
            "user@[999.999.999.999]",
            "plainaddress",
            "@missing-local.com",
            "missing-at-sign.com",
            "a@b",
            "a@-bad.com",
            "a@bad-.com",
            "a@bad..com",
            "a..b@example.com",
            ".a@example.com",
            "b.@example.com",
            "foo bar@example.com",
            "foo@bar@baz.com",
            "a@LOCALHOST",
            "a@Localhost",
            "a@example.com-",
            "a@-example.com",
            "a@1.2",
            "user@[001.002.003.004]",
            "user@[1.2.3.256]",
            "user@[1.2.3]",
            "user@[1.2.3.4.5]",
            "user@[1::2::3]",
            "user@[1:2:3:4:5:6:7:8:9]",
            "user@[abc]",
            "user@[12:34]",
            "user@[]",
            "a@example.com.",
            "a@.example.com",
            "a@exam_ple.com",
            "ünïcode@example.com",
            "a@ü",
            "a@münchen..de",
            "a@exam ple.com",
            "",
            "@",
            "a@",
            &overlong_noshape,
            &overlong,
            &bad_label,
            &bad_tld,
            &idn60,
        ];
        for email in invalid {
            assert!(!email_is_valid(email), "should reject {email:?}");
        }
    }

    // -- §5 forms + telemetry ----------------------------------------------------

    #[test]
    fn presence_matches_python_falsiness() {
        assert!(!form_field_is_present(&None));
        assert!(!form_field_is_present(&Some("".to_owned())));
        assert!(form_field_is_present(&Some("x".to_owned())));
    }

    #[test]
    fn telemetry_coercion_matches_django() {
        // Verified against BooleanField.get_prep_value, Django 4.2.30.
        assert_eq!(
            coerce_telemetry(&TelemetryRaw::Missing),
            TelemetryStored::Bool(true)
        );
        for s in ["t", "True", "1"] {
            assert_eq!(
                coerce_telemetry(&TelemetryRaw::Text(s.to_owned())),
                TelemetryStored::Bool(true),
                "{s:?}"
            );
        }
        for s in ["f", "False", "0"] {
            assert_eq!(
                coerce_telemetry(&TelemetryRaw::Text(s.to_owned())),
                TelemetryStored::Bool(false),
                "{s:?}"
            );
        }
        for s in ["true", "false", "TRUE", "FALSE", "yes", "", "banana", "2"] {
            assert_eq!(
                coerce_telemetry(&TelemetryRaw::Text(s.to_owned())),
                TelemetryStored::Invalid,
                "{s:?}"
            );
        }
    }

    // -- §6 signup branches ---------------------------------------------------------

    #[test]
    fn signup_no_instance() {
        let snap = SignupSnapshot {
            instance_exists: false,
            ..open_signup_snapshot()
        };
        let out = decide_signup(BASE, &full_signup_form(), &snap);
        assert_eq!(out.code, Some(5000));
        assert_eq!(
            out.location,
            "http://localhost:8000/god-mode/?error_code=5000&error_message=INSTANCE_NOT_CONFIGURED"
        );
    }

    #[test]
    fn signup_admin_already_exists() {
        let snap = SignupSnapshot {
            admin_exists: true,
            ..open_signup_snapshot()
        };
        let out = decide_signup(BASE, &full_signup_form(), &snap);
        assert_eq!(out.code, Some(5150));
        assert_eq!(
            out.location,
            "http://localhost:8000/god-mode/?error_code=5150&error_message=ADMIN_ALREADY_EXIST"
        );
    }

    #[test]
    fn signup_required_fields_echo_payload() {
        // Missing everything: False defaults echoed, True telemetry default.
        let form = SignupForm {
            email: None,
            password: None,
            first_name: None,
            last_name: String::new(),
            company_name: String::new(),
            telemetry: TelemetryRaw::Missing,
        };
        let out = decide_signup(BASE, &form, &open_signup_snapshot());
        assert_eq!(out.code, Some(5155));
        assert_eq!(
            out.location,
            "http://localhost:8000/god-mode/?error_code=5155&error_message=\
             REQUIRED_ADMIN_EMAIL_PASSWORD_FIRST_NAME&email=False&first_name=False\
             &last_name=&company_name=&is_telemetry_enabled=True"
        );
        // Empty-string fields are equally missing.
        let form = SignupForm {
            email: Some(String::new()),
            ..full_signup_form()
        };
        let out = decide_signup(BASE, &form, &open_signup_snapshot());
        assert_eq!(out.code, Some(5155));
        assert!(out.location.contains("&email=&first_name=Ada"));
    }

    #[test]
    fn signup_invalid_email_uses_normalized_echo() {
        let form = SignupForm {
            email: Some("  Admin@Example.COM  ".to_owned()),
            ..full_signup_form()
        };
        let snap = SignupSnapshot {
            email_valid: false,
            ..open_signup_snapshot()
        };
        let out = decide_signup(BASE, &form, &snap);
        assert_eq!(out.code, Some(5160));
        assert!(out.location.contains("error_code=5160"), "{}", out.location);
        assert!(
            out.location.contains("&email=admin%40example.com"),
            "{}",
            out.location
        );
    }

    #[test]
    fn signup_existing_user() {
        let snap = SignupSnapshot {
            user_exists: true,
            ..open_signup_snapshot()
        };
        let out = decide_signup(BASE, &full_signup_form(), &snap);
        assert_eq!(out.code, Some(5180));
        assert!(
            out.location.contains("&email=admin%40example.com"),
            "{}",
            out.location
        );
    }

    #[test]
    fn signup_weak_password_rejects_below_3() {
        for (score, accepted) in [(0, false), (1, false), (2, false), (3, true), (4, true)] {
            let snap = SignupSnapshot {
                password_score: score,
                ..open_signup_snapshot()
            };
            let out = decide_signup(BASE, &full_signup_form(), &snap);
            assert_eq!(out.code.is_none(), accepted, "score {score}");
            if !accepted {
                assert_eq!(out.code, Some(5021));
                assert_eq!(
                    out.location,
                    "http://localhost:8000/god-mode/?error_code=5021&error_message=PASSWORD_TOO_WEAK\
                     &email=admin%40example.com&first_name=Ada&last_name=Lovelace\
                     &company_name=Acme&is_telemetry_enabled=True"
                );
            }
        }
    }

    #[test]
    fn signup_success_redirects_to_general() {
        let out = decide_signup(BASE, &full_signup_form(), &open_signup_snapshot());
        assert_eq!(out.code, None);
        assert_eq!(out.location, "http://localhost:8000/god-mode/general/");
    }

    // -- §6 signin branches ----------------------------------------------------------

    fn open_signin_snapshot() -> SigninSnapshot {
        SigninSnapshot {
            instance_exists: true,
            email_valid: true,
            user: Some(SigninUserState {
                is_active: true,
                password_ok: true,
                is_instance_admin: true,
            }),
        }
    }

    fn signin_form() -> SigninForm {
        SigninForm {
            email: Some("Admin@Example.COM ".to_owned()),
            password: Some("ContractPass123!".to_owned()),
        }
    }

    #[test]
    fn signin_no_instance() {
        let snap = SigninSnapshot {
            instance_exists: false,
            ..open_signin_snapshot()
        };
        let out = decide_signin(BASE, &signin_form(), &snap);
        assert_eq!(out.code, Some(5000));
    }

    #[test]
    fn signin_required_fields() {
        let form = SigninForm {
            email: None,
            password: None,
        };
        let out = decide_signin(BASE, &form, &open_signin_snapshot());
        assert_eq!(out.code, Some(5170));
        assert_eq!(
            out.location,
            "http://localhost:8000/god-mode/?error_code=5170&error_message=\
             REQUIRED_ADMIN_EMAIL_PASSWORD&email=False"
        );
    }

    #[test]
    fn signin_invalid_email() {
        let snap = SigninSnapshot {
            email_valid: false,
            ..open_signin_snapshot()
        };
        let out = decide_signin(BASE, &signin_form(), &snap);
        assert_eq!(out.code, Some(5160));
        assert!(
            out.location.contains("&email=admin%40example.com"),
            "{}",
            out.location
        );
    }

    #[test]
    fn signin_unknown_user() {
        let snap = SigninSnapshot {
            user: None,
            ..open_signin_snapshot()
        };
        let out = decide_signin(BASE, &signin_form(), &snap);
        assert_eq!(out.code, Some(5185));
        assert!(
            out.location.contains("&email=admin%40example.com"),
            "{}",
            out.location
        );
    }

    #[test]
    fn signin_deactivated_user_has_no_payload() {
        let snap = SigninSnapshot {
            user: Some(SigninUserState {
                is_active: false,
                password_ok: true,
                is_instance_admin: true,
            }),
            ..open_signin_snapshot()
        };
        let out = decide_signin(BASE, &signin_form(), &snap);
        assert_eq!(out.code, Some(5190));
        assert_eq!(
            out.location,
            "http://localhost:8000/god-mode/?error_code=5190&error_message=ADMIN_USER_DEACTIVATED"
        );
    }

    #[test]
    fn signin_wrong_password_and_non_admin_share_code() {
        for user in [
            SigninUserState {
                is_active: true,
                password_ok: false,
                is_instance_admin: true,
            },
            SigninUserState {
                is_active: true,
                password_ok: true,
                is_instance_admin: false,
            },
        ] {
            let snap = SigninSnapshot {
                user: Some(user),
                ..open_signin_snapshot()
            };
            let out = decide_signin(BASE, &signin_form(), &snap);
            assert_eq!(out.code, Some(5175));
            assert_eq!(
                out.location,
                "http://localhost:8000/god-mode/?error_code=5175&error_message=\
                 ADMIN_AUTHENTICATION_FAILED&email=admin%40example.com"
            );
        }
    }

    #[test]
    fn signin_success_redirects_to_general() {
        let out = decide_signin(BASE, &signin_form(), &open_signin_snapshot());
        assert_eq!(out.code, None);
        assert_eq!(out.location, "http://localhost:8000/god-mode/general/");
    }

    // -- §7 password + session ----------------------------------------------------------

    #[test]
    fn password_round_trip_matches_django_vector() {
        // make_password('ContractPass123!', salt=...) on Django 4.2.30.
        let encoded = encode_password("ContractPass123!", "somesalt123456789012", 600_000);
        assert_eq!(
            encoded,
            "pbkdf2_sha256$600000$somesalt123456789012$lVXkSZvJRte6fmi7qdGDROCQUoWXPnVpACM8z1Nfpps="
        );
        assert!(check_password("ContractPass123!", &encoded));
        assert!(!check_password("wrong", &encoded));
        assert!(!check_password("anything", "not-a-hash"));
        assert!(!check_password("anything", "bcrypt$abc"));
    }

    #[test]
    fn client_ip_matches_get_client_ip() {
        assert_eq!(
            client_ip(Some("1.2.3.4, 5.6.7.8"), Some("9.9.9.9")),
            Some("1.2.3.4".to_owned())
        );
        // First entry is NOT stripped (verbatim Python).
        assert_eq!(
            client_ip(Some(" 1.2.3.4"), None),
            Some(" 1.2.3.4".to_owned())
        );
        assert_eq!(
            client_ip(Some(""), Some("9.9.9.9")),
            Some("9.9.9.9".to_owned())
        );
        assert_eq!(client_ip(None, None), None);
    }

    #[test]
    fn session_payload_shape_matches_user_login() {
        let info = device_info(
            "pytest-agent",
            Some("127.0.0.1"),
            "http://localhost:3000/god-mode/",
        );
        assert_eq!(
            info,
            serde_json::json!({
                "user_agent": "pytest-agent",
                "ip_address": "127.0.0.1",
                "domain": "http://localhost:3000/god-mode/",
            })
        );
        let payload = admin_session_payload("user-pk-1", "session-hash", info);
        assert_eq!(payload["_auth_user_id"], "user-pk-1");
        assert_eq!(
            payload["_auth_user_backend"],
            "django.contrib.auth.backends.ModelBackend"
        );
        assert_eq!(payload["_auth_user_hash"], "session-hash");
        assert!(payload["device_info"].is_object());
        assert_eq!(ADMIN_SESSION_COOKIE_NAME, "admin-session-id");
    }
}
