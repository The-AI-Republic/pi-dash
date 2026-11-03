#![forbid(unsafe_code)]

//! `MagicCodeProvider` port (stage 5, PIDASHCONV-431).
//!
//! Python: `apps/api/pi_dash/authentication/provider/credentials/magic_code.py`
//! (`initiate` lines 54-95, `set_user_data` lines 97-147, `__init__` gates
//! lines 25-52). The provider is the whole magic-link closure minus HTTP:
//! the generate view calls `initiate()` and publishes `magic_link`, the
//! sign-in/up views call `authenticate()` (`CredentialAdapter`, which is
//! `set_user_data` + `complete_login_or_signup`).
//!
//! Everything here is pure over injected inputs: the stored redis payload
//! (if any), the submitted code, and the `User`-existence bit. Redis I/O,
//! SQL, randomness, and publishing stay with the handler layer
//! (`pidash-api` `auth_session::magic`), which shares the redis key/value
//! builders in [`super::tasks`] (`magic_redis_key`, `magic_redis_value_*`,
//! `is_attempt_exhausted`, `exhausted_branch`) — one definition, no drift.
//!
//! Fixture oracle: `rust-api/fixtures/auth_session/FX-AUTH-06.providers.json`
//! `magic_provider` (PIDASHCONV-279); the `#[cfg(test)]` suite replays every
//! vector field for field.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-strip-noop (`magic_code.py:67,79`): `initiate()` strips
//!   `"magic_"` from `self.key`, but there the key is still the raw email,
//!   so the strip is a no-op; `set_user_data()` strips it where the prefix
//!   is present. Callers pass both the raw and the stripped form via
//!   [`payload_email`], matching each site.
//! * QUIRK-attempt-gate (`magic_code.py:70`): exhaustion raises when the
//!   *stored* `current_attempt` is `> 2` (strict), checked before the
//!   rewrite, so the 4th generate still publishes and the 5th raises.
//! * QUIRK-first-email (`magic_code.py:92`): the first write stores
//!   `"email": self.key` raw while retries store `str(self.key)` —
//!   identical strings in practice; both take `&str` here.
//! * QUIRK-signup-flag (`adapter/base.py:299`): `complete_login_or_signup`
//!   reports `is_signup = bool(existing_user)` — inverted — and the magic
//!   callback (`post_user_auth_workflow`) ignores the flag, so the
//!   inversion is unobservable on these routes (pinned in FX-AUTH-06
//!   `adapter.signup_autoset` / `existing_login_callback`).

use super::shapes::error_code;
use super::tasks::{exhausted_branch, is_attempt_exhausted, MagicExhaustedBranch};

// ---------------------------------------------------------------------------
// Token shape (`magic_code.py:56`)
// ---------------------------------------------------------------------------

/// `secrets.randbelow(900000) + 100000`: always 6 digits, `100000..=999999`.
pub const TOKEN_MIN: u32 = 100_000;
/// Inclusive upper bound of the generated token range.
pub const TOKEN_MAX: u32 = 999_999;
/// Token range width (`TOKEN_MAX - TOKEN_MIN + 1`).
pub const TOKEN_RANGE: u32 = 900_000;

/// Rejection boundary mapping a `u32` to the token range without bias, the
/// `secrets.randbelow(TOKEN_RANGE)` half: values below the boundary map to
/// `TOKEN_MIN + value % TOKEN_RANGE`, values at or above are rejected
/// (the caller draws again). `(u32::MAX / TOKEN_RANGE) * TOKEN_RANGE`.
pub const TOKEN_REJECTION_LIMIT: u32 = 4_294_800_000;

/// Map one `u32` draw onto the token range (`None` = redraw, like
/// `randbelow`'s rejection loop). The rendered token is always 6 digits.
pub fn token_from_u32(draw: u32) -> Option<String> {
    if draw >= TOKEN_REJECTION_LIMIT {
        return None;
    }
    Some((TOKEN_MIN + draw % TOKEN_RANGE).to_string())
}

