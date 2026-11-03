#![forbid(unsafe_code)]

//! App issue-engagement serializers: activity, reaction-lite, subscriber, sync guards.
//!
//! Port of `apps/api/pi_dash/app/serializers/issue.py`:
//!
//! * `:63-76` (`_issue_is_actively_synced`)
//! * `:79-87` (`_comment_is_actively_synced`)
//! * `:105-123` (`IssueFlatSerializer`, nested by the activity serializer)
//! * `:521-541` (`IssueActivitySerializer`)
//! * `:909-916` (`IssueReactionLiteSerializer`)
//! * `:1441-1447` (`IssueSubscriberSerializer`)
//!
//! These are pure output shapes plus existence-probe decision kernels: each
//! `to_representation` takes a row borrowed from the caller and returns a
//! `serde::Serialize` view whose fields are the live DRF wire fields in DRF
//! order (declared fields first — base `id`, then the subclass declared —
//! then concrete columns, then forward relations). UUID and FK primary keys
//! render as strings (`PrimaryKeyRelatedField`, read-only); a null FK renders
//! `null`. Datetimes and dates cross this boundary already rendered as DRF
//! `iso-8601` strings — formatting owns to the DB edge, so rendering here is
//! a byte-exact passthrough.
//!
//! Nested details reuse the merged app ports (call, don't copy):
//! `actor_detail` is D-25's app `UserLiteSerializer` port
//! (`app_project::ser_shared`, `user.py:141-153`), `project_detail` /
//! `workspace_detail` are D-25's app lite ports (`app_project::ser_member`,
//! `project.py:120-133` / `workspace.py:79-83`). The space `lite.rs` twins
//! are NOT used: space workspace-lite lacks `logo_url` and space
//! project-lite carries `icon_prop`/`emoji` instead of
//! `cover_image_url`/`logo_props`/`is_default`. The app `IssueFlatSerializer`
//! (11 keys) is ported here as [`AppIssueFlatRow`] / [`AppIssueFlatView`]:
//! no merged port covers it — space `issue_flat` lacks `complexity_score` —
//! and no other D-26 sub-issue owns it (only FX-ISS-05 references it).
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `IssueSubscriberSerializer.read_only_fields` (`issue.py:1445`) constrains
//! writes, of which the shape ports have none. `IssueReactionLiteSerializer`'s
//! `DynamicBaseSerializer`
//! expand path is dead for this class: it has zero direct call sites (only
//! the `base.py:93,167` expansion-map references) and the map constructs it
//! without `expand` args, so `to_representation` always returns the plain
//! 5-key shape.
//!
//! Ported bugs (translate, don't redesign): none in these units — pure reads
//! plus existence probes; the Python is self-consistent here.

use serde::Serialize;

use crate::app_project::ser_member::{ProjectLiteView, WorkspaceLiteView};
use crate::app_project::ser_shared::UserLiteView;

/// `IssueActivitySerializer` wire keys (`issue.py:521-541`), in live-DRF
/// order: declared `id` (from `BaseSerializer`), the five subclass declared
/// fields, concrete `IssueActivity` columns (`db/models/issue.py:514-537`),
/// then forward relations (`created_by`, `updated_by`, `project`,
/// `workspace`, `issue`, `issue_comment`, `actor`).
///
/// NOTE (fixture erratum): FX-ISS-05 says "19 model fields + 5 = 24 keys",
/// forgetting that `id` is itself declared. Live Django 4.2.30 renders 25
/// keys; this const follows Django.
pub const ISSUE_ACTIVITY_ALL_FIELDS: [&str; 25] = [
    "id",
    "actor_detail",
    "issue_detail",
    "project_detail",
    "workspace_detail",
    "source_data",
    "created_at",
    "updated_at",
    "deleted_at",
    "verb",
    "field",
    "old_value",
    "new_value",
    "comment",
    "attachments",
    "old_identifier",
    "new_identifier",
    "epoch",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
    "issue_comment",
    "actor",
];

