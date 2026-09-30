#![forbid(unsafe_code)]

//! D-16 password-management + CSRF pure kernel (stage 5, PIDASHCONV-434).
//!
//! Ports the decision half of the password/CSRF closure; the HTTP shell
//! lives in `pidash_api::auth_session::password`:
//!
//! * `ChangePasswordEndpoint` (`authentication/views/common.py:47-96`):
//!   old-password skip for `is_password_autoset` (`:52`), `MISSING_PASSWORD`
//!   for a missing old (`:53-60`) or new (`:63-71`) password,
//!   `INCORRECT_OLD_PASSWORD` (`:73-80`), `zxcvbn < 3` rejection (`:82-89`),
//!   success sets the password, clears `is_password_autoset`, re-logs in
//!   (`:91-96`).
//! * `SetUserPasswordEndpoint` (`common.py:99-138`): `PASSWORD_ALREADY_SET`
//!   (`:105-112`), `INVALID_PASSWORD` for a missing (`:114-120`) or weak
//!   (`:122-128`) password — weak answers 5020 here, not 5021 — success
//!   sets the password, re-logs in, answers the `UserSerializer` body
//!   (`:130-138`).
//! * `ForgotPasswordEndpoint` + `ForgotPasswordSpaceEndpoint`
//!   (`views/app/password_management.py:45-96`,
//!   `views/space/password_management.py:45-108`): instance gate, SMTP
//!   gate, `validate_email`, user-exists branch publishing
//!   `forgot_password.delay`. The gate order lives in
//!   [`tasks::forgot_password_decision`](super::tasks::forgot_password_decision);
//!   this module owns the bodies.
//! * `ResetPasswordEndpoint` + `ResetPasswordSpaceEndpoint`
//!   (`app/password_management.py:99-176`, `space/...:111-160`): the
//!   Django 4.2 `PasswordResetTokenGenerator` recipe, the uidb64 codec,
//!   and the app-vs-space branch mapping (the app inner
//!   `except (ValueError, DoesNotExist)` swallows the UTF-8 decode error
//!   so its EXPIRED branch is dead; the space view has no decode guard at
//!   all).
//! * `CSRFTokenEndpoint` (`common.py:28-35`) + `csrf_failure`
//!   (`common.py:38-44`): bodies reuse `pidash_auth::csrf` and
//!   [`super::shapes::csrf_context`]; only the cookie rendering lives here.
//!
//! Fixtures: `rust-api/fixtures/auth_session/FX-AUTH-08.handlers_magic_password.json`
//! (PIDASHCONV-279); the `#[cfg(test)]` module replays the password/CSRF
//! part of it.
//!
//! Ported bugs (also listed in the PR; translated, not fixed):
//! - reset-app EXPIRED branch is dead (inner `except (ValueError,
//!   User.DoesNotExist)` catches the `DjangoUnicodeDecodeError`, a
//!   `ValueError` subclass, so bad-UTF-8 uids answer 5125, never 5130).
//! - reset-space has no decode/lookup guard: unknown ids raise
//!   `DoesNotExist` (HTTP 500) and non-UUID uids raise `ValidationError`
//!   (HTTP 500 on both surfaces).
//! - reset-app failure targets carry no trailing slash
//!   (`accounts/reset-password?<params>`) while space targets carry a
//!   double slash (`<base>/accounts/reset-password/?<params>`); the app
//!   success target is `sign-in?success=True` (capital T) while the space
//!   success target is the bare space base.
//! - forgot-space fetches `EMAIL_HOST_USER`/`EMAIL_HOST_PASSWORD` but gates
//!   on `EMAIL_HOST` only.
//! - set-password answers `INVALID_PASSWORD` (5020) for weak passwords
//!   where change-password answers `PASSWORD_TOO_WEAK` (5021).
//! - `PasswordResetTokenGenerator` tokens are single-use: `set_password`
//!   rewrites the password field the token HMAC covers.

use serde_json::Value;

use super::shapes::{error_dict_json, error_pairs, quote_plus};

// ---------------------------------------------------------------------------
// Shared bodies
// ---------------------------------------------------------------------------

/// `{"message": "Password updated successfully"}` (`common.py:96`).
pub const CHANGE_PASSWORD_SUCCESS_BODY: &str = r#"{"message":"Password updated successfully"}"#;

/// `{"message": "Check your email to reset your password"}`
/// (`app/password_management.py:88-91`, space twin `:100-103`).
pub const FORGOT_PASSWORD_SUCCESS_BODY: &str =
    r#"{"message":"Check your email to reset your password"}"#;

// ---------------------------------------------------------------------------
// Password strength (`zxcvbn(password)["score"] < 3`)
// ---------------------------------------------------------------------------

/// Score a password with the same estimator family the Python views call
/// (`zxcvbn(password)` with no user inputs). Returns 0-4.
pub fn password_score(password: &str) -> u8 {
    zxcvbn::zxcvbn(password, &[]).score() as u8
}

/// Whether the score rejects the password (`results["score"] < 3`,
/// `common.py:84,123`, `password_management.py:146`, space twin `:140`).
pub fn password_is_weak(password: &str) -> bool {
    password_score(password) < super::shapes::PASSWORD_MIN_SCORE
}

// ---------------------------------------------------------------------------
// Salt (`make_password` salt generation)
// ---------------------------------------------------------------------------

/// Fresh password salt: 22 ASCII alphanumerics, like Django's
/// `get_random_string(22)` for `PBKDF2PasswordHasher.encode`.
pub fn generate_password_salt() -> String {
    use rand::distr::{Alphanumeric, SampleString};
    Alphanumeric.sample_string(&mut rand::rng(), 22)
}

