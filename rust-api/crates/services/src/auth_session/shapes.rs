#![forbid(unsafe_code)]

//! D-16 authentication error/shape kernel (stage 5, PIDASHCONV-340).
//!
//! Ports the units every D-16 handler renders through:
//!
//! * `AUTHENTICATION_ERROR_CODES` + `AuthenticationException.get_error_dict`
//!   (`apps/api/pi_dash/authentication/adapter/error.py:5-92`).
//! * `auth_exception_handler` 429/401 bodies
//!   (`authentication/adapter/exception.py:17-34`); the dead-code
//!   `throttle_failure_view` 429 bodies
//!   (`authentication/rate_limit.py:21-28,40-47`).
//! * `get_safe_redirect_url` + `validate_next_path`
//!   (`pi_dash/utils/path_validator.py:13-145`), `base_host`
//!   (`authentication/utils/host.py:16-67`), `get_redirection_path`
//!   (`authentication/utils/redirection_path.py:8-46`).
//! * `csrf_failure` context (`authentication/views/common.py:37-45`).
//! * `zxcvbn` score `< 3` rejection (`views/common.py:82-88,118-123`).
//! * `UserSerializer` field list, reference only
//!   (`app/serializers/user.py:15-60`; serializer owned elsewhere).
//!
//! Per-endpoint JSON-400 vs 302-redirect mapping is [`EndpointKind`] +
//! [`endpoint_kind`]; the HTTP shell over this kernel lives in
//! `pidash_api::auth_session::render`.
//!
//! Fixtures: `rust-api/fixtures/auth_session/FX-AUTH-01.errors.json`
//! (PIDASHCONV-279) and `FX-AUTH-02.redirects.json`; the `#[cfg(test)]`
//! module replays both byte-identical.
//!
//! Ported bugs (also listed in the PR; translated, not fixed):
//! - `BUG-1 (adapter/error.py:82)`: `AuthenticationException.__init__`
//!   binds one shared `payload={}` dict, so mutations leak across
//!   instances. Rust values are owned per instance, so the aliasing is
//!   not reproducible; every handler below constructs a fresh payload
//!   per request, which matches all recorded vectors.
//! - `BUG-6`: `get_redirection_path` returns slash-less paths
//!   (`onboarding`, `<slug>`, `invitations`, `create-workspace`) that
//!   `validate_next_path` rejects, so app success redirects land on the
//!   bare base URL ([`select_redirection_path`] keeps the slash-less
//!   strings; FX-02 Q2).
//! - `BUG-7`: `throttle_failure_view` is dead code (zero callers); the
//!   live 429 path is DRF-default through `auth_exception_handler`.
//!   Both bodies are identical and both are pinned here.
//! - `BUG-8` quirks kept: `urlencode({"success": True})` renders
//!   `success=True` (capital T); missing-email posts carry
//!   `email=False` ([`python_bool`]); space reset targets carry a
//!   double slash (handler-owned composition, FX-02 Q4).

use serde_json::Value;

