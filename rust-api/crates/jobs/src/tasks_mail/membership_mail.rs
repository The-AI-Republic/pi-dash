//! The four D-07 membership mail tasks (T6, PIDASHCONV-217).
//!
//! Ports (translation only):
//!
//! * `user_activation_email(current_site, user_id)`
//!   (`bgtasks/user_activation_email_task.py:23-69`): `User.objects.get`
//!   by id (any miss → `log_exception`, swallow); subject
//!   `{first_name or display_name or email} has been activated on Pi Dash`
//!   (Python `or` chain — empty strings fall through, `None` email renders
//!   `"None"` via `str()`); context `{email, profile_url}` with
//!   `profile_url = current_site + "/profile"`; template
//!   `emails/user/user_activation.html`.
//! * `user_deactivation_email(current_site, user_id)`
//!   (`bgtasks/user_deactivation_email_task.py:23-71`): same shape;
//!   subject `... has been deactivated on Pi Dash`; context
//!   `{email, login_url}` with `login_url = current_site + "/login"`;
//!   template `emails/user/user_deactivation.html`.
//! * `project_add_user_email(current_site, project_member_id, invitor_id)`
//!   (`bgtasks/project_add_user_email_task.py:25-89`): invitor
//!   `User.objects.get(pk)` first, then `ProjectMember.objects.get(pk)`
//!   (Python order kept), then the lazy FK reads (`project.name`,
//!   `workspace.name`/`slug`, `member.email`); `project_url =
//!   {site}/{slug}/projects/{project_id}/issues`; fixed subject `You have
//!   been invited to a Pi Dash project`; context `{project_name,
//!   workspace_name, email, inviter_first_name, project_url}` (note: the
//!   inviter name is the bare `first_name`, no `or` fallback chain);
//!   template `emails/notifications/project_addition.html`.
//! * `workspace_invitation(email, workspace_id, token, current_site,
//!   inviter)` (`bgtasks/workspace_invitation_task.py:23-89`): inviter
//!   `User.objects.get(email=inviter)` — a miss here is NOT in the silent
//!   tuple, so it takes the `log_exception` path; `Workspace` /
//!   `WorkspaceMemberInvite` misses return silently; `relative_link =
//!   /workspace-invitations/?invitation_id={id}&slug={slug}&token={token}`,
//!   `abs_url = str(current_site) + relative_link`; context `{email,
//!   first_name, workspace_name, abs_url}`; template
//!   `emails/invitations/workspace_invitation.html`; the plain-text body is
//!   written back (`invite.message = text_content; .save()`) BEFORE the
//!   SMTP send, so a failed send still leaves the message stored.
//!
//! SQL semantics: `users` has no `deleted_at` column (plain `WHERE`
//! lookups); every other table here is soft-deletable, so the default
//! manager scope (`deleted_at IS NULL`) is applied — the same convention
//! as the sibling jobs (`license/tasks.rs:36`). Sends go through T5's
//! shared [`super::mail_send::send_mail`] (reused, never forked); the
//! invite `message` UPDATE renders via the same [`super::mail_send`]
//! helpers, which is byte-identical to the send's own render (all four
//! templates are deterministic `{{ var }}` interpolation, like T5's).
//! Celery arity is enforced exactly like [`super::auth_mail`]; every
//! handler returns `Ok(Verdict::Ack)` (swallow, never retry).
//!
//! Registration lives in [`super::register_membership_mail_tasks`]
//! (`mod.rs`).
//!
//! PORT BUGS (ported, not fixed): none in these four bodies beyond the
//! shared `str(None) == "None"` email rendering, which falls out of
//! [`super::mail_send::py_str`] exactly like Python's `str()`.

use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use super::mail_send::{escape_context, plain_text_from_html, py_str, render_template, send_mail};
use crate::queue::JobRow;
use crate::worker::{Handler, Verdict};

/// `pi_dash.bgtasks.user_activation_email_task.user_activation_email`.
pub const USER_ACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.user_activation_email_task.user_activation_email";
/// `pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email`.
pub const USER_DEACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email";
/// `pi_dash.bgtasks.project_add_user_email_task.project_add_user_email`.
pub const PROJECT_ADD_USER_EMAIL_TASK: &str =
    "pi_dash.bgtasks.project_add_user_email_task.project_add_user_email";