// ---------------------------------------------------------------------------
// Change-password decision (`common.py:47-96`)
// ---------------------------------------------------------------------------

/// Branch taken by `ChangePasswordEndpoint.post`, in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangePasswordOutcome {
    /// `!is_password_autoset` and no old password (`:52-60`).
    MissingOld,
    /// No new password (`:63-71`).
    MissingNew,
    /// `!is_password_autoset` and the old password does not verify
    /// (`:73-80`).
    WrongOld,
    /// `zxcvbn(new) < 3` (`:82-89`).
    TooWeak,
    /// Set the password, clear `is_password_autoset`, re-login (`:91-96`).
    Ok,
}

/// Evaluate the branches in source order. `old_present` / `new_present`
/// are `bool(request.data.get(..., False))` (missing, null, `""` and
/// `false` are all absent); `old_matches` is `user.check_password(old)`,
/// only consulted when `!is_autoset` — autoset users skip old-password
/// verification entirely (`:52`).
pub fn decide_change_password(
    is_autoset: bool,
    old_present: bool,
    new_present: bool,
    old_matches: bool,
    new_weak: bool,
) -> ChangePasswordOutcome {
    if !is_autoset && !old_present {
        return ChangePasswordOutcome::MissingOld;
    }
    if !new_present {
        return ChangePasswordOutcome::MissingNew;
    }
    if !is_autoset && !old_matches {
        return ChangePasswordOutcome::WrongOld;
    }
    if new_weak {
        return ChangePasswordOutcome::TooWeak;
    }
    ChangePasswordOutcome::Ok
}

/// JSON 400 body pairs for a denied outcome, in `get_error_dict` order.
pub fn change_password_error_pairs(outcome: ChangePasswordOutcome) -> Vec<(String, Value)> {
    match outcome {
        ChangePasswordOutcome::MissingOld => error_pairs(
            5138,
            "MISSING_PASSWORD",
            &[(
                "error",
                super::shapes::ParamValue::Str("Old password is missing".to_owned()),
            )],
        ),
        ChangePasswordOutcome::MissingNew => error_pairs(
            5138,
            "MISSING_PASSWORD",
            &[(
                "error",
                super::shapes::ParamValue::Str("Old or new password is missing".to_owned()),
            )],
        ),
        ChangePasswordOutcome::WrongOld => error_pairs(
            5135,
            "INCORRECT_OLD_PASSWORD",
            &[(
                "error",
                super::shapes::ParamValue::Str("Old password is not correct".to_owned()),
            )],
        ),
        ChangePasswordOutcome::TooWeak => error_pairs(5021, "PASSWORD_TOO_WEAK", &[]),
        ChangePasswordOutcome::Ok => Vec::new(),
    }
}

/// Byte-exact JSON 400 body for a denied outcome.
pub fn change_password_error_json(outcome: ChangePasswordOutcome) -> String {
    error_dict_json(&change_password_error_pairs(outcome))
}

// ---------------------------------------------------------------------------
// Set-password decision (`common.py:99-138`)
// ---------------------------------------------------------------------------

/// Branch taken by `SetUserPasswordEndpoint.post`, in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetPasswordOutcome {
    /// The password is not autoset (`:105-112`).
    AlreadySet,
    /// Missing (`:114-120`) or weak (`:122-128`) password — both answer
    /// `INVALID_PASSWORD` (5020), never `PASSWORD_TOO_WEAK`.
    Invalid,
    /// Set the password, re-login, answer the `UserSerializer` body
    /// (`:130-138`).
    Ok,
}

/// Evaluate the branches in source order. `password_present` is
/// `bool(request.data.get("password", False))`; `password_weak` is
/// `zxcvbn(password)["score"] < 3`.
pub fn decide_set_password(
    is_autoset: bool,
    password_present: bool,
    password_weak: bool,
) -> SetPasswordOutcome {
    if !is_autoset {
        return SetPasswordOutcome::AlreadySet;
    }
    if !password_present || password_weak {
        return SetPasswordOutcome::Invalid;
    }
    SetPasswordOutcome::Ok
}

/// JSON 400 body pairs for a denied outcome, in `get_error_dict` order.
pub fn set_password_error_pairs(outcome: SetPasswordOutcome) -> Vec<(String, Value)> {
    match outcome {
        SetPasswordOutcome::AlreadySet => error_pairs(
            5145,
            "PASSWORD_ALREADY_SET",
            &[(
                "error",
                super::shapes::ParamValue::Str(
                    "Your password is already set please change your password from profile"
                        .to_owned(),
                ),
            )],
        ),
        SetPasswordOutcome::Invalid => error_pairs(5020, "INVALID_PASSWORD", &[]),
        SetPasswordOutcome::Ok => Vec::new(),
    }
}

/// Byte-exact JSON 400 body for a denied outcome.
pub fn set_password_error_json(outcome: SetPasswordOutcome) -> String {
    error_dict_json(&set_password_error_pairs(outcome))
}

// ---------------------------------------------------------------------------
// Forgot-password bodies (`app/password_management.py:45-96`)
// ---------------------------------------------------------------------------

/// JSON 400 body pairs per gate, in `get_error_dict` order. The gate order
/// itself is [`super::tasks::forgot_password_decision`]; these are the
/// bodies each failing gate answers (no payload on any of them).
pub fn forgot_password_error_pairs(
    decision: super::tasks::ForgotPasswordDecision,
) -> Vec<(String, Value)> {
    use super::tasks::ForgotPasswordDecision;
    match decision {
        ForgotPasswordDecision::Emit => Vec::new(),
        ForgotPasswordDecision::InstanceNotConfigured => {
            error_pairs(5000, "INSTANCE_NOT_CONFIGURED", &[])
        }
        ForgotPasswordDecision::SmtpNotConfigured => error_pairs(5025, "SMTP_NOT_CONFIGURED", &[]),
        ForgotPasswordDecision::InvalidEmail => error_pairs(5005, "INVALID_EMAIL", &[]),
        ForgotPasswordDecision::UserDoesNotExist => error_pairs(5060, "USER_DOES_NOT_EXIST", &[]),
    }
}