/// `AUTHENTICATION_ERROR_CODES` (`adapter/error.py:5-74`), source order.
pub const AUTHENTICATION_ERROR_CODES: &[(&str, i32)] = &[
    ("INSTANCE_NOT_CONFIGURED", 5000),
    ("INVALID_EMAIL", 5005),
    ("EMAIL_REQUIRED", 5010),
    ("SIGNUP_DISABLED", 5015),
    ("MAGIC_LINK_LOGIN_DISABLED", 5016),
    ("PASSWORD_LOGIN_DISABLED", 5018),
    ("USER_ACCOUNT_DEACTIVATED", 5019),
    ("INVALID_PASSWORD", 5020),
    ("PASSWORD_TOO_WEAK", 5021),
    ("SMTP_NOT_CONFIGURED", 5025),
    ("USER_ALREADY_EXIST", 5030),
    ("AUTHENTICATION_FAILED_SIGN_UP", 5035),
    ("REQUIRED_EMAIL_PASSWORD_SIGN_UP", 5040),
    ("INVALID_EMAIL_SIGN_UP", 5045),
    ("INVALID_EMAIL_MAGIC_SIGN_UP", 5050),
    ("MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED", 5055),
    ("EMAIL_PASSWORD_AUTHENTICATION_DISABLED", 5056),
    ("USER_DOES_NOT_EXIST", 5060),
    ("AUTHENTICATION_FAILED_SIGN_IN", 5065),
    ("REQUIRED_EMAIL_PASSWORD_SIGN_IN", 5070),
    ("INVALID_EMAIL_SIGN_IN", 5075),
    ("INVALID_EMAIL_MAGIC_SIGN_IN", 5080),
    ("MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED", 5085),
    ("INVALID_MAGIC_CODE_SIGN_IN", 5090),
    ("INVALID_MAGIC_CODE_SIGN_UP", 5092),
    ("EXPIRED_MAGIC_CODE_SIGN_IN", 5095),
    ("EXPIRED_MAGIC_CODE_SIGN_UP", 5097),
    ("EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN", 5100),
    ("EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP", 5102),
    ("OAUTH_NOT_CONFIGURED", 5104),
    ("GOOGLE_NOT_CONFIGURED", 5105),
    ("GITHUB_NOT_CONFIGURED", 5110),
    ("GITHUB_USER_NOT_IN_ORG", 5122),
    ("GITLAB_NOT_CONFIGURED", 5111),
    ("GITEA_NOT_CONFIGURED", 5112),
    ("GOOGLE_OAUTH_PROVIDER_ERROR", 5115),
    ("GITHUB_OAUTH_PROVIDER_ERROR", 5120),
    ("GITLAB_OAUTH_PROVIDER_ERROR", 5121),
    ("GITEA_OAUTH_PROVIDER_ERROR", 5123),
    ("INVALID_PASSWORD_TOKEN", 5125),
    ("EXPIRED_PASSWORD_TOKEN", 5130),
    ("INCORRECT_OLD_PASSWORD", 5135),
    ("MISSING_PASSWORD", 5138),
    ("INVALID_NEW_PASSWORD", 5140),
    ("PASSWORD_ALREADY_SET", 5145),
    ("ADMIN_ALREADY_EXIST", 5150),
    ("REQUIRED_ADMIN_EMAIL_PASSWORD_FIRST_NAME", 5155),
    ("INVALID_ADMIN_EMAIL", 5160),
    ("INVALID_ADMIN_PASSWORD", 5165),
    ("REQUIRED_ADMIN_EMAIL_PASSWORD", 5170),
    ("ADMIN_AUTHENTICATION_FAILED", 5175),
    ("ADMIN_USER_ALREADY_EXIST", 5180),
    ("ADMIN_USER_DOES_NOT_EXIST", 5185),
    ("ADMIN_USER_DEACTIVATED", 5190),
    ("RATE_LIMIT_EXCEEDED", 5900),
    ("AUTHENTICATION_FAILED", 5999),
];

/// Look up a code by name, mirroring `AUTHENTICATION_ERROR_CODES[name]`.
pub fn error_code(name: &str) -> Option<i32> {
    AUTHENTICATION_ERROR_CODES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// One error-dict / redirect param value (`adapter/error.py:86-92` merges
/// `payload` over `{"error_code", "error_message"}` in insertion order;
/// a payload key equal to `error_code`/`error_message` overwrites it).
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Int(i64),
    Bool(bool),
    Str(String),
    /// Python `None`: renders as JSON `null` with the key present
    /// (None-vs-absent parity: omitting the pair omits the key).
    Null,
}

impl ParamValue {
    fn json(&self) -> Value {
        match self {
            ParamValue::Int(i) => Value::from(*i),
            ParamValue::Bool(b) => Value::from(*b),
            ParamValue::Str(s) => Value::from(s.clone()),
            ParamValue::Null => Value::Null,
        }
    }

    fn urlenc(&self) -> String {
        match self {
            ParamValue::Int(i) => i.to_string(),
            ParamValue::Bool(b) => python_bool(*b).to_owned(),
            ParamValue::Str(s) => quote_plus(s),
            // `urlencode({"a": None})` renders `a=None`.
            ParamValue::Null => "None".to_owned(),
        }
    }
}

/// Python `str(True)` / `str(False)` (`urllib.parse.urlencode` renders
/// bools capitalised; FX-02 Q3 `success=True`, FX-07 `email=False`).
pub fn python_bool(b: bool) -> &'static str {
    if b {
        "True"
    } else {
        "False"
    }
}

/// `AuthenticationException.get_error_dict()` as ordered pairs:
/// `error_code`, `error_message`, then payload in order
/// (`adapter/error.py:86-92`).
pub fn error_pairs(
    code: i32,
    message: &str,
    payload: &[(&str, ParamValue)],
) -> Vec<(String, Value)> {
    let mut out = Vec::with_capacity(2 + payload.len());
    out.push(("error_code".to_owned(), Value::from(code)));
    out.push(("error_message".to_owned(), Value::from(message)));
    for (k, v) in payload {
        if let Some(slot) = out.iter_mut().find(|(ek, _)| ek == k) {
            slot.1 = v.json();
        } else {
            out.push((k.to_string(), v.json()));
        }
    }
    out
}

/// Byte-exact JSON rendering of [`error_pairs`] (key order preserved;
/// this crate's `serde_json` has no `preserve_order`, so a `Value`
/// object would iterate alphabetically and break DRF byte parity).
pub fn error_dict_json(pairs: &[(String, Value)]) -> String {
    let mut s = String::from("{");
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&serde_json::to_string(k).expect("key serializes"));
        s.push(':');
        s.push_str(&v.to_string());
    }
    s.push('}');
    s
}