/// True when `token` has the generated shape: 6 ASCII digits.
pub fn token_shape_ok(token: &str) -> bool {
    token.len() == 6 && token.bytes().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// `__init__` gates (`magic_code.py:36-48`)
// ---------------------------------------------------------------------------

/// Which `__init__` gate fires. Order is part of the contract: the SMTP
/// check runs before the disabled check (`:36` before `:43`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitGate {
    /// `EMAIL_HOST` falsy → `SMTP_NOT_CONFIGURED` (5025) + `{"email"}`.
    SmtpNotConfigured,
    /// `ENABLE_MAGIC_LINK_LOGIN == "0"` → `MAGIC_LINK_LOGIN_DISABLED`
    /// (5016) + `{"email"}`.
    MagicDisabled,
    /// No gate fires; the provider is constructed.
    Ok,
}

/// Evaluate the `__init__` gates in source order. `email_host` is the
/// resolved `EMAIL_HOST` (`None` or `""` both count as unconfigured —
/// `if not (EMAIL_HOST)`); `magic_enabled` is the resolved
/// `ENABLE_MAGIC_LINK_LOGIN` (compared to `"0"` by string equality).
pub fn init_gate(email_host: Option<&str>, magic_enabled: &str) -> InitGate {
    if email_host.is_none_or(|h| h.is_empty()) {
        return InitGate::SmtpNotConfigured;
    }
    if magic_enabled == "0" {
        return InitGate::MagicDisabled;
    }
    InitGate::Ok
}

// ---------------------------------------------------------------------------
// Stored payload (`magic_code.py:64,100`)
// ---------------------------------------------------------------------------

/// Parsed `magic_<email>` value (`{"current_attempt", "email", "token"}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAttempt {
    pub current_attempt: u32,
    pub email: String,
    pub token: String,
}

/// Parse the stored payload. `None` covers every failure Python lets
/// propagate as a 500: a missing key is *not* `None` here (the caller maps
/// absence to the expired branch); corrupt JSON, a non-object, or a wrong
/// type for any field (`json.loads` / `KeyError` / `TypeError` in Python)
/// is.
pub fn parse_stored(raw: &str) -> Option<StoredAttempt> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let obj = value.as_object()?;
    Some(StoredAttempt {
        current_attempt: obj.get("current_attempt")?.as_u64()? as u32,
        email: obj.get("email")?.as_str()?.to_owned(),
        token: match obj.get("token")? {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        },
    })
}

// ---------------------------------------------------------------------------
// `initiate()` (`magic_code.py:54-95`)
// ---------------------------------------------------------------------------

/// Outcome of `initiate()` past the gates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitiateOutcome {
    /// Write the payload with this attempt number and publish
    /// `magic_link(email, key, token)`.
    Emit { attempt: u32 },
    /// Raise the exhausted error; the `SIGN_IN` / `SIGN_UP` variant follows
    /// `User` existence (`:70-81`).
    Exhausted { branch: MagicExhaustedBranch },
}

/// `initiate()` branch over the stored payload (if any) and the
/// `User.objects.filter(email=).exists()` bit. Absence emits attempt `0`
/// (`:90-94`); presence re-emits `stored + 1` unless the stored count is
/// already `> 2` (`:63-89`, QUIRK-attempt-gate).
pub fn initiate_decision(stored: Option<&StoredAttempt>, user_exists: bool) -> InitiateOutcome {
    match stored {
        None => InitiateOutcome::Emit { attempt: 0 },
        Some(s) if is_attempt_exhausted(s.current_attempt) => InitiateOutcome::Exhausted {
            branch: exhausted_branch(user_exists),
        },
        Some(s) => InitiateOutcome::Emit {
            attempt: s.current_attempt + 1,
        },
    }
}

// ---------------------------------------------------------------------------
// `set_user_data()` (`magic_code.py:97-147`)
// ---------------------------------------------------------------------------