/// Byte-exact JSON 400 body for a failing gate.
pub fn forgot_password_error_json(decision: super::tasks::ForgotPasswordDecision) -> String {
    error_dict_json(&forgot_password_error_pairs(decision))
}

// ---------------------------------------------------------------------------
// Reset-token recipe (`generate_password_token`, Django 4.2
// `PasswordResetTokenGenerator`)
// ---------------------------------------------------------------------------

/// `key_salt` for `salted_hmac` in `make_token`/`check_token`
/// (`django/contrib/auth/tokens.py:7`).
pub const PASSWORD_RESET_KEY_SALT: &str = "django.contrib.auth.tokens.PasswordResetTokenGenerator";

/// Token lifetime (`settings/common.py:344`, overriding Django's 259200s
/// default). Tokens older than this fail `check_token`.
pub const PASSWORD_RESET_TIMEOUT_SECS: i64 = 3600;

/// Seconds between 1970-01-01 and 2001-01-01 (the token epoch).
pub const TOKEN_EPOCH_OFFSET_SECS: i64 = 978_307_200;

/// `int_to_base36` (`django/utils/http.py`): lowercase, `0` for zero.
pub fn base36_encode(mut number: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if number == 0 {
        return "0".to_owned();
    }
    let mut out = Vec::new();
    while number > 0 {
        out.push(DIGITS[(number % 36) as usize] as char);
        number /= 36;
    }
    out.iter().rev().collect()
}

/// `base36_to_int` (`django/utils/http.py`): at most 13 chars, ASCII
/// alphanumerics, case-insensitive like Python's `int(s, 36)`.
pub fn base36_decode(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 13 {
        return None;
    }
    let mut value: u64 = 0;
    for c in s.chars() {
        let digit = match c {
            '0'..='9' => c as u64 - '0' as u64,
            'a'..='z' => c as u64 - 'a' as u64 + 10,
            'A'..='Z' => c as u64 - 'A' as u64 + 10,
            _ => return None,
        };
        value = value.checked_mul(36)?.checked_add(digit)?;
    }
    Some(value)
}

/// `salted_hmac(key_salt, value, secret).hexdigest()` with SHA-256
/// (`django/utils/crypto.py`): `key = sha256(key_salt + secret)`, then
/// `HMAC-SHA256(key, value)`, lowercase hex.
pub fn salted_hmac_hex(key_salt: &str, value: &str, secret: &str) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    let key = {
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(key_salt.as_bytes());
        hasher.update(secret.as_bytes());
        hasher.finalize()
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    mac.update(value.as_bytes());
    let digest = mac.finalize().into_bytes();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// `hexdigest()[::2]`: every even-indexed char of the hex digest.
pub fn even_hex_chars(hexdigest: &str) -> String {
    hexdigest.chars().step_by(2).collect()
}

/// Token timestamp: seconds since 2001-01-01 (`_num_seconds(_now())`).
pub fn token_seconds(now_unix: i64) -> u64 {
    (now_unix - TOKEN_EPOCH_OFFSET_SECS).max(0) as u64
}

/// `login_timestamp` in the token hash: `""` when `last_login` is NULL,
/// else the naive UTC wall time without microseconds
/// (`str(dt.replace(microsecond=0, tzinfo=None))`, `"YYYY-MM-DD HH:MM:SS"`).
pub fn login_timestamp_str(last_login_unix: Option<i64>) -> String {
    match last_login_unix {
        None => String::new(),
        Some(ts) => {
            let secs = ts.max(0) as u64;
            let days = secs / 86_400;
            let rem = secs % 86_400;
            let (y, m, d) = civil_from_days(days);
            format!(
                "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
                rem / 3600,
                (rem % 3600) / 60,
                rem % 60
            )
        }
    }
}

/// Days since 1970-01-01 to civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: u64) -> (i64, u64, u64) {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    y += i64::from(m <= 2);
    (y, m as u64, d as u64)
}

/// `generate_password_token(user)` (`app/password_management.py:38-42`,
/// space twin `:38-42`): `(uidb64, token)`.
pub fn make_password_token(
    secret_key: &str,
    user_pk: &str,
    password_field: &str,
    last_login_unix: Option<i64>,
    email: &str,
    now_unix: i64,
) -> (String, String) {
    let uidb64 = uidb64_encode(user_pk);
    let ts = token_seconds(now_unix);
    let token = make_token_with_timestamp(
        secret_key,
        user_pk,
        password_field,
        last_login_unix,
        email,
        ts,
    );
    (uidb64, token)
}

/// `_make_token_with_timestamp` (`tokens.py:150-162`).
pub fn make_token_with_timestamp(
    secret_key: &str,
    user_pk: &str,
    password_field: &str,
    last_login_unix: Option<i64>,
    email: &str,
    timestamp_secs: u64,
) -> String {
    let value = format!(
        "{user_pk}{password_field}{}{timestamp_secs}{email}",
        login_timestamp_str(last_login_unix),
    );
    let digest = salted_hmac_hex(PASSWORD_RESET_KEY_SALT, &value, secret_key);
    format!(
        "{}-{}",
        base36_encode(timestamp_secs),
        even_hex_chars(&digest)
    )
}

