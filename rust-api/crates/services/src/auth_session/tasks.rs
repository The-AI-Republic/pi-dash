#![forbid(unsafe_code)]

//! Publish-side task emits for the D-16 authentication domain (stage 5,
//! PIDASHCONV-405).
//!
//! Ports the three `.delay(...)` touchpoints this domain owns, plus the
//! redis pre-state the magic publish depends on:
//!
//! * `magic_link.delay(email, key, token)`
//!   (`views/app/magic.py:54`, `views/space/magic.py:50`), fired after a
//!   successful `MagicCodeProvider.initiate()` (`provider/credentials/
//!   magic_code.py:54-95`).
//! * `user_activation_email.delay(current_site, user_id)`
//!   (`adapter/base.py:230`, inside `save_user_data`), fired only when the
//!   user `is_active` is still false.
//! * `forgot_password.delay(first_name, email, uidb64, token, current_site)`
//!   (`views/app/password_management.py:87`,
//!   `views/space/password_management.py:99`), fired only in the
//!   user-exists branch, after the `INSTANCE_NOT_CONFIGURED`,
//!   `SMTP_NOT_CONFIGURED` and `INVALID_EMAIL` gates.
//!
//! The task bodies are D-07 and already live in the jobs crate
//! (`jobs/src/tasks_mail/auth_mail.rs` for `magic_link` + `forgot_password`,
//! `jobs/src/tasks_mail/membership_mail.rs` for `user_activation_email`);
//! nothing here executes a task and no worker `Registry` entry is added.
//!
//! Fixture oracle: `rust-api/fixtures/auth_session/FX-AUTH-09.tasks.json`
//! (PIDASHCONV-279) pins the three wire names, the positional arg orders
//! and one observed vector each; the redis payload vectors come from
//! `FX-AUTH-06.providers.json` `magic_provider` (`init_first.redis`,
//! `init_second_attempt`, `exhaust_signin`, `exhaust_signup`). The tests
//! below replay every vector field for field.
//!
//! Wire contract (Porting guide Jobs plane, F-09): `.delay(*args)` is a
//! deferred publish of a first-attempt Celery protocol v2 message with the
//! call-site args positional, `kwargs = {}`, no countdown/eta (`eta` null,
//! `retries` 0, `timelimit` `[null, null]`). All three tasks are bare
//! `@shared_task` (`bgtasks/magic_link_code_task.py:23`,
//! `bgtasks/forgot_password_task.py:23`,
//! `bgtasks/user_activation_email_task.py:23`), so the wire names are the
//! dotted module paths in [`MAGIC_LINK_TASK`] / [`FORGOT_PASSWORD_TASK`] /
//! [`USER_ACTIVATION_EMAIL_TASK`] and there is no queue override.
//!
//! Crate-graph note: `pidash-jobs` depends on `pidash-services`, so this
//! module cannot name `jobs::celery::CeleryTaskMessage` (that would be a
//! dependency cycle). It publishes the Celery-format body parts instead —
//! [`MagicLinkEmit::task_name`] + [`MagicLinkEmit::args`] (and the two
//! twins; kwargs are always empty) — and the handlers (PIDASHCONV-422/431/
//! 434, in the `api` crate which already depends on `pidash-jobs`) wrap
//! them with `CeleryTaskMessage::new(task, args, Map::new())` plus
//! `queue::enqueue`, exactly like the intake/view handlers do
//! (`api/src/space/intake.rs`). `user.id` reaches `.delay()` as the raw
//! UUID object, which kombu's JSON encoder renders as a string, so the
//! field takes the string form.
//!
//! Redis pre-state (publish dependency, not a publish): `initiate()`
//! writes `magic_<email>` with a 600s expiry before the view publishes.
//! The `SET` itself runs in the handler; this module owns the pure key,
//! value-byte and branch builders so both sides share one definition.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-strip-noop (`magic_code.py:67,79`): `initiate()` strips
//!   `"magic_"` from `self.key`, but there `self.key` is still the raw
//!   email, so the strip is a no-op; `set_user_data()` strips it where the
//!   prefix is present. [`magic_redis_key`] prepends unconditionally, and
//!   the exhausted `SIGN_UP` payload keeps the raw key, matching both.
//! * QUIRK-first-email (`magic_code.py:92`): the first write stores
//!   `"email": self.key` raw while retries store `str(self.key)` —
//!   identical strings in practice; both builders take `&str`.
//! * QUIRK-attempt-gate (`magic_code.py:70`): exhaustion raises when the
//!   *stored* `current_attempt` is `> 2` (strict), checked before the
//!   rewrite, so the 4th generate still publishes and the 5th raises.
//!   [`is_attempt_exhausted`] keeps the strict predicate.
//! * QUIRK-activation-order (`adapter/base.py:228-233`): the activation
//!   mail is enqueued *before* `user.is_active = True` is saved, so a
//!   crash between the two re-sends on the next login. The emit carries no
//!   ordering; handlers keep the call order.
//! * QUIRK-forgot-first-name (`password_management.py:87`): the emit uses
//!   `user.first_name` as stored (possibly `""`), never a fallback.