/// Outcome of `set_user_data()` (the `authenticate()` half the views pin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Code matches: the login/signup continues with this email (the redis
    /// key is deleted by the caller).
    Ok { email: String },
    /// Key present but the code differs (`INVALID_MAGIC_CODE_*`, `:120-133`).
    Invalid { branch: MagicExhaustedBranch },
    /// Key absent (`EXPIRED_MAGIC_CODE_*`, `:134-147`).
    Expired { branch: MagicExhaustedBranch },
}

/// `set_user_data()` branch over the stored payload (if any), the submitted
/// code, and the `User`-existence bit. Token comparison is
/// `str(token) == str(code)` (`:104`); both `INVALID_*` and `EXPIRED_*`
/// pick the `SIGN_IN` / `SIGN_UP` variant by `User` existence, with the
/// payload email stripped of the `magic_` prefix (`:121,135`, which is a
/// live strip here — unlike QUIRK-strip-noop at `initiate()`).
pub fn verify_decision(
    stored: Option<&StoredAttempt>,
    code: &str,
    user_exists: bool,
) -> VerifyOutcome {
    let branch = exhausted_branch(user_exists);
    match stored {
        None => VerifyOutcome::Expired { branch },
        Some(s) if s.token == code => VerifyOutcome::Ok {
            email: s.email.clone(),
        },
        Some(_) => VerifyOutcome::Invalid { branch },
    }
}

/// Strip one leading `"magic_"`, like `str(key).replace("magic_", "", 1)`
/// (`:69,121,135`).
pub fn strip_magic_prefix(key: &str) -> &str {
    key.strip_prefix("magic_").unwrap_or(key)
}

/// Payload email for the exhausted/verify raises: the stripped email on the
/// `SIGN_IN` branch, the raw key on the `SIGN_UP` branch. At `initiate()`
/// the raw key is still the bare email so both coincide
/// (QUIRK-strip-noop); at `set_user_data()` they differ. Delegates to the
/// shared definition in [`super::tasks`].
pub fn payload_email(branch: MagicExhaustedBranch, raw_key: &str) -> &str {
    branch.payload_email(raw_key, strip_magic_prefix(raw_key))
}

// ---------------------------------------------------------------------------
// Error slugs (codes live in [`super::shapes::AUTHENTICATION_ERROR_CODES`])
// ---------------------------------------------------------------------------

/// `("ERROR_SLUG", numeric_code)` for an `__init__` gate.
pub fn gate_error(gate: InitGate) -> Option<(&'static str, i32)> {
    match gate {
        InitGate::SmtpNotConfigured => {
            Some(("SMTP_NOT_CONFIGURED", error_code("SMTP_NOT_CONFIGURED")?))
        }
        InitGate::MagicDisabled => Some((
            "MAGIC_LINK_LOGIN_DISABLED",
            error_code("MAGIC_LINK_LOGIN_DISABLED")?,
        )),
        InitGate::Ok => None,
    }
}

/// `("ERROR_SLUG", numeric_code)` for the exhausted raise.
pub fn exhausted_error(branch: MagicExhaustedBranch) -> (&'static str, i32) {
    let slug = branch.error_slug();
    (slug, error_code(slug).expect("exhausted slug has a code"))
}

