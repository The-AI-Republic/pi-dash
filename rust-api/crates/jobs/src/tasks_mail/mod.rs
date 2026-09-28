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
//! The send/stack task bodies that call these helpers live in
//! [`email_notification`] (PIDASHCONV-213, part 2); the
//! `notification_task.py` mention/comment helpers (PIDASHCONV-214/215)
//! and the single-task mail senders (PIDASHCONV-216/217) extend this
//! module tree the same way.
//!
//! T5 (PIDASHCONV-216) adds [`auth_mail`] (the four auth mail tasks) and
//! [`mail_send`] (the shared send pipeline T6 reuses, never forks).
//! T6 (PIDASHCONV-217) adds [`membership_mail`] (the four membership mail
//! tasks) via [`register_membership_mail_tasks`].
//! [`register_auth_mail_tasks`] only builds the handler table — flipping
//! the live worker to these handlers is the domain gate's call
//! (PIDASHCONV-218, after the PIDASHCONV-21 proxy pass), so every name in
//! [`TASK_NAMES`] still routes to `PythonOwned` (see
//! [`crate::worker::route_for`]); the domain gate flips ownership after
//! the proxy pass, mirroring the `tasks_cleanup` export tasks.
//! [`is_mail_task`] is the routing predicate and [`TASK_NAMES`]
//! is pinned against the `F-WIRE-MAIL` fixture below.
//!
//! Ownership: [`email_notification::register_email_notification_tasks`]
//! builds the handler table for the two mail task names, but the worker
//! binary does not call it yet — flipping now would steal live traffic
//! from the Python workers while the concrete SMTP/template/Redis
//! clients still live there (see the seam traits in
//! [`email_notification`]). Every name in [`TASK_NAMES`] therefore
//! routes to `PythonOwned` (see [`crate::worker::route_for`]); the
//! domain gate flips ownership after the PIDASHCONV-21 proxy pass,
//! mirroring the `tasks_cleanup` export tasks. [`is_mail_task`] is the
//! routing predicate and [`TASK_NAMES`] is pinned against the
//! `F-WIRE-MAIL` fixture below.

pub mod auth_mail;
pub mod email_notification;
pub mod mail_send;
pub mod membership_mail;
pub mod notifications;

pub use email_notification::register_email_notification_tasks;

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
/// (`notification_task.py:191`). Handler in [`notifications`].
pub const NOTIFICATIONS_TASK: &str = "pi_dash.bgtasks.notification_task.notifications";
/// `pi_dash.bgtasks.magic_link_code_task.magic_link`
/// (`magic_link_code_task.py:23`). Handler in [`auth_mail`].
pub const MAGIC_LINK_TASK: &str = "pi_dash.bgtasks.magic_link_code_task.magic_link";
/// `pi_dash.bgtasks.forgot_password_task.forgot_password`
/// (`forgot_password_task.py:23`). Handler in [`auth_mail`].
pub const FORGOT_PASSWORD_TASK: &str = "pi_dash.bgtasks.forgot_password_task.forgot_password";
/// `pi_dash.bgtasks.user_activation_email_task.user_activation_email`
/// (`user_activation_email_task.py:23`). Handler in [`membership_mail`]
/// (T6, PIDASHCONV-217).
pub const USER_ACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.user_activation_email_task.user_activation_email";
/// `pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email`
/// (`user_deactivation_email_task.py:23`). Handler in [`membership_mail`]
/// (T6, PIDASHCONV-217).
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
/// (`project_add_user_email_task.py:25`). Handler in [`membership_mail`]
/// (T6, PIDASHCONV-217).
pub const PROJECT_ADD_USER_EMAIL_TASK: &str =
    "pi_dash.bgtasks.project_add_user_email_task.project_add_user_email";
/// `pi_dash.bgtasks.workspace_invitation_task.workspace_invitation`
/// (`workspace_invitation_task.py:23`). Handler in [`membership_mail`]
/// (T6, PIDASHCONV-217).
pub const WORKSPACE_INVITATION_TASK: &str =
    "pi_dash.bgtasks.workspace_invitation_task.workspace_invitation";