/// `check_token` (`tokens.py:164-192`): constant-time compare against the
/// current timestamp's token, then the timeout. `SECRET_KEY_FALLBACKS`
/// is unset in this project, so only `SECRET_KEY` is tried.
pub fn check_password_token(
    secret_key: &str,
    user_pk: &str,
    password_field: &str,
    last_login_unix: Option<i64>,
    email: &str,
    token: &str,
    now_unix: i64,
) -> bool {
    if user_pk.is_empty() || token.is_empty() {
        return false;
    }
    let mut parts = token.split('-');
    let ts_b36 = match (parts.next(), parts.next(), parts.next()) {
        (Some(ts), Some(_), None) => ts,
        _ => return false,
    };
    let ts = match base36_decode(ts_b36) {
        Some(ts) => ts,
        None => return false,
    };
    let expected = make_token_with_timestamp(
        secret_key,
        user_pk,
        password_field,
        last_login_unix,
        email,
        ts,
    );
    if !constant_time_eq(&expected, token) {
        return false;
    }
    let age = token_seconds(now_unix) as i64 - ts as i64;
    age <= PASSWORD_RESET_TIMEOUT_SECS
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// uidb64 codec (`urlsafe_base64_encode(smart_bytes(user.id))` /
// `smart_str(urlsafe_base64_decode(uidb64))`)
// ---------------------------------------------------------------------------

/// Encode a user pk (`urlsafe_b64encode(str(pk)).rstrip(b"=")`).
pub fn uidb64_encode(user_pk: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(user_pk.as_bytes())
}

/// Why a uidb64 could not be decoded to a pk string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UidDecodeError {
    /// Bad padding / undecodable (`binascii.Error`, a `ValueError`).
    BadEncoding,
    /// Decoded bytes are not UTF-8 (`DjangoUnicodeDecodeError`, also a
    /// `ValueError` subclass — which is why the app inner `except`
    /// swallows it and the app EXPIRED branch is dead).
    BadUtf8,
}

/// Decode a uidb64 to the pk string (`urlsafe_b64decode` /
/// `smart_str`). `binascii.a2b_base64` ignores non-alphabet bytes, so
/// they are filtered before the length check; only a `% 4 == 1`
/// remainder (after filtering) fails the decode. Like
/// `urlsafe_b64decode`, the URL-safe pair translates to the standard
/// pair (`-` → `+`, `_` → `/`) before the standard-alphabet decode, so
/// `+`/`/` in the input decode rather than error.
pub fn uidb64_decode(uidb64: &str) -> Result<String, UidDecodeError> {
    let filtered: String = uidb64
        .bytes()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'=' | b'+' | b'/'))
        .map(|b| match b {
            b'-' => '+',
            b'_' => '/',
            _ => b as char,
        })
        .collect();
    if filtered.len() % 4 == 1 {
        return Err(UidDecodeError::BadEncoding);
    }
    let padded = match filtered.len() % 4 {
        0 => filtered,
        n => filtered + &"=".repeat(4 - n),
    };
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(padded.as_bytes())
        .map_err(|_| UidDecodeError::BadEncoding)?;
    String::from_utf8(bytes).map_err(|_| UidDecodeError::BadUtf8)
}

// ---------------------------------------------------------------------------
// Reset redirect locations (byte-exact `Location` values)
// ---------------------------------------------------------------------------

/// App error target: `urljoin(app_base, "accounts/reset-password?" +
/// urlencode(params))` (`app/password_management.py:112-116,125-129,
/// 138-142,151-155,172-176`). Bases are origins, so this is
/// `{base}/accounts/reset-password?error_code=..&error_message=..`.
pub fn reset_app_error_location(base: &str, code: i32, message: &str) -> String {
    format!(
        "{}/accounts/reset-password?error_code={code}&error_message={}",
        base.trim_end_matches('/'),
        quote_plus(message),
    )
}

/// App success target: `urljoin(app_base, "sign-in?" +
/// urlencode({"success": True}))` (`:162-166`) — capital-T `True`
/// (ported quirk BUG-8).
pub fn reset_app_success_location(base: &str) -> String {
    format!("{}/sign-in?success=True", base.trim_end_matches('/'))
}

/// Space error target: `f"{space_base}/accounts/reset-password/?
/// {urlencode(params)}"` (`space/...:125,135,145,159`) — the space base
/// ends in `/`, so the target keeps the double slash (ported quirk).
pub fn reset_space_error_location(space_base: &str, code: i32, message: &str) -> String {
    format!(
        "{space_base}/accounts/reset-password/?error_code={code}&error_message={}",
        quote_plus(message),
    )
}

/// Space success target: the bare space base
/// (`HttpResponseRedirect(base_host(...))`, `:153`).
pub fn reset_space_success_location(space_base: &str) -> String {
    space_base.to_owned()
}

// ---------------------------------------------------------------------------
// Reset branch mapping (app vs space exception layout)
// ---------------------------------------------------------------------------

/// How `ResetPasswordEndpoint` (app) maps a failure to a redirect. The
/// inner `try` catches `(ValueError, User.DoesNotExist)` around both the
/// decode and the lookup (`:103-116`); `check_token` false (`:119-129`),
/// missing password (`:131-142`) and weak password (`:144-155`) each
/// redirect; the outer `except DjangoUnicodeDecodeError` (`:167-176`) is
/// dead because the inner `except` already catches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetAppFailure {
    InvalidToken,
    MissingPassword,
    TooWeak,
}

/// How `ResetPasswordSpaceEndpoint` (space) maps a failure. There is no
/// decode/lookup guard (`:115-116`): only `DjangoUnicodeDecodeError`
/// maps to a redirect (EXPIRED, `:154-160`); bad tokens, missing and weak
/// passwords redirect like the app twin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetSpaceFailure {
    InvalidToken,
    ExpiredToken,
    MissingPassword,
    TooWeak,
}