use serde_json::{Map, Value};

/// Celery wire name for `magic_link` (bare `@shared_task` default; also
/// pinned by the jobs crate at `jobs/src/tasks_mail/auth_mail.rs`).
pub const MAGIC_LINK_TASK: &str = "pi_dash.bgtasks.magic_link_code_task.magic_link";
/// Celery wire name for `forgot_password` (bare default; also pinned at
/// `jobs/src/tasks_mail/auth_mail.rs`).
pub const FORGOT_PASSWORD_TASK: &str = "pi_dash.bgtasks.forgot_password_task.forgot_password";
/// Celery wire name for `user_activation_email` (bare default; also pinned
/// at `jobs/src/tasks_mail/membership_mail.rs`).
pub const USER_ACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.user_activation_email_task.user_activation_email";

/// Positional arg order of `magic_link.delay(email, key, token)`
/// (`views/app/magic.py:54`, `views/space/magic.py:50`).
pub const MAGIC_LINK_ARG_ORDER: &[&str] = &["email", "key", "token"];
/// Positional arg order of
/// `forgot_password.delay(first_name, email, uidb64, token, current_site)`
/// (`views/app/password_management.py:87`,
/// `views/space/password_management.py:99`).
pub const FORGOT_PASSWORD_ARG_ORDER: &[&str] =
    &["first_name", "email", "uidb64", "token", "current_site"];
/// Positional arg order of
/// `user_activation_email.delay(current_site, user_id)`
/// (`adapter/base.py:230`).
pub const USER_ACTIVATION_EMAIL_ARG_ORDER: &[&str] = &["current_site", "user_id"];

/// One `magic_link.delay(email, key, token)` call. `key` is the redis key
/// returned by `initiate()` (`magic_<email>`), `token` the 6-digit code;
/// both render as strings on the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct MagicLinkEmit {
    pub email: String,
    pub key: String,
    pub token: String,
}

impl MagicLinkEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        MAGIC_LINK_TASK
    }

    /// `.delay()` args in [`MAGIC_LINK_ARG_ORDER`]; kwargs are `{}`.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::String(self.email.clone()),
            Value::String(self.key.clone()),
            Value::String(self.token.clone()),
        ]
    }

    /// `.delay()` args as ordered pairs in [`MAGIC_LINK_ARG_ORDER`].
    pub fn args_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("email", Value::String(self.email.clone())),
            ("key", Value::String(self.key.clone())),
            ("token", Value::String(self.token.clone())),
        ]
    }
}

/// One `forgot_password.delay(first_name, email, uidb64, token,
/// current_site)` call. `first_name` is `user.first_name` as stored
/// (possibly `""`); `current_site` is `base_host(...)` with `is_app=True`
/// on the app path and `is_space=True` on the space path.
#[derive(Debug, Clone, PartialEq)]
pub struct ForgotPasswordEmit {
    pub first_name: String,
    pub email: String,
    pub uidb64: String,
    pub token: String,
    pub current_site: String,
}

impl ForgotPasswordEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        FORGOT_PASSWORD_TASK
    }

    /// `.delay()` args in [`FORGOT_PASSWORD_ARG_ORDER`]; kwargs are `{}`.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::String(self.first_name.clone()),
            Value::String(self.email.clone()),
            Value::String(self.uidb64.clone()),
            Value::String(self.token.clone()),
            Value::String(self.current_site.clone()),
        ]
    }

    /// `.delay()` args as ordered pairs in [`FORGOT_PASSWORD_ARG_ORDER`].
    pub fn args_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("first_name", Value::String(self.first_name.clone())),
            ("email", Value::String(self.email.clone())),
            ("uidb64", Value::String(self.uidb64.clone())),
            ("token", Value::String(self.token.clone())),
            ("current_site", Value::String(self.current_site.clone())),
        ]
    }
}