/// `IssueReactionLiteSerializer` wire keys (`issue.py:909-916`), in
/// `Meta.fields` order.
pub const ISSUE_REACTION_LITE_FIELDS: [&str; 5] =
    ["id", "actor", "issue", "reaction", "display_name"];

/// `IssueSubscriberSerializer` wire keys (`issue.py:1441-1447`), in live-DRF
/// order: declared `id`, concrete columns, then forward relations.
///
/// NOTE (fixture erratum): FX-ISS-05 lists model-definition order
/// (`created_by`/`updated_by` before `deleted_at`); live Django renders
/// `deleted_at` with the concrete columns, before the relations.
pub const ISSUE_SUBSCRIBER_ALL_FIELDS: [&str; 10] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
    "subscriber",
];

/// App `IssueFlatSerializer` wire keys (`issue.py:105-123`), in
/// `Meta.fields` order. The app twin of space `ISSUE_FLAT_FIELDS`: same keys
/// plus `complexity_score` after `priority`.
pub const APP_ISSUE_FLAT_FIELDS: [&str; 11] = [
    "id",
    "name",
    "description_json",
    "description_html",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "is_draft",
];

/// The `get_source_data` dict (`issue.py:528-535`): `source` / `source_email`
/// are nullable (`IntakeIssue.source :70`, `source_email :71`), `extra` is a
/// JSON object (`JSONField(default=dict) :74`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivitySourceData<'a> {
    pub source: Option<&'a str>,
    pub source_email: Option<&'a str>,
    pub extra: &'a serde_json::Value,
}

/// A database row for `IssueActivity` rendering (`db/models/issue.py:514-537`
/// over `ProjectBaseModel`).
///
/// `actor` / `issue` / `issue_comment` are nullable FKs (`:515,523-528,529-534`)
/// so their id columns and the `actor_detail` / `issue_detail` nests are
/// `Option` (a null source renders the nest as `null`). `project` /
/// `workspace` are non-null (`project.py:303-304`), hence plain views.
/// `source_data` is the resolved `get_source_data` value: the caller maps the
/// `activity.py:66-75` prefetch (attached non-empty `source_data` list) to its
/// first element and every other case — no prefetch, empty list, null issue —
/// to `None`, which is exactly the `:529` guard chain.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueActivityRow<'a> {
    pub id: &'a str,
    pub actor_detail: Option<UserLiteView<'a>>,
    pub issue_detail: Option<AppIssueFlatView<'a>>,
    pub project_detail: ProjectLiteView<'a>,
    pub workspace_detail: WorkspaceLiteView<'a>,
    pub source_data: Option<ActivitySourceData<'a>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub verb: &'a str,
    pub field: Option<&'a str>,
    pub old_value: Option<&'a str>,
    pub new_value: Option<&'a str>,
    pub comment: &'a str,
    pub attachments: Vec<&'a str>,
    pub old_identifier: Option<&'a str>,
    pub new_identifier: Option<&'a str>,
    pub epoch: Option<f64>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: Option<&'a str>,
    pub issue_comment: Option<&'a str>,
    pub actor: Option<&'a str>,
}

/// `IssueActivitySerializer.to_representation` output (`issue.py:521-541`), in
/// [`ISSUE_ACTIVITY_ALL_FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueActivityView<'a> {
    pub id: &'a str,
    pub actor_detail: Option<UserLiteView<'a>>,
    pub issue_detail: Option<AppIssueFlatView<'a>>,
    pub project_detail: ProjectLiteView<'a>,
    pub workspace_detail: WorkspaceLiteView<'a>,
    pub source_data: Option<ActivitySourceData<'a>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub verb: &'a str,
    pub field: Option<&'a str>,
    pub old_value: Option<&'a str>,
    pub new_value: Option<&'a str>,
    pub comment: &'a str,
    pub attachments: Vec<&'a str>,
    pub old_identifier: Option<&'a str>,
    pub new_identifier: Option<&'a str>,
    pub epoch: Option<f64>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: Option<&'a str>,
    pub issue_comment: Option<&'a str>,
    pub actor: Option<&'a str>,
}