/// `pi_dash.bgtasks.workspace_invitation_task.workspace_invitation`.
pub const WORKSPACE_INVITATION_TASK: &str =
    "pi_dash.bgtasks.workspace_invitation_task.workspace_invitation";

/// All four names, in fixture order.
pub const MEMBERSHIP_MAIL_TASKS: [&str; 4] = [
    USER_ACTIVATION_EMAIL_TASK,
    USER_DEACTIVATION_EMAIL_TASK,
    PROJECT_ADD_USER_EMAIL_TASK,
    WORKSPACE_INVITATION_TASK,
];

/// `emails/user/user_activation.html` — serves `user_activation_email`.
pub const USER_ACTIVATION_TEMPLATE: &str =
    include_str!("../../../../../apps/api/templates/emails/user/user_activation.html");
/// `emails/user/user_deactivation.html` — serves `user_deactivation_email`.
pub const USER_DEACTIVATION_TEMPLATE: &str =
    include_str!("../../../../../apps/api/templates/emails/user/user_deactivation.html");
/// `emails/notifications/project_addition.html` — serves
/// `project_add_user_email`.
pub const PROJECT_ADDITION_TEMPLATE: &str =
    include_str!("../../../../../apps/api/templates/emails/notifications/project_addition.html");
/// `emails/invitations/workspace_invitation.html` — serves
/// `workspace_invitation`.
pub const WORKSPACE_INVITATION_TEMPLATE: &str =
    include_str!("../../../../../apps/api/templates/emails/invitations/workspace_invitation.html");

/// Python `first_name or display_name or email`: empty strings fall
/// through; the caller passes the email already through [`py_str`] (so a
/// NULL email is `"None"`, exactly like `str(user.email)` in the f-string).
pub fn or_name(first_name: &str, display_name: &str, email_str: &str) -> String {
    if !first_name.is_empty() {
        first_name.to_owned()
    } else if !display_name.is_empty() {
        display_name.to_owned()
    } else {
        email_str.to_owned()
    }
}

/// `user_activation_email_task.py:28` subject.
pub fn activation_subject(name: &str) -> String {
    format!("{name} has been activated on Pi Dash")
}

/// `user_deactivation_email_task.py:28` subject.
pub fn deactivation_subject(name: &str) -> String {
    format!("{name} has been deactivated on Pi Dash")
}

/// `user_activation_email_task.py:30`: `current_site + "/profile"`.
pub fn profile_url(current_site: &str) -> String {
    format!("{current_site}/profile")
}

/// `user_deactivation_email_task.py:30`: `current_site + "/login"`.
pub fn login_url(current_site: &str) -> String {
    format!("{current_site}/login")
}

/// `project_add_user_email_task.py:36`: fixed subject.
pub const PROJECT_ADD_USER_SUBJECT: &str = "You have been invited to a Pi Dash project";

/// `project_add_user_email_task.py:35`: raw interpolation, no encoding.
pub fn project_url(current_site: &str, workspace_slug: &str, project_id: &Uuid) -> String {
    format!("{current_site}/{workspace_slug}/projects/{project_id}/issues")
}

/// `workspace_invitation_task.py:31-33`: raw interpolation, no encoding.
pub fn invite_relative_link(invite_id: &Uuid, workspace_slug: &str, token: &str) -> String {
    format!("/workspace-invitations/?invitation_id={invite_id}&slug={workspace_slug}&token={token}")
}

/// `workspace_invitation_task.py:36`: `str(current_site) + relative_link`.
pub fn invite_abs_url(current_site: &str, relative_link: &str) -> String {
    format!("{current_site}{relative_link}")
}

/// `workspace_invitation_task.py:49` subject.
pub fn invite_subject(name: &str, workspace_name: &str) -> String {
    format!("{name} has invited you to join them in {workspace_name} on Pi Dash")
}

/// Extract the exact positional string args: `len != arity` or non-empty
/// kwargs is Python's `TypeError` — swallowed, so `None` here (same as
/// [`super::auth_mail`]).
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