/// App failure to `(code, message)` redirect params.
pub fn reset_app_failure_params(failure: ResetAppFailure) -> (i32, &'static str) {
    match failure {
        ResetAppFailure::InvalidToken => (5125, "INVALID_PASSWORD_TOKEN"),
        ResetAppFailure::MissingPassword => (5020, "INVALID_PASSWORD"),
        ResetAppFailure::TooWeak => (5021, "PASSWORD_TOO_WEAK"),
    }
}

/// Space failure to `(code, message)` redirect params.
pub fn reset_space_failure_params(failure: ResetSpaceFailure) -> (i32, &'static str) {
    match failure {
        ResetSpaceFailure::InvalidToken => (5125, "INVALID_PASSWORD_TOKEN"),
        ResetSpaceFailure::ExpiredToken => (5130, "EXPIRED_PASSWORD_TOKEN"),
        ResetSpaceFailure::MissingPassword => (5020, "INVALID_PASSWORD"),
        ResetSpaceFailure::TooWeak => (5021, "PASSWORD_TOO_WEAK"),
    }
}

// ---------------------------------------------------------------------------
// `UserSerializer` body (`app/serializers/user.py:15-60`)
// ---------------------------------------------------------------------------

/// DRF `DateTimeField` rendering for a UTC instant (`iso-8601`): `Z`
/// suffix, microseconds iff nonzero. `None` renders as JSON null (the
/// caller maps `None` before calling).
pub fn drf_datetime(unix_secs: i64, micros: u32) -> String {
    let secs = unix_secs.max(0) as u64;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    if micros == 0 {
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
            rem / 3600,
            (rem % 3600) / 60,
            rem % 60
        )
    } else {
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:06}Z",
            rem / 3600,
            (rem % 3600) / 60,
            rem % 60,
            micros % 1_000_000,
        )
    }
}

/// One `users` row for the set-password response, in `User._meta` field
/// order minus `password` ([`super::shapes::USER_SERIALIZER_FIELDS`]).
/// UUIDs render as strings, datetimes via [`drf_datetime`], FKs as the
/// referenced pk or null — exactly DRF `ModelSerializer` rendering for
/// this all-read row (no `validate_*` runs on output).
#[derive(Debug, Clone, PartialEq)]
pub struct UserSnapshot {
    pub last_login: Option<(i64, u32)>,
    pub id: String,
    pub username: String,
    pub mobile_number: Option<String>,
    pub email: Option<String>,
    pub display_name: String,
    pub first_name: String,
    pub last_name: String,
    pub avatar: String,
    pub avatar_asset: Option<String>,
    pub cover_image: Option<String>,
    pub cover_image_asset: Option<String>,
    pub date_joined: (i64, u32),
    pub created_at: (i64, u32),
    pub updated_at: (i64, u32),
    pub last_location: String,
    pub created_location: String,
    pub is_superuser: bool,
    pub is_managed: bool,
    pub is_password_expired: bool,
    pub is_active: bool,
    pub is_staff: bool,
    pub is_email_verified: bool,
    pub is_password_autoset: bool,
    pub is_password_reset_required: bool,
    pub token: String,
    pub last_active: Option<(i64, u32)>,
    pub last_login_time: Option<(i64, u32)>,
    pub last_logout_time: Option<(i64, u32)>,
    pub last_login_ip: String,
    pub last_logout_ip: String,
    pub last_login_medium: String,
    pub last_login_uagent: String,
    pub token_updated_at: Option<(i64, u32)>,
    pub is_bot: bool,
    pub bot_type: Option<String>,
    pub user_timezone: String,
    pub is_email_valid: bool,
    pub masked_at: Option<(i64, u32)>,
}

fn opt_str(value: &Option<String>) -> Value {
    match value {
        Some(s) => Value::String(s.clone()),
        None => Value::Null,
    }
}

fn opt_dt(value: &Option<(i64, u32)>) -> Value {
    match value {
        Some((secs, micros)) => Value::String(drf_datetime(*secs, *micros)),
        None => Value::Null,
    }
}