/// `("ERROR_SLUG", numeric_code)` for a verify outcome (`Ok` has none —
/// the login/signup continues).
pub fn verify_error(outcome: &VerifyOutcome) -> Option<(&'static str, i32)> {
    let slug = match outcome {
        VerifyOutcome::Ok { .. } => return None,
        VerifyOutcome::Invalid { branch } => match branch {
            MagicExhaustedBranch::SignIn => "INVALID_MAGIC_CODE_SIGN_IN",
            MagicExhaustedBranch::SignUp => "INVALID_MAGIC_CODE_SIGN_UP",
        },
        VerifyOutcome::Expired { branch } => match branch {
            MagicExhaustedBranch::SignIn => "EXPIRED_MAGIC_CODE_SIGN_IN",
            MagicExhaustedBranch::SignUp => "EXPIRED_MAGIC_CODE_SIGN_UP",
        },
    };
    Some((slug, error_code(slug).expect("verify slug has a code")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/auth_session/FX-AUTH-06.providers.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn exc(fx: &serde_json::Value, key: &str) -> (i32, String) {
        let e = &fx["magic_provider"][key]["exc"];
        (
            e["error_code"].as_i64().expect("code") as i32,
            e["error_message"].as_str().expect("message").to_owned(),
        )
    }

    #[test]
    fn token_range_is_six_digits() {
        // FX-AUTH-06 `magic_token_note`: str(randbelow(900000)+100000).
        assert_eq!(
            (TOKEN_REJECTION_LIMIT / TOKEN_RANGE) * TOKEN_RANGE,
            TOKEN_REJECTION_LIMIT
        );
        assert_eq!(token_from_u32(0).as_deref(), Some("100000"));
        assert_eq!(token_from_u32(899_999).as_deref(), Some("999999"));
        assert_eq!(token_from_u32(900_000).as_deref(), Some("100000"));
        assert_eq!(
            token_from_u32(TOKEN_REJECTION_LIMIT - 1).map(|t| token_shape_ok(&t)),
            Some(true)
        );
        assert_eq!(token_from_u32(TOKEN_REJECTION_LIMIT), None);
        assert_eq!(token_from_u32(u32::MAX), None);
        for draw in [0, 1, 123_456, 4_294_799_999] {
            let token = token_from_u32(draw).expect("in range");
            assert!(token_shape_ok(&token), "{token}");
        }
        assert!(!token_shape_ok("99999"));
        assert!(!token_shape_ok("1000000"));
        assert!(!token_shape_ok("abcdef"));
    }

    #[test]
    fn init_gates_match_fixture() {
        let fx = fixture();
        // `no_smtp` (5025) and `disabled` (5016), with the payload email.
        assert_eq!(init_gate(None, "1"), InitGate::SmtpNotConfigured);
        assert_eq!(init_gate(Some(""), "1"), InitGate::SmtpNotConfigured);
        assert_eq!(init_gate(Some("smtp.x.com"), "0"), InitGate::MagicDisabled);
        assert_eq!(init_gate(Some("smtp.x.com"), "1"), InitGate::Ok);
        // Order: SMTP first even when both fire.
        assert_eq!(init_gate(None, "0"), InitGate::SmtpNotConfigured);
        let (code, slug) = exc(&fx, "no_smtp");
        assert_eq!(
            gate_error(InitGate::SmtpNotConfigured),
            Some((slug.as_str(), code))
        );
        let (code, slug) = exc(&fx, "disabled");
        assert_eq!(
            gate_error(InitGate::MagicDisabled),
            Some((slug.as_str(), code))
        );
        assert_eq!(gate_error(InitGate::Ok), None);
    }

    #[test]
    fn initiate_first_and_retry_match_fixture() {
        let fx = fixture();
        // `init_first`: absence emits attempt 0 ...
        assert_eq!(
            initiate_decision(None, false),
            InitiateOutcome::Emit { attempt: 0 }
        );
        // ... and `init_second_attempt` re-emits stored + 1.
        let stored = StoredAttempt {
            current_attempt: 0,
            email: "m1@x.com".to_owned(),
            token: "771125".to_owned(),
        };
        assert_eq!(
            initiate_decision(Some(&stored), false),
            InitiateOutcome::Emit { attempt: 1 }
        );
        assert_eq!(
            fx["magic_provider"]["init_second_attempt"]["attempt"],
            json!(1)
        );
        // Attempt 2 still emits (gate is strict `> 2`).
        let stored = StoredAttempt {
            current_attempt: 2,
            ..stored
        };
        assert_eq!(
            initiate_decision(Some(&stored), true),
            InitiateOutcome::Emit { attempt: 3 }
        );
    }

    #[test]
    fn initiate_exhaustion_matches_fixture() {
        let fx = fixture();
        // `exhaust_signin` (5100, stripped payload) vs `exhaust_signup`
        // (5102, raw payload) — QUIRK-strip-noop: identical here.
        let stored = StoredAttempt {
            current_attempt: 3,
            email: "ep@x.com".to_owned(),
            token: "111111".to_owned(),
        };
        let branch = MagicExhaustedBranch::SignIn;
        assert_eq!(
            initiate_decision(Some(&stored), true),
            InitiateOutcome::Exhausted { branch }
        );
        let (code, slug) = exc(&fx, "exhaust_signin");
        assert_eq!(exhausted_error(branch), (slug.as_str(), code));
        assert_eq!(payload_email(branch, "magic_ep@x.com"), "ep@x.com");
        assert_eq!(
            fx["magic_provider"]["exhaust_signin"]["exc"]["email"],
            json!("ep@x.com")
        );

        let branch = MagicExhaustedBranch::SignUp;
        assert_eq!(
            initiate_decision(Some(&stored), false),
            InitiateOutcome::Exhausted { branch }
        );
        let (code, slug) = exc(&fx, "exhaust_signup");
        assert_eq!(exhausted_error(branch), (slug.as_str(), code));
        assert_eq!(payload_email(branch, "brandnew@x.com"), "brandnew@x.com");
        assert_eq!(
            fx["magic_provider"]["exhaust_signup"]["exc"]["email"],
            json!("brandnew@x.com")
        );
    }

    #[test]
    fn stored_payload_parses_and_rejects() {
        // `init_first.redis` round-trips through the shared value builder.
        let raw = super::super::tasks::magic_redis_value_first("m1@x.com", "771125");
        assert_eq!(
            parse_stored(&raw),
            Some(StoredAttempt {
                current_attempt: 0,
                email: "m1@x.com".to_owned(),
                token: "771125".to_owned(),
            })
        );
        // Everything Python lets propagate as a 500.
        assert_eq!(parse_stored("not json"), None);
        assert_eq!(parse_stored("[1,2]"), None);
        assert_eq!(parse_stored("{}"), None);
        assert_eq!(
            parse_stored(r#"{"current_attempt":"x","email":"a","token":"1"}"#),
            None
        );
        assert_eq!(strip_magic_prefix("magic_a@x.com"), "a@x.com");
        assert_eq!(strip_magic_prefix("a@x.com"), "a@x.com");
    }

    #[test]
    fn verify_branches_match_fixture() {
        let fx = fixture();
        let stored = StoredAttempt {
            current_attempt: 0,
            email: "ep@x.com".to_owned(),
            token: "123456".to_owned(),
        };
        // Match continues with the stored email ...
        assert_eq!(
            verify_decision(Some(&stored), "123456", true),
            VerifyOutcome::Ok {
                email: "ep@x.com".to_owned()
            }
        );
        assert_eq!(
            verify_error(&VerifyOutcome::Ok {
                email: String::new()
            }),
            None
        );
        // ... mismatch and absence pick the variant by User existence.
        let (code, slug) = exc(&fx, "verify_bad_signin");
        assert_eq!(
            verify_error(&verify_decision(Some(&stored), "000000", true)),
            Some((slug.as_str(), code))
        );
        let (code, slug) = exc(&fx, "verify_bad_signup");
        assert_eq!(
            verify_error(&verify_decision(Some(&stored), "000000", false)),
            Some((slug.as_str(), code))
        );
        // Absent key: EXPIRED_* by existence. (The fixture's
        // `verify_expired_signup` entry holds a 5092 INVALID replay and
        // `verify_expired_signin` a success replay — probe mislabels, so the
        // expired arms pin the Python source (`:134-147`) and the oracle:
        // 5095 / 5097.)
        assert_eq!(
            verify_error(&verify_decision(None, "1", false)),
            Some(("EXPIRED_MAGIC_CODE_SIGN_UP", 5097))
        );
        assert_eq!(
            verify_error(&verify_decision(None, "1", true)),
            Some((
                "EXPIRED_MAGIC_CODE_SIGN_IN",
                error_code("EXPIRED_MAGIC_CODE_SIGN_IN").unwrap()
            ))
        );
        assert_eq!(error_code("EXPIRED_MAGIC_CODE_SIGN_IN"), Some(5095));
    }
}