/// 429 body shared by `throttle_failure_view` (dead code, BUG-7) and the
/// live `auth_exception_handler` Throttled path (`exception.py:26-31`).
pub fn throttle_error_pairs() -> Vec<(String, Value)> {
    error_pairs(5900, "RATE_LIMIT_EXCEEDED", &[])
}

/// 401 body: DRF `NotAuthenticated` default, passed through untouched
/// by `auth_exception_handler` (`exception.py:22-24`).
pub fn not_authenticated_body() -> Value {
    serde_json::json!({"detail": "Authentication credentials were not provided."})
}

/// How a D-16 endpoint renders an `AuthenticationException` (FX-01
/// `http_mapping`; sources under `authentication/views/app|space/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointKind {
    /// `Response(exc.get_error_dict(), 400)`.
    Json400,
    /// `HttpResponseRedirect(get_safe_redirect_url(base, next, params))`.
    Redirect302,
}

/// Endpoints answering JSON 400 (`check` app+space, `magic-generate`
/// app+space caught branch, `forgot-password` app+space,
/// `change-password` / `set-password`).
pub const JSON_400_SLUGS: &[&str] = &[
    "email_check_app",
    "email_check_space",
    "magic_generate_app",
    "magic_generate_space",
    "forgot_password_app",
    "forgot_password_space",
    "change_password",
    "set_password",
];

/// Endpoints answering 302 (`sign-in/up`, `magic sign-in/up`,
/// `sign-out`, `reset-password`, each app+space).
pub const REDIRECT_302_SLUGS: &[&str] = &[
    "sign_in_app",
    "sign_up_app",
    "sign_in_space",
    "sign_up_space",
    "magic_sign_in_app",
    "magic_sign_up_app",
    "magic_sign_in_space",
    "magic_sign_up_space",
    "sign_out_app",
    "sign_out_space",
    "reset_password_app",
    "reset_password_space",
];

/// Classify an endpoint slug, or `None` for non-error endpoints
/// (`get-csrf-token/` answers 200 only).
pub fn endpoint_kind(slug: &str) -> Option<EndpointKind> {
    if JSON_400_SLUGS.contains(&slug) {
        Some(EndpointKind::Json400)
    } else if REDIRECT_302_SLUGS.contains(&slug) {
        Some(EndpointKind::Redirect302)
    } else {
        None
    }
}

/// `urllib.parse.quote_plus` with `safe=''`: letters, digits and
/// `_.-~` pass through, space becomes `+`, every other UTF-8 byte
/// becomes uppercase `%XX`.
pub fn quote_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `urllib.parse.urlsplit` scheme split: `ALPHA *( ALPHA / DIGIT /
/// "+" / "-" / "." )` before the first `:`, only when no `/?#`
/// precedes it; the scheme lowercases (`path_validator.py` relies on
/// `urlparse`, so `HTTPS://…` parses with scheme `https`).
fn split_scheme(s: &str) -> (String, &str) {
    let mut colon = None;
    for (i, c) in s.char_indices() {
        if c == ':' {
            colon = Some(i);
            break;
        }
        if matches!(c, '/' | '?' | '#') {
            break;
        }
    }
    match colon {
        Some(i) => {
            let cand = &s[..i];
            let mut chars = cand.chars();
            let valid = !cand.is_empty()
                && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
            if valid {
                (cand.to_ascii_lowercase(), &s[i + 1..])
            } else {
                (String::new(), s)
            }
        }
        None => (String::new(), s),
    }
}

/// Authority split for the post-scheme remainder: a leading `//`
/// opens an authority running to the next `/?#` (empty authority, as
/// in `///x`, yields netloc `""` and path `/x`, matching `urlparse`).
fn split_authority(rest: &str) -> (&str, &str) {
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        (&after[..end], &after[end..])
    } else {
        ("", rest)
    }
}

/// Path without query/fragment (`urlparse(...).path`).
fn path_only(s: &str) -> &str {
    let end = s.find(['?', '#']).unwrap_or(s.len());
    &s[..end]
}

/// Django `url_has_allowed_host_and_scheme` (`django/utils/http.py`):
/// strip, then both the raw and backslash-normalised URLs must have an
/// allowed netloc and an `http`/`https` (or absent) scheme.
pub fn url_has_allowed_host_and_scheme(url: &str, allowed_hosts: &[&str]) -> bool {
    let url = url.trim();
    if url.is_empty() {
        return false;
    }
    let normalised = url.replace('\\', "/");
    check_allowed(url, allowed_hosts) && check_allowed(&normalised, allowed_hosts)
}

