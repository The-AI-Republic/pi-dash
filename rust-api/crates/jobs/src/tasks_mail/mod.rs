//! Mail + notification background tasks (D-07, jobs layer).
//!
//! Port of the pure helpers in
//! `apps/api/pi_dash/bgtasks/email_notification_task.py`:
//!
//! - [`email_notification::remove_unwanted_characters`] (`:27-31`)
//! - [`email_notification::acquire_lock`] / [`email_notification::release_lock`]
//!   (`:34-43`)
//! - [`email_notification::create_payload`] (`:87-127`, including the
//!   PORT BUG at `:116`)
//!
//! The send/stack task bodies that call these helpers land in
//! PIDASHCONV-213 on top of this module; the `notification_task.py`
//! mention/comment helpers (PIDASHCONV-214/215) and the single-task mail
//! senders (PIDASHCONV-216/217) extend this module tree the same way.
//!
//! T5 (PIDASHCONV-216) adds [`auth_mail`] (the four auth mail tasks) and
//! [`mail_send`] (the shared send pipeline T6 reuses, never forks).
//! [`register_auth_mail_tasks`] only builds the handler table — flipping
//! the live worker to these handlers is the domain gate's call
//! (PIDASHCONV-218, after the PIDASHCONV-21 proxy pass), so every name in
//! [`TASK_NAMES`] still routes to `PythonOwned` (see
//! [`crate::worker::route_for`]); the domain gate flips ownership after
//! the proxy pass, mirroring the `tasks_cleanup` export tasks.
//! [`is_mail_task`] is the routing predicate and [`TASK_NAMES`]
//! is pinned against the `F-WIRE-MAIL` fixture below.

pub mod auth_mail;
pub mod email_notification;
pub mod mail_send;

use crate::worker::Registry;

/// `pi_dash.bgtasks.email_notification_task.stack_email_notification`
/// (`email_notification_task.py:47`, `@shared_task` with no name override).
pub const STACK_EMAIL_NOTIFICATION_TASK: &str =
    "pi_dash.bgtasks.email_notification_task.stack_email_notification";
/// `pi_dash.bgtasks.email_notification_task.send_email_notification`
/// (`email_notification_task.py:153`).
pub const SEND_EMAIL_NOTIFICATION_TASK: &str =
    "pi_dash.bgtasks.email_notification_task.send_email_notification";
/// `pi_dash.bgtasks.notification_task.notifications`
/// (`notification_task.py:191`). Handler lands in PIDASHCONV-215.
pub const NOTIFICATIONS_TASK: &str = "pi_dash.bgtasks.notification_task.notifications";
/// `pi_dash.bgtasks.magic_link_code_task.magic_link`
/// (`magic_link_code_task.py:23`). Handler in [`auth_mail`].
pub const MAGIC_LINK_TASK: &str = "pi_dash.bgtasks.magic_link_code_task.magic_link";
/// `pi_dash.bgtasks.forgot_password_task.forgot_password`
/// (`forgot_password_task.py:23`). Handler in [`auth_mail`].
pub const FORGOT_PASSWORD_TASK: &str = "pi_dash.bgtasks.forgot_password_task.forgot_password";
/// `pi_dash.bgtasks.user_activation_email_task.user_activation_email`
/// (`user_activation_email_task.py:23`). Handler lands in PIDASHCONV-216.
pub const USER_ACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.user_activation_email_task.user_activation_email";
/// `pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email`
/// (`user_deactivation_email_task.py:23`). Handler lands in PIDASHCONV-216.
pub const USER_DEACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email";
/// `pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation`
/// (`user_email_update_task.py:22`). Handler lands in PIDASHCONV-217.
pub const SEND_EMAIL_UPDATE_CONFIRMATION_TASK: &str =
    "pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation";
/// `pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code`
/// (`user_email_update_task.py:67`). Handler lands in PIDASHCONV-217.
pub const SEND_EMAIL_UPDATE_MAGIC_CODE_TASK: &str =
    "pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code";
/// `pi_dash.bgtasks.project_add_user_email_task.project_add_user_email`
/// (`project_add_user_email_task.py:25`). Handler lands in PIDASHCONV-217.
pub const PROJECT_ADD_USER_EMAIL_TASK: &str =
    "pi_dash.bgtasks.project_add_user_email_task.project_add_user_email";
/// `pi_dash.bgtasks.workspace_invitation_task.workspace_invitation`
/// (`workspace_invitation_task.py:23`). Handler lands in PIDASHCONV-217.
pub const WORKSPACE_INVITATION_TASK: &str =
    "pi_dash.bgtasks.workspace_invitation_task.workspace_invitation";

