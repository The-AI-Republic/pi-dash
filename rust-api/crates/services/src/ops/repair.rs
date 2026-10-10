//! D-37 data-repair commands: decisions (stage 7, PIDASHCONV-808).
//!
//! Pure per-command logic behind the five repair commands
//! (`apps/api/pi_dash/db/management/commands/copy_issue_comment_to_description.py`,
//! `fix_duplicate_sequences.py`, `sync_issue_version.py`,
//! `sync_issue_description_version.py`, `update_deleted_workspace_slug.py`;
//! drift baseline `01a93e17216faea7bfc156b0f864cbbe420d1c52`). Every
//! function here is pure over injected rows and strings, so the F37-05 /
//! F37-06 vectors replay without a database. SQL text and execution live
//! in [`pidash_db::ops::repair`]; prompts, printing, exit codes and the
//! Celery publish live in the binary's `ops::repair`.
//!
//! This crate carries no `sqlx` dependency (see the `RunCreationStore`
//! precedent): nothing here touches a pool. Message text is exact —
//! verified byte for byte against live Django (piped, so no ANSI style
//! codes) — and every error string below is the text the binary prefixes
//! with `CommandError: ` on stderr (Django 4.2's `run_from_argv`
//! rendering; the F37-05 fixture's `Error: ` prefix predates it).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `batch_size` reaches the task as the raw `input()` string
//!   ([`sync_message`]).
//! * The slug success line prints the already-mutated slug twice
//!   ([`SlugDecision::Write`]).
//! * The countdown keeps CPython `int()` conversion
//!   ([`pidash_db::ops::repair::parse_py_int`]).

use chrono::{DateTime, Utc};
use serde_json::Map;
use uuid::Uuid;

use pidash_db::ops::repair::{
    parse_issue_identifier, parse_py_int, slug_already_stamped, stamped_slug,
    version_sync_kwarg_pairs, CommentRow, NewDescription, ProjectRow, WorkspaceRow,
    TASK_SCHEDULE_ISSUE_DESCRIPTION_VERSION, TASK_SCHEDULE_ISSUE_VERSION,
};

/// `copy` success line (`copy_issue_comment_to_description.py:53`).
pub const COPY_DONE_LINE: &str = "Successfully Copied IssueComment to Description";
/// `fix` final line (`fix_duplicate_sequences.py:93`).
pub const FIX_DONE_LINE: &str = "Sequence IDs updated successfully";
/// `sync_issue_version` success line (`sync_issue_version.py:21`).
pub const SYNC_VERSION_DONE_LINE: &str = "Successfully created issue version task";
/// `sync_issue_description_version` success line
/// (`sync_issue_description_version.py:23`).
pub const SYNC_DESCRIPTION_DONE_LINE: &str = "Successfully created issue description version task";

/// `fix` count line (`fix_duplicate_sequences.py:58`): the raw identifier
/// echoes back verbatim.
pub fn fix_found_line(found: usize, issue_identifier: &str) -> String {
    format!("{found} issues found with identifier {issue_identifier}")
}

/// Validated `fix` inputs, in Django's check order
/// (`fix_duplicate_sequences.py:28-47`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFixRequest {
    pub workspace_slug: String,
    pub project_identifier: String,
    pub sequence: i64,
}

/// Parse `fix` inputs. `Err` is the exact `CommandError` text.
pub fn parse_fix_request(
    workspace_slug: &str,
    issue_identifier: &str,
) -> Result<ParsedFixRequest, String> {
    if workspace_slug.is_empty() {
        return Err("Workspace slug is required".to_owned());
    }
    if issue_identifier.is_empty() {
        return Err("Issue identifier is required".to_owned());
    }
    let (project_identifier, sequence) = parse_issue_identifier(issue_identifier)?;
    Ok(ParsedFixRequest {
        workspace_slug: workspace_slug.to_owned(),
        project_identifier,
        sequence,
    })
}

/// Django `get()` 0/1/N decision over the fetched candidates
/// (`fix_duplicate_sequences.py:50`). `Err` is the exact `CommandError`
/// text.
pub fn resolve_project_id(rows: &[ProjectRow]) -> Result<Uuid, String> {
    match rows {
        [] => Err("Project matching query does not exist.".to_owned()),
        [single] => Ok(single.id),
        many => Err(format!(
            "get() returned more than one Project -- it returned {}!",
            many.len()
        )),
    }
}