fn check_allowed(url: &str, allowed_hosts: &[&str]) -> bool {
    // `///` prefix and over-long URLs are unsafe; `len` counts code
    // points as Python `len()` does.
    if url.starts_with("///") || url.chars().count() > 2048 {
        return false;
    }
    let (scheme, rest) = split_scheme(url);
    let (netloc, _) = split_authority(rest);
    if netloc.is_empty() && !scheme.is_empty() {
        return false;
    }
    // Leading control characters are unsafe; only Cc is reachable in
    // kernel-built URLs (next_path is validated, params url-encoded).
    if url.chars().next().is_some_and(|c| c.is_control()) {
        return false;
    }
    let scheme = if scheme.is_empty() && !netloc.is_empty() {
        "http".to_owned()
    } else {
        scheme
    };
    (netloc.is_empty() || allowed_hosts.contains(&netloc))
        && (scheme.is_empty() || scheme == "http" || scheme == "https")
}

/// `validate_next_path` (`path_validator.py:88-126`): backslashes are
/// deleted, absolute URLs degrade to their path, and anything that is
/// not a `/`-rooted, traversal-free, pattern-clean path becomes `""`.
pub fn validate_next_path(next_path: &str) -> String {
    if next_path.is_empty() {
        return String::new();
    }
    // `len(next_path) > 500` counts code points.
    if next_path.chars().count() > 500 {
        return String::new();
    }
    let scrubbed = next_path.replace('\\', "");
    let (scheme, rest) = split_scheme(&scrubbed);
    let (netloc, path) = split_authority(rest);
    let candidate = if !scheme.is_empty() || !netloc.is_empty() {
        path_only(path).to_owned()
    } else {
        scrubbed
    };
    if candidate.is_empty() || !candidate.starts_with('/') {
        return String::new();
    }
    if candidate.contains("..") {
        return String::new();
    }
    if contains_suspicious_patterns(&candidate) {
        return String::new();
    }
    candidate
}

/// Lowercase substring scan of the `path_validator.py:21-38` patterns.
fn contains_suspicious_patterns(path: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "javascript:",
        "data:",
        "vbscript:",
        "file:",
        "ftp:",
        "%2e%2e",
        "%2f%2f",
        "%5c%5c",
        "<script",
        "<iframe",
        "<object",
        "<embed",
        "<form",
        "onload=",
        "onerror=",
        "onclick=",
    ];
    let lower = path.to_lowercase();
    PATTERNS.iter().any(|p| lower.contains(p))
}

/// Netloc of an absolute base URL (`urlparse(base).netloc`).
pub fn netloc_of(base_url: &str) -> &str {
    let (_, rest) = split_scheme(base_url);
    let (netloc, _) = split_authority(rest);
    netloc
}

/// `get_allowed_hosts` (`path_validator.py:70-86`): netloc of
/// `WEB_URL or APP_BASE_URL`, plus `ADMIN_BASE_URL` / `SPACE_BASE_URL`
/// when set.
pub fn allowed_hosts_for<'a>(
    web_url: Option<&'a str>,
    app_base_url: Option<&'a str>,
    admin_base_url: Option<&'a str>,
    space_base_url: Option<&'a str>,
) -> Vec<&'a str> {
    let base_origin = non_empty(web_url).or(non_empty(app_base_url));
    let mut out = Vec::new();
    if let Some(origin) = base_origin {
        out.push(netloc_of(origin));
    }
    for extra in [admin_base_url, space_base_url].into_iter().flatten() {
        if !extra.is_empty() {
            out.push(netloc_of(extra));
        }
    }
    out
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.is_empty())
}

/// `get_safe_redirect_url` (`path_validator.py:129-145`): the
/// validated `next_path` goes in raw, `params` go through
/// `urlencode`; when the composed URL fails the allowed-host check
/// the `next_path` is dropped and only the encoded params survive.
pub fn get_safe_redirect_url(
    base_url: &str,
    next_path: &str,
    params: &[(&str, ParamValue)],
    allowed_hosts: &[&str],
) -> String {
    let validated = validate_next_path(next_path);
    let base = base_url.trim_end_matches('/');
    let mut parts: Vec<String> = Vec::new();
    if !validated.is_empty() {
        parts.push(format!("next_path={validated}"));
    }
    let mut encoded = String::new();
    if !params.is_empty() {
        encoded = params
            .iter()
            .map(|(k, v)| format!("{}={}", quote_plus(k), v.urlenc()))
            .collect::<Vec<_>>()
            .join("&");
        parts.push(encoded.clone());
    }
    let url = if parts.is_empty() {
        base.to_owned()
    } else {
        format!("{base}/?{}", parts.join("&"))
    };
    if url_has_allowed_host_and_scheme(&url, allowed_hosts) {
        url
    } else if encoded.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{encoded}")
    }
}