// `MAGIC_LINK_TASK` / `FORGOT_PASSWORD_TASK` live here (routing) and in
// [`auth_mail`] (handlers) with identical values; the handler module's
// copies are used qualified so there is exactly one definition each.
pub use auth_mail::{
    forgot_password_handler, magic_link_handler, update_confirm_handler, update_magic_handler,
    AUTH_MAIL_TASKS, UPDATE_CONFIRM_TASK, UPDATE_MAGIC_TASK,
};
pub use membership_mail::{
    project_add_user_email_handler, user_activation_email_handler, user_deactivation_email_handler,
    workspace_invitation_handler, MEMBERSHIP_MAIL_TASKS,
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

/// Register the T4 `notifications` task name on `registry` (F-WIRE-MAIL name,
/// plain `@shared_task`: default ack-on-success, no `autoretry_for` — the
/// handler always acknowledges, mirroring the swallow-everything body).
/// Like [`register_auth_mail_tasks`], this only builds the handler table —
/// flipping the live worker to it is the domain gate's call (PIDASHCONV-218,
/// after the PIDASHCONV-21 proxy pass), so the name still routes to
/// `PythonOwned` (see [`crate::worker::route_for`]).
pub fn register_notifications_task(registry: &mut Registry, pool: sqlx::PgPool) {
    registry.register(
        NOTIFICATIONS_TASK,
        notifications::notifications_handler(pool),
    );
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

/// Register all four T6 task names on `registry` (F-WIRE-MAIL names, plain
/// `@shared_task`: default ack-on-success, no `autoretry_for` — handlers
/// always acknowledge, mirroring the swallow-everything bodies).
pub fn register_membership_mail_tasks(registry: &mut Registry, pool: sqlx::PgPool) {
    registry.register(
        membership_mail::USER_ACTIVATION_EMAIL_TASK,
        user_activation_email_handler(pool.clone()),
    );
    registry.register(
        membership_mail::USER_DEACTIVATION_EMAIL_TASK,
        user_deactivation_email_handler(pool.clone()),
    );
    registry.register(
        membership_mail::PROJECT_ADD_USER_EMAIL_TASK,
        project_add_user_email_handler(pool.clone()),
    );
    registry.register(
        membership_mail::WORKSPACE_INVITATION_TASK,
        workspace_invitation_handler(pool),
    );
}

/// Shared mention-component parser.
///
/// This is the single implementation of the BeautifulSoup query both mail
/// helpers build on — `soup.find_all("mention-component",
/// attrs={"entity_name": "user_mention"})` followed by
/// `tag["entity_identifier"]` — so `extract_mentions` here and T2's
/// `process_mention` cannot drift apart (T2 must reuse this, never fork it).
///
/// Returns `None` when any matched tag has no `entity_identifier`
/// attribute: Python's list comprehension raises `KeyError` there, which the
/// caller's broad `except` turns into `[]`. An empty `Vec` means zero
/// matched tags. Matching is case-insensitive on the tag and attribute
/// names (html.parser lowercases both) and exact on the `entity_name`
/// value. Comment nodes and `<script>`/`<style>` CDATA bodies never yield
/// tags, mirroring html.parser.
pub(crate) fn mention_ids_in_html(html: &str) -> Option<Vec<String>> {
    let bytes = html.as_bytes();
    let len = bytes.len();
    let mut pos = 0;
    let mut ids: Vec<String> = Vec::new();
    while pos < len {
        let open = match find_byte(bytes, pos, b'<') {
            Some(i) => i,
            None => break,
        };
        let rest = &html[open..];
        if rest.starts_with("<!--") {
            match html[open + 4..].find("-->") {
                Some(end) => pos = open + 4 + end + 3,
                None => break,
            }
            continue;
        }
        if rest.starts_with("</") || rest.starts_with("<!") || rest.starts_with("<?") {
            match tag_end(bytes, open + 2) {
                Some(end) => pos = end,
                None => break,
            }
            continue;
        }
        let (name, attrs_end, self_closing, close) = match parse_open_tag(bytes, open) {
            Some(parsed) => parsed,
            None => break,
        };
        let _ = attrs_end;
        if name == "script" || name == "style" {
            if self_closing {
                pos = close;
                continue;
            }
            match find_close_tag(bytes, close, &name) {
                Some(end) => pos = end,
                None => break,
            }
            continue;
        }
        if name == "mention-component" {
            let attrs = parse_attrs(&html[open..close]);
            let is_user_mention = attrs.iter().any(|(key, value)| {
                key == "entity_name" && value.as_deref() == Some("user_mention")
            });
            if is_user_mention {
                // `tag["entity_identifier"]` raises `KeyError` only when the
                // attribute is absent; a bare attribute with no `=` parses to
                // `""` under html.parser, so it yields an empty-string id.
                let identifier = match attrs.iter().find(|(key, _)| key == "entity_identifier") {
                    Some((_, value)) => value.clone().unwrap_or_default(),
                    None => return None,
                };
                ids.push(identifier);
            }
        }
        pos = close;
    }
    Some(ids)
}

/// Index of the first `needle` at or after `from`, or `None`.
fn find_byte(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes[from..]
        .iter()
        .position(|&b| b == needle)
        .map(|i| from + i)
}

/// Index just past the `>` closing the tag opened at `open` (the byte after
/// `<` is `start`). Respects single/double quotes. `None` when unterminated.
fn tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    let mut quote = 0u8;
    while i < bytes.len() {
        let b = bytes[i];
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
        } else if b == b'"' || b == b'\'' {
            quote = b;
        } else if b == b'>' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Parse `<name attrs...>` at `open`. Returns the lowercased tag name, the
/// byte offset where attributes start, whether the tag is self-closing, and
/// the offset just past `>`. All offsets are absolute into the source.
fn parse_open_tag(bytes: &[u8], open: usize) -> Option<(String, usize, bool, usize)> {
    let close = tag_end(bytes, open + 1)?;
    let mut i = open + 1;
    while i < close && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let name_start = i;
    while i < close && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' && bytes[i] != b'>' {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = html_slice(bytes, name_start, i)?.to_ascii_lowercase();
    let mut self_closing = false;
    let mut j = close - 1;
    while j > open && bytes[j].is_ascii_whitespace() {
        j -= 1;
    }
    if bytes[j] == b'/' {
        self_closing = true;
    }
    Some((name, i, self_closing, close))
}

/// Parse the attribute list of one already-bounded open tag
/// (`tag_source` spans `<` through `>` inclusive). Names are lowercased;
/// later duplicates win, mirroring BeautifulSoup's dict conversion. A bare
/// name with no `=` carries `None`.
fn parse_attrs(tag_source: &str) -> Vec<(String, Option<String>)> {
    let bytes = tag_source.as_bytes();
    let mut attrs: Vec<(String, Option<String>)> = Vec::new();
    let mut i = 1;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' && bytes[i] != b'>'
    {
        i += 1;
    }
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'>' || bytes[i] == b'/' {
            break;
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        if i == name_start {
            i += 1;
            continue;
        }
        let name = match html_slice(bytes, name_start, i) {
            Some(s) => s.to_ascii_lowercase(),
            None => break,
        };
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value: Option<String> = None;
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let value_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                value = html_slice(bytes, value_start, i).map(str::to_owned);
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                let value_start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'>'
                    && bytes[i] != b'/'
                {
                    i += 1;
                }
                value = html_slice(bytes, value_start, i).map(str::to_owned);
            }
        }
        if let Some(slot) = attrs.iter_mut().find(|(key, _)| *key == name) {
            slot.1 = value;
        } else {
            attrs.push((name, value));
        }
    }
    attrs
}