/// `UserSerializer(user).data` as ordered pairs in
/// `USER_SERIALIZER_FIELDS` order. The HTTP shell renders these through
/// its order-preserving JSON layer (this crate's `serde_json` has no
/// `preserve_order`, so a `Value::Object` here would sort keys and
/// break DRF byte parity).
pub fn user_serializer_pairs(user: &UserSnapshot) -> Vec<(String, Value)> {
    let dt = |(secs, micros): (i64, u32)| Value::String(drf_datetime(secs, micros));
    vec![
        ("last_login".to_owned(), opt_dt(&user.last_login)),
        ("id".to_owned(), Value::String(user.id.clone())),
        ("username".to_owned(), Value::String(user.username.clone())),
        ("mobile_number".to_owned(), opt_str(&user.mobile_number)),
        ("email".to_owned(), opt_str(&user.email)),
        (
            "display_name".to_owned(),
            Value::String(user.display_name.clone()),
        ),
        (
            "first_name".to_owned(),
            Value::String(user.first_name.clone()),
        ),
        (
            "last_name".to_owned(),
            Value::String(user.last_name.clone()),
        ),
        ("avatar".to_owned(), Value::String(user.avatar.clone())),
        ("avatar_asset".to_owned(), opt_str(&user.avatar_asset)),
        ("cover_image".to_owned(), opt_str(&user.cover_image)),
        (
            "cover_image_asset".to_owned(),
            opt_str(&user.cover_image_asset),
        ),
        ("date_joined".to_owned(), dt(user.date_joined)),
        ("created_at".to_owned(), dt(user.created_at)),
        ("updated_at".to_owned(), dt(user.updated_at)),
        (
            "last_location".to_owned(),
            Value::String(user.last_location.clone()),
        ),
        (
            "created_location".to_owned(),
            Value::String(user.created_location.clone()),
        ),
        ("is_superuser".to_owned(), Value::Bool(user.is_superuser)),
        ("is_managed".to_owned(), Value::Bool(user.is_managed)),
        (
            "is_password_expired".to_owned(),
            Value::Bool(user.is_password_expired),
        ),
        ("is_active".to_owned(), Value::Bool(user.is_active)),
        ("is_staff".to_owned(), Value::Bool(user.is_staff)),
        (
            "is_email_verified".to_owned(),
            Value::Bool(user.is_email_verified),
        ),
        (
            "is_password_autoset".to_owned(),
            Value::Bool(user.is_password_autoset),
        ),
        (
            "is_password_reset_required".to_owned(),
            Value::Bool(user.is_password_reset_required),
        ),
        ("token".to_owned(), Value::String(user.token.clone())),
        ("last_active".to_owned(), opt_dt(&user.last_active)),
        ("last_login_time".to_owned(), opt_dt(&user.last_login_time)),
        (
            "last_logout_time".to_owned(),
            opt_dt(&user.last_logout_time),
        ),
        (
            "last_login_ip".to_owned(),
            Value::String(user.last_login_ip.clone()),
        ),
        (
            "last_logout_ip".to_owned(),
            Value::String(user.last_logout_ip.clone()),
        ),
        (
            "last_login_medium".to_owned(),
            Value::String(user.last_login_medium.clone()),
        ),
        (
            "last_login_uagent".to_owned(),
            Value::String(user.last_login_uagent.clone()),
        ),
        (
            "token_updated_at".to_owned(),
            opt_dt(&user.token_updated_at),
        ),
        ("is_bot".to_owned(), Value::Bool(user.is_bot)),
        ("bot_type".to_owned(), opt_str(&user.bot_type)),
        (
            "user_timezone".to_owned(),
            Value::String(user.user_timezone.clone()),
        ),
        (
            "is_email_valid".to_owned(),
            Value::Bool(user.is_email_valid),
        ),
        ("masked_at".to_owned(), opt_dt(&user.masked_at)),
    ]
}

// ---------------------------------------------------------------------------
// CSRF cookie (`get_token` response cookie)
// ---------------------------------------------------------------------------

/// `CSRF_COOKIE_AGE` default (`django/middleware/csrf.py`): one year.
pub const CSRF_COOKIE_AGE_SECS: i64 = 31_449_600;