/// `log_exception` (`utils/exception_logger.py:12-23`): the
/// `pi_dash.exception` error log. Every handler swallows via this and
/// still acknowledges.
fn log_swallowed(task: &str, error: &str) {
    tracing::error!(target: "pi_dash.exception", "{task}: {error}");
}

/// One `users` row as the activation/deactivation tasks read it
/// (`User.objects.get(id=...)` — no soft-delete scope: `users` has no
/// `deleted_at` column).
struct MailUser {
    first_name: String,
    display_name: String,
    email: Option<String>,
}

async fn load_user_by_id(pool: &PgPool, user_id: &Uuid) -> Result<Option<MailUser>, sqlx::Error> {
    let row: Option<(String, String, Option<String>)> =
        sqlx::query_as("SELECT first_name, display_name, email FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(first_name, display_name, email)| MailUser {
        first_name,
        display_name,
        email,
    }))
}

async fn load_user_by_email(pool: &PgPool, email: &str) -> Result<Option<MailUser>, sqlx::Error> {
    let row: Option<(String, String, Option<String>)> =
        sqlx::query_as("SELECT first_name, display_name, email FROM users WHERE email = $1")
            .bind(email)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(first_name, display_name, email)| MailUser {
        first_name,
        display_name,
        email,
    }))
}

/// Handler for [`USER_ACTIVATION_EMAIL_TASK`]:
/// `user_activation_email(current_site, user_id)`.
pub fn user_activation_email_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match positional(&job, 2) {
                Some(args) => {
                    let current_site = &args[0];
                    match Uuid::parse_str(&args[1]) {
                        Ok(user_id) => match load_user_by_id(&pool, &user_id).await {
                            Ok(Some(user)) => {
                                let email_str = user.email.as_deref().map_or_else(
                                    || py_str(&serde_json::Value::Null),
                                    str::to_owned,
                                );
                                let name =
                                    or_name(&user.first_name, &user.display_name, &email_str);
                                let context = escape_context(&[
                                    ("email", email_str.clone()),
                                    ("profile_url", profile_url(current_site)),
                                ]);
                                send_mail(
                                    &pool,
                                    USER_ACTIVATION_TEMPLATE,
                                    &activation_subject(&name),
                                    &context,
                                    &email_str,
                                    "Email sent successfully.",
                                )
                                .await;
                            }
                            Ok(None) => {
                                log_swallowed(USER_ACTIVATION_EMAIL_TASK, "User.DoesNotExist");
                            }
                            Err(error) => {
                                log_swallowed(USER_ACTIVATION_EMAIL_TASK, &error.to_string());
                            }
                        },
                        Err(error) => {
                            log_swallowed(USER_ACTIVATION_EMAIL_TASK, &error.to_string());
                        }
                    }
                }
                None => {
                    log_swallowed(USER_ACTIVATION_EMAIL_TASK, "bad args");
                }
            }
            Ok(Verdict::Ack)
        })
    })
}

/// Handler for [`USER_DEACTIVATION_EMAIL_TASK`]:
/// `user_deactivation_email(current_site, user_id)`.
pub fn user_deactivation_email_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match positional(&job, 2) {
                Some(args) => {
                    let current_site = &args[0];
                    match Uuid::parse_str(&args[1]) {
                        Ok(user_id) => match load_user_by_id(&pool, &user_id).await {
                            Ok(Some(user)) => {
                                let email_str = user.email.as_deref().map_or_else(
                                    || py_str(&serde_json::Value::Null),
                                    str::to_owned,
                                );
                                let name =
                                    or_name(&user.first_name, &user.display_name, &email_str);
                                let context = escape_context(&[
                                    ("email", email_str.clone()),
                                    ("login_url", login_url(current_site)),
                                ]);
                                send_mail(
                                    &pool,
                                    USER_DEACTIVATION_TEMPLATE,
                                    &deactivation_subject(&name),
                                    &context,
                                    &email_str,
                                    "Email sent successfully.",
                                )
                                .await;
                            }
                            Ok(None) => {
                                log_swallowed(USER_DEACTIVATION_EMAIL_TASK, "User.DoesNotExist");
                            }
                            Err(error) => {
                                log_swallowed(USER_DEACTIVATION_EMAIL_TASK, &error.to_string());
                            }
                        },
                        Err(error) => {
                            log_swallowed(USER_DEACTIVATION_EMAIL_TASK, &error.to_string());
                        }
                    }
                }
                None => {
                    log_swallowed(USER_DEACTIVATION_EMAIL_TASK, "bad args");
                }
            }
            Ok(Verdict::Ack)
        })
    })
}