/// One `user_activation_email.delay(current_site, user_id)` call.
/// `user_id` is the raw `user.id` UUID object, rendered as a string by
/// kombu's JSON encoder, so the field takes the string form.
#[derive(Debug, Clone, PartialEq)]
pub struct UserActivationEmailEmit {
    pub current_site: String,
    pub user_id: String,
}

impl UserActivationEmailEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        USER_ACTIVATION_EMAIL_TASK
    }

    /// `.delay()` args in [`USER_ACTIVATION_EMAIL_ARG_ORDER`]; kwargs are `{}`.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::String(self.current_site.clone()),
            Value::String(self.user_id.clone()),
        ]
    }

    /// `.delay()` args as ordered pairs in [`USER_ACTIVATION_EMAIL_ARG_ORDER`].
    pub fn args_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("current_site", Value::String(self.current_site.clone())),
            ("user_id", Value::String(self.user_id.clone())),
        ]
    }
}

// ---------------------------------------------------------------------------
// Triggering conditions (pure predicates over handler-observed state)
// ---------------------------------------------------------------------------

/// `adapter/base.py:228`: the activation mail fires only while the user is
/// still inactive; the handler then flips `is_active` to true.
pub fn should_send_activation_email(user_is_active_before_save: bool) -> bool {
    !user_is_active_before_save
}

/// Forgot-password outcome after the gate sequence
/// (`views/app|space/password_management.py` `ForgotPassword*Endpoint`):
/// instance setup, then SMTP, then email shape, then user existence. Only
/// [`ForgotPasswordDecision::Emit`] publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgotPasswordDecision {
    Emit,
    InstanceNotConfigured,
    SmtpNotConfigured,
    InvalidEmail,
    UserDoesNotExist,
}

/// Evaluate the forgot-password gates in source order. `instance_setup_done`
/// is `instance is not None and instance.is_setup_done`; `smtp_configured`
/// is the `EMAIL_HOST` truthiness check; `email_valid` is
/// `validate_email` passing; `user_exists` is the
/// `User.objects.filter(email=).first()` hit.
pub fn forgot_password_decision(
    instance_setup_done: bool,
    smtp_configured: bool,
    email_valid: bool,
    user_exists: bool,
) -> ForgotPasswordDecision {
    if !instance_setup_done {
        return ForgotPasswordDecision::InstanceNotConfigured;
    }
    if !smtp_configured {
        return ForgotPasswordDecision::SmtpNotConfigured;
    }
    if !email_valid {
        return ForgotPasswordDecision::InvalidEmail;
    }
    if !user_exists {
        return ForgotPasswordDecision::UserDoesNotExist;
    }
    ForgotPasswordDecision::Emit
}

// ---------------------------------------------------------------------------
// Magic redis pre-state (`magic_code.py:54-95`, `initiate()`)
// ---------------------------------------------------------------------------

/// Redis key prefix: `key = "magic_" + str(self.key)`
/// (`magic_code.py:58`).
pub const MAGIC_REDIS_KEY_PREFIX: &str = "magic_";
/// Redis TTL seconds for the magic payload (`ex = 600`, `:89,94`).
pub const MAGIC_REDIS_EXPIRY_SECS: u64 = 600;
/// Exhaustion bound: raises when the *stored* `current_attempt` is `> 2`
/// (`magic_code.py:70`, strict comparison).
pub const MAGIC_MAX_STORED_ATTEMPTS: u32 = 2;

/// Redis key for the magic payload: `"magic_"` prepended unconditionally
/// (`magic_code.py:58`).
pub fn magic_redis_key(email: &str) -> String {
    format!("{MAGIC_REDIS_KEY_PREFIX}{email}")
}

/// First-generate value bytes (`magic_code.py:91-94`): `current_attempt`
/// 0 with `json.dumps` default separators and insertion key order
/// (`current_attempt`, `email`, `token`).
pub fn magic_redis_value_first(email: &str, token: &str) -> String {
    magic_redis_value(email, token, 0)
}

/// Retry-generate value bytes (`magic_code.py:84-89`): stored
/// `current_attempt + 1`, same separators and key order.
pub fn magic_redis_value_retry(email: &str, token: &str, stored_attempt: u32) -> String {
    magic_redis_value(email, token, stored_attempt + 1)
}

fn magic_redis_value(email: &str, token: &str, current_attempt: u32) -> String {
    format!(
        "{{\"current_attempt\": {current_attempt}, \"email\": {}, \"token\": {}}}",
        json_quoted(email),
        json_quoted(token),
    )
}

