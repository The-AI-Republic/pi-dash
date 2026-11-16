//! The four D-07 auth mail tasks (T5, PIDASHCONV-216).
//!
//! Ports (translation only):
//!
//! * `magic_link(email, key, token)` (`bgtasks/magic_link_code_task.py:23-64`):
//!   subject `Your unique Pi Dash login code is {token}`, context
//!   `{code: token, email}`, template `emails/auth/magic_signin.html`.
//!   PORT NOTE: `key` is accepted positionally but never referenced in the
//!   body — the signature is kept verbatim (arity 3 still enforced).
//! * `forgot_password(first_name, email, uidb64, token, current_site)`
//!   (`bgtasks/forgot_password_task.py:23-72`): `relative_link` +
//!   `abs_url = str(current_site) + relative_link`, fixed subject, context
//!   `{first_name, forgot_password_url, email}`, template
//!   `emails/auth/forgot_password.html`. NOTE: the success log has NO
//!   trailing period (`:68`), unlike the other three.
//! * `send_email_update_magic_code(email, token)`
//!   (`bgtasks/user_email_update_task.py:22-65`): same `magic_signin.html`
//!   template as `magic_link` with subject `Verify your new email address`.
//! * `send_email_update_confirmation(email)`
//!   (`bgtasks/user_email_update_task.py:67-114`): context `{email}`,
//!   template `emails/user/email_updated.html`, subject `Pi Dash email
//!   address successfully updated`, interpolated success log (`:108`).
//!
//! None of the four reads the database for its payload (contexts are pure
//! functions of the Celery args); the only DB touch is the shared email
//! configuration read inside [`super::mail_send::send_mail`]. Celery arity
//! is enforced exactly: anything but the exact positional count (or any
//! non-empty kwargs) is a `TypeError` in Python — caught by the broad
//! `except` and swallowed — so it is swallowed here too. Every handler
//! returns `Ok(Verdict::Ack)`: these tasks never signal retry.
//!
//! Registration lives in [`super::register_auth_mail_tasks`] (`mod.rs`).

use std::sync::Arc;

use sqlx::PgPool;

use super::mail_send::{
    escape_context, py_str, send_mail, EMAIL_UPDATED_TEMPLATE, FORGOT_PASSWORD_TEMPLATE,
    MAGIC_SIGNIN_TEMPLATE,
};
use crate::queue::JobRow;
use crate::worker::{Handler, Verdict};

/// `pi_dash.bgtasks.magic_link_code_task.magic_link`.
pub const MAGIC_LINK_TASK: &str = "pi_dash.bgtasks.magic_link_code_task.magic_link";
/// `pi_dash.bgtasks.forgot_password_task.forgot_password`.
pub const FORGOT_PASSWORD_TASK: &str = "pi_dash.bgtasks.forgot_password_task.forgot_password";
/// `pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code`.
pub const UPDATE_MAGIC_TASK: &str =
    "pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code";
/// `pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation`.
pub const UPDATE_CONFIRM_TASK: &str =
    "pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation";

/// All four names, in fixture order.
pub const AUTH_MAIL_TASKS: [&str; 4] = [
    MAGIC_LINK_TASK,
    FORGOT_PASSWORD_TASK,
    UPDATE_MAGIC_TASK,
    UPDATE_CONFIRM_TASK,
];

/// `magic_link.py:34` subject.
pub fn magic_link_subject(token: &str) -> String {
    format!("Your unique Pi Dash login code is {token}")
}

/// `forgot_password.py:26`: no URL-encoding — raw interpolation, ported
/// as-is.
pub fn forgot_relative_link(uidb64: &str, token: &str, email: &str) -> String {
    format!("/accounts/reset-password/?uidb64={uidb64}&token={token}&email={email}")
}

/// `forgot_password.py:27`: `str(current_site) + relative_link`.
pub fn forgot_abs_url(current_site: &str, relative_link: &str) -> String {
    format!("{current_site}{relative_link}")
}

/// `forgot_password.py:33`: fixed subject.
pub const FORGOT_PASSWORD_SUBJECT: &str =
    "A new password to your Pi Dash account has been requested";