/// Handler for [`PROJECT_ADD_USER_EMAIL_TASK`]:
/// `project_add_user_email(current_site, project_member_id, invitor_id)`.
///
/// Read order mirrors Python (invitor first, then the member, then the
/// lazy FK hops); every miss takes the single `except Exception`
/// (`log_exception`) path.
pub fn project_add_user_email_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match positional(&job, 3) {
                Some(args) => {
                    let current_site = args[0].clone();
                    let outcome: Result<(), String> = async {
                        let member_id = Uuid::parse_str(&args[1])
                            .map_err(|e| e.to_string())?;
                        let invitor_id = Uuid::parse_str(&args[2])
                            .map_err(|e| e.to_string())?;
                        // `User.objects.get(pk=invitor_id)` first, like
                        // Python `:29-30` (bare `first_name`, no fallback).
                        let inviter: (String,) = sqlx::query_as(
                            "SELECT first_name FROM users WHERE id = $1",
                        )
                        .bind(invitor_id)
                        .fetch_optional(&pool)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| "User.DoesNotExist".to_owned())?;
                        // `ProjectMember.objects.get(pk=...)` (default
                        // manager: soft-deleted rows are invisible).
                        let member_row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
                            "SELECT project_id, member_id FROM project_members WHERE id = $1 AND deleted_at IS NULL",
                        )
                        .bind(member_id)
                        .fetch_optional(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                        let (project_id, member_id) =
                            member_row.ok_or_else(|| "ProjectMember.DoesNotExist".to_owned())?;
                        // Lazy `project_member.project.name` hop.
                        let project_row: Option<(String, Uuid)> = sqlx::query_as(
                            "SELECT name, workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL",
                        )
                        .bind(project_id)
                        .fetch_optional(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                        let (project_name, workspace_id) =
                            project_row.ok_or_else(|| "Project.DoesNotExist".to_owned())?;
                        // Lazy `project_member.workspace.name`/`slug` hop.
                        let workspace_row: Option<(String, String)> = sqlx::query_as(
                            "SELECT name, slug FROM workspaces WHERE id = $1 AND deleted_at IS NULL",
                        )
                        .bind(workspace_id)
                        .fetch_optional(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                        let (workspace_name, workspace_slug) =
                            workspace_row.ok_or_else(|| "Workspace.DoesNotExist".to_owned())?;
                        // Lazy `project_member.member.email` hop (nullable
                        // FK; NULL email renders `"None"` like `str()`).
                        let member_id =
                            member_id.ok_or_else(|| "ProjectMember.member is None".to_owned())?;
                        let email_row: Option<(Option<String>,)> = sqlx::query_as(
                            "SELECT email FROM users WHERE id = $1",
                        )
                        .bind(member_id)
                        .fetch_optional(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                        let member_email = email_row
                            .ok_or_else(|| "User.DoesNotExist".to_owned())?
                            .0
                            .unwrap_or_else(|| py_str(&serde_json::Value::Null));
                        let url = project_url(&current_site, &workspace_slug, &project_id);
                        let context = escape_context(&[
                            ("project_name", project_name),
                            ("workspace_name", workspace_name),
                            ("email", member_email.clone()),
                            ("inviter_first_name", inviter.0),
                            ("project_url", url),
                        ]);
                        send_mail(
                            &pool,
                            PROJECT_ADDITION_TEMPLATE,
                            PROJECT_ADD_USER_SUBJECT,
                            &context,
                            &member_email,
                            "Email sent successfully.",
                        )
                        .await;
                        Ok(())
                    }
                    .await;
                    if let Err(error) = outcome {
                        log_swallowed(PROJECT_ADD_USER_EMAIL_TASK, &error);
                    }
                }
                None => {
                    log_swallowed(PROJECT_ADD_USER_EMAIL_TASK, "bad args");
                }
            }
            Ok(Verdict::Ack)
        })
    })
}

