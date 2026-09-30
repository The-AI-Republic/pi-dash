//! Email-session closure kernel (D-16, PIDASHCONV-422).
//!
//! Pure branch evaluators + 302/200 builders for the eight
//! email-session endpoints:
//!
//! * `POST sign-in/`, `POST sign-up/` (app + `spaces/` twins)
//!   (`authentication/views/app/email.py:26-238`,
//!   `authentication/views/space/email.py:25-191`);
//! * `POST email-check/`, `POST spaces/email-check/`
//!   (`views/app/check.py:34-103`, `views/space/check.py:34-101`);
//! * `POST sign-out/`, `POST spaces/sign-out/`
//!   (`views/app/signout.py:16-28`, `views/space/signout.py:17-33`);
//! * the `EmailProvider` branch order
//!   (`provider/credentials/email.py:18-96`).
//!
//! Calling convention (same as the D-01 `handlers_auth_forms` kernel):
//! every database-, secret- or settings-derived input arrives as a
//! caller-supplied value — row snapshots, validation bits, config
//! strings. Nothing here performs I/O. The HTTP wiring lives in
//! `pidash_api::auth_session::email`.
//!
//! Fixtures: `rust-api/fixtures/auth_session/FX-AUTH-06.providers.json`
//! (email-provider part) + `FX-AUTH-07.handlers_email.json`; the
//! `#[cfg(test)]` module replays both.
//!
//! Ported quirks (kept, listed in the PR; translated, not fixed):
//! - `Q-missing-email`: `request.POST.get("email", False)` yields
//!   boolean `False` when the key is absent, and the REQUIRED_*
//!   payload carries `str(email)` — i.e. the literal `"False"`
//!   ([`python_bool`]; FX-07 `signin_app.no_email_key`).
//! - `Q-slashless-success`: the app success path feeds
//!   `get_redirection_path` output (`onboarding`, `<slug>`, …) through
//!   `get_safe_redirect_url`, which drops slash-less paths, so success
//!   lands on the bare base URL (BUG-6 in [`shapes`]).
//! - `Q-space-success`: the space success path uses
//!   `validate_next_path` + an allowed-host check with a bare-base
//!   fallback, so success without `next_path` lands on the slash-less
//!   `.../spaces` (FX-07 `signin_space.ok`).
//! - `Q-anon-signout`: the sign-out `except` branch swallows the
//!   failure and redirects to the same target (FX-07
//!   `signout.anonymous`).
//! - `Q-provider-order`: the view's own `USER_DOES_NOT_EXIST` /
//!   `USER_ALREADY_EXIST` pre-checks run before `EmailProvider` is
//!   constructed, so the `EMAIL_PASSWORD_AUTHENTICATION_DISABLED`
//!   gate only fires when the pre-checks pass.

use serde_json::Value;

use super::shapes::{
    error_pairs, get_safe_redirect_url, python_bool, url_has_allowed_host_and_scheme,
    validate_next_path, ParamValue,
};
use crate::v1_projects::ser_collab::is_valid_email;

// ---------------------------------------------------------------------------
// Form-field semantics (`request.POST.get(key, False)`)
// ---------------------------------------------------------------------------

/// One `request.POST.get(key, False)` read: Django's `QueryDict.get`
/// returns the last value for repeated keys, `False` when absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField<'a> {
    /// Key absent: Python `False`.
    Missing,
    /// Key present (last value wins), even when empty.
    Present(&'a str),
}

/// Last-wins lookup over urlencoded pairs, mirroring `QueryDict.get`.
pub fn form_field<'a>(pairs: &'a [(String, String)], key: &str) -> FormField<'a> {
    let mut out = FormField::Missing;
    for (k, v) in pairs {
        if k == key {
            out = FormField::Present(v.as_str());
        }
    }
    out
}

/// Django `if not email`: missing is falsy, present is falsy only when
/// the string is empty.
pub fn field_is_missing(field: FormField<'_>) -> bool {
    match field {
        FormField::Missing => true,
        FormField::Present(value) => value.is_empty(),
    }
}