/// `user_email_update_task.py:33`: fixed subject.
pub const UPDATE_MAGIC_SUBJECT: &str = "Verify your new email address";
/// `user_email_update_task.py:84`: fixed subject.
pub const UPDATE_CONFIRM_SUBJECT: &str = "Pi Dash email address successfully updated";

/// Extract the exact positional string args: `len != arity` or non-empty
/// kwargs is Python's `TypeError` — swallowed, so `None` here.
fn positional(job: &JobRow, arity: usize) -> Option<Vec<String>> {
    match &job.kwargs {
        serde_json::Value::Object(map) if map.is_empty() => {}
        serde_json::Value::Null => {}
        _ => return None,
    }
    let args = job.args.as_array()?;
    if args.len() != arity {
        return None;
    }
    Some(args.iter().map(py_str).collect())
}

/// Handler for [`MAGIC_LINK_TASK`]: `magic_link(email, key, token)`.
/// `key` (`args[1]`) is accepted and ignored, exactly like Python.
pub fn magic_link_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match positional(&job, 3) {
                Some(args) => {
                    let subject = magic_link_subject(&args[2]);
                    let context =
                        escape_context(&[("code", args[2].clone()), ("email", args[0].clone())]);
                    send_mail(
                        &pool,
                        MAGIC_SIGNIN_TEMPLATE,
                        &subject,
                        &context,
                        &args[0],
                        "Email sent successfully.",
                    )
                    .await;
                }
                None => {
                    tracing::error!(target: "pi_dash.exception", "bad args for {MAGIC_LINK_TASK}");
                }
            }
            Ok(Verdict::Ack)
        })
    })
}

/// Handler for [`FORGOT_PASSWORD_TASK`]:
/// `forgot_password(first_name, email, uidb64, token, current_site)`.
pub fn forgot_password_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match positional(&job, 5) {
                Some(args) => {
                    let relative = forgot_relative_link(&args[2], &args[3], &args[1]);
                    let abs_url = forgot_abs_url(&args[4], &relative);
                    let context = escape_context(&[
                        ("first_name", args[0].clone()),
                        ("forgot_password_url", abs_url),
                        ("email", args[1].clone()),
                    ]);
                    send_mail(
                        &pool,
                        FORGOT_PASSWORD_TEMPLATE,
                        FORGOT_PASSWORD_SUBJECT,
                        &context,
                        &args[1],
                        // No trailing period — verbatim from `:68`.
                        "Email sent successfully",
                    )
                    .await;
                }
                None => {
                    tracing::error!(target: "pi_dash.exception", "bad args for {FORGOT_PASSWORD_TASK}");
                }
            }
            Ok(Verdict::Ack)
        })
    })
}

/// Handler for [`UPDATE_MAGIC_TASK`]:
/// `send_email_update_magic_code(email, token)`.
pub fn update_magic_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match positional(&job, 2) {
                Some(args) => {
                    let context =
                        escape_context(&[("code", args[1].clone()), ("email", args[0].clone())]);
                    send_mail(
                        &pool,
                        MAGIC_SIGNIN_TEMPLATE,
                        UPDATE_MAGIC_SUBJECT,
                        &context,
                        &args[0],
                        "Email sent successfully.",
                    )
                    .await;
                }
                None => {
                    tracing::error!(target: "pi_dash.exception", "bad args for {UPDATE_MAGIC_TASK}");
                }
            }
            Ok(Verdict::Ack)
        })
    })
}