/// Exhaustion predicate (`magic_code.py:70`): `data["current_attempt"] > 2`.
pub fn is_attempt_exhausted(stored_current_attempt: u32) -> bool {
    stored_current_attempt > MAGIC_MAX_STORED_ATTEMPTS
}

/// Which exhausted error the raise carries (`magic_code.py:71-83`):
/// `SIGN_IN` when a `User` with the email exists, else `SIGN_UP`. The
/// `SIGN_IN` payload email is the `magic_`-stripped key; the `SIGN_UP`
/// payload keeps the raw key (a no-op difference at `initiate()`, where
/// the key is still the raw email — QUIRK-strip-noop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicExhaustedBranch {
    SignIn,
    SignUp,
}

impl MagicExhaustedBranch {
    /// Error slug for this branch (codes live in
    /// [`super::shapes::AUTHENTICATION_ERROR_CODES`]: 5100 / 5102).
    pub fn error_slug(&self) -> &'static str {
        match self {
            MagicExhaustedBranch::SignIn => "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN",
            MagicExhaustedBranch::SignUp => "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP",
        }
    }

    /// Error payload email for this branch: stripped for `SIGN_IN`, raw
    /// for `SIGN_UP`.
    pub fn payload_email<'a>(&self, raw_key: &'a str, stripped_email: &'a str) -> &'a str {
        match self {
            MagicExhaustedBranch::SignIn => stripped_email,
            MagicExhaustedBranch::SignUp => raw_key,
        }
    }
}

/// Select the exhausted branch by `User` existence
/// (`User.objects.filter(email=email).exists()`, `magic_code.py:73`).
pub fn exhausted_branch(user_exists: bool) -> MagicExhaustedBranch {
    if user_exists {
        MagicExhaustedBranch::SignIn
    } else {
        MagicExhaustedBranch::SignUp
    }
}

/// Python `json.dumps(s)` for one string with default `ensure_ascii=True`:
/// double-quoted with `\"` / `\\` / short escapes and `\uXXXX` for other
/// control and non-ASCII code points. Matches what `ri.set(key,
/// json.dumps(value))` stores for the email/token fields.
fn json_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x7F => out.push(c),
            c if (c as u32) <= 0xFFFF => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => {
                // Astral plane: surrogate pair, like CPython's ensure_ascii.
                let v = c as u32 - 0x10000;
                out.push_str(&format!(
                    "\\u{:04x}\\u{:04x}",
                    0xD800 + (v >> 10),
                    0xDC00 + (v & 0x3FF)
                ));
            }
        }
    }
    out.push('"');
    out
}