/// Port of `IssueActivitySerializer` (`issue.py:521-541`).
pub fn issue_activity_to_representation<'a>(
    row: &'a IssueActivityRow<'a>,
) -> IssueActivityView<'a> {
    IssueActivityView {
        id: row.id,
        actor_detail: row.actor_detail.clone(),
        issue_detail: row.issue_detail.clone(),
        project_detail: row.project_detail.clone(),
        workspace_detail: row.workspace_detail.clone(),
        source_data: row.source_data.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        verb: row.verb,
        field: row.field,
        old_value: row.old_value,
        new_value: row.new_value,
        comment: row.comment,
        attachments: row.attachments.clone(),
        old_identifier: row.old_identifier,
        new_identifier: row.new_identifier,
        epoch: row.epoch,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        issue: row.issue,
        issue_comment: row.issue_comment,
        actor: row.actor,
    }
}

/// A database row for `IssueReaction` lite rendering: `actor` / `issue` are
/// non-null FKs (`db/models/issue.py:726-751`, `CASCADE`), `reaction` is
/// non-null text, and `display_name` is the resolved
/// `actor.display_name` (`issue.py:910`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueReactionLiteRow<'a> {
    pub id: &'a str,
    pub actor: &'a str,
    pub issue: &'a str,
    pub reaction: &'a str,
    pub display_name: &'a str,
}

/// `IssueReactionLiteSerializer.to_representation` output (`issue.py:909-916`),
/// in [`ISSUE_REACTION_LITE_FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssueReactionLiteView<'a> {
    pub id: &'a str,
    pub actor: &'a str,
    pub issue: &'a str,
    pub reaction: &'a str,
    pub display_name: &'a str,
}

/// Port of `IssueReactionLiteSerializer` (`issue.py:909-916`).
pub fn issue_reaction_lite_to_representation<'a>(
    row: &'a IssueReactionLiteRow<'a>,
) -> IssueReactionLiteView<'a> {
    IssueReactionLiteView {
        id: row.id,
        actor: row.actor,
        issue: row.issue,
        reaction: row.reaction,
        display_name: row.display_name,
    }
}

/// A database row for `IssueSubscriber` rendering
/// (`db/models/issue.py:700-724`): `project` / `workspace` / `issue` /
/// `subscriber` are non-null FKs; audit columns follow `AuditModel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueSubscriberRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub subscriber: &'a str,
}

/// `IssueSubscriberSerializer.to_representation` output (`issue.py:1441-1447`),
/// in [`ISSUE_SUBSCRIBER_ALL_FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssueSubscriberView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub subscriber: &'a str,
}

/// Port of `IssueSubscriberSerializer` (`issue.py:1441-1447`).
pub fn issue_subscriber_to_representation<'a>(
    row: &'a IssueSubscriberRow<'a>,
) -> IssueSubscriberView<'a> {
    IssueSubscriberView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        issue: row.issue,
        subscriber: row.subscriber,
    }
}

/// A database row for the app flat issue shape (`issue.py:105-123`):
/// `description_json` is a JSON object, `complexity_score` / `sequence_id`
/// are integers, `start_date` / `target_date` are nullable DRF dates
/// (passthrough), `sort_order` is a float, `is_draft` a bool.
#[derive(Debug, Clone, PartialEq)]
pub struct AppIssueFlatRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub is_draft: bool,
}

/// App `IssueFlatSerializer.to_representation` output (`issue.py:105-123`), in
/// [`APP_ISSUE_FLAT_FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppIssueFlatView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub is_draft: bool,
}

/// Port of the app `IssueFlatSerializer` (`issue.py:105-123`).
pub fn app_issue_flat_to_representation<'a>(row: &'a AppIssueFlatRow<'a>) -> AppIssueFlatView<'a> {
    AppIssueFlatView {
        id: row.id,
        name: row.name,
        description_json: row.description_json,
        description_html: row.description_html,
        priority: row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date,
        target_date: row.target_date,
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        is_draft: row.is_draft,
    }
}