/// `str(email)` for the REQUIRED_* payload: missing renders as
/// `"False"` ([`python_bool`]), present renders verbatim (pre-strip).
pub fn field_payload_str(field: FormField<'_>) -> String {
    match field {
        FormField::Missing => python_bool(false).to_owned(),
        FormField::Present(value) => value.to_owned(),
    }
}

/// `email.strip().lower()` after the presence check (same operation as
/// the D-01 `normalize_email`; `trim` + `to_lowercase`).
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

// ---------------------------------------------------------------------------
// Error codes used by this closure (mirror `adapter/error.py` values)
// ---------------------------------------------------------------------------

pub const INSTANCE_NOT_CONFIGURED: i32 = 5000;
pub const USER_ALREADY_EXIST: i32 = 5030;
pub const AUTHENTICATION_FAILED_SIGN_IN: i32 = 5065;
pub const REQUIRED_EMAIL_PASSWORD_SIGN_IN: i32 = 5070;
pub const INVALID_EMAIL_SIGN_IN: i32 = 5075;
pub const REQUIRED_EMAIL_PASSWORD_SIGN_UP: i32 = 5040;
pub const INVALID_EMAIL_SIGN_UP: i32 = 5045;
pub const EMAIL_PASSWORD_AUTHENTICATION_DISABLED: i32 = 5056;
pub const USER_DOES_NOT_EXIST: i32 = 5060;
pub const EMAIL_REQUIRED: i32 = 5010;
pub const INVALID_EMAIL: i32 = 5005;
pub const PASSWORD_TOO_WEAK: i32 = 5021;
pub const SIGNUP_DISABLED: i32 = 5015;

// ---------------------------------------------------------------------------
// Provider outcome (`EmailProvider.set_user_data` + init gate)
// ---------------------------------------------------------------------------

/// What `EmailProvider.authenticate()` raised or returned, in the order
/// `email.py` observes it: the `ENABLE_EMAIL_PASSWORD == "0"` init gate
/// fires before the existence/password checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderOutcome {
    /// `EMAIL_PASSWORD_AUTHENTICATION_DISABLED` (no payload).
    Disabled,
    /// Sign-in with an unknown address (view pre-check; provider
    /// re-raises the same `USER_DOES_NOT_EXIST` + email payload).
    NoUser { email: String },
    /// Sign-up for an existing address (`USER_ALREADY_EXIST`, no
    /// payload in the provider raise; the view pre-check carries one).
    AlreadyExists,
    /// Wrong password (`AUTHENTICATION_FAILED_SIGN_IN` + email).
    /// `is_signup` is always false on this path, so the SIGN_UP
    /// variant is dead — kept as a variant for fidelity.
    BadPassword { email: String },
    /// `zxcvbn` score < 3 on sign-up (`PASSWORD_TOO_WEAK` + email).
    WeakPassword { email: String },
    /// `ENABLE_SIGNUP == "0"` with no invite (`SIGNUP_DISABLED` +
    /// email).
    SignupDisabled { email: String },
    /// Authenticated.
    Ok,
}

impl ProviderOutcome {
    /// `(code, message)` for the redirect params.
    pub fn error_slug(&self) -> Option<(i32, &'static str)> {
        match self {
            ProviderOutcome::Disabled => Some((
                EMAIL_PASSWORD_AUTHENTICATION_DISABLED,
                "EMAIL_PASSWORD_AUTHENTICATION_DISABLED",
            )),
            ProviderOutcome::NoUser { .. } => Some((USER_DOES_NOT_EXIST, "USER_DOES_NOT_EXIST")),
            ProviderOutcome::AlreadyExists => Some((USER_ALREADY_EXIST, "USER_ALREADY_EXIST")),
            ProviderOutcome::BadPassword { .. } => Some((
                AUTHENTICATION_FAILED_SIGN_IN,
                "AUTHENTICATION_FAILED_SIGN_IN",
            )),
            ProviderOutcome::WeakPassword { .. } => Some((PASSWORD_TOO_WEAK, "PASSWORD_TOO_WEAK")),
            ProviderOutcome::SignupDisabled { .. } => Some((SIGNUP_DISABLED, "SIGNUP_DISABLED")),
            ProviderOutcome::Ok => None,
        }
    }