/// Handler for [`UPDATE_CONFIRM_TASK`]:
/// `send_email_update_confirmation(email)`.
pub fn update_confirm_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match positional(&job, 1) {
                Some(args) => {
                    let context = escape_context(&[("email", args[0].clone())]);
                    // The only interpolated success log (`:108`).
                    let success = format!(
                        "Email update confirmation sent successfully to {}.",
                        args[0]
                    );
                    send_mail(
                        &pool,
                        EMAIL_UPDATED_TEMPLATE,
                        UPDATE_CONFIRM_SUBJECT,
                        &context,
                        &args[0],
                        &success,
                    )
                    .await;
                }
                None => {
                    tracing::error!(target: "pi_dash.exception", "bad args for {UPDATE_CONFIRM_TASK}");
                }
            }
            Ok(Verdict::Ack)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn job(args: serde_json::Value, kwargs: serde_json::Value) -> JobRow {
        JobRow {
            id: 1,
            celery_id: "test-id".to_owned(),
            task: MAGIC_LINK_TASK.to_owned(),
            args,
            kwargs,
            queue: "celery".to_owned(),
            status: "queued".to_owned(),
            attempts: 0,
            max_retries: 3,
            visible_at: Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: Utc::now(),
            last_error: None,
        }
    }

    #[test]
    fn subjects_contexts_and_links_match_fixtures() {
        // `F-AUTH.tasks.json` verbatim expectations.
        assert_eq!(
            magic_link_subject("842103"),
            "Your unique Pi Dash login code is 842103"
        );
        let relative = forgot_relative_link("uidb64", "token", "contract@example.com");
        assert_eq!(
            relative,
            "/accounts/reset-password/?uidb64=uidb64&token=token&email=contract@example.com"
        );
        assert_eq!(
            forgot_abs_url("example.com", &relative),
            "example.com/accounts/reset-password/?uidb64=uidb64&token=token&email=contract@example.com"
        );
        assert_eq!(
            FORGOT_PASSWORD_SUBJECT,
            "A new password to your Pi Dash account has been requested"
        );
        assert_eq!(UPDATE_MAGIC_SUBJECT, "Verify your new email address");
        assert_eq!(
            UPDATE_CONFIRM_SUBJECT,
            "Pi Dash email address successfully updated"
        );
    }

    #[test]
    fn task_names_match_oracle_wire() {
        // `test_mail_tasks.py::MAIL_TASKS` keys: the names the worker must
        // own for the proxy pass.
        assert_eq!(
            MAGIC_LINK_TASK,
            "pi_dash.bgtasks.magic_link_code_task.magic_link"
        );
        assert_eq!(
            FORGOT_PASSWORD_TASK,
            "pi_dash.bgtasks.forgot_password_task.forgot_password"
        );
        assert_eq!(
            UPDATE_MAGIC_TASK,
            "pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code"
        );
        assert_eq!(
            UPDATE_CONFIRM_TASK,
            "pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation"
        );
    }

    #[test]
    fn arity_is_exact_like_python() {
        use serde_json::json;
        assert!(positional(&job(json!(["a", "k", "t"]), json!({})), 3).is_some());
        // Too few / too many positionals: `TypeError` in Python.
        assert!(positional(&job(json!(["a", "k"]), json!({})), 3).is_none());
        assert!(positional(&job(json!(["a", "k", "t", "x"]), json!({})), 3).is_none());
        // Unexpected keyword: `TypeError` in Python.
        assert!(positional(&job(json!(["a", "k", "t"]), json!({"email": "a"})), 3).is_none());
        // Non-array args: `TypeError` in Python.
        assert!(positional(&job(json!("nope"), json!({})), 3).is_none());
    }

    #[tokio::test]
    async fn handlers_always_ack_even_on_bad_args() {
        // Swallow, never retry-signal: a malformed job still acknowledges.
        // Bad args never reach the pool, so a lazy (unconnected) pool is
        // enough — no database is touched.
        use serde_json::json;
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/nope").expect("lazy");
        let bad = job(json!([]), json!({}));
        assert_eq!(
            magic_link_handler(pool.clone())(bad.clone()).await,
            Ok(Verdict::Ack)
        );
        assert_eq!(
            forgot_password_handler(pool.clone())(bad.clone()).await,
            Ok(Verdict::Ack)
        );
        assert_eq!(
            update_magic_handler(pool.clone())(bad.clone()).await,
            Ok(Verdict::Ack)
        );
        assert_eq!(update_confirm_handler(pool)(bad).await, Ok(Verdict::Ack));
    }
}