/// Handler for [`WORKSPACE_INVITATION_TASK`]:
/// `workspace_invitation(email, workspace_id, token, current_site,
/// inviter)`.
///
/// `User.DoesNotExist` (inviter lookup by email) takes the generic
/// `log_exception` path; only `Workspace` / `WorkspaceMemberInvite`
/// misses return silently. The plain-text body is stored on the invite
/// BEFORE the send, exactly like Python `:62-63`.
pub fn workspace_invitation_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            match positional(&job, 5) {
                Some(args) => {
                    let outcome: Result<Option<()>, String> = async {
                        let email = args[0].clone();
                        let workspace_id = Uuid::parse_str(&args[1])
                            .map_err(|e| e.to_string())?;
                        let token = args[2].clone();
                        let current_site = args[3].clone();
                        let inviter = args[4].clone();
                        // `User.objects.get(email=inviter)`: NOT in the
                        // silent tuple — a miss logs.
                        let user = load_user_by_email(&pool, &inviter)
                            .await
                            .map_err(|e| e.to_string())?
                            .ok_or_else(|| "User.DoesNotExist".to_owned())?;
                        // `Workspace.objects.get(pk=...)`: silent miss.
                        let workspace_row: Option<(String, String)> = sqlx::query_as(
                            "SELECT name, slug FROM workspaces WHERE id = $1 AND deleted_at IS NULL",
                        )
                        .bind(workspace_id)
                        .fetch_optional(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                        let Some((workspace_name, workspace_slug)) = workspace_row else {
                            return Ok(None);
                        };
                        // `WorkspaceMemberInvite.objects.get(token, email)`:
                        // silent miss.
                        let invite_row: Option<(Uuid,)> = sqlx::query_as(
                            "SELECT id FROM workspace_member_invites WHERE token = $1 AND email = $2 AND deleted_at IS NULL",
                        )
                        .bind(&token)
                        .bind(&email)
                        .fetch_optional(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                        let Some((invite_id,)) = invite_row else {
                            return Ok(None);
                        };
                        let email_str = user.email.as_deref().map_or_else(
                            || py_str(&serde_json::Value::Null),
                            str::to_owned,
                        );
                        let name = or_name(&user.first_name, &user.display_name, &email_str);
                        let relative = invite_relative_link(&invite_id, &workspace_slug, &token);
                        let abs_url = invite_abs_url(&current_site, &relative);
                        let context = escape_context(&[
                            ("email", email.clone()),
                            ("first_name", name.clone()),
                            ("workspace_name", workspace_name.clone()),
                            ("abs_url", abs_url),
                        ]);
                        // `render_to_string` + plain text, then the DB
                        // write BEFORE the send (`:55-63`).
                        let html = render_template(WORKSPACE_INVITATION_TEMPLATE, &context)?;
                        let text = plain_text_from_html(&html);
                        sqlx::query(
                            "UPDATE workspace_member_invites SET message = $1, updated_at = NOW() WHERE id = $2",
                        )
                        .bind(&text)
                        .bind(invite_id)
                        .execute(&pool)
                        .await
                        .map_err(|e| e.to_string())?;
                        send_mail(
                            &pool,
                            WORKSPACE_INVITATION_TEMPLATE,
                            &invite_subject(&name, &workspace_name),
                            &context,
                            &email,
                            // No trailing period — verbatim from `:82`.
                            "Email sent successfully",
                        )
                        .await;
                        Ok(Some(()))
                    }
                    .await;
                    match outcome {
                        Ok(_) => {}
                        Err(error) => {
                            log_swallowed(WORKSPACE_INVITATION_TASK, &error);
                        }
                    }
                }
                None => {
                    log_swallowed(WORKSPACE_INVITATION_TASK, "bad args");
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
            task: USER_ACTIVATION_EMAIL_TASK.to_owned(),
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
    fn or_chain_falls_through_empty_strings() {
        // `F-MEMBER.tasks.json`: Python `or` — empty falls through.
        assert_eq!(or_name("Ada", "D", "a@x"), "Ada");
        assert_eq!(or_name("", "D", "a@x"), "D");
        assert_eq!(or_name("", "", "a@x"), "a@x");
        // `str(None) == "None"` for a NULL email.
        assert_eq!(or_name("", "", "None"), "None");
    }

    #[test]
    fn subjects_contexts_and_links_match_fixtures() {
        // `F-MEMBER.tasks.json` verbatim expectations.
        assert_eq!(
            activation_subject("Ada"),
            "Ada has been activated on Pi Dash"
        );
        assert_eq!(
            deactivation_subject("Ada"),
            "Ada has been deactivated on Pi Dash"
        );
        assert_eq!(profile_url("example.com"), "example.com/profile");
        assert_eq!(login_url("example.com"), "example.com/login");
        assert_eq!(
            PROJECT_ADD_USER_SUBJECT,
            "You have been invited to a Pi Dash project"
        );
        let project_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("static");
        assert_eq!(
            project_url("example.com", "acme", &project_id),
            "example.com/acme/projects/11111111-1111-1111-1111-111111111111/issues"
        );
        let invite_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("static");
        let relative = invite_relative_link(&invite_id, "acme", "tok");
        assert_eq!(
            relative,
            "/workspace-invitations/?invitation_id=22222222-2222-2222-2222-222222222222&slug=acme&token=tok"
        );
        assert_eq!(
            invite_abs_url("example.com", &relative),
            "example.com/workspace-invitations/?invitation_id=22222222-2222-2222-2222-222222222222&slug=acme&token=tok"
        );
        assert_eq!(
            invite_subject("Inviter", "Acme"),
            "Inviter has invited you to join them in Acme on Pi Dash"
        );
    }

    #[test]
    fn task_names_match_oracle_wire() {
        // `test_mail_tasks.py::MAIL_TASKS` keys for these four tasks.
        assert_eq!(
            USER_ACTIVATION_EMAIL_TASK,
            "pi_dash.bgtasks.user_activation_email_task.user_activation_email"
        );
        assert_eq!(
            USER_DEACTIVATION_EMAIL_TASK,
            "pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email"
        );
        assert_eq!(
            PROJECT_ADD_USER_EMAIL_TASK,
            "pi_dash.bgtasks.project_add_user_email_task.project_add_user_email"
        );
        assert_eq!(
            WORKSPACE_INVITATION_TASK,
            "pi_dash.bgtasks.workspace_invitation_task.workspace_invitation"
        );
        assert_eq!(MEMBERSHIP_MAIL_TASKS.len(), 4);
    }

    #[test]
    fn arity_is_exact_like_python() {
        use serde_json::json;
        assert!(positional(&job(json!(["s", "u"]), json!({})), 2).is_some());
        assert!(positional(&job(json!(["s"]), json!({})), 2).is_none());
        assert!(positional(&job(json!(["s", "u", "x"]), json!({})), 2).is_none());
        assert!(positional(&job(json!(["s", "u"]), json!({"a": 1})), 2).is_none());
        assert!(positional(&job(json!(["s", "m", "i"]), json!({})), 3).is_some());
        assert!(positional(&job(json!(["e", "w", "t", "s", "i"]), json!({})), 5).is_some());
    }

    #[tokio::test]
    async fn handlers_always_ack_even_on_bad_args() {
        // Swallow, never retry-signal: malformed jobs acknowledge without
        // touching the pool (lazy pool, no database).
        use serde_json::json;
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/nope").expect("lazy");
        let bad = job(json!([]), json!({}));
        assert_eq!(
            user_activation_email_handler(pool.clone())(bad.clone()).await,
            Ok(Verdict::Ack)
        );
        assert_eq!(
            user_deactivation_email_handler(pool.clone())(bad.clone()).await,
            Ok(Verdict::Ack)
        );
        assert_eq!(
            project_add_user_email_handler(pool.clone())(bad.clone()).await,
            Ok(Verdict::Ack)
        );
        assert_eq!(
            workspace_invitation_handler(pool)(bad).await,
            Ok(Verdict::Ack)
        );
    }
}