/// Settings read by `base_host` (`authentication/utils/host.py:19-67`).
pub struct HostSettings<'a> {
    pub web_url: Option<&'a str>,
    pub app_base_url: Option<&'a str>,
    pub admin_base_url: Option<&'a str>,
    pub space_base_url: Option<&'a str>,
    /// `ADMIN_BASE_PATH`, default `/god-mode/`.
    pub admin_base_path: Option<&'a str>,
    /// `SPACE_BASE_PATH`, default `/spaces/`.
    pub space_base_path: Option<&'a str>,
}

fn base_origin<'a>(settings: &'a HostSettings<'a>) -> &'a str {
    non_empty(settings.web_url)
        .or(non_empty(settings.app_base_url))
        .unwrap_or("")
}

/// Slash-normalised `*_BASE_PATH` (`host.py:30-37,46-53`): a missing
/// setting falls back to the default; a missing leading/trailing
/// slash is added (an empty string setting yields `"/"`).
fn normalise_dir(path: Option<&str>, default: &str) -> String {
    let mut dir = path.unwrap_or(default).to_owned();
    if !dir.starts_with('/') {
        dir.insert(0, '/');
    }
    if !dir.ends_with('/') {
        dir.push('/');
    }
    dir
}

/// `base_host` (`host.py:19-67`): `WEB_URL or APP_BASE_URL` is the
/// origin; admin/space append their base path to their own base URL
/// when set, else to the origin; app returns `APP_BASE_URL` when set.
pub fn base_host(settings: &HostSettings, is_admin: bool, is_space: bool, is_app: bool) -> String {
    let origin = base_origin(settings);
    if is_admin {
        let dir = normalise_dir(settings.admin_base_path, "/god-mode/");
        match non_empty(settings.admin_base_url) {
            Some(base) => format!("{base}{dir}"),
            None => format!("{origin}{dir}"),
        }
    } else if is_space {
        let dir = normalise_dir(settings.space_base_path, "/spaces/");
        match non_empty(settings.space_base_url) {
            Some(base) => format!("{base}{dir}"),
            None => format!("{origin}{dir}"),
        }
    } else if is_app {
        non_empty(settings.app_base_url)
            .unwrap_or(origin)
            .to_owned()
    } else {
        origin.to_owned()
    }
}

/// Where `get_redirection_path` sends the user
/// (`redirection_path.py:8-46`); the DB reads (profile, workspace
/// membership, invites) stay in the queries layer, this is the pure
/// branch order over their results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectionTarget<'a> {
    /// Profile missing or not onboarded.
    Onboarding,
    /// Last or fallback workspace slug (slash-less by construction).
    Workspace(&'a str),
    /// Unaccepted workspace invites only.
    Invitations,
    /// No workspace and no invites.
    CreateWorkspace,
}

/// Branch order of `get_redirection_path`: onboarding first, then the
/// active last workspace, then the earliest active membership, then
/// invites, else create-workspace.
pub fn select_redirection_path<'a>(
    is_onboarded: bool,
    last_workspace_slug: Option<&'a str>,
    fallback_slug: Option<&'a str>,
    has_invite: bool,
) -> RedirectionTarget<'a> {
    if !is_onboarded {
        return RedirectionTarget::Onboarding;
    }
    if let Some(slug) = last_workspace_slug {
        return RedirectionTarget::Workspace(slug);
    }
    if let Some(slug) = fallback_slug {
        return RedirectionTarget::Workspace(slug);
    }
    if has_invite {
        return RedirectionTarget::Invitations;
    }
    RedirectionTarget::CreateWorkspace
}

/// The slash-less path strings (`onboarding`, `<slug>`,
/// `invitations`, `create-workspace`) that `validate_next_path`
/// rejects downstream (BUG-6, FX-02 Q2).
pub fn redirection_path_str<'a>(target: &'a RedirectionTarget<'a>) -> &'a str {
    match target {
        RedirectionTarget::Onboarding => "onboarding",
        RedirectionTarget::Workspace(slug) => slug,
        RedirectionTarget::Invitations => "invitations",
        RedirectionTarget::CreateWorkspace => "create-workspace",
    }
}

/// `zxcvbn` rejection threshold (`views/common.py:82-88,118-123`):
/// scores below 3 are rejected; the scoring call itself keeps the
/// same semantics, no new password policy.
pub const PASSWORD_MIN_SCORE: u8 = 3;

/// Whether a `zxcvbn` score is rejected (`results["score"] < 3`).
pub fn is_password_too_weak(score: u8) -> bool {
    score < PASSWORD_MIN_SCORE
}