// `MAGIC_LINK_TASK` / `FORGOT_PASSWORD_TASK` live here (routing) and in
// [`auth_mail`] (handlers) with identical values; the handler module's
// copies are used qualified so there is exactly one definition each.
pub use auth_mail::{
    forgot_password_handler, magic_link_handler, update_confirm_handler, update_magic_handler,
    AUTH_MAIL_TASKS, UPDATE_CONFIRM_TASK, UPDATE_MAGIC_TASK,
};

/// Every live D-07 Celery task name (11 tasks; `project_invitation_task.py`
/// is dead code — no caller in `apps/api/pi_dash` — so it has no name here).
pub const TASK_NAMES: [&str; 11] = [
    STACK_EMAIL_NOTIFICATION_TASK,
    SEND_EMAIL_NOTIFICATION_TASK,
    NOTIFICATIONS_TASK,
    MAGIC_LINK_TASK,
    FORGOT_PASSWORD_TASK,
    USER_ACTIVATION_EMAIL_TASK,
    USER_DEACTIVATION_EMAIL_TASK,
    SEND_EMAIL_UPDATE_CONFIRMATION_TASK,
    SEND_EMAIL_UPDATE_MAGIC_CODE_TASK,
    PROJECT_ADD_USER_EMAIL_TASK,
    WORKSPACE_INVITATION_TASK,
];

/// True for the eleven D-07 mail/notification task names. The worker
/// forwards them to the Python plane until the domain gate flips ownership.
pub fn is_mail_task(task: &str) -> bool {
    TASK_NAMES.contains(&task)
}

/// The T5 handler table exists but is not called yet: every D-07 name must
/// stay Python-owned until the domain gate flips ownership after the
/// PIDASHCONV-21 proxy pass.
pub fn assert_python_owned(registry: &Registry, task: &str) -> bool {
    !registry.owns(task) && is_mail_task(task)
}

/// Register all four T5 task names on `registry` (F-WIRE-MAIL names, plain
/// `@shared_task`: default ack-on-success, no `autoretry_for` — handlers
/// always acknowledge, mirroring the swallow-everything bodies).
pub fn register_auth_mail_tasks(registry: &mut Registry, pool: sqlx::PgPool) {
    registry.register(auth_mail::MAGIC_LINK_TASK, magic_link_handler(pool.clone()));
    registry.register(
        auth_mail::FORGOT_PASSWORD_TASK,
        forgot_password_handler(pool.clone()),
    );
    registry.register(
        auth_mail::UPDATE_MAGIC_TASK,
        update_magic_handler(pool.clone()),
    );
    registry.register(auth_mail::UPDATE_CONFIRM_TASK, update_confirm_handler(pool));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_task_names_match_wire_fixture() {
        // `rust-api/fixtures/tasks_mail/wire/F-WIRE-MAIL.wire.json`: the
        // oracle's 11 live task names must be exactly this module's names.
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/tasks_mail/wire/F-WIRE-MAIL.wire.json"
        ))
        .expect("wire fixture must parse");
        let messages = fixture["messages"].as_object().expect("messages object");
        assert_eq!(messages.len(), TASK_NAMES.len(), "wire task count");
        for name in messages.keys() {
            assert!(is_mail_task(name), "{name} must be recognised");
        }
        for name in TASK_NAMES {
            assert!(
                messages.contains_key(name),
                "{name} must be in the wire fixture"
            );
        }
    }

    #[test]
    fn mail_tasks_route_python_owned() {
        let registry = Registry::new();
        for name in TASK_NAMES {
            assert!(
                assert_python_owned(&registry, name),
                "{name} must stay Python-owned"
            );
        }
        assert!(!is_mail_task(
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
        ));
        assert!(!is_mail_task(""));
    }

    #[tokio::test]
    async fn registry_owns_exactly_the_four_names() {
        // `connect_lazy` needs a Tokio context even though it never
        // connects — no database is touched.
        let pool = sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/nope").expect("lazy");
        let mut registry = Registry::new();
        register_auth_mail_tasks(&mut registry, pool);
        for name in AUTH_MAIL_TASKS {
            assert!(registry.owns(name), "{name}");
        }
        // Neighbors stay Python-owned: T6 names and the dead invitation
        // task are NOT registered here.
        assert!(!registry.owns("pi_dash.bgtasks.user_activation_email_task.user_activation_email"));
        assert!(!registry.owns("pi_dash.bgtasks.project_invitation_task.project_invitation"));
    }
}