/// Byte-slice helper: every boundary this parser records sits on an ASCII
/// character, so slicing is always valid UTF-8.
fn html_slice(bytes: &[u8], start: usize, end: usize) -> Option<&str> {
    if start > end || end > bytes.len() {
        return None;
    }
    std::str::from_utf8(&bytes[start..end]).ok()
}

/// Offset just past the `</name>` closing tag at or after `from`
/// (case-insensitive), or `None` when there is none.
fn find_close_tag(bytes: &[u8], from: usize, name: &str) -> Option<usize> {
    let mut pos = from;
    while let Some(open) = find_byte(bytes, pos, b'<') {
        if bytes.get(open + 1) == Some(&b'/') {
            let mut i = open + 2;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let name_start = i;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && bytes[i] != b'>'
                && bytes[i] != b'/'
            {
                i += 1;
            }
            if let Some(found) = html_slice(bytes, name_start, i) {
                if found.eq_ignore_ascii_case(name) {
                    return tag_end(bytes, open + 2);
                }
            }
            pos = tag_end(bytes, open + 2)?;
        } else {
            pos = tag_end(bytes, open + 1)?;
        }
    }
    None
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

    #[tokio::test]
    async fn membership_registry_owns_exactly_the_four_names() {
        let pool = sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/nope").expect("lazy");
        let mut registry = Registry::new();
        register_membership_mail_tasks(&mut registry, pool);
        for name in MEMBERSHIP_MAIL_TASKS {
            assert!(registry.owns(name), "{name}");
        }
        // T5 names and the dead invitation task are NOT registered here.
        assert!(!registry.owns("pi_dash.bgtasks.magic_link_code_task.magic_link"));
        assert!(!registry.owns("pi_dash.bgtasks.project_invitation_task.project_invitation"));
    }

    #[tokio::test]
    async fn registry_owns_only_the_notifications_name() {
        // T4 (PIDASHCONV-215): registering the notifications task owns
        // exactly `NOTIFICATIONS_TASK`; every neighbor stays Python-owned
        // until the domain gate flips routing (PIDASHCONV-218).
        let pool = sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/nope").expect("lazy");
        let mut registry = Registry::new();
        register_notifications_task(&mut registry, pool);
        assert!(registry.owns(NOTIFICATIONS_TASK));
        assert!(registry.owns("pi_dash.bgtasks.notification_task.notifications"));
        for name in TASK_NAMES {
            if name != NOTIFICATIONS_TASK {
                assert!(!registry.owns(name), "{name}");
            }
        }
        assert!(assert_python_owned(&Registry::new(), NOTIFICATIONS_TASK));
    }

    #[test]
    fn parses_two_mentions_in_document_order() {
        let html = "<p>hi <mention-component entity_name=\"user_mention\" entity_identifier=\"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb\"></mention-component> and <mention-component entity_identifier=\"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\" entity_name=\"user_mention\"/></p>";
        assert_eq!(
            mention_ids_in_html(html),
            Some(vec![
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_owned(),
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned(),
            ])
        );
    }

    #[test]
    fn ignores_other_entities_and_comments_and_scripts() {
        let html = "<!-- <mention-component entity_name=\"user_mention\" entity_identifier=\"cccc\"></mention-component> -->\
            <script>var x = '<mention-component entity_name=\"user_mention\" entity_identifier=\"dddd\"></mention-component>';</script>\
            <mention-component entity_name=\"emoji\" entity_identifier=\"eeee\"></mention-component>\
            <MENTION-COMPONENT ENTITY_NAME=\"user_mention\" ENTITY_IDENTIFIER=\"ffff\"></MENTION-COMPONENT>";
        assert_eq!(mention_ids_in_html(html), Some(vec!["ffff".to_owned()]));
    }

    #[test]
    fn missing_identifier_fails_the_whole_parse() {
        let html = "<mention-component entity_name=\"user_mention\" entity_identifier=\"aaaa\"></mention-component>\
            <mention-component entity_name=\"user_mention\"></mention-component>";
        assert_eq!(mention_ids_in_html(html), None);
    }

    #[test]
    fn nested_tags_both_match() {
        let html = "<mention-component entity_name=\"user_mention\" entity_identifier=\"aaaa\">\
            <mention-component entity_name=\"user_mention\" entity_identifier=\"bbbb\"></mention-component>\
            </mention-component>";
        assert_eq!(
            mention_ids_in_html(html),
            Some(vec!["aaaa".to_owned(), "bbbb".to_owned()])
        );
    }

    #[test]
    fn empty_identifier_is_still_a_match() {
        let html = "<mention-component entity_name=\"user_mention\" entity_identifier=\"\"></mention-component>";
        assert_eq!(mention_ids_in_html(html), Some(vec!["".to_owned()]));
    }

    #[test]
    fn bare_identifier_without_value_yields_empty_string() {
        // html.parser gives a valueless attribute `""` instead of raising,
        // so only a wholly absent attribute fails the parse.
        let html = "<mention-component entity_name=\"user_mention\" entity_identifier></mention-component>";
        assert_eq!(mention_ids_in_html(html), Some(vec!["".to_owned()]));
    }

    #[test]
    fn single_quoted_and_unquoted_values_match() {
        let html = "<mention-component entity_name='user_mention' entity_identifier='aaaa'></mention-component>\
            <mention-component entity_name=user_mention entity_identifier=bbbb></mention-component>";
        assert_eq!(
            mention_ids_in_html(html),
            Some(vec!["aaaa".to_owned(), "bbbb".to_owned()])
        );
    }

    #[test]
    fn registered_mail_tasks_route_local() {
        // `register_email_notification_tasks` owns exactly the two
        // `email_notification_task.py` names (PIDASHCONV-213); the other
        // nine D-07 names stay Python-owned until their own layer issues.
        fn ack() -> crate::worker::Handler {
            std::sync::Arc::new(|_: crate::queue::JobRow| {
                Box::pin(async { Ok(crate::worker::Verdict::Ack) })
                    as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
            })
        }
        let mut registry = Registry::new();
        register_email_notification_tasks(&mut registry, ack(), ack());
        assert_eq!(
            crate::worker::route_for(&registry, STACK_EMAIL_NOTIFICATION_TASK),
            crate::worker::Route::Local
        );
        assert_eq!(
            crate::worker::route_for(&registry, SEND_EMAIL_NOTIFICATION_TASK),
            crate::worker::Route::Local
        );
        assert!(!assert_python_owned(
            &registry,
            STACK_EMAIL_NOTIFICATION_TASK
        ));
        assert!(!assert_python_owned(
            &registry,
            SEND_EMAIL_NOTIFICATION_TASK
        ));
        assert!(assert_python_owned(&registry, NOTIFICATIONS_TASK));
    }
}