/// Empty kwargs object for `CeleryTaskMessage::new`: all three publishes
/// are positional-only (`body_kwargs {}` in FX-AUTH-09).
pub fn empty_kwargs() -> Map<String, Value> {
    Map::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- FX-AUTH-09 wire vectors -------------------------------------------

    #[test]
    fn task_names_match_jobs_wire_owners() {
        // Must stay identical to the D-07 registrations:
        // jobs/src/tasks_mail/auth_mail.rs (magic_link, forgot_password)
        // and jobs/src/tasks_mail/membership_mail.rs (user_activation_email).
        assert_eq!(
            MAGIC_LINK_TASK,
            "pi_dash.bgtasks.magic_link_code_task.magic_link"
        );
        assert_eq!(
            FORGOT_PASSWORD_TASK,
            "pi_dash.bgtasks.forgot_password_task.forgot_password"
        );
        assert_eq!(
            USER_ACTIVATION_EMAIL_TASK,
            "pi_dash.bgtasks.user_activation_email_task.user_activation_email"
        );
    }

    #[test]
    fn magic_link_payload_replays_fx09_call_site() {
        // FX-AUTH-09 `call_sites.magic_link.observed[0]` (+ `wire.magic_link`
        // `body_args`): views/app/magic.py:54, views/space/magic.py:50.
        let emit = MagicLinkEmit {
            email: "h@x.com".to_owned(),
            key: "magic_h@x.com".to_owned(),
            token: "815926".to_owned(),
        };
        assert_eq!(emit.task_name(), MAGIC_LINK_TASK);
        assert_eq!(
            emit.args(),
            vec![json!("h@x.com"), json!("magic_h@x.com"), json!("815926"),]
        );
        assert_eq!(
            emit.args_pairs()
                .iter()
                .map(|(k, _)| *k)
                .collect::<Vec<_>>(),
            MAGIC_LINK_ARG_ORDER
        );
        assert_eq!(MAGIC_LINK_ARG_ORDER, &["email", "key", "token"]);
        // Positional-only publish: `body_kwargs {}`.
        assert!(empty_kwargs().is_empty());
    }

    #[test]
    fn forgot_password_payload_replays_fx09_call_site() {
        // FX-AUTH-09 `wire.forgot_password.body_args` (5 positional):
        // views/app/password_management.py:87,
        // views/space/password_management.py:99.
        let emit = ForgotPasswordEmit {
            first_name: "Ada".to_owned(),
            email: "h@x.com".to_owned(),
            uidb64: "ZjEzZjlk".to_owned(),
            token: "cvxsd1-abc123".to_owned(),
            current_site: "http://localhost:3000".to_owned(),
        };
        assert_eq!(emit.task_name(), FORGOT_PASSWORD_TASK);
        assert_eq!(
            emit.args(),
            vec![
                json!("Ada"),
                json!("h@x.com"),
                json!("ZjEzZjlk"),
                json!("cvxsd1-abc123"),
                json!("http://localhost:3000"),
            ]
        );
        assert_eq!(
            emit.args_pairs()
                .iter()
                .map(|(k, _)| *k)
                .collect::<Vec<_>>(),
            FORGOT_PASSWORD_ARG_ORDER
        );
        assert_eq!(
            FORGOT_PASSWORD_ARG_ORDER,
            &["first_name", "email", "uidb64", "token", "current_site"]
        );
        assert!(empty_kwargs().is_empty());
    }

    #[test]
    fn user_activation_email_payload_replays_fx09_call_site() {
        // FX-AUTH-09 `call_sites.user_activation_email` (args
        // [current_site, user_id]) + `wire.user_activation_email.body_args`:
        // adapter/base.py:230.
        let emit = UserActivationEmailEmit {
            current_site: "http://localhost:8000".to_owned(),
            user_id: "11111111-1111-1111-1111-111111111111".to_owned(),
        };
        assert_eq!(emit.task_name(), USER_ACTIVATION_EMAIL_TASK);
        assert_eq!(
            emit.args(),
            vec![
                json!("http://localhost:8000"),
                json!("11111111-1111-1111-1111-111111111111"),
            ]
        );
        assert_eq!(
            emit.args_pairs()
                .iter()
                .map(|(k, _)| *k)
                .collect::<Vec<_>>(),
            USER_ACTIVATION_EMAIL_ARG_ORDER
        );
        assert_eq!(
            USER_ACTIVATION_EMAIL_ARG_ORDER,
            &["current_site", "user_id"]
        );
        assert!(empty_kwargs().is_empty());
    }

    #[test]
    fn wire_bodies_match_fx09_protocol_v2_shape() {
        // FX-AUTH-09 `wire.*`: body `[args, kwargs, embed]` with
        // `body_kwargs {}` and all-null embed; headers carry first-attempt
        // semantics (`eta`/`expires` null, `retries` 0,
        // `timelimit` [null, null], `root_id` = id, `parent_id` null,
        // `lang` "py"). The header/body serialization itself is F-09's
        // tested property (`jobs/src/celery.rs`); the emit supplies the
        // identical inputs: task name, positional args, empty kwargs.
        let args = MagicLinkEmit {
            email: "h@x.com".to_owned(),
            key: "magic_h@x.com".to_owned(),
            token: "815926".to_owned(),
        }
        .args();
        let body =
            json!([args, {}, {"callbacks": null, "errbacks": null, "chain": null, "chord": null}]);
        assert_eq!(
            body,
            json!([
                ["h@x.com", "magic_h@x.com", "815926"],
                {},
                {"callbacks": null, "errbacks": null, "chain": null, "chord": null},
            ])
        );
    }

    // -- Triggering conditions ------------------------------------------------

    #[test]
    fn activation_fires_only_for_inactive_users() {
        // adapter/base.py:228-233: `if not user.is_active: delay(...)`.
        assert!(should_send_activation_email(false));
        assert!(!should_send_activation_email(true));
    }

    #[test]
    fn forgot_password_gates_run_in_source_order() {
        // Gate order: instance → SMTP → email shape → user existence.
        // Only the all-true row emits; USER_DOES_NOT_EXIST otherwise.
        assert_eq!(
            forgot_password_decision(true, true, true, true),
            ForgotPasswordDecision::Emit
        );
        assert_eq!(
            forgot_password_decision(false, true, true, true),
            ForgotPasswordDecision::InstanceNotConfigured
        );
        assert_eq!(
            forgot_password_decision(true, false, true, true),
            ForgotPasswordDecision::SmtpNotConfigured
        );
        assert_eq!(
            forgot_password_decision(true, true, false, true),
            ForgotPasswordDecision::InvalidEmail
        );
        assert_eq!(
            forgot_password_decision(true, true, true, false),
            ForgotPasswordDecision::UserDoesNotExist
        );
        // Earlier gates win over later ones.
        assert_eq!(
            forgot_password_decision(false, false, false, false),
            ForgotPasswordDecision::InstanceNotConfigured
        );
        assert_eq!(
            forgot_password_decision(true, false, false, false),
            ForgotPasswordDecision::SmtpNotConfigured
        );
    }

    // -- FX-AUTH-06 redis pre-state vectors ------------------------------------

    #[test]
    fn magic_redis_key_prepends_prefix() {
        // magic_code.py:58: `key = "magic_" + str(self.key)`.
        assert_eq!(magic_redis_key("m1@x.com"), "magic_m1@x.com");
        assert_eq!(MAGIC_REDIS_EXPIRY_SECS, 600);
    }

    #[test]
    fn magic_redis_first_write_replays_fx06() {
        // FX-AUTH-06 `magic_provider.init_first.redis`:
        // {"current_attempt": 0, "email": "m1@x.com", "token": "771125"}.
        // Byte-identical to `json.dumps(value)`: insertion key order with
        // `", "` / `": "` separators.
        assert_eq!(
            magic_redis_value_first("m1@x.com", "771125"),
            r#"{"current_attempt": 0, "email": "m1@x.com", "token": "771125"}"#
        );
    }

    #[test]
    fn magic_redis_retry_increments_attempt() {
        // FX-AUTH-06 `init_second_attempt.attempt == 1`:
        // `current_attempt = data["current_attempt"] + 1`.
        assert_eq!(
            magic_redis_value_retry("m1@x.com", "771125", 0),
            r#"{"current_attempt": 1, "email": "m1@x.com", "token": "771125"}"#
        );
        assert_eq!(
            magic_redis_value_retry("m1@x.com", "771125", 2),
            r#"{"current_attempt": 3, "email": "m1@x.com", "token": "771125"}"#
        );
    }

    #[test]
    fn attempt_exhaustion_uses_strict_predicate() {
        // magic_code.py:70 `if data["current_attempt"] > 2`: stored 2
        // still publishes (QUIRK-attempt-gate), stored 3 raises.
        assert!(!is_attempt_exhausted(0));
        assert!(!is_attempt_exhausted(1));
        assert!(!is_attempt_exhausted(2));
        assert!(is_attempt_exhausted(3));
        assert_eq!(MAGIC_MAX_STORED_ATTEMPTS, 2);
    }

    #[test]
    fn exhausted_branch_selects_by_user_existence() {
        // FX-AUTH-06 `exhaust_signin` (5100, existing user) vs
        // `exhaust_signup` (5102, new user); codes resolved through the
        // shapes kernel so the two cannot drift apart.
        assert_eq!(exhausted_branch(true), MagicExhaustedBranch::SignIn);
        assert_eq!(exhausted_branch(false), MagicExhaustedBranch::SignUp);
        assert_eq!(
            MagicExhaustedBranch::SignIn.error_slug(),
            "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN"
        );
        assert_eq!(
            MagicExhaustedBranch::SignUp.error_slug(),
            "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP"
        );
        assert_eq!(
            super::super::shapes::error_code(MagicExhaustedBranch::SignIn.error_slug()),
            Some(5100)
        );
        assert_eq!(
            super::super::shapes::error_code(MagicExhaustedBranch::SignUp.error_slug()),
            Some(5102)
        );
        // SIGN_IN payload carries the stripped email, SIGN_UP the raw key.
        assert_eq!(
            MagicExhaustedBranch::SignIn.payload_email("ep@x.com", "ep@x.com"),
            "ep@x.com"
        );
        assert_eq!(
            MagicExhaustedBranch::SignUp.payload_email("brandnew@x.com", "brandnew@x.com"),
            "brandnew@x.com"
        );
    }

    #[test]
    fn json_quoted_matches_python_dumps_for_edge_chars() {
        assert_eq!(json_quoted("a@x.com"), "\"a@x.com\"");
        assert_eq!(json_quoted("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_quoted("a\nb"), "\"a\\nb\"");
    }
}