/// `csrf_failure` render context (`views/common.py:37-45`):
/// `{"reason", "root_url"}` with `root_url = base_host(request)`.
/// The template only reads `root_url`; `reason` is still passed.
pub fn csrf_context(reason: &str, root_url: &str) -> Vec<(String, Value)> {
    vec![
        ("reason".to_owned(), Value::from(reason)),
        ("root_url".to_owned(), Value::from(root_url)),
    ]
}

/// `UserSerializer.Meta.fields` (`app/serializers/user.py:30-32`):
/// every `User._meta` field in order except `password`. Reference
/// only — the serializer is owned elsewhere.
pub const USER_SERIALIZER_FIELDS: &[&str] = &[
    "last_login",
    "id",
    "username",
    "mobile_number",
    "email",
    "display_name",
    "first_name",
    "last_name",
    "avatar",
    "avatar_asset",
    "cover_image",
    "cover_image_asset",
    "date_joined",
    "created_at",
    "updated_at",
    "last_location",
    "created_location",
    "is_superuser",
    "is_managed",
    "is_password_expired",
    "is_active",
    "is_staff",
    "is_email_verified",
    "is_password_autoset",
    "is_password_reset_required",
    "token",
    "last_active",
    "last_login_time",
    "last_logout_time",
    "last_login_ip",
    "last_logout_ip",
    "last_login_medium",
    "last_login_uagent",
    "token_updated_at",
    "is_bot",
    "bot_type",
    "user_timezone",
    "is_email_valid",
    "masked_at",
];