/// `Set-Cookie` value for the `csrftoken` cookie after
/// `CSRFTokenEndpoint.get` (or a `login()` rotation): Django `set_cookie`
/// field order (`expires`, `Max-Age`, `Path`, then flags), `HttpOnly`
/// forced on by this project (`settings/common.py:611`), `SameSite=Lax`.
pub fn csrf_set_cookie_value(secret: &str, expires_http_date: &str, secure: bool) -> String {
    let mut out = format!(
        "csrftoken={secret}; expires={expires_http_date}; Max-Age={CSRF_COOKIE_AGE_SECS}; Path=/"
    );
    if secure {
        out.push_str("; Secure");
    }
    out.push_str("; HttpOnly; SameSite=Lax");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_session::tasks::ForgotPasswordDecision;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/auth_session/FX-AUTH-08.handlers_magic_password.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn change_password_branches_match_fixture() {
        let fx = fixture();
        let change = fx.get("change_password").expect("change_password");
        // Order: missing-old, wrong-old, weak-new, missing-new, ok.
        assert_eq!(
            decide_change_password(false, false, true, false, false),
            ChangePasswordOutcome::MissingOld
        );
        assert_eq!(
            change_password_error_json(ChangePasswordOutcome::MissingOld),
            serde_json::to_string(&change["missing_old"]["body"]).expect("json"),
        );
        assert_eq!(
            decide_change_password(false, true, true, false, false),
            ChangePasswordOutcome::WrongOld
        );
        assert_eq!(
            change_password_error_json(ChangePasswordOutcome::WrongOld),
            serde_json::to_string(&change["wrong_old"]["body"]).expect("json"),
        );
        assert_eq!(
            decide_change_password(false, true, true, true, true),
            ChangePasswordOutcome::TooWeak
        );
        assert_eq!(
            change_password_error_json(ChangePasswordOutcome::TooWeak),
            serde_json::to_string(&change["weak_new"]["body"]).expect("json"),
        );
        assert_eq!(
            decide_change_password(false, true, false, true, false),
            ChangePasswordOutcome::MissingNew
        );
        assert_eq!(
            change_password_error_json(ChangePasswordOutcome::MissingNew),
            serde_json::to_string(&change["missing_new"]["body"]).expect("json"),
        );
        // Autoset users skip the old-password check entirely.
        assert_eq!(
            decide_change_password(true, false, true, false, false),
            ChangePasswordOutcome::Ok
        );
        assert_eq!(
            CHANGE_PASSWORD_SUCCESS_BODY,
            serde_json::to_string(&change["ok"]["body"]).expect("json"),
        );
    }

    #[test]
    fn set_password_branches_match_fixture() {
        let fx = fixture();
        let set = fx.get("set_password").expect("set_password");
        assert_eq!(
            decide_set_password(false, true, false),
            SetPasswordOutcome::AlreadySet
        );
        assert_eq!(
            set_password_error_json(SetPasswordOutcome::AlreadySet),
            serde_json::to_string(&set["already_set"]["body"]).expect("json"),
        );
        // Missing and weak both answer INVALID_PASSWORD (5020), never 5021.
        assert_eq!(
            decide_set_password(true, false, false),
            SetPasswordOutcome::Invalid
        );
        assert_eq!(
            set_password_error_json(SetPasswordOutcome::Invalid),
            serde_json::to_string(&set["invalid"]["body"]).expect("json"),
        );
        assert_eq!(
            decide_set_password(true, true, true),
            SetPasswordOutcome::Invalid
        );
        assert_eq!(
            set_password_error_json(SetPasswordOutcome::Invalid),
            serde_json::to_string(&set["weak"]["body"]).expect("json"),
        );
        assert_eq!(
            decide_set_password(true, true, false),
            SetPasswordOutcome::Ok
        );
    }

    #[test]
    fn forgot_password_bodies_match_fixture() {
        let fx = fixture();
        let forgot = fx.get("forgot").expect("forgot");
        assert_eq!(
            FORGOT_PASSWORD_SUCCESS_BODY,
            serde_json::to_string(&forgot["app_ok"]["body"]).expect("json"),
        );
        assert_eq!(
            forgot_password_error_json(ForgotPasswordDecision::UserDoesNotExist),
            serde_json::to_string(&forgot["app_no_user"]["body"]).expect("json"),
        );
        assert_eq!(
            forgot_password_error_json(ForgotPasswordDecision::InvalidEmail),
            serde_json::to_string(&forgot["app_invalid"]["body"]).expect("json"),
        );
        assert_eq!(
            forgot_password_error_json(ForgotPasswordDecision::Emit),
            "{}",
        );
        // Gate order: instance, SMTP, email, user (kernel in tasks.rs).
        assert_eq!(
            super::super::tasks::forgot_password_decision(false, true, true, true),
            ForgotPasswordDecision::InstanceNotConfigured
        );
        assert_eq!(
            super::super::tasks::forgot_password_decision(true, false, true, true),
            ForgotPasswordDecision::SmtpNotConfigured
        );
    }

    #[test]
    fn reset_redirects_match_fixture() {
        let fx = fixture();
        let app_base = "http://localhost:3000";
        let space_base = "http://localhost:8000/spaces/";
        let app = fx.get("reset_app").expect("reset_app");
        assert_eq!(
            reset_app_success_location(app_base),
            app["ok"]["location"].as_str().expect("str"),
        );
        for (key, failure) in [
            ("bad_token", ResetAppFailure::InvalidToken),
            ("missing_password", ResetAppFailure::MissingPassword),
            ("weak_password", ResetAppFailure::TooWeak),
        ] {
            let (code, message) = reset_app_failure_params(failure);
            assert_eq!(
                reset_app_error_location(app_base, code, message),
                app[key]["location"].as_str().expect("str"),
                "{key}",
            );
        }
        let space = fx.get("reset_space").expect("reset_space");
        let (code, message) = reset_space_failure_params(ResetSpaceFailure::InvalidToken);
        assert_eq!(
            reset_space_error_location(space_base, code, message),
            space["bad_token"]["location"].as_str().expect("str"),
        );
        // Space success is the bare base (trailing slash kept).
        assert_eq!(
            reset_space_success_location(space_base),
            space_base.to_string()
        );
        // Space EXPIRED target keeps the double slash.
        let (code, message) = reset_space_failure_params(ResetSpaceFailure::ExpiredToken);
        let location = reset_space_error_location(space_base, code, message);
        assert!(location.starts_with(&format!("{space_base}/accounts/reset-password/")));
        assert!(location.contains("error_code=5130"));
    }

    #[test]
    fn token_recipe_matches_django_vectors() {
        // Golden vector minted by Django 4.2's own
        // `PasswordResetTokenGenerator` (frozen `_now`, SECRET_KEY
        // `test-secret-key`): real-Django output, not a reimplementation
        // echo. The uidb64 equals the FX-AUTH-08 `token_pair_sample`
        // value byte-for-byte.
        let pk = "f13f9dd9-29f5-4607-8281-502aab1a120b";
        assert_eq!(
            uidb64_encode(pk),
            "ZjEzZjlkZDktMjlmNS00NjA3LTgyODEtNTAyYWFiMWExMjBi"
        );
        assert_eq!(uidb64_decode(&uidb64_encode(pk)).expect("decodes"), pk);
        assert_eq!(base36_encode(0), "0");
        assert_eq!(base36_encode(35), "z");
        assert_eq!(base36_encode(36), "10");
        assert_eq!(base36_decode("dfndxq"), Some(812_345_678));
        assert_eq!(
            make_token_with_timestamp(
                "test-secret-key",
                pk,
                "pbkdf2_sha256$600000$abc$def",
                None,
                "h@x.com",
                812_345_678,
            ),
            "dfndxq-2fe902a40075a42302fbf5f7fd630cbd"
        );
        // Round trip: mint then check, with and without last_login.
        let now = TOKEN_EPOCH_OFFSET_SECS + 812_345_678;
        let (uid, token) = make_password_token(
            "test-secret-key",
            pk,
            "pbkdf2_sha256$600000$abc$def",
            None,
            "h@x.com",
            now,
        );
        assert_eq!(uidb64_decode(&uid).expect("uid"), pk);
        assert!(check_password_token(
            "test-secret-key",
            pk,
            "pbkdf2_sha256$600000$abc$def",
            None,
            "h@x.com",
            &token,
            now,
        ));
        // Wrong secret, wrong password field, tampered hash, bad shape,
        // and expired tokens all fail.
        assert!(!check_password_token(
            "other-secret",
            pk,
            "pbkdf2_sha256$600000$abc$def",
            None,
            "h@x.com",
            &token,
            now,
        ));
        assert!(!check_password_token(
            "test-secret-key",
            pk,
            "pbkdf2_sha256$600000$changed",
            None,
            "h@x.com",
            &token,
            now,
        ));
        assert!(!check_password_token(
            "test-secret-key",
            pk,
            "pbkdf2_sha256$600000$abc$def",
            None,
            "h@x.com",
            "bogus-token",
            now,
        ));
        assert!(!check_password_token(
            "test-secret-key",
            pk,
            "pbkdf2_sha256$600000$abc$def",
            None,
            "h@x.com",
            &token,
            now + PASSWORD_RESET_TIMEOUT_SECS + 1,
        ));
        // last_login participates in the hash (None vs set differ).
        let with_login = make_token_with_timestamp(
            "test-secret-key",
            pk,
            "pbkdf2_sha256$600000$abc$def",
            Some(1_700_000_000),
            "h@x.com",
            812_345_678,
        );
        assert_ne!(with_login, "dfndxq-2fe902a40075a42302fbf5f7fd630cbd");
        assert_eq!(login_timestamp_str(None), "");
        assert_eq!(
            login_timestamp_str(Some(1_700_000_000)),
            "2023-11-14 22:13:20"
        );
        // Decode errors: bad padding vs bad UTF-8 (the app/space split).
        assert_eq!(uidb64_decode("a"), Err(UidDecodeError::BadEncoding));
        assert_eq!(uidb64_decode("__4AYmFk"), Err(UidDecodeError::BadUtf8),);
        assert_eq!(uidb64_decode("bm90LWEtdXVpZA"), Ok("not-a-uuid".to_owned()),);
    }

    #[test]
    fn user_serializer_keys_match_fixture() {
        let fx = fixture();
        let keys: Vec<String> = fx
            .get("set_password")
            .expect("sp")
            .get("ok_keys")
            .expect("keys")
            .get("keys")
            .expect("list")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let user = UserSnapshot {
            last_login: None,
            id: "11111111-1111-1111-1111-111111111111".to_owned(),
            username: "u".to_owned(),
            mobile_number: None,
            email: Some("u@x.com".to_owned()),
            display_name: String::new(),
            first_name: String::new(),
            last_name: String::new(),
            avatar: String::new(),
            avatar_asset: None,
            cover_image: None,
            cover_image_asset: None,
            date_joined: (1_700_000_000, 0),
            created_at: (1_700_000_000, 123_456),
            updated_at: (1_700_000_001, 0),
            last_location: String::new(),
            created_location: String::new(),
            is_superuser: false,
            is_managed: false,
            is_password_expired: false,
            is_active: true,
            is_staff: false,
            is_email_verified: false,
            is_password_autoset: false,
            is_password_reset_required: false,
            token: String::new(),
            last_active: None,
            last_login_time: None,
            last_logout_time: None,
            last_login_ip: String::new(),
            last_logout_ip: String::new(),
            last_login_medium: "email".to_owned(),
            last_login_uagent: String::new(),
            token_updated_at: None,
            is_bot: false,
            bot_type: None,
            user_timezone: "UTC".to_owned(),
            is_email_valid: false,
            masked_at: None,
        };
        let pairs = user_serializer_pairs(&user);
        let names: Vec<String> = pairs.iter().map(|(k, _)| k.clone()).collect();
        // Same key set as the fixture (fixture lists them alphabetically;
        // the pairs keep `USER_SERIALIZER_FIELDS` model order).
        let mut a = names.clone();
        let mut b = keys.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b);
        assert_eq!(
            names,
            crate::auth_session::shapes::USER_SERIALIZER_FIELDS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        );
        let by_name = |k: &str| {
            pairs
                .iter()
                .find(|(name, _)| name == k)
                .expect("key")
                .1
                .clone()
        };
        assert_eq!(
            by_name("id"),
            Value::from("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(by_name("email"), Value::from("u@x.com"));
        assert_eq!(by_name("is_password_autoset"), Value::Bool(false));
        assert_eq!(by_name("mobile_number"), Value::Null);
        assert_eq!(by_name("avatar_asset"), Value::Null);
        assert_eq!(by_name("date_joined"), Value::from("2023-11-14T22:13:20Z"));
        // Microseconds render iff nonzero (DRF `isoformat` parity).
        assert_eq!(
            by_name("created_at"),
            Value::from("2023-11-14T22:13:20.123456Z")
        );
        assert_eq!(drf_datetime(1_700_000_000, 0), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn csrf_cookie_and_scoring_match_contract() {
        let fx = fixture();
        let csrf = fx.get("csrf_token").expect("csrf");
        assert_eq!(csrf["status"], 200);
        assert_eq!(csrf["len"], 64);
        // Estimator agreement on the contract passwords (Python zxcvbn
        // 4.4.28 prints 0 and 4 for these; the Rust crate must agree or
        // the branch mapping diverges).
        assert!(password_is_weak("123"));
        assert_eq!(password_score("123"), 0);
        assert!(!password_is_weak("Contract-Strong-Pass-2!y"));
        assert!(!password_is_weak("Contract-Strong-Pass-1!x"));
        // Salt shape: 22 alphanumerics like `get_random_string(22)`.
        let salt = generate_password_salt();
        assert_eq!(salt.len(), 22);
        assert!(salt.bytes().all(|b| b.is_ascii_alphanumeric()));
        // Cookie value shape (Django `set_cookie` field order).
        let cookie = csrf_set_cookie_value(
            "abcdefghijABCDEFGHIJ0123456789ab",
            "Wed, 30 Sep 2026 12:00:00 GMT",
            false,
        );
        assert_eq!(
            cookie,
            "csrftoken=abcdefghijABCDEFGHIJ0123456789ab; expires=Wed, 30 Sep 2026 12:00:00 GMT; Max-Age=31449600; Path=/; HttpOnly; SameSite=Lax"
        );
    }
}