    /// Payload carried by the provider raise (`email` where Python
    /// passes one; none otherwise).
    pub fn error_email(&self) -> Option<&str> {
        match self {
            ProviderOutcome::NoUser { email }
            | ProviderOutcome::BadPassword { email }
            | ProviderOutcome::WeakPassword { email }
            | ProviderOutcome::SignupDisabled { email } => Some(email.as_str()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Redirect builders
// ---------------------------------------------------------------------------

/// 302 `Location` for an error redirect: `get_safe_redirect_url` over
/// the `get_error_dict` pairs in order (`error_code`, `error_message`,
/// then payload).
pub fn error_location(
    base_url: &str,
    next_path: Option<&str>,
    code: i32,
    message: &str,
    payload: &[(&str, ParamValue)],
    allowed_hosts: &[&str],
) -> String {
    let pairs = error_pairs(code, message, payload);
    let params: Vec<(&str, ParamValue)> = pairs
        .iter()
        .map(|(k, v)| {
            (
                k.as_str(),
                match v {
                    Value::Number(n) => ParamValue::Int(n.as_i64().unwrap_or(0)),
                    Value::Bool(b) => ParamValue::Bool(*b),
                    Value::String(s) => ParamValue::Str(s.clone()),
                    _ => ParamValue::Str(v.to_string()),
                },
            )
        })
        .collect();
    get_safe_redirect_url(base_url, next_path.unwrap_or(""), &params, allowed_hosts)
}

/// App success target: `next_path` verbatim when posted, else the
/// `get_redirection_path` output — both through
/// `get_safe_redirect_url` with no params (`email.py:94-105`).
pub fn app_success_location(
    base_url: &str,
    next_path: Option<&str>,
    redirection_path: &str,
    allowed_hosts: &[&str],
) -> String {
    let path = match next_path {
        Some(next) if !next.is_empty() => next,
        _ => redirection_path,
    };
    get_safe_redirect_url(base_url, path, &[], allowed_hosts)
}

/// Space success target: `validate_next_path` + allowed-host check
/// with a bare-base fallback (`space/email.py:88-94`).
pub fn space_success_location(
    base_url: &str,
    next_path: Option<&str>,
    allowed_hosts: &[&str],
) -> String {
    let validated = validate_next_path(next_path.unwrap_or(""));
    let url = format!("{}{}", base_url.trim_end_matches('/'), validated);
    if url_has_allowed_host_and_scheme(&url, allowed_hosts) {
        url
    } else {
        base_url.to_owned()
    }
}

// ---------------------------------------------------------------------------
// Sign-in / sign-up branch evaluation
// ---------------------------------------------------------------------------

/// Which flavour of the email form is being evaluated (drives the
/// REQUIRED_*/INVALID_* code selection; app vs space only changes the
/// base URL the wiring passes to the builders).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailForm {
    SignIn,
    SignUp,
}

impl EmailForm {
    fn required(&self) -> (i32, &'static str) {
        match self {
            EmailForm::SignIn => (
                REQUIRED_EMAIL_PASSWORD_SIGN_IN,
                "REQUIRED_EMAIL_PASSWORD_SIGN_IN",
            ),
            EmailForm::SignUp => (
                REQUIRED_EMAIL_PASSWORD_SIGN_UP,
                "REQUIRED_EMAIL_PASSWORD_SIGN_UP",
            ),
        }
    }

    fn invalid(&self) -> (i32, &'static str) {
        match self {
            EmailForm::SignIn => (INVALID_EMAIL_SIGN_IN, "INVALID_EMAIL_SIGN_IN"),
            EmailForm::SignUp => (INVALID_EMAIL_SIGN_UP, "INVALID_EMAIL_SIGN_UP"),
        }
    }

    fn unknown(&self) -> (i32, &'static str) {
        match self {
            EmailForm::SignIn => (USER_DOES_NOT_EXIST, "USER_DOES_NOT_EXIST"),
            EmailForm::SignUp => (USER_ALREADY_EXIST, "USER_ALREADY_EXIST"),
        }
    }
}

/// Caller-supplied snapshot for one sign-in/sign-up attempt, in the
/// order the view reads it.
pub struct EmailAttempt<'a> {
    pub form: EmailForm,
    /// `Instance.objects.first()` passes the setup gate.
    pub instance_ok: bool,
    pub email: FormField<'a>,
    pub password: FormField<'a>,
    /// Normalized email (wiring lowercases+strips after the presence
    /// check); `None` when the presence check failed.
    pub normalized_email: Option<&'a str>,
    /// `User.objects.filter(email=).first()` (sign-in) /
    /// `.exists()` (sign-up) verdict on the normalized email.
    pub user_exists: bool,
    /// Provider verdict (only consulted when the view's own checks
    /// pass).
    pub provider: ProviderOutcome,
    pub next_path: Option<&'a str>,
}

/// The view's decision: which redirect to issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailDecision {
    /// `INSTANCE_NOT_CONFIGURED` (no payload).
    NotConfigured,
    /// REQUIRED_* with the raw-email payload.
    Required {
        code: i32,
        message: &'static str,
        email: String,
    },
    /// INVALID_* with the normalized-email payload.
    Invalid {
        code: i32,
        message: &'static str,
        email: String,
    },
    /// USER_DOES_NOT_EXIST (sign-in, unknown) / USER_ALREADY_EXIST
    /// (sign-up, known), with the normalized-email payload.
    Known {
        code: i32,
        message: &'static str,
        email: String,
    },
    /// A provider raise (DISABLED / re-raised existence / bad
    /// password / weak password / signup-disabled).
    Provider {
        code: i32,
        message: &'static str,
        email: Option<String>,
    },
    /// Authenticated: issue the session and redirect to success.
    Authenticated,
}

/// Evaluate the view body in source order (`app/email.py:26-132` for
/// sign-in, `:135-238` for sign-up; space twins identical up to the
/// success target).
pub fn evaluate_email_attempt(attempt: &EmailAttempt<'_>) -> EmailDecision {
    if !attempt.instance_ok {
        return EmailDecision::NotConfigured;
    }
    if field_is_missing(attempt.email) || field_is_missing(attempt.password) {
        let (code, message) = attempt.form.required();
        return EmailDecision::Required {
            code,
            message,
            email: field_payload_str(attempt.email),
        };
    }
    let normalized = attempt.normalized_email.unwrap_or("");
    if !is_valid_email(normalized) {
        let (code, message) = attempt.form.invalid();
        return EmailDecision::Invalid {
            code,
            message,
            email: normalized.to_owned(),
        };
    }
    let mismatch = match attempt.form {
        EmailForm::SignIn => !attempt.user_exists,
        EmailForm::SignUp => attempt.user_exists,
    };
    if mismatch {
        let (code, message) = attempt.form.unknown();
        return EmailDecision::Known {
            code,
            message,
            email: normalized.to_owned(),
        };
    }
    match &attempt.provider {
        ProviderOutcome::Ok => EmailDecision::Authenticated,
        other => {
            let (code, message) = other.error_slug().expect("provider error has a slug");
            EmailDecision::Provider {
                code,
                message,
                email: other.error_email().map(str::to_owned),
            }
        }
    }
}

/// Render the decision to a 302 `Location` (the wiring supplies the
/// surface base + allowed hosts; `redirection_path` is the
/// `get_redirection_path` output for app success).
pub fn email_decision_location(
    decision: &EmailDecision,
    form: EmailForm,
    space: bool,
    base_url: &str,
    next_path: Option<&str>,
    redirection_path: &str,
    allowed_hosts: &[&str],
) -> String {
    let _ = form;
    match decision {
        EmailDecision::NotConfigured => error_location(
            base_url,
            next_path,
            INSTANCE_NOT_CONFIGURED,
            "INSTANCE_NOT_CONFIGURED",
            &[],
            allowed_hosts,
        ),
        EmailDecision::Required {
            code,
            message,
            email,
        } => error_location(
            base_url,
            next_path,
            *code,
            message,
            &[("email", ParamValue::Str(email.clone()))],
            allowed_hosts,
        ),
        EmailDecision::Invalid {
            code,
            message,
            email,
        } => error_location(
            base_url,
            next_path,
            *code,
            message,
            &[("email", ParamValue::Str(email.clone()))],
            allowed_hosts,
        ),
        EmailDecision::Known {
            code,
            message,
            email,
        } => error_location(
            base_url,
            next_path,
            *code,
            message,
            &[("email", ParamValue::Str(email.clone()))],
            allowed_hosts,
        ),
        EmailDecision::Provider {
            code,
            message,
            email,
        } => {
            let payload: Vec<(&str, ParamValue)> = match email {
                Some(email) => vec![("email", ParamValue::Str(email.clone()))],
                None => vec![],
            };
            error_location(base_url, next_path, *code, message, &payload, allowed_hosts)
        }
        EmailDecision::Authenticated => {
            if space {
                space_success_location(base_url, next_path, allowed_hosts)
            } else {
                app_success_location(base_url, next_path, redirection_path, allowed_hosts)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Email-check (`views/app|space/check.py`)
// ---------------------------------------------------------------------------

/// Caller-supplied snapshot for one email-check call.
pub struct EmailCheck<'a> {
    pub instance_ok: bool,
    /// Raw JSON `email` member: falsy (`False`, `""`, missing…)
    /// becomes `None` here (the wiring coerces `str(value)` for
    /// truthy values exactly like the view).
    pub email_raw: Option<&'a str>,
    /// `is_password_autoset` of the existing user, if any.
    pub existing_autoset: Option<bool>,
    pub smtp_configured: bool,
    pub magic_enabled: bool,
}

/// The 200 shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailCheckBody {
    pub existing: bool,
    pub status: &'static str,
}

/// The check's decision: a 400 error dict or the 200 shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailCheckDecision {
    NotConfigured,
    Required,
    Invalid,
    Checked(EmailCheckBody),
}

impl EmailCheckDecision {
    pub fn http_status(&self) -> u16 {
        match self {
            EmailCheckDecision::Checked(_) => 200,
            _ => 400,
        }
    }

    /// Ordered error-dict pairs for the 400 arms (`get_error_dict`
    /// order; neither arm carries a payload).
    pub fn error_pairs(&self) -> Vec<(String, Value)> {
        match self {
            EmailCheckDecision::NotConfigured => {
                error_pairs(INSTANCE_NOT_CONFIGURED, "INSTANCE_NOT_CONFIGURED", &[])
            }
            EmailCheckDecision::Required => error_pairs(EMAIL_REQUIRED, "EMAIL_REQUIRED", &[]),
            EmailCheckDecision::Invalid => error_pairs(INVALID_EMAIL, "INVALID_EMAIL", &[]),
            EmailCheckDecision::Checked(_) => vec![],
        }
    }

    pub fn body_json(&self) -> Value {
        match self {
            EmailCheckDecision::Checked(body) => serde_json::json!({
                "existing": body.existing,
                "status": body.status,
            }),
            _ => {
                let pairs = self.error_pairs();
                let mut map = serde_json::Map::with_capacity(pairs.len());
                for (k, v) in pairs {
                    map.insert(k, v);
                }
                Value::Object(map)
            }
        }
    }
}

/// Evaluate the check body in source order (`check.py:34-103`).
pub fn evaluate_email_check(check: &EmailCheck<'_>) -> EmailCheckDecision {
    if !check.instance_ok {
        return EmailCheckDecision::NotConfigured;
    }
    let Some(raw) = check.email_raw else {
        return EmailCheckDecision::Required;
    };
    let email = normalize_email(raw);
    if !is_valid_email(&email) {
        return EmailCheckDecision::Invalid;
    }
    let magic = check.smtp_configured && check.magic_enabled;
    match check.existing_autoset {
        Some(autoset) => EmailCheckDecision::Checked(EmailCheckBody {
            existing: true,
            status: if autoset && magic {
                "MAGIC_CODE"
            } else {
                "CREDENTIAL"
            },
        }),
        None => EmailCheckDecision::Checked(EmailCheckBody {
            existing: false,
            status: if magic { "MAGIC_CODE" } else { "CREDENTIAL" },
        }),
    }
}

// ---------------------------------------------------------------------------
// Sign-out (`views/app|space/signout.py`)
// ---------------------------------------------------------------------------

/// App sign-out target: always the app base, authenticated or not
/// (the `except` branch redirects identically).
pub fn signout_app_location(base_url: &str) -> String {
    base_url.to_owned()
}

/// Space sign-out target: `get_safe_redirect_url` over `next_path`
/// with no params, on both arms.
pub fn signout_space_location(
    base_url: &str,
    next_path: Option<&str>,
    allowed_hosts: &[&str],
) -> String {
    get_safe_redirect_url(base_url, next_path.unwrap_or(""), &[], allowed_hosts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Probe settings from FX-AUTH-07 (`_method`):
    /// `WEB_URL=http://localhost:8000 APP_BASE_URL=http://localhost:3000`.
    /// `get_allowed_hosts` keeps only the `WEB_URL` netloc here
    /// (`SPACE_BASE_URL` unset), so app-error URLs fall back to the
    /// no-slash `base?params` form while space URLs keep `/?`.
    const APP_BASE: &str = "http://localhost:3000";
    const SPACE_BASE: &str = "http://localhost:8000/spaces/";
    const SPACE_BARE: &str = "http://localhost:8000/spaces";
    const HOSTS: &[&str] = &["localhost:8000"];

    fn attempt<'a>(
        form: EmailForm,
        email: FormField<'a>,
        password: FormField<'a>,
        normalized: Option<&'a str>,
        user_exists: bool,
        provider: ProviderOutcome,
        next_path: Option<&'a str>,
    ) -> EmailAttempt<'a> {
        EmailAttempt {
            form,
            instance_ok: true,
            email,
            password,
            normalized_email: normalized,
            user_exists,
            provider,
            next_path,
        }
    }

    fn location(
        decision: &EmailDecision,
        form: EmailForm,
        space: bool,
        next_path: Option<&str>,
        redirection_path: &str,
    ) -> String {
        let base = if space { SPACE_BASE } else { APP_BASE };
        email_decision_location(
            decision,
            form,
            space,
            base,
            next_path,
            redirection_path,
            HOSTS,
        )
    }

    // FX-AUTH-07 `signin_app.ok`: 302 to the bare app base (the
    // `get_redirection_path` output is slash-less, so the safe-URL
    // builder drops it — BUG-6).
    #[test]
    fn signin_ok_lands_on_bare_app_base() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            FormField::Present("h@x.com"),
            FormField::Present("pw"),
            Some("h@x.com"),
            true,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(decision, EmailDecision::Authenticated);
        assert_eq!(
            location(&decision, EmailForm::SignIn, false, None, "onboarding"),
            APP_BASE
        );
    }