/// `UserSerializer.Meta.read_only_fields` (`user.py:34-58`).
pub const USER_SERIALIZER_READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "username",
    "mobile_number",
    "email",
    "token",
    "created_at",
    "updated_at",
    "is_superuser",
    "is_staff",
    "is_managed",
    "last_active",
    "last_login_time",
    "last_logout_time",
    "last_login_ip",
    "last_logout_ip",
    "last_login_uagent",
    "last_location",
    "last_login_medium",
    "created_location",
    "is_bot",
    "is_password_autoset",
    "is_email_verified",
    "is_active",
    "token_updated_at",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/auth_session/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn err(code: &str, payload: &[(&str, ParamValue)]) -> String {
        let pairs = error_pairs(error_code(code).expect("known code"), code, payload);
        error_dict_json(&pairs)
    }

    #[test]
    fn code_table_matches_fixture() {
        let fx = fixture("FX-AUTH-01.errors.json");
        let codes = fx.get("codes").expect("codes");
        assert_eq!(codes.as_object().expect("obj").len(), 56);
        assert_eq!(AUTHENTICATION_ERROR_CODES.len(), 56);
        for (name, code) in AUTHENTICATION_ERROR_CODES {
            assert_eq!(codes.get(*name), Some(&Value::from(*code)), "code {name}");
        }
        // Source order is byte-identical too: every table name appears
        // in the fixture text in the same sequence.
        let raw = std::fs::read_to_string(format!(
            "{}/../../fixtures/auth_session/FX-AUTH-01.errors.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("fixture exists");
        let mut cursor = 0;
        for (name, _) in AUTHENTICATION_ERROR_CODES {
            let needle = format!("\"{name}\"");
            let at = raw[cursor..].find(&needle).expect("name present");
            cursor += at + needle.len();
        }
    }

    #[test]
    fn no_payload_vectors_replay() {
        let fx = fixture("FX-AUTH-01.errors.json");
        let vectors = fx.get("no_payload_vectors").expect("vectors");
        for (name, code) in AUTHENTICATION_ERROR_CODES {
            let want = vectors.get(*name).expect("vector");
            let pairs = error_pairs(*code, name, &[]);
            assert_eq!(error_dict_json(&pairs), want.to_string(), "{name}");
            let parsed: Value = error_dict_json(&pairs).parse().expect("json");
            assert_eq!(&parsed, want, "{name}");
        }
    }

    #[test]
    fn with_payload_vectors_replay() {
        let fx = fixture("FX-AUTH-01.errors.json");
        let vectors = fx.get("with_payload_vectors").expect("vectors");
        let cases: &[(&str, &str, &str, &str)] = &[
            (
                "AUTHENTICATION_FAILED_SIGN_IN",
                "AUTHENTICATION_FAILED_SIGN_IN",
                "email",
                "u@x.com",
            ),
            (
                "INVALID_EMAIL_signin_payload",
                "INVALID_EMAIL_SIGN_IN",
                "email",
                "bad",
            ),
            (
                "MISSING_PASSWORD_change",
                "MISSING_PASSWORD",
                "error",
                "Old password is missing",
            ),
        ];
        for (key, code, pkey, pval) in cases {
            let want = vectors.get(*key).expect("vector");
            let got = err(code, &[(*pkey, ParamValue::Str(pval.to_string()))]);
            // Content parity is order-insensitive (`Value` equality);
            // byte order is asserted off the literal below.
            let parsed: Value = got.parse().expect("json");
            assert_eq!(&parsed, want, "{key}");
        }
        assert_eq!(
            err(
                "AUTHENTICATION_FAILED_SIGN_IN",
                &[("email", ParamValue::Str("u@x.com".to_owned()))]
            ),
            "{\"error_code\":5065,\"error_message\":\"AUTHENTICATION_FAILED_SIGN_IN\",\"email\":\"u@x.com\"}"
        );
        // An explicit null stays present while an omitted pair stays absent.
        let pairs = error_pairs(
            5065,
            "AUTHENTICATION_FAILED_SIGN_IN",
            &[("email", ParamValue::Null)],
        );
        assert_eq!(
            error_dict_json(&pairs),
            "{\"error_code\":5065,\"error_message\":\"AUTHENTICATION_FAILED_SIGN_IN\",\"email\":null}"
        );
        let bare = error_pairs(5065, "AUTHENTICATION_FAILED_SIGN_IN", &[]);
        assert!(!error_dict_json(&bare).contains("email"));
    }

    #[test]
    fn throttle_and_unauthenticated_bodies() {
        let fx = fixture("FX-AUTH-01.errors.json");
        let bodies = fx.get("status_bodies").expect("bodies");
        let view = bodies.get("throttle_failure_view").expect("view");
        assert_eq!(view.get("status").expect("s"), &Value::from(429));
        let want429 = view.get("body").expect("body");
        let got = error_dict_json(&throttle_error_pairs());
        assert_eq!(got, want429.to_string());
        let live429 = bodies
            .get("production_drf_throttled_via_handler")
            .expect("live")
            .get("body")
            .expect("body");
        assert_eq!(got, live429.to_string());
        let want401 = bodies
            .get("production_not_authenticated_via_handler")
            .expect("401")
            .get("body")
            .expect("body");
        assert_eq!(&not_authenticated_body(), want401);
    }

    #[test]
    fn endpoint_mapping_partition() {
        assert_eq!(JSON_400_SLUGS.len(), 8);
        assert_eq!(REDIRECT_302_SLUGS.len(), 12);
        for slug in JSON_400_SLUGS {
            assert_eq!(endpoint_kind(slug), Some(EndpointKind::Json400));
            assert!(!REDIRECT_302_SLUGS.contains(slug));
        }
        for slug in REDIRECT_302_SLUGS {
            assert_eq!(endpoint_kind(slug), Some(EndpointKind::Redirect302));
        }
        assert_eq!(endpoint_kind("csrf_token"), None);
        assert_eq!(endpoint_kind("oauth_google"), None);
    }

    #[test]
    fn validate_next_path_table_replays() {
        let fx = fixture("FX-AUTH-02.redirects.json");
        let table = fx.get("validate_next_path").expect("table");
        for (input, want) in table.as_object().expect("obj") {
            assert_eq!(
                &validate_next_path(input),
                want.as_str().expect("str"),
                "{input:?}"
            );
        }
    }

    #[test]
    fn safe_url_cases_replay() {
        let fx = fixture("FX-AUTH-02.redirects.json");
        let cases = fx.get("safe_url_cases").expect("cases");
        let allowed = ["localhost:8000"];
        let app = "http://localhost:3000";
        let space = "http://localhost:8000/spaces/";
        let err_params = |code: i64, msg: &str, email: Option<ParamValue>| {
            let mut v = vec![
                ("error_code", ParamValue::Int(code)),
                ("error_message", ParamValue::Str(msg.to_owned())),
            ];
            if let Some(email) = email {
                v.push(("email", email));
            }
            v
        };
        let get = |key: &str| {
            cases
                .get(key)
                .expect("case")
                .as_str()
                .expect("str")
                .to_owned()
        };

        assert_eq!(
            get_safe_redirect_url(
                app,
                "/onboarding",
                &err_params(
                    5065,
                    "AUTHENTICATION_FAILED_SIGN_IN",
                    Some(ParamValue::Str("u@x.com".to_owned()))
                ),
                &allowed
            ),
            get("signin_fail_app_next")
        );
        assert_eq!(
            get_safe_redirect_url(
                app,
                "",
                &err_params(5000, "INSTANCE_NOT_CONFIGURED", None),
                &allowed
            ),
            get("signin_fail_app_no_next")
        );
        assert_eq!(
            get_safe_redirect_url(
                space,
                "/invitations",
                &err_params(
                    5030,
                    "USER_ALREADY_EXIST",
                    Some(ParamValue::Str("u@x.com".to_owned()))
                ),
                &allowed
            ),
            get("signup_fail_space_next")
        );
        assert_eq!(
            get_safe_redirect_url(
                space,
                "https://evil.com/x",
                &err_params(
                    5090,
                    "INVALID_MAGIC_CODE_SIGN_IN",
                    Some(ParamValue::Str("u@x.com".to_owned()))
                ),
                &allowed
            ),
            get("magic_fail_space_evil_next")
        );
        assert_eq!(
            get_safe_redirect_url(app, "/onboarding", &[], &allowed),
            get("success_no_params_next")
        );
        assert_eq!(
            get_safe_redirect_url(app, "", &[], &allowed),
            get("success_no_next")
        );
    }

    #[test]
    fn base_host_cases_replay() {
        let fx = fixture("FX-AUTH-02.redirects.json");
        let want = fx.get("base_host").expect("base_host");
        let settings = HostSettings {
            web_url: Some("http://localhost:8000"),
            app_base_url: Some("http://localhost:3000"),
            admin_base_url: None,
            space_base_url: None,
            admin_base_path: None,
            space_base_path: None,
        };
        let get = |key: &str| want.get(key).expect("case").as_str().expect("str");
        assert_eq!(base_host(&settings, false, false, true), get("app"));
        assert_eq!(base_host(&settings, false, true, false), get("space"));
        assert_eq!(base_host(&settings, false, false, false), get("plain"));
        assert_eq!(base_host(&settings, true, false, false), get("admin"));
        // `allowed_hosts_for` under the probe settings is WEB_URL only,
        // which is why app-base URLs fail the host check (FX-02 Q1).
        assert_eq!(
            allowed_hosts_for(
                settings.web_url,
                settings.app_base_url,
                settings.admin_base_url,
                settings.space_base_url
            ),
            vec!["localhost:8000"]
        );
    }

    #[test]
    fn redirection_matrix_replays() {
        let fx = fixture("FX-AUTH-02.redirects.json");
        let matrix = fx.get("redirection_path_matrix").expect("matrix");
        let get = |key: &str| {
            matrix
                .get(key)
                .expect("case")
                .as_str()
                .expect("str")
                .to_owned()
        };
        assert_eq!(
            redirection_path_str(&select_redirection_path(false, None, None, false)),
            get("new_user_no_profile")
        );
        assert_eq!(
            redirection_path_str(&select_redirection_path(true, Some("ws1"), None, false)),
            get("onboarded_last_workspace")
        );
        assert_eq!(
            redirection_path_str(&select_redirection_path(true, None, Some("ws1"), false)),
            get("onboarded_fallback_member")
        );
        assert_eq!(
            redirection_path_str(&select_redirection_path(true, None, None, true)),
            get("has_invite_only")
        );
        assert_eq!(
            redirection_path_str(&select_redirection_path(true, None, None, false)),
            get("nothing")
        );
        // Slash-less outputs are rejected downstream (BUG-6).
        for target in [
            select_redirection_path(false, None, None, false),
            select_redirection_path(true, Some("ws1"), None, false),
        ] {
            assert_eq!(validate_next_path(redirection_path_str(&target)), "");
        }
    }

    #[test]
    fn python_scalar_quirks() {
        assert_eq!(python_bool(true), "True");
        assert_eq!(python_bool(false), "False");
        assert_eq!(quote_plus("u@x.com"), "u%40x.com");
        assert_eq!(quote_plus("a b"), "a+b");
        assert_eq!(quote_plus("ABC-_.~09"), "ABC-_.~09");
        assert!(is_password_too_weak(2));
        assert!(!is_password_too_weak(3));
        assert_eq!(PASSWORD_MIN_SCORE, 3);
    }

    #[test]
    fn csrf_context_shape() {
        let ctx = csrf_context("<reason arg>", "http://localhost:8000");
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx[0].0, "reason");
        assert_eq!(ctx[1].0, "root_url");
        assert_eq!(ctx[1].1, Value::from("http://localhost:8000"));
    }

    #[test]
    fn user_serializer_field_list() {
        let fx = fixture("FX-AUTH-08.handlers_magic_password.json");
        let keys = fx
            .get("set_password")
            .expect("sp")
            .get("ok_keys")
            .expect("keys")
            .get("keys")
            .expect("list");
        let mut want: Vec<&str> = keys
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        want.sort_unstable();
        let mut got: Vec<&str> = USER_SERIALIZER_FIELDS.to_vec();
        got.sort_unstable();
        assert_eq!(got, want);
        assert_eq!(USER_SERIALIZER_FIELDS.len(), 39);
        // Render order is `_meta` order minus `password`.
        assert_eq!(USER_SERIALIZER_FIELDS[0], "last_login");
        assert_eq!(
            USER_SERIALIZER_FIELDS[USER_SERIALIZER_FIELDS.len() - 1],
            "masked_at"
        );
        assert!(!USER_SERIALIZER_FIELDS.contains(&"password"));
        assert_eq!(USER_SERIALIZER_READ_ONLY_FIELDS.len(), 24);
    }
}