/// Duplicate-count gate (`fix_duplicate_sequences.py:55-56`). `Err` is
/// the exact `CommandError` text.
pub fn check_duplicate_count(count: usize) -> Result<(), String> {
    if count > 1 {
        Ok(())
    } else {
        Err("No duplicate issues found with the given identifier".to_owned())
    }
}

/// One copy batch planned (`copy_issue_comment_to_description.py:29-51`):
/// the `Description` mapping plus the zip-aligned
/// `(comment_id, description_id)` links. `ids` must align with
/// `comments` (the caller mints one fresh `Uuid` per row, as Django's
/// client-side `uuid4` default does); `now` stamps both audit columns
/// (Django's `auto_now_add`/`auto_now` overwrite, one batch `now` — see
/// the db module docs).
pub fn plan_copy_batch(
    comments: Vec<CommentRow>,
    now: DateTime<Utc>,
    ids: Vec<Uuid>,
) -> (Vec<NewDescription>, Vec<(Uuid, Uuid)>) {
    assert_eq!(
        comments.len(),
        ids.len(),
        "one fresh id per comment (zip-aligned bulk_create)"
    );
    let mut descriptions = Vec::with_capacity(comments.len());
    let mut links = Vec::with_capacity(comments.len());
    for (comment, id) in comments.into_iter().zip(ids) {
        links.push((comment.id, id));
        descriptions.push(NewDescription {
            id,
            created_at: now,
            updated_at: now,
            description_json: comment.comment_json,
            description_html: comment.comment_html,
            description_stripped: comment.comment_stripped,
            project_id: Some(comment.project_id),
            created_by_id: comment.created_by_id,
            updated_by_id: comment.updated_by_id,
            workspace_id: comment.workspace_id,
        });
    }
    (descriptions, links)
}

/// Slug decision (`update_deleted_workspace_slug.py:30-69`): either a
/// line to print, or a write to apply and then announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlugDecision {
    /// Print the line; no write (`:33`, `:38-40`, `:45-49`, `:59`).
    Print(String),
    /// Apply the write, then print `success_line` (`:61-69`).
    Write {
        workspace_id: Uuid,
        name: String,
        new_slug: String,
        success_line: String,
    },
}

/// Decide the slug branch. Every line is byte-exact (verified against
/// live Django); the write path's line carries the ported
/// mutated-slug-twice bug (`:67`).
pub fn decide_slug(workspace: Option<&WorkspaceRow>, slug: &str, dry_run: bool) -> SlugDecision {
    let Some(workspace) = workspace else {
        return SlugDecision::Print(format!("Workspace with slug '{slug}' not found."));
    };
    if workspace.deleted_at.is_none() {
        return SlugDecision::Print(format!(
            "Workspace '{}' (slug: {}) is not deleted.",
            workspace.name, workspace.slug
        ));
    }
    if slug_already_stamped(&workspace.slug) {
        return SlugDecision::Print(format!(
            "Workspace '{}' (slug: {}) already has a timestamp appended.",
            workspace.name, workspace.slug
        ));
    }
    let deleted_at = workspace.deleted_at.expect("guarded above");
    let new_slug = stamped_slug(&workspace.slug, &deleted_at);
    if dry_run {
        return SlugDecision::Print(format!(
            "Would update workspace '{}' slug from '{}' to '{new_slug}'",
            workspace.name, workspace.slug
        ));
    }
    // Ported bug (:67): both slots show the NEW slug.
    let success_line = format!(
        "Updated workspace '{}' slug from '{new_slug}' to '{new_slug}'",
        workspace.name
    );
    SlugDecision::Write {
        workspace_id: workspace.id,
        name: workspace.name.clone(),
        new_slug,
        success_line,
    }
}

/// Save-failure line (`update_deleted_workspace_slug.py:71`): stdout,
/// exit 0. `{error}` is driver-specific (Django: psycopg text).
pub fn slug_save_error_line(name: &str, error: &dyn std::fmt::Display) -> String {
    format!("Error updating workspace '{name}': {error}")
}

/// Which version task to schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    IssueVersion,
    IssueDescriptionVersion,
}