    // FX-AUTH-07 `signin_app.bad_password` (+ FX-AUTH-06
    // `signin_bad_pw`: 5065/AUTHENTICATION_FAILED_SIGN_IN + email).
    #[test]
    fn signin_bad_password_carries_5065() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            FormField::Present("h@x.com"),
            FormField::Present("wrong"),
            Some("h@x.com"),
            true,
            ProviderOutcome::BadPassword {
                email: "h@x.com".to_owned(),
            },
            None,
        ));
        assert_eq!(
            location(&decision, EmailForm::SignIn, false, None, "onboarding"),
            "http://localhost:3000?error_code=5065&error_message=AUTHENTICATION_FAILED_SIGN_IN&email=h%40x.com"
        );
    }

    // FX-AUTH-07 `signin_app.missing` (no password key at all).
    #[test]
    fn signin_missing_password_carries_5070() {
        let pairs = vec![("email".to_owned(), "h@x.com".to_owned())];
        let email = form_field(&pairs, "email");
        let password = form_field(&pairs, "password");
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            email,
            password,
            None,
            false,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(
            decision,
            EmailDecision::Required {
                code: 5070,
                message: "REQUIRED_EMAIL_PASSWORD_SIGN_IN",
                email: "h@x.com".to_owned(),
            }
        );
        assert_eq!(
            location(&decision, EmailForm::SignIn, false, None, "onboarding"),
            "http://localhost:3000?error_code=5070&error_message=REQUIRED_EMAIL_PASSWORD_SIGN_IN&email=h%40x.com"
        );
    }

    // FX-AUTH-07 `signin_app.no_user` (FX-AUTH-06 `signin_no_user`).
    #[test]
    fn signin_unknown_user_carries_5060() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            FormField::Present("nouser@x.com"),
            FormField::Present("pw"),
            Some("nouser@x.com"),
            false,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(
            location(&decision, EmailForm::SignIn, false, None, "onboarding"),
            "http://localhost:3000?error_code=5060&error_message=USER_DOES_NOT_EXIST&email=nouser%40x.com"
        );
    }

    // FX-AUTH-07 `signin_app.no_email_key`: the absent key posts
    // `email='False'`.
    #[test]
    fn signin_missing_email_key_posts_false() {
        let pairs = vec![("password".to_owned(), "pw".to_owned())];
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            form_field(&pairs, "email"),
            form_field(&pairs, "password"),
            None,
            false,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(
            decision,
            EmailDecision::Required {
                code: 5070,
                message: "REQUIRED_EMAIL_PASSWORD_SIGN_IN",
                email: "False".to_owned(),
            }
        );
        assert_eq!(
            location(&decision, EmailForm::SignIn, false, None, "onboarding"),
            "http://localhost:3000?error_code=5070&error_message=REQUIRED_EMAIL_PASSWORD_SIGN_IN&email=False"
        );
    }

    // FX-AUTH-07 `signup_app.ok` / `signup_app.exists` (FX-AUTH-06
    // `signup_exists`: 5030/USER_ALREADY_EXIST).
    #[test]
    fn signup_known_user_carries_5030() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignUp,
            FormField::Present("h@x.com"),
            FormField::Present("pw"),
            Some("h@x.com"),
            true,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(
            location(&decision, EmailForm::SignUp, false, None, "onboarding"),
            "http://localhost:3000?error_code=5030&error_message=USER_ALREADY_EXIST&email=h%40x.com"
        );
    }

    #[test]
    fn signup_ok_lands_on_bare_app_base() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignUp,
            FormField::Present("new@x.com"),
            FormField::Present("Fresh-Strong-Pass-9!x"),
            Some("new@x.com"),
            false,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(decision, EmailDecision::Authenticated);
        assert_eq!(
            location(&decision, EmailForm::SignUp, false, None, "onboarding"),
            APP_BASE
        );
    }

    // FX-AUTH-06 `disabled_gate`: 5056 with no payload.
    #[test]
    fn disabled_gate_carries_5056_without_payload() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            FormField::Present("h@x.com"),
            FormField::Present("pw"),
            Some("h@x.com"),
            true,
            ProviderOutcome::Disabled,
            None,
        ));
        assert_eq!(
            location(&decision, EmailForm::SignIn, false, None, "onboarding"),
            "http://localhost:3000?error_code=5056&error_message=EMAIL_PASSWORD_AUTHENTICATION_DISABLED"
        );
    }

    // Instance gate on every flavour.
    #[test]
    fn instance_not_configured_has_no_payload() {
        let attempt = EmailAttempt {
            instance_ok: false,
            ..attempt(
                EmailForm::SignIn,
                FormField::Present("h@x.com"),
                FormField::Present("pw"),
                Some("h@x.com"),
                true,
                ProviderOutcome::Ok,
                None,
            )
        };
        let decision = evaluate_email_attempt(&attempt);
        assert_eq!(decision, EmailDecision::NotConfigured);
        assert_eq!(
            location(&decision, EmailForm::SignIn, false, None, "onboarding"),
            "http://localhost:3000?error_code=5000&error_message=INSTANCE_NOT_CONFIGURED"
        );
    }

    #[test]
    fn invalid_email_carries_5075() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            FormField::Present("bad"),
            FormField::Present("pw"),
            Some("bad"),
            false,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(
            location(&decision, EmailForm::SignIn, false, None, "onboarding"),
            "http://localhost:3000?error_code=5075&error_message=INVALID_EMAIL_SIGN_IN&email=bad"
        );
    }

    // FX-AUTH-07 `signin_space.ok` / `signin_space.bad` /
    // `signup_space.ok`.
    #[test]
    fn space_success_lands_on_bare_spaces_base() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            FormField::Present("h@x.com"),
            FormField::Present("pw"),
            Some("h@x.com"),
            true,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(
            location(&decision, EmailForm::SignIn, true, None, "onboarding"),
            SPACE_BARE
        );
    }

    #[test]
    fn space_failure_uses_space_base() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignIn,
            FormField::Present("h@x.com"),
            FormField::Present("wrong"),
            Some("h@x.com"),
            true,
            ProviderOutcome::BadPassword {
                email: "h@x.com".to_owned(),
            },
            None,
        ));
        assert_eq!(
            location(&decision, EmailForm::SignIn, true, None, "onboarding"),
            "http://localhost:8000/spaces/?error_code=5065&error_message=AUTHENTICATION_FAILED_SIGN_IN&email=h%40x.com"
        );
    }

    #[test]
    fn space_signup_ok_lands_on_bare_spaces_base() {
        let decision = evaluate_email_attempt(&attempt(
            EmailForm::SignUp,
            FormField::Present("new@x.com"),
            FormField::Present("Fresh-Strong-Pass-9!x"),
            Some("new@x.com"),
            false,
            ProviderOutcome::Ok,
            None,
        ));
        assert_eq!(
            location(&decision, EmailForm::SignUp, true, None, "onboarding"),
            SPACE_BARE
        );
    }

    // FX-AUTH-07 `email_check`: all six vectors.
    #[test]
    fn email_check_vectors() {
        let check =
            |email_raw: Option<&str>, existing_autoset: Option<bool>, smtp: bool, magic: bool| {
                evaluate_email_check(&EmailCheck {
                    instance_ok: true,
                    email_raw,
                    existing_autoset,
                    smtp_configured: smtp,
                    magic_enabled: magic,
                })
            };
        // existing credential user
        assert_eq!(
            check(Some("h@x.com"), Some(false), true, true),
            EmailCheckDecision::Checked(EmailCheckBody {
                existing: true,
                status: "CREDENTIAL",
            })
        );
        // new address while SMTP+magic are on
        assert_eq!(
            check(Some("new@x.com"), None, true, true),
            EmailCheckDecision::Checked(EmailCheckBody {
                existing: false,
                status: "MAGIC_CODE",
            })
        );
        // missing email
        assert_eq!(check(None, None, true, true), EmailCheckDecision::Required);
        // invalid email
        assert_eq!(
            check(Some("not-an-email"), None, true, true),
            EmailCheckDecision::Invalid
        );
        // existing password-autoset user
        assert_eq!(
            check(Some("a@x.com"), Some(true), true, true),
            EmailCheckDecision::Checked(EmailCheckBody {
                existing: true,
                status: "MAGIC_CODE",
            })
        );
        // new address with SMTP off
        assert_eq!(
            check(Some("new@x.com"), None, false, true),
            EmailCheckDecision::Checked(EmailCheckBody {
                existing: false,
                status: "CREDENTIAL",
            })
        );
        assert_eq!(check(None, None, true, true).http_status(), 400);
        assert_eq!(
            check(Some("new@x.com"), None, true, true).http_status(),
            200
        );
    }

    // FX-AUTH-07 `signout`: app / space / anonymous.
    #[test]
    fn signout_targets() {
        assert_eq!(signout_app_location(APP_BASE), APP_BASE);
        assert_eq!(
            signout_space_location(SPACE_BASE, Some("/onboarding"), HOSTS),
            "http://localhost:8000/spaces/?next_path=/onboarding"
        );
        assert_eq!(signout_space_location(SPACE_BASE, None, HOSTS), SPACE_BARE);
    }
}