/// Django `.exists()` probe for `GitIssueSync.objects.filter(issue=issue)`
/// (`issue.py:76`): `git_issue_syncs` (`db/models/integration/git.py:152-168`,
/// `issue_id` FK, `git_issue_sync_issue_idx`) with the default
/// `SoftDeletionManager` guard (`db/mixins.py:56-58` — the sync models carry
/// no `.objects` override, so soft-deleted rows do not count).
pub const GIT_ISSUE_SYNC_PROBE_SQL: &str =
    "SELECT 1 FROM git_issue_syncs WHERE issue_id = $1 AND deleted_at IS NULL LIMIT 1";

/// Django `.exists()` probe for `GithubIssueSync.objects.filter(issue=issue)`
/// (`issue.py:76`): `github_issue_syncs`
/// (`db/models/integration/github.py:76-97`) with the soft-delete guard.
pub const GITHUB_ISSUE_SYNC_PROBE_SQL: &str =
    "SELECT 1 FROM github_issue_syncs WHERE issue_id = $1 AND deleted_at IS NULL LIMIT 1";

/// Django `.exists()` probe for `GitCommentSync.objects.filter(comment=comment)`
/// (`issue.py:85`): `git_comment_syncs`
/// (`db/models/integration/git.py:190-206`) with the soft-delete guard.
pub const GIT_COMMENT_SYNC_PROBE_SQL: &str =
    "SELECT 1 FROM git_comment_syncs WHERE comment_id = $1 AND deleted_at IS NULL LIMIT 1";

/// Django `.exists()` probe for
/// `GithubCommentSync.objects.filter(comment=comment)` (`issue.py:86`):
/// `github_comment_syncs` (`db/models/integration/github.py:104-118`) with
/// the soft-delete guard.
pub const GITHUB_COMMENT_SYNC_PROBE_SQL: &str =
    "SELECT 1 FROM github_comment_syncs WHERE comment_id = $1 AND deleted_at IS NULL LIMIT 1";

/// Port of `_issue_is_actively_synced` (`issue.py:63-76`).
///
/// `external_source` is `issue.external_source` (`None` covers both
/// `issue is None` and a null source, `:67`); `annotated_is_synced` is the
/// `is_synced` annotation when the queryset carries it (`:70-72`). The two
/// probes run the [`GIT_ISSUE_SYNC_PROBE_SQL`] /
/// [`GITHUB_ISSUE_SYNC_PROBE_SQL`] checks; `FnOnce` closures (plus `||`)
/// preserve the Python short-circuit, so the probe counts are exactly
/// Django's: 0 when the source is empty or the annotation is present, 1 when
/// the git probe hits, 2 otherwise.
///
/// D-32's `api/src/app_intake/issues.rs:1243` copy is api-private and
/// unusable from services; this is the services copy, not a dup.
pub fn issue_is_actively_synced(
    external_source: Option<&str>,
    annotated_is_synced: Option<bool>,
    git_sync_exists: impl FnOnce() -> bool,
    github_sync_exists: impl FnOnce() -> bool,
) -> bool {
    let Some(source) = external_source else {
        return false;
    };
    if source.is_empty() {
        return false;
    }
    if let Some(annotated) = annotated_is_synced {
        return annotated;
    }
    git_sync_exists() || github_sync_exists()
}