impl SyncKind {
    fn task(&self) -> &'static str {
        match self {
            SyncKind::IssueVersion => TASK_SCHEDULE_ISSUE_VERSION,
            SyncKind::IssueDescriptionVersion => TASK_SCHEDULE_ISSUE_DESCRIPTION_VERSION,
        }
    }

    /// The success line for this task.
    pub fn done_line(&self) -> &'static str {
        match self {
            SyncKind::IssueVersion => SYNC_VERSION_DONE_LINE,
            SyncKind::IssueDescriptionVersion => SYNC_DESCRIPTION_DONE_LINE,
        }
    }
}

/// A `.delay` call planned: task name plus insertion-ordered kwargs.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncMessage {
    pub task: &'static str,
    pub kwargs: Map<String, serde_json::Value>,
}

/// Plan the `.delay` (`sync_issue_version.py:19`,
/// `sync_issue_description_version.py:21`). `batch_size` passes through
/// as the raw string (ported bug); the countdown keeps CPython `int()`.
/// `Err` is the `int()` failure text (Django tracebacks here; the binary
/// renders it as a `CommandError` instead — documented there).
pub fn sync_message(
    kind: SyncKind,
    batch_size: &str,
    countdown_raw: &str,
) -> Result<SyncMessage, String> {
    let countdown = parse_py_int(countdown_raw)?;
    let mut kwargs = Map::with_capacity(2);
    for (key, value) in version_sync_kwarg_pairs(batch_size, countdown) {
        kwargs.insert(key, value);
    }
    Ok(SyncMessage {
        task: kind.task(),
        kwargs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(slug: &str, deleted_at: Option<DateTime<Utc>>) -> WorkspaceRow {
        WorkspaceRow {
            id: Uuid::new_v4(),
            name: "Del WS".to_owned(),
            slug: slug.to_owned(),
            deleted_at,
        }
    }

    #[test]
    fn fix_request_validates_in_django_order() {
        let parsed = parse_fix_request("ws", "FX-7").unwrap();
        assert_eq!(parsed.workspace_slug, "ws");
        assert_eq!(parsed.project_identifier, "FX");
        assert_eq!(parsed.sequence, 7);
        assert_eq!(
            parse_fix_request("", "FX-7"),
            Err("Workspace slug is required".to_owned())
        );
        assert_eq!(
            parse_fix_request("ws", ""),
            Err("Issue identifier is required".to_owned())
        );
        assert_eq!(
            parse_fix_request("ws", "FX"),
            Err("Invalid issue identifier format".to_owned())
        );
        assert_eq!(
            parse_fix_request("ws", "FX-x"),
            Err("Invalid integer string".to_owned())
        );
        assert_eq!(
            fix_found_line(3, "FX-7"),
            "3 issues found with identifier FX-7"
        );
    }

    #[test]
    fn project_resolution_matches_get() {
        assert_eq!(
            resolve_project_id(&[]),
            Err("Project matching query does not exist.".to_owned())
        );
        let id = Uuid::new_v4();
        assert_eq!(resolve_project_id(&[ProjectRow { id }]), Ok(id));
        let rows = vec![ProjectRow { id }, ProjectRow { id: Uuid::new_v4() }];
        assert_eq!(
            resolve_project_id(&rows),
            Err("get() returned more than one Project -- it returned 2!".to_owned())
        );
        assert!(check_duplicate_count(2).is_ok());
        assert_eq!(
            check_duplicate_count(1),
            Err("No duplicate issues found with the given identifier".to_owned())
        );
        assert_eq!(
            check_duplicate_count(0),
            Err("No duplicate issues found with the given identifier".to_owned())
        );
    }

    #[test]
    fn copy_batch_maps_and_zip_aligns() {
        let now = Utc::now();
        let comments = vec![
            CommentRow {
                id: Uuid::new_v4(),
                comment_json: serde_json::json!({"a": 1}),
                comment_html: "<p>one</p>".to_owned(),
                comment_stripped: Some("one".to_owned()),
                project_id: Uuid::new_v4(),
                created_by_id: None,
                updated_by_id: Some(Uuid::new_v4()),
                workspace_id: Uuid::new_v4(),
            },
            CommentRow {
                id: Uuid::new_v4(),
                comment_json: serde_json::json!({}),
                comment_html: String::new(),
                comment_stripped: Some(String::new()),
                project_id: Uuid::new_v4(),
                created_by_id: None,
                updated_by_id: None,
                workspace_id: Uuid::new_v4(),
            },
        ];
        let ids = vec![Uuid::new_v4(), Uuid::new_v4()];
        let (descriptions, links) = plan_copy_batch(comments.clone(), now, ids.clone());
        assert_eq!(
            links,
            vec![(comments[0].id, ids[0]), (comments[1].id, ids[1])]
        );
        assert_eq!(descriptions[0].id, ids[0]);
        assert_eq!(descriptions[0].created_at, now);
        assert_eq!(descriptions[0].updated_at, now);
        assert_eq!(descriptions[0].description_json, comments[0].comment_json);
        assert_eq!(descriptions[0].description_html, "<p>one</p>");
        assert_eq!(descriptions[0].description_stripped, Some("one".to_owned()));
        assert_eq!(descriptions[0].project_id, Some(comments[0].project_id));
        assert_eq!(descriptions[0].created_by_id, None);
        assert_eq!(descriptions[0].updated_by_id, comments[0].updated_by_id);
        assert_eq!(descriptions[0].workspace_id, comments[0].workspace_id);
        assert_eq!(descriptions[1].description_html, "");
    }

    #[test]
    fn slug_branches_render_byte_exact() {
        assert_eq!(
            decide_slug(None, "nosuch", false),
            SlugDecision::Print("Workspace with slug 'nosuch' not found.".to_owned())
        );
        assert_eq!(
            decide_slug(Some(&workspace("live", None)), "live", false),
            SlugDecision::Print("Workspace 'Del WS' (slug: live) is not deleted.".to_owned())
        );
        let deleted = DateTime::parse_from_rfc3339("2024-05-06T07:08:09Z")
            .unwrap()
            .to_utc();
        assert_eq!(
            decide_slug(
                Some(&workspace("ws__1714976889", Some(deleted))),
                "ws__1714976889",
                false
            ),
            SlugDecision::Print(
                "Workspace 'Del WS' (slug: ws__1714976889) already has a timestamp appended."
                    .to_owned()
            )
        );
        assert_eq!(
            decide_slug(Some(&workspace("del", Some(deleted))), "del", true),
            SlugDecision::Print(
                "Would update workspace 'Del WS' slug from 'del' to 'del__1714979289'".to_owned()
            )
        );
        let ws = workspace("del", Some(deleted));
        let SlugDecision::Write {
            new_slug,
            success_line,
            name,
            workspace_id,
        } = decide_slug(Some(&ws), "del", false)
        else {
            panic!("expected a write");
        };
        assert_eq!(workspace_id, ws.id);
        assert_eq!(name, "Del WS");
        assert_eq!(new_slug, "del__1714979289");
        assert_eq!(
            success_line,
            "Updated workspace 'Del WS' slug from 'del__1714979289' to 'del__1714979289'"
        );
        assert_eq!(
            slug_save_error_line("Del WS", &"boom"),
            "Error updating workspace 'Del WS': boom"
        );
    }

    #[test]
    fn sync_messages_carry_task_kwargs_and_lines() {
        let msg = sync_message(SyncKind::IssueVersion, "5000", "300").unwrap();
        assert_eq!(msg.task, TASK_SCHEDULE_ISSUE_VERSION);
        let keys: Vec<&str> = msg.kwargs.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["batch_size", "countdown"]);
        assert_eq!(msg.kwargs["batch_size"], serde_json::json!("5000"));
        assert_eq!(msg.kwargs["countdown"], serde_json::json!(300));
        assert_eq!(SyncKind::IssueVersion.done_line(), SYNC_VERSION_DONE_LINE);

        let msg = sync_message(SyncKind::IssueDescriptionVersion, "100", " 60 ").unwrap();
        assert_eq!(msg.task, TASK_SCHEDULE_ISSUE_DESCRIPTION_VERSION);
        assert_eq!(msg.kwargs["countdown"], serde_json::json!(60));
        assert_eq!(
            SyncKind::IssueDescriptionVersion.done_line(),
            SYNC_DESCRIPTION_DONE_LINE
        );

        assert_eq!(
            sync_message(SyncKind::IssueVersion, "5", "soon"),
            Err("invalid literal for int() with base 10: 'soon'".to_owned())
        );
    }
}