/// Port of `_comment_is_actively_synced` (`issue.py:79-87`): the comment
/// twin has NO annotated shortcut — any set `external_source` runs the
/// probes. Probe/closure contract mirrors
/// [`issue_is_actively_synced`].
pub fn comment_is_actively_synced(
    external_source: Option<&str>,
    git_sync_exists: impl FnOnce() -> bool,
    github_sync_exists: impl FnOnce() -> bool,
) -> bool {
    let Some(source) = external_source else {
        return false;
    };
    if source.is_empty() {
        return false;
    }
    git_sync_exists() || github_sync_exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::cell::Cell;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_issues/serializers/FX-ISS-05.engage.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Top-level JSON key order of a struct's serialization, read off the
    /// serialized string: struct serialization always emits declaration
    /// order, while `Value` objects iterate alphabetically.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                }
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    fn lite_nests<'a>(
        logo_props: &'a Value,
        flat_description: &'a Value,
    ) -> (
        UserLiteView<'a>,
        AppIssueFlatView<'a>,
        ProjectLiteView<'a>,
        WorkspaceLiteView<'a>,
    ) {
        let actor = UserLiteView {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Fix",
            last_name: "Ture",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "fx-user",
        };
        let issue = AppIssueFlatView {
            id: "44444444-4444-4444-4444-444444444444",
            name: "I",
            description_json: flat_description,
            description_html: "<p>i</p>",
            priority: "high",
            complexity_score: 3,
            start_date: Some("2026-11-04"),
            target_date: None,
            sequence_id: 7,
            sort_order: 1.5,
            is_draft: false,
        };
        let project = ProjectLiteView {
            id: "33333333-3333-3333-3333-333333333333",
            identifier: "P1",
            name: "Proj",
            cover_image: None,
            cover_image_url: None,
            logo_props,
            description: "d",
            is_default: false,
        };
        let workspace = WorkspaceLiteView {
            name: "WS",
            slug: "ws",
            id: "22222222-2222-2222-2222-222222222222",
            logo_url: None,
        };
        (actor, issue, project, workspace)
    }

    #[test]
    fn reaction_lite_order_replays_fixture() {
        let golden = fixture();
        let pinned: Vec<String> = golden["issue_reaction_lite"]["fields_in_order"]
            .as_array()
            .expect("fields_in_order array")
            .iter()
            .map(|key| key.as_str().expect("key str").to_string())
            .collect();
        assert_eq!(
            ISSUE_REACTION_LITE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            pinned,
        );
    }

    #[test]
    fn activity_order_matches_live_django() {
        // Live Django 4.2.30 `IssueActivitySerializer().get_fields()` order
        // (in-venv probe; in-memory instances, no DB). The fixture's
        // "19 + 5 = 24" omits base-declared `id` — Django renders 25.
        assert_eq!(
            ISSUE_ACTIVITY_ALL_FIELDS.to_vec(),
            vec![
                "id",
                "actor_detail",
                "issue_detail",
                "project_detail",
                "workspace_detail",
                "source_data",
                "created_at",
                "updated_at",
                "deleted_at",
                "verb",
                "field",
                "old_value",
                "new_value",
                "comment",
                "attachments",
                "old_identifier",
                "new_identifier",
                "epoch",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "issue",
                "issue_comment",
                "actor",
            ],
        );
    }

    #[test]
    fn subscriber_order_matches_live_django() {
        // Live Django `IssueSubscriberSerializer().get_fields()` order. The
        // fixture lists model-definition order; Django groups `deleted_at`
        // with the concrete columns, before the forward relations.
        assert_eq!(
            ISSUE_SUBSCRIBER_ALL_FIELDS.to_vec(),
            vec![
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "issue",
                "subscriber",
            ],
        );
    }

    #[test]
    fn activity_full_replays_wire_bytes() {
        let logo_props = serde_json::json!({"a": 1});
        let flat_description = serde_json::json!({"x": 1});
        let (actor, issue, project, workspace) = lite_nests(&logo_props, &flat_description);
        let row = IssueActivityRow {
            id: "55555555-5555-5555-5555-555555555555",
            actor_detail: Some(actor),
            issue_detail: Some(issue),
            project_detail: project,
            workspace_detail: workspace,
            source_data: None,
            created_at: "2026-10-02T12:34:56.123456Z",
            updated_at: "2026-10-03T01:02:03Z",
            deleted_at: None,
            verb: "updated",
            field: Some("state"),
            old_value: Some("a"),
            new_value: Some("b"),
            comment: "c",
            attachments: vec!["https://x/y"],
            old_identifier: Some("66666666-6666-6666-6666-666666666666"),
            new_identifier: None,
            epoch: Some(1720000000.5),
            created_by: None,
            updated_by: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            issue: Some("44444444-4444-4444-4444-444444444444"),
            issue_comment: None,
            actor: Some("11111111-1111-1111-1111-111111111111"),
        };
        let view = issue_activity_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            ISSUE_ACTIVITY_ALL_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"55555555-5555-5555-5555-555555555555","actor_detail":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Fix","last_name":"Ture","avatar":"","avatar_url":null,"is_bot":false,"display_name":"fx-user"},"issue_detail":{"id":"44444444-4444-4444-4444-444444444444","name":"I","description_json":{"x":1},"description_html":"<p>i</p>","priority":"high","complexity_score":3,"start_date":"2026-11-04","target_date":null,"sequence_id":7,"sort_order":1.5,"is_draft":false},"project_detail":{"id":"33333333-3333-3333-3333-333333333333","identifier":"P1","name":"Proj","cover_image":null,"cover_image_url":null,"logo_props":{"a":1},"description":"d","is_default":false},"workspace_detail":{"name":"WS","slug":"ws","id":"22222222-2222-2222-2222-222222222222","logo_url":null},"source_data":null,"created_at":"2026-10-02T12:34:56.123456Z","updated_at":"2026-10-03T01:02:03Z","deleted_at":null,"verb":"updated","field":"state","old_value":"a","new_value":"b","comment":"c","attachments":["https://x/y"],"old_identifier":"66666666-6666-6666-6666-666666666666","new_identifier":null,"epoch":1720000000.5,"created_by":null,"updated_by":null,"project":"33333333-3333-3333-3333-333333333333","workspace":"22222222-2222-2222-2222-222222222222","issue":"44444444-4444-4444-4444-444444444444","issue_comment":null,"actor":"11111111-1111-1111-1111-111111111111"}"#,
        );
    }

    #[test]
    fn activity_sparse_renders_nulls_like_django() {
        let logo_props = serde_json::json!({"a": 1});
        let flat_description = serde_json::json!({"x": 1});
        let (_, _, project, workspace) = lite_nests(&logo_props, &flat_description);
        let row = IssueActivityRow {
            id: "55555555-5555-5555-5555-555555555555",
            actor_detail: None,
            issue_detail: None,
            project_detail: project,
            workspace_detail: workspace,
            source_data: None,
            created_at: "2026-10-02T12:34:56.123456Z",
            updated_at: "2026-10-03T01:02:03Z",
            deleted_at: Some("2026-10-03T01:02:03Z"),
            verb: "created",
            field: None,
            old_value: None,
            new_value: None,
            comment: "",
            attachments: Vec::new(),
            old_identifier: None,
            new_identifier: None,
            epoch: None,
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            updated_by: Some("11111111-1111-1111-1111-111111111111"),
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            issue: None,
            issue_comment: None,
            actor: None,
        };
        let view = issue_activity_to_representation(&row);
        let rendered = serde_json::to_string(&view).expect("serializes");
        assert_eq!(
            rendered,
            r#"{"id":"55555555-5555-5555-5555-555555555555","actor_detail":null,"issue_detail":null,"project_detail":{"id":"33333333-3333-3333-3333-333333333333","identifier":"P1","name":"Proj","cover_image":null,"cover_image_url":null,"logo_props":{"a":1},"description":"d","is_default":false},"workspace_detail":{"name":"WS","slug":"ws","id":"22222222-2222-2222-2222-222222222222","logo_url":null},"source_data":null,"created_at":"2026-10-02T12:34:56.123456Z","updated_at":"2026-10-03T01:02:03Z","deleted_at":"2026-10-03T01:02:03Z","verb":"created","field":null,"old_value":null,"new_value":null,"comment":"","attachments":[],"old_identifier":null,"new_identifier":null,"epoch":null,"created_by":"11111111-1111-1111-1111-111111111111","updated_by":"11111111-1111-1111-1111-111111111111","project":"33333333-3333-3333-3333-333333333333","workspace":"22222222-2222-2222-2222-222222222222","issue":null,"issue_comment":null,"actor":null}"#,
        );
    }

    #[test]
    fn activity_source_data_renders_prefetch_head() {
        // `get_source_data` (:528-535): the attached `source_data` list's
        // head renders as {source, source_email, extra}; absent without the
        // `activity_type=issue-property` prefetch.
        let extra = serde_json::json!({"k": "v"});
        let source = ActivitySourceData {
            source: Some("EMAIL"),
            source_email: Some("a@b.c"),
            extra: &extra,
        };
        assert_eq!(
            serde_json::to_string(&source).expect("serializes"),
            r#"{"source":"EMAIL","source_email":"a@b.c","extra":{"k":"v"}}"#,
        );
        let none: Option<ActivitySourceData<'_>> = None;
        assert_eq!(serde_json::to_string(&none).expect("serializes"), "null",);
    }

    #[test]
    fn reaction_lite_replays_wire_bytes() {
        let row = IssueReactionLiteRow {
            id: "77777777-7777-7777-7777-777777777777",
            actor: "11111111-1111-1111-1111-111111111111",
            issue: "44444444-4444-4444-4444-444444444444",
            reaction: "+1",
            display_name: "fx-user",
        };
        let view = issue_reaction_lite_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            ISSUE_REACTION_LITE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"77777777-7777-7777-7777-777777777777","actor":"11111111-1111-1111-1111-111111111111","issue":"44444444-4444-4444-4444-444444444444","reaction":"+1","display_name":"fx-user"}"#,
        );
    }

    #[test]
    fn subscriber_replays_wire_bytes() {
        let row = IssueSubscriberRow {
            id: "88888888-8888-8888-8888-888888888888",
            created_at: "2026-10-02T12:34:56.123456Z",
            updated_at: "2026-10-03T01:02:03Z",
            deleted_at: None,
            created_by: None,
            updated_by: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            issue: "44444444-4444-4444-4444-444444444444",
            subscriber: "11111111-1111-1111-1111-111111111111",
        };
        let view = issue_subscriber_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            ISSUE_SUBSCRIBER_ALL_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"88888888-8888-8888-8888-888888888888","created_at":"2026-10-02T12:34:56.123456Z","updated_at":"2026-10-03T01:02:03Z","deleted_at":null,"created_by":null,"updated_by":null,"project":"33333333-3333-3333-3333-333333333333","workspace":"22222222-2222-2222-2222-222222222222","issue":"44444444-4444-4444-4444-444444444444","subscriber":"11111111-1111-1111-1111-111111111111"}"#,
        );
    }

    #[test]
    fn issue_flat_replays_wire_bytes() {
        let description_json = serde_json::json!({"x": 1});
        let row = AppIssueFlatRow {
            id: "44444444-4444-4444-4444-444444444444",
            name: "I",
            description_json: &description_json,
            description_html: "<p>i</p>",
            priority: "high",
            complexity_score: 3,
            start_date: None,
            target_date: Some("2026-11-04"),
            sequence_id: 7,
            sort_order: 1.5,
            is_draft: false,
        };
        let view = app_issue_flat_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            APP_ISSUE_FLAT_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"44444444-4444-4444-4444-444444444444","name":"I","description_json":{"x":1},"description_html":"<p>i</p>","priority":"high","complexity_score":3,"start_date":null,"target_date":"2026-11-04","sequence_id":7,"sort_order":1.5,"is_draft":false}"#,
        );
    }

    /// Fixture `predicates._issue_is_actively_synced.goldens`: outcome plus
    /// the exact probe counts (`queries`), via counting closures.
    #[test]
    fn issue_predicate_replays_goldens() {
        let git_calls = Cell::new(0u32);
        let github_calls = Cell::new(0u32);
        let probes = |git_hit: bool, github_hit: bool| {
            git_calls.set(0);
            github_calls.set(0);
            let git_calls = &git_calls;
            let github_calls = &github_calls;
            let git = move || {
                git_calls.set(git_calls.get() + 1);
                git_hit
            };
            let github = move || {
                github_calls.set(github_calls.get() + 1);
                github_hit
            };
            (git, github)
        };

        // issue=None / external_source None/'' → False, 0 queries.
        let (git, github) = probes(true, true);
        assert!(!issue_is_actively_synced(None, None, git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (0, 0));
        let (git, github) = probes(true, true);
        assert!(!issue_is_actively_synced(Some(""), None, git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (0, 0));

        // Annotated is_synced True/False → bool(annotated), 0 queries.
        let (git, github) = probes(true, true);
        assert!(issue_is_actively_synced(
            Some("github"),
            Some(true),
            git,
            github
        ));
        assert_eq!((git_calls.get(), github_calls.get()), (0, 0));
        let (git, github) = probes(true, true);
        assert!(!issue_is_actively_synced(
            Some("github"),
            Some(false),
            git,
            github
        ));
        assert_eq!((git_calls.get(), github_calls.get()), (0, 0));

        // Set, no annotation, no sync rows → False, 2 queries.
        let (git, github) = probes(false, false);
        assert!(!issue_is_actively_synced(Some("github"), None, git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (1, 1));

        // Set, git row → True, 1 query (short-circuit).
        let (git, github) = probes(true, true);
        assert!(issue_is_actively_synced(Some("git"), None, git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (1, 0));

        // Set, github row only → True, 2 queries.
        let (git, github) = probes(false, true);
        assert!(issue_is_actively_synced(Some("github"), None, git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (1, 1));
    }

    /// Fixture `predicates._comment_is_actively_synced.goldens`: no annotated
    /// shortcut — any set source queries.
    #[test]
    fn comment_predicate_replays_goldens() {
        let git_calls = Cell::new(0u32);
        let github_calls = Cell::new(0u32);
        let probes = |git_hit: bool, github_hit: bool| {
            git_calls.set(0);
            github_calls.set(0);
            let git_calls = &git_calls;
            let github_calls = &github_calls;
            let git = move || {
                git_calls.set(git_calls.get() + 1);
                git_hit
            };
            let github = move || {
                github_calls.set(github_calls.get() + 1);
                github_hit
            };
            (git, github)
        };

        let (git, github) = probes(true, true);
        assert!(!comment_is_actively_synced(None, git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (0, 0));
        let (git, github) = probes(true, true);
        assert!(!comment_is_actively_synced(Some(""), git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (0, 0));

        let (git, github) = probes(false, false);
        assert!(!comment_is_actively_synced(Some("github"), git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (1, 1));

        let (git, github) = probes(true, false);
        assert!(comment_is_actively_synced(Some("git"), git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (1, 0));

        let (git, github) = probes(false, true);
        assert!(comment_is_actively_synced(Some("github"), git, github));
        assert_eq!((git_calls.get(), github_calls.get()), (1, 1));
    }

    #[test]
    fn probe_sql_targets_sync_tables() {
        for (sql, table, column) in [
            (GIT_ISSUE_SYNC_PROBE_SQL, "git_issue_syncs", "issue_id"),
            (
                GITHUB_ISSUE_SYNC_PROBE_SQL,
                "github_issue_syncs",
                "issue_id",
            ),
            (
                GIT_COMMENT_SYNC_PROBE_SQL,
                "git_comment_syncs",
                "comment_id",
            ),
            (
                GITHUB_COMMENT_SYNC_PROBE_SQL,
                "github_comment_syncs",
                "comment_id",
            ),
        ] {
            assert!(sql.contains(table), "probe names its sync table: {sql}");
            assert!(sql.contains(column), "probe filters its FK column: {sql}");
            assert!(
                sql.contains("deleted_at IS NULL"),
                "probe keeps the SoftDeletionManager guard: {sql}"
            );
        }
    }
}
