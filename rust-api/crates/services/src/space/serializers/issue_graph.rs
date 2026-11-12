//! Space issue-graph serializers: relations, links, comments, full issue.
//!
//! Port of `apps/api/pi_dash/space/serializer/issue.py` (470 lines), every
//! class except `LabelSerializer` / `LabelLiteSerializer` (owned by
//! PIDASHCONV-164, `taxonomy.rs`):
//!
//! * `issue.py:41-47` (`IssueStateFlatSerializer`)
//! * `issue.py:60-66` (`IssueProjectLiteSerializer`)
//! * `issue.py:69-75` (`IssueRelationSerializer`, source `related_issue`)
//! * `issue.py:78-84` (`RelatedIssueSerializer`, source `issue`)
//! * `issue.py:87-100` (`IssueCycleDetailSerializer`)
//! * `issue.py:103-116` (`IssueModuleDetailSerializer`)
//! * `issue.py:119-139` (`IssueLinkSerializer` + `create()` duplicate guard)
//! * `issue.py:142-154` (`IssueAttachmentSerializer`)
//! * `issue.py:157-161` (`IssueReactionSerializer`, 5-key)
//! * `issue.py:164-201` (`IssueSerializer`, `exclude = ["workpad"]`)
//! * `issue.py:204-220` (`IssueFlatSerializer`, 10-key — canonical owner;
//!   `intake.rs` keeps its frozen snapshot copy, also `ISSUE_FLAT_FIELDS`)
//! * `issue.py:223-228` (`CommentReactionLiteSerializer`)
//! * `issue.py:231-250` (`IssueCommentSerializer`)
//! * `issue.py:255-422` (`IssueCreateSerializer`: validate, create, update,
//!   `to_representation`)
//! * `issue.py:425-429` (`CommentReactionSerializer`)
//! * `issue.py:432-436` (`IssueVoteSerializer`, 5-key, all read-only)
//! * `issue.py:439-464` (`IssuePublicSerializer`)
//!
//! These are pure output shapes plus write-path decision kernels: each
//! `to_representation` takes a row borrowed from the caller and returns a
//! `serde::Serialize` view whose fields are the live DRF wire fields in
//! live-DRF order: `[pk] + declared(base-first) + concrete columns +
//! forward relations` (`ModelSerializer.get_default_field_names`, DRF
//! 3.15.2) — every FK and M2M trails after the last concrete column.
//! UUID and FK primary keys render as strings (`PrimaryKeyRelatedField`,
//! read-only); a null FK renders `null`. Datetimes and dates cross this
//! boundary already rendered as DRF `iso-8601` strings — formatting owns
//! to the DB edge, so rendering here is a byte-exact passthrough.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` (`issue.py:57,66,75,84,93-100,109-116,125-133,146-154,
//! 161,186-193,242-250,278-285,429,436,464`) constrain writes, of which the
//! shape ports have none. The write kernels below (`validate_create`,
//! `plan_create`, `plan_update`, `check_issue_link_duplicate`) are the
//! executable half of those constraints.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-link-guard (`issue.py:136-139`): `create()` matches the raw `url`
//!   exactly — a trailing slash or `http`-vs-`https` twin bypasses the
//!   guard, non-URL strings pass (no `URLValidator`), and there is no
//!   `update()` guard (contrast the app twin,
//!   `app/serializers/issue.py:835-849`, which normalizes and guards
//!   updates). [`check_issue_link_duplicate`] ports the exact-match,
//!   create-only check.
//! * BUG-pod-dangle (`issue.py:195-201`): `get_assigned_pod_detail` has no
//!   guard — a dangling `assigned_pod` FK raises instead of rendering
//!   `None`. Callers resolve the pod before building the row; a missing pod
//!   for a non-null FK is a caller error, like the Python raise.
//! * BUG-default-assignee (`issue.py:346-356`): the empty/`None` assignee
//!   fallback assigns `default_assignee_id` with NO membership check
//!   (contrast the app twin, which requires role >= 15,
//!   `app/serializers/issue.py:422-430`). [`CreatePlan::fallback_assignee`]
//!   ports the unchecked fallback.
//! * BUG-stale-echo-inverted (`issue.py:287-291`): `to_representation`
//!   reads LIVE `assignees`/`labels` relations (DB-truthful); the app twin
//!   echoes `initial_data` (can echo ids the write filtered out). Opposite
//!   staleness directions — ported as documented behavior.

use serde::Serialize;

use super::lite::{ProjectLiteView, StateLiteView, StateView, UserLiteView, WorkspaceLiteView};
use super::taxonomy::{CycleView, LabelView, ModuleView};

/// Re-export of the label lite shape owned by PIDASHCONV-164
/// (`taxonomy.rs`, `issue.py:467-470`, `[id, name, color]`): the
/// `LabelLiteSerializer` fixture case resolves here so the shape is defined
/// exactly once.
pub use super::taxonomy::{label_lite_to_representation, LabelLiteRow, LabelLiteView};

/// `IssueStateFlatSerializer` field list (`issue.py:41-47`, `Meta.fields`
/// order): `id`, `sequence_id`, `name`, then the two declared nests.
pub const ISSUE_STATE_FLAT_FIELDS: [&str; 5] = [
    "id",
    "sequence_id",
    "name",
    "state_detail",
    "project_detail",
];

/// A database row for the state-flat issue nest.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueStateFlatRow<'a> {
    pub id: &'a str,
    pub sequence_id: i32,
    pub name: &'a str,
    pub state_detail: Option<StateLiteView<'a>>,
    pub project_detail: ProjectLiteView<'a>,
}

/// `IssueStateFlatSerializer.to_representation` output (`issue.py:41-47`):
/// `state_detail` is `StateLiteSerializer(source="state")`,
/// `project_detail` is `ProjectLiteSerializer(source="project")`.
/// `Issue.state` is nullable (`db/models/issue.py:122-128`), so a null
/// source renders `state_detail: null` (DRF renders `None` for a null nest
/// source); `Issue.project` is required, so `project_detail` is always an
/// object.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueStateFlatView<'a> {
    pub id: &'a str,
    pub sequence_id: i32,
    pub name: &'a str,
    pub state_detail: Option<StateLiteView<'a>>,
    pub project_detail: ProjectLiteView<'a>,
}

/// Port of `IssueStateFlatSerializer` (`issue.py:41-47`).
pub fn issue_state_flat_to_representation<'a>(
    row: &'a IssueStateFlatRow<'a>,
) -> IssueStateFlatView<'a> {
    IssueStateFlatView {
        id: row.id,
        sequence_id: row.sequence_id,
        name: row.name,
        state_detail: row.state_detail.clone(),
        project_detail: row.project_detail.clone(),
    }
}

/// `IssueProjectLiteSerializer` field list (`issue.py:60-66`,
/// `Meta.fields` order, all read-only).
pub const ISSUE_PROJECT_LITE_FIELDS: [&str; 4] = ["id", "project_detail", "name", "sequence_id"];

/// A database row for the project-lite issue nest.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueProjectLiteRow<'a> {
    pub id: &'a str,
    pub project_detail: ProjectLiteView<'a>,
    pub name: &'a str,
    pub sequence_id: i32,
}

/// `IssueProjectLiteSerializer.to_representation` output (`issue.py:60-66`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueProjectLiteView<'a> {
    pub id: &'a str,
    pub project_detail: ProjectLiteView<'a>,
    pub name: &'a str,
    pub sequence_id: i32,
}

/// Port of `IssueProjectLiteSerializer` (`issue.py:60-66`).
pub fn issue_project_lite_to_representation<'a>(
    row: &'a IssueProjectLiteRow<'a>,
) -> IssueProjectLiteView<'a> {
    IssueProjectLiteView {
        id: row.id,
        project_detail: row.project_detail.clone(),
        name: row.name,
        sequence_id: row.sequence_id,
    }
}

/// The relation serializers' shared field list (`issue.py:69-84`,
/// `Meta.fields` order): the declared nest first, then the model columns
/// `relation_type`, `related_issue`, `issue`, `id` (definition order,
/// `db/models/issue.py:396-401`).
pub const ISSUE_RELATION_FIELDS: [&str; 5] = [
    "issue_detail",
    "relation_type",
    "related_issue",
    "issue",
    "id",
];

/// A database row for an issue-relation edge: the embedded endpoint plus the
/// raw FK uuid strings and the relation type.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueRelationRow<'a> {
    pub issue_detail: IssueProjectLiteView<'a>,
    pub relation_type: &'a str,
    pub related_issue: &'a str,
    pub issue: &'a str,
    pub id: &'a str,
}

/// `IssueRelationSerializer.to_representation` output (`issue.py:69-75`):
/// `issue_detail` is `IssueProjectLiteSerializer(source="related_issue")` —
/// the OUTGOING endpoint. Raw `related_issue`/`issue` FK ids render
/// alongside the nest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueRelationView<'a> {
    pub issue_detail: IssueProjectLiteView<'a>,
    pub relation_type: &'a str,
    pub related_issue: &'a str,
    pub issue: &'a str,
    pub id: &'a str,
}

/// Port of `IssueRelationSerializer` (`issue.py:69-75`).
pub fn issue_relation_to_representation<'a>(
    row: &'a IssueRelationRow<'a>,
) -> IssueRelationView<'a> {
    IssueRelationView {
        issue_detail: row.issue_detail.clone(),
        relation_type: row.relation_type,
        related_issue: row.related_issue,
        issue: row.issue,
        id: row.id,
    }
}

/// `RelatedIssueSerializer.to_representation` output (`issue.py:78-84`):
/// same wire keys as [`IssueRelationView`], but `issue_detail` is
/// `IssueProjectLiteSerializer(source="issue")` — the INCOMING endpoint.
/// One struct serves both (same `Meta`); the two constructors record which
/// endpoint the caller resolved.
pub fn related_issue_to_representation<'a>(row: &'a IssueRelationRow<'a>) -> IssueRelationView<'a> {
    issue_relation_to_representation(row)
}

/// The `CycleIssue` `fields = "__all__"` wire keys (`issue.py:87-100`), in
/// live-DRF order (probed `IssueCycleDetailSerializer().fields` minus the
/// declared nest): `id`, the concrete audit datetimes, then every column
/// trailing as relations — `created_by`, `updated_by`, `project`,
/// `workspace`, `issue`, `cycle` (`db/models/cycle.py:104-110`).
pub const CYCLE_ISSUE_ALL_FIELDS: [&str; 10] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
    "cycle",
];

/// A database row for `CycleIssue` rendering: required `issue`/`cycle` FK
/// uuid strings plus the embedded [`CycleView`].
#[derive(Debug, Clone, PartialEq)]
pub struct IssueCycleDetailRow<'a> {
    pub id: &'a str,
    pub cycle_detail: CycleView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub cycle: &'a str,
}

/// `IssueCycleDetailSerializer.to_representation` output (`issue.py:87-100`),
/// in live-DRF wire order (probed): `id`, the declared `cycle_detail` nest
/// (`CycleBaseSerializer(source="cycle")`), then the concrete datetimes,
/// then the trailing relations.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueCycleDetailView<'a> {
    pub id: &'a str,
    pub cycle_detail: CycleView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
    pub cycle: &'a str,
}

/// Port of `IssueCycleDetailSerializer` (`issue.py:87-100`).
pub fn issue_cycle_detail_to_representation<'a>(
    row: &'a IssueCycleDetailRow<'a>,
) -> IssueCycleDetailView<'a> {
    IssueCycleDetailView {
        id: row.id,
        cycle_detail: row.cycle_detail.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        issue: row.issue,
        cycle: row.cycle,
    }
}

/// The `ModuleIssue` `fields = "__all__"` wire keys (`issue.py:103-116`), in
/// live-DRF order (probed `IssueModuleDetailSerializer().fields` minus the
/// declared nest): `id`, the concrete audit datetimes, then every column
/// trailing as relations — `created_by`, `updated_by`, `project`,
/// `workspace`, `module`, `issue` (`db/models/module.py:152-154`).
pub const MODULE_ISSUE_ALL_FIELDS: [&str; 10] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "module",
    "issue",
];

/// A database row for `ModuleIssue` rendering: required `module`/`issue` FK
/// uuid strings plus the embedded [`ModuleView`].
#[derive(Debug, Clone, PartialEq)]
pub struct IssueModuleDetailRow<'a> {
    pub id: &'a str,
    pub module_detail: ModuleView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub module: &'a str,
    pub issue: &'a str,
}

/// `IssueModuleDetailSerializer.to_representation` output
/// (`issue.py:103-116`), in live-DRF wire order (probed): `id`, the
/// declared `module_detail` nest (`ModuleBaseSerializer(source="module")`),
/// then the concrete datetimes, then the trailing relations.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueModuleDetailView<'a> {
    pub id: &'a str,
    pub module_detail: ModuleView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub module: &'a str,
    pub issue: &'a str,
}

/// Port of `IssueModuleDetailSerializer` (`issue.py:103-116`).
pub fn issue_module_detail_to_representation<'a>(
    row: &'a IssueModuleDetailRow<'a>,
) -> IssueModuleDetailView<'a> {
    IssueModuleDetailView {
        id: row.id,
        module_detail: row.module_detail.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        module: row.module,
        issue: row.issue,
    }
}

/// The `IssueLink` `fields = "__all__"` wire keys (`issue.py:119-139`), in
/// live-DRF order (probed `IssueLinkSerializer().fields` minus the declared
/// nest): `id`, the concrete columns (`created_at`, `updated_at`,
/// `deleted_at`, `title`, `url`, `metadata`), then the forward relations
/// trailing (`created_by`, `updated_by`, `project`, `workspace`, `issue`).
pub const ISSUE_LINK_ALL_FIELDS: [&str; 12] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "title",
    "url",
    "metadata",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
];

/// A database row for `IssueLink` rendering. `title` is nullable
/// (`db/models/issue.py:472`); `url` is a required `TextField` (`:473`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueLinkRow<'a> {
    pub id: &'a str,
    pub created_by_detail: UserLiteView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub title: Option<&'a str>,
    pub url: &'a str,
    pub metadata: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
}

/// `IssueLinkSerializer.to_representation` output (`issue.py:119-139`), in
/// live-DRF wire order (probed): `id`, the declared `created_by_detail`
/// nest (`UserLiteSerializer(source="created_by")`), then the concrete
/// columns, then the trailing relations.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueLinkView<'a> {
    pub id: &'a str,
    pub created_by_detail: UserLiteView<'a>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub title: Option<&'a str>,
    pub url: &'a str,
    pub metadata: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub issue: &'a str,
}

/// Port of `IssueLinkSerializer` (`issue.py:119-139`). Field-for-field copy.
pub fn issue_link_to_representation<'a>(row: &'a IssueLinkRow<'a>) -> IssueLinkView<'a> {
    IssueLinkView {
        id: row.id,
        created_by_detail: row.created_by_detail.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        title: row.title,
        url: row.url,
        metadata: row.metadata,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        issue: row.issue,
    }
}

/// The duplicate-URL `ValidationError` message (`issue.py:138`):
/// `"URL already exists for this Issue"`.
pub const ISSUE_LINK_DUPLICATE_MESSAGE: &str = "URL already exists for this Issue";

/// The duplicate-URL guard's HTTP status (`issue.py:136-139`, golden
/// `duplicate_error` status).
pub const ISSUE_LINK_DUPLICATE_STATUS: u16 = 400;

/// Failure of [`check_issue_link_duplicate`]: the exact-match guard fired.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("URL already exists for this Issue")]
pub struct DuplicateIssueLink;

/// Port of `IssueLinkSerializer.create()` (`issue.py:136-139`):
/// `IssueLink.objects.filter(url=..., issue_id=...).exists()` raises
/// `ValidationError({"error": ...})`. The caller owns the `EXISTS` query
/// (same SQL semantics: exact match on the raw `url` string within the
/// issue — BUG-link-guard above); `url_exists` is its boolean result.
pub fn check_issue_link_duplicate(url_exists: bool) -> Result<(), DuplicateIssueLink> {
    if url_exists {
        return Err(DuplicateIssueLink);
    }
    Ok(())
}

/// The `FileAsset` `fields = "__all__"` wire keys (`issue.py:142-154`), in
/// live-DRF order (probed `IssueAttachmentSerializer().fields`): `id`, the
/// concrete columns (`created_at`, `updated_at`, `deleted_at`, then
/// `FileAsset`'s own columns, `db/models/asset.py:45-62`), then the forward
/// relations trailing (`created_by`, `updated_by`, `user`, `workspace`,
/// `draft_issue`, `project`, `issue`, `comment`, `page`).
pub const FILE_ASSET_ALL_FIELDS: [&str; 24] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "attributes",
    "asset",
    "entity_type",
    "entity_identifier",
    "is_deleted",
    "is_archived",
    "external_id",
    "external_source",
    "size",
    "is_uploaded",
    "storage_metadata",
    "created_by",
    "updated_by",
    "user",
    "workspace",
    "draft_issue",
    "project",
    "issue",
    "comment",
    "page",
];

/// A database row for `FileAsset` rendering. Every FK (`user`, `workspace`,
/// `draft_issue`, `project`, `issue`, `comment`, `page`) is nullable
/// (`asset.py:47-53`); `asset` is the stored file path
/// (`FileField(upload_to=...)`, `asset.py:46`); `size` is a float
/// (`asset.py:60`); `storage_metadata` is nullable JSON (`asset.py:62`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueAttachmentRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub attributes: &'a serde_json::Value,
    pub asset: &'a str,
    pub entity_type: Option<&'a str>,
    pub entity_identifier: Option<&'a str>,
    pub is_deleted: bool,
    pub is_archived: bool,
    pub external_id: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub size: f64,
    pub is_uploaded: bool,
    pub storage_metadata: Option<&'a serde_json::Value>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub draft_issue: Option<&'a str>,
    pub project: Option<&'a str>,
    pub issue: Option<&'a str>,
    pub comment: Option<&'a str>,
    pub page: Option<&'a str>,
}

/// `IssueAttachmentSerializer.to_representation` output (`issue.py:142-154`,
/// `fields = "__all__"`), in live-DRF wire order: no declared nests
/// (contrast the app twin, which adds an `asset_url` extra — port the
/// absence).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueAttachmentView<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub attributes: &'a serde_json::Value,
    pub asset: &'a str,
    pub entity_type: Option<&'a str>,
    pub entity_identifier: Option<&'a str>,
    pub is_deleted: bool,
    pub is_archived: bool,
    pub external_id: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub size: f64,
    pub is_uploaded: bool,
    pub storage_metadata: Option<&'a serde_json::Value>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub draft_issue: Option<&'a str>,
    pub project: Option<&'a str>,
    pub issue: Option<&'a str>,
    pub comment: Option<&'a str>,
    pub page: Option<&'a str>,
}

/// Port of `IssueAttachmentSerializer` (`issue.py:142-154`).
/// Field-for-field copy.
pub fn issue_attachment_to_representation<'a>(
    row: &'a IssueAttachmentRow<'a>,
) -> IssueAttachmentView<'a> {
    IssueAttachmentView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        attributes: row.attributes,
        asset: row.asset,
        entity_type: row.entity_type,
        entity_identifier: row.entity_identifier,
        is_deleted: row.is_deleted,
        is_archived: row.is_archived,
        external_id: row.external_id,
        external_source: row.external_source,
        size: row.size,
        is_uploaded: row.is_uploaded,
        storage_metadata: row.storage_metadata,
        created_by: row.created_by,
        updated_by: row.updated_by,
        user: row.user,
        workspace: row.workspace,
        draft_issue: row.draft_issue,
        project: row.project,
        issue: row.issue,
        comment: row.comment,
        page: row.page,
    }
}

/// `IssueReactionSerializer` field list (`issue.py:157-161`,
/// `Meta.fields` order — the wire order, NOT model order): only `reaction`
/// is writable; `issue`, `workspace`, `project`, `actor` are read-only.
/// No `actor_detail`, no `id` (contrast the app twin).
pub const ISSUE_REACTION_FIELDS: [&str; 5] = ["issue", "reaction", "workspace", "project", "actor"];

/// A database row for `IssueReaction` rendering
/// (`db/models/issue.py:726-733`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueReactionRow<'a> {
    pub issue: &'a str,
    pub reaction: &'a str,
    pub workspace: &'a str,
    pub project: &'a str,
    pub actor: &'a str,
}

/// `IssueReactionSerializer.to_representation` output (`issue.py:157-161`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueReactionView<'a> {
    pub issue: &'a str,
    pub reaction: &'a str,
    pub workspace: &'a str,
    pub project: &'a str,
    pub actor: &'a str,
}

/// Port of `IssueReactionSerializer` (`issue.py:157-161`).
pub fn issue_reaction_to_representation<'a>(
    row: &'a IssueReactionRow<'a>,
) -> IssueReactionView<'a> {
    IssueReactionView {
        issue: row.issue,
        reaction: row.reaction,
        workspace: row.workspace,
        project: row.project,
        actor: row.actor,
    }
}

/// The `Issue` model field set minus `workpad`: the `exclude = ["workpad"]`
/// body shared by `IssueSerializer` (`issue.py:180-185`) and
/// `IssueCreateSerializer` (`issue.py:273-277`). Single source of truth in
/// `intake.rs` (same model, same exclusion); re-exported here so the
/// issue-graph key-set tests pin one list.
pub use super::intake::ISSUE_STATE_INTAKE_MODEL_FIELDS as ISSUE_MODEL_FIELDS_NO_WORKPAD;

/// The `assigned_pod_detail` shape (`issue.py:195-201`): `None` when
/// `assigned_pod_id` is `None`, else `PodMiniSerializer`
/// (`runner/serializers.py:133-142`, `fields = ["id", "name", "is_default",
/// "project", "project_identifier"]` where `project` is the FK uuid and
/// `project_identifier` is `project.identifier`). Recorded shape only — the
/// lazy import and DB fetch stay caller-side.
pub const POD_MINI_FIELDS: [&str; 5] =
    ["id", "name", "is_default", "project", "project_identifier"];

/// A resolved `PodMiniSerializer` row.
#[derive(Debug, Clone, PartialEq)]
pub struct PodMiniRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub is_default: bool,
    pub project: &'a str,
    pub project_identifier: &'a str,
}

/// `PodMiniSerializer.to_representation` output
/// (`runner/serializers.py:133-142`), in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PodMiniView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub is_default: bool,
    pub project: &'a str,
    pub project_identifier: &'a str,
}

/// Record of `get_assigned_pod_detail` (`issue.py:195-201`): the caller
/// resolved the pod (or found `assigned_pod_id` null); this renders the
/// recorded shape. A non-null FK with no resolvable pod raises in Python
/// (BUG-pod-dangle) — the caller must not build `Some` without a row.
pub fn pod_mini_to_representation<'a>(row: &'a PodMiniRow<'a>) -> PodMiniView<'a> {
    PodMiniView {
        id: row.id,
        name: row.name,
        is_default: row.is_default,
        project: row.project,
        project_identifier: row.project_identifier,
    }
}

/// The 14 declared nests of `IssueSerializer` in declaration order
/// (`issue.py:165-178` plus `assigned_pod_detail` at `:195-201`, golden
/// `declared_nests_in_order`).
pub const ISSUE_DECLARED_NESTS: [&str; 14] = [
    "project_detail",
    "state_detail",
    "parent_detail",
    "label_details",
    "assignee_details",
    "related_issues",
    "issue_relations",
    "issue_cycle",
    "issue_module",
    "issue_link",
    "issue_attachment",
    "sub_issues_count",
    "issue_reactions",
    "assigned_pod_detail",
];

/// A database row for the full issue serializer. Nullable FKs (`parent`,
/// `state`, `assigned_pod`) pair with `Option` nests; `issue_cycle` /
/// `issue_module` are singular reverse edges (the views resolve them to a
/// single object, no `many=True`, `issue.py:172-173`); `sub_issues_count`
/// is the annotated read-only integer
/// (`space/views/issue.py:119`, `Issue.issue_objects.filter(parent=...)`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueRow<'a> {
    pub id: &'a str,
    pub project_detail: ProjectLiteView<'a>,
    pub state_detail: Option<StateView<'a>>,
    pub parent_detail: Option<IssueStateFlatView<'a>>,
    pub label_details: Vec<LabelView<'a>>,
    pub assignee_details: Vec<UserLiteView<'a>>,
    pub related_issues: Vec<IssueRelationView<'a>>,
    pub issue_relations: Vec<IssueRelationView<'a>>,
    pub issue_cycle: Option<IssueCycleDetailView<'a>>,
    pub issue_module: Option<IssueModuleDetailView<'a>>,
    pub issue_link: Vec<IssueLinkView<'a>>,
    pub issue_attachment: Vec<IssueAttachmentView<'a>>,
    pub sub_issues_count: i64,
    pub issue_reactions: Vec<IssueReactionView<'a>>,
    pub assigned_pod_detail: Option<PodMiniView<'a>>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub point: Option<i32>,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub description_binary: Option<&'a str>,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub is_draft: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub git_work_branch: &'a str,
    pub created_via: Option<&'a str>,
    pub agent_executor: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub parent: Option<&'a str>,
    pub state: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub r#type: Option<&'a str>,
    pub assigned_pod: Option<&'a str>,
    pub assignees: Vec<&'a str>,
    pub labels: Vec<&'a str>,
}

/// `IssueSerializer.to_representation` output (`issue.py:164-201`), in
/// live-DRF wire order (probed): `id`, the 14 declared fields (DRF
/// `[pk] + declared + fields + relations`), then the concrete `Issue`
/// columns, then the trailing relations. `state_detail` is the FULL
/// `StateSerializer` (18-key, not Lite); `parent_detail` is
/// `IssueStateFlatSerializer(source="parent")`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueView<'a> {
    pub id: &'a str,
    pub project_detail: ProjectLiteView<'a>,
    pub state_detail: Option<StateView<'a>>,
    pub parent_detail: Option<IssueStateFlatView<'a>>,
    pub label_details: Vec<LabelView<'a>>,
    pub assignee_details: Vec<UserLiteView<'a>>,
    pub related_issues: Vec<IssueRelationView<'a>>,
    pub issue_relations: Vec<IssueRelationView<'a>>,
    pub issue_cycle: Option<IssueCycleDetailView<'a>>,
    pub issue_module: Option<IssueModuleDetailView<'a>>,
    pub issue_link: Vec<IssueLinkView<'a>>,
    pub issue_attachment: Vec<IssueAttachmentView<'a>>,
    pub sub_issues_count: i64,
    pub issue_reactions: Vec<IssueReactionView<'a>>,
    pub assigned_pod_detail: Option<PodMiniView<'a>>,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub point: Option<i32>,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub description_binary: Option<&'a str>,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub archived_at: Option<&'a str>,
    pub is_draft: bool,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub git_work_branch: &'a str,
    pub created_via: Option<&'a str>,
    pub agent_executor: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub parent: Option<&'a str>,
    pub state: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub r#type: Option<&'a str>,
    pub assigned_pod: Option<&'a str>,
    pub assignees: Vec<&'a str>,
    pub labels: Vec<&'a str>,
}

/// Port of `IssueSerializer` (`issue.py:164-201`). Field-for-field copy;
/// `workpad` is absent by construction (`Meta.exclude = ["workpad"]`,
/// `issue.py:182-185`: the agent scratchpad must never leak to guest Space
/// readers).
pub fn issue_to_representation<'a>(row: &'a IssueRow<'a>) -> IssueView<'a> {
    IssueView {
        id: row.id,
        project_detail: row.project_detail.clone(),
        state_detail: row.state_detail.clone(),
        parent_detail: row.parent_detail.clone(),
        label_details: row.label_details.clone(),
        assignee_details: row.assignee_details.clone(),
        related_issues: row.related_issues.clone(),
        issue_relations: row.issue_relations.clone(),
        issue_cycle: row.issue_cycle.clone(),
        issue_module: row.issue_module.clone(),
        issue_link: row.issue_link.clone(),
        issue_attachment: row.issue_attachment.clone(),
        sub_issues_count: row.sub_issues_count,
        issue_reactions: row.issue_reactions.clone(),
        assigned_pod_detail: row.assigned_pod_detail.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        point: row.point,
        name: row.name,
        description_json: row.description_json,
        description_html: row.description_html,
        description_stripped: row.description_stripped,
        description_binary: row.description_binary,
        priority: row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date,
        target_date: row.target_date,
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        completed_at: row.completed_at,
        archived_at: row.archived_at,
        is_draft: row.is_draft,
        external_source: row.external_source,
        external_id: row.external_id,
        git_work_branch: row.git_work_branch,
        created_via: row.created_via,
        agent_executor: row.agent_executor,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        parent: row.parent,
        state: row.state,
        estimate_point: row.estimate_point,
        r#type: row.r#type,
        assigned_pod: row.assigned_pod,
        assignees: row.assignees.clone(),
        labels: row.labels.clone(),
    }
}

/// `IssueFlatSerializer` field list (`space/serializer/issue.py:204-220`):
/// flat issue columns only — notably NO `complexity_score` (the app twin
/// adds it). Canonical owner: this module (`intake.rs` keeps its frozen
/// snapshot copy for `IntakeIssueSerializer.issue_detail`, which predates
/// this issue and is read-only here).
pub const ISSUE_FLAT_FIELDS: [&str; 10] = [
    "id",
    "name",
    "description_json",
    "description_html",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "is_draft",
];

/// A database row for the flat issue shape.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueFlatRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub priority: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub is_draft: bool,
}

/// `IssueFlatSerializer.to_representation` output (`issue.py:204-220`), in
/// `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueFlatView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description_json: &'a serde_json::Value,
    pub description_html: &'a str,
    pub priority: &'a str,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub is_draft: bool,
}

/// Port of `IssueFlatSerializer` (`space/serializer/issue.py:204-220`).
pub fn issue_flat_to_representation<'a>(row: &'a IssueFlatRow<'a>) -> IssueFlatView<'a> {
    IssueFlatView {
        id: row.id,
        name: row.name,
        description_json: row.description_json,
        description_html: row.description_html,
        priority: row.priority,
        start_date: row.start_date,
        target_date: row.target_date,
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        is_draft: row.is_draft,
    }
}

/// `CommentReactionLiteSerializer` field list (`issue.py:223-228`,
/// `Meta.fields` order).
pub const COMMENT_REACTION_LITE_FIELDS: [&str; 4] = ["id", "reaction", "comment", "actor_detail"];

/// A database row for the comment-reaction lite shape. `actor` is a
/// required FK (`db/models/issue.py:753-759`).
#[derive(Debug, Clone, PartialEq)]
pub struct CommentReactionLiteRow<'a> {
    pub id: &'a str,
    pub reaction: &'a str,
    pub comment: &'a str,
    pub actor_detail: UserLiteView<'a>,
}

/// `CommentReactionLiteSerializer.to_representation` output
/// (`issue.py:223-228`): `actor_detail` is
/// `UserLiteSerializer(source="actor")`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommentReactionLiteView<'a> {
    pub id: &'a str,
    pub reaction: &'a str,
    pub comment: &'a str,
    pub actor_detail: UserLiteView<'a>,
}

/// Port of `CommentReactionLiteSerializer` (`issue.py:223-228`).
pub fn comment_reaction_lite_to_representation<'a>(
    row: &'a CommentReactionLiteRow<'a>,
) -> CommentReactionLiteView<'a> {
    CommentReactionLiteView {
        id: row.id,
        reaction: row.reaction,
        comment: row.comment,
        actor_detail: row.actor_detail.clone(),
    }
}

/// The `IssueComment` `fields = "__all__"` wire keys (`issue.py:231-250`),
/// in live-DRF order (probed `IssueCommentSerializer().fields` minus the
/// declared nests): `id`, the concrete columns (`attachments`/`labels` are
/// `ArrayField`s, `issue.py:563-566`), then the forward relations trailing
/// (`created_by`, `updated_by`, `project`, `workspace`, the `description`
/// one-to-one, `issue`, `actor`, `parent`).
pub const ISSUE_COMMENT_ALL_FIELDS: [&str; 24] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "comment_stripped",
    "comment_json",
    "comment_html",
    "attachments",
    "labels",
    "access",
    "external_source",
    "external_id",
    "speaker_type",
    "speaker_label",
    "speaker_agent_run_id",
    "edited_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "description",
    "issue",
    "actor",
    "parent",
];

/// A database row for `IssueComment` rendering. `actor` is nullable
/// (`db/models/issue.py:569-573`: system comments); `issue_detail` is the
/// 10-key space flat shape (NO `complexity_score`); `is_member` is the
/// annotated read-only boolean (`space/views/issue.py:242-249`: `Exists`
/// over active project membership); `comment_reactions` is the reverse
/// relation through `CommentReactionLiteSerializer`. No `is_synced` column
/// and no synced-comment validate exist here (the app twin has both,
/// `app/serializers/issue.py:956,971-991`) — port the absence.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueCommentRow<'a> {
    pub id: &'a str,
    pub actor_detail: Option<UserLiteView<'a>>,
    pub issue_detail: IssueFlatView<'a>,
    pub project_detail: ProjectLiteView<'a>,
    pub workspace_detail: WorkspaceLiteView<'a>,
    pub comment_reactions: Vec<CommentReactionLiteView<'a>>,
    pub is_member: bool,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub comment_stripped: &'a str,
    pub comment_json: &'a serde_json::Value,
    pub comment_html: &'a str,
    pub attachments: Vec<&'a str>,
    pub labels: Vec<&'a str>,
    pub access: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub speaker_type: &'a str,
    pub speaker_label: &'a str,
    pub speaker_agent_run_id: Option<&'a str>,
    pub edited_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub description: Option<&'a str>,
    pub issue: &'a str,
    pub actor: Option<&'a str>,
    pub parent: Option<&'a str>,
}

/// `IssueCommentSerializer.to_representation` output (`issue.py:231-250`), in
/// live-DRF wire order (probed): `id`, the six declared fields (DRF
/// `[pk] + declared + fields + relations`), then the concrete columns,
/// then the trailing relations.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueCommentView<'a> {
    pub id: &'a str,
    pub actor_detail: Option<UserLiteView<'a>>,
    pub issue_detail: IssueFlatView<'a>,
    pub project_detail: ProjectLiteView<'a>,
    pub workspace_detail: WorkspaceLiteView<'a>,
    pub comment_reactions: Vec<CommentReactionLiteView<'a>>,
    pub is_member: bool,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub comment_stripped: &'a str,
    pub comment_json: &'a serde_json::Value,
    pub comment_html: &'a str,
    pub attachments: Vec<&'a str>,
    pub labels: Vec<&'a str>,
    pub access: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub speaker_type: &'a str,
    pub speaker_label: &'a str,
    pub speaker_agent_run_id: Option<&'a str>,
    pub edited_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub description: Option<&'a str>,
    pub issue: &'a str,
    pub actor: Option<&'a str>,
    pub parent: Option<&'a str>,
}

/// Port of `IssueCommentSerializer` (`issue.py:231-250`).
/// Field-for-field copy.
pub fn issue_comment_to_representation<'a>(row: &'a IssueCommentRow<'a>) -> IssueCommentView<'a> {
    IssueCommentView {
        id: row.id,
        actor_detail: row.actor_detail.clone(),
        issue_detail: row.issue_detail.clone(),
        project_detail: row.project_detail.clone(),
        workspace_detail: row.workspace_detail.clone(),
        comment_reactions: row.comment_reactions.clone(),
        is_member: row.is_member,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        comment_stripped: row.comment_stripped,
        comment_json: row.comment_json,
        comment_html: row.comment_html,
        attachments: row.attachments.clone(),
        labels: row.labels.clone(),
        access: row.access,
        external_source: row.external_source,
        external_id: row.external_id,
        speaker_type: row.speaker_type,
        speaker_label: row.speaker_label,
        speaker_agent_run_id: row.speaker_agent_run_id,
        edited_at: row.edited_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        description: row.description,
        issue: row.issue,
        actor: row.actor,
        parent: row.parent,
    }
}

/// The `IssueCreateSerializer` write keys (`issue.py:261-271`): bare
/// `assignees` / `labels` id lists (`ListField(PrimaryKeyRelatedField)`,
/// `write_only=True`, `required=False`) — NOT `assignee_ids`/`label_ids`
/// like the app twin.
pub const ISSUE_CREATE_WRITE_KEYS: [&str; 2] = ["assignees", "labels"];

/// The `validate()` date message (`issue.py:294-299`): raised as a plain
/// `ValidationError("...")`, so DRF wraps it as
/// `{"non_field_errors": ["Start date cannot exceed target date"]}`.
pub const ISSUE_CREATE_DATE_MESSAGE: &str = "Start date cannot exceed target date";

/// The `validate()` HTML message (`issue.py:302-308`): raised as
/// `ValidationError({"error": ...})`.
pub const ISSUE_CREATE_HTML_MESSAGE: &str = "html content is not valid";

/// The `validate()` binary message (`issue.py:310-313`): raised as
/// `ValidationError({"description_binary": ...})`.
pub const ISSUE_CREATE_BINARY_MESSAGE: &str = "Invalid binary data";

/// Which `validate()` guard fired (`issue.py:293-315`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CreateValidationError {
    /// `start_date > target_date` (both present, `:294-299`).
    #[error("Start date cannot exceed target date")]
    StartExceedsTarget,
    /// `validate_html_content` returned invalid (`:302-305`).
    #[error("html content is not valid")]
    InvalidHtml,
    /// `validate_binary_data` returned invalid (`:310-313`).
    #[error("Invalid binary data")]
    InvalidBinary,
}

/// The caller-resolved verdict of `validate_html_content`
/// (`utils/content_validator.py:211-243`, nh3 sanitize): the sanitizer
/// itself lives at the DB/edge boundary (no Rust twin in this crate), so
/// `validate_create` takes the verdict and ports only the serializer's
/// decision — reject, or substitute the sanitized HTML (`:306-308`).
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlCheck<'a> {
    pub is_valid: bool,
    pub sanitized: Option<&'a str>,
}

/// The caller-resolved verdict of `validate_binary_data`
/// (`utils/content_validator.py:29-73`).
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryCheck {
    pub is_valid: bool,
}

/// Validated (and HTML-sanitized) create/update input
/// (`issue.py:293-315`): `None` dates skip the comparison (`:294-298`
/// requires BOTH present); empty/absent `description_html` skips the HTML
/// check (`:302` truthiness); empty/absent `description_binary` skips the
/// binary check (`:310` truthiness). No state/project/parent/estimate/pod/
/// triage/sync guards exist here (all in the app twin) — port the absence.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedCreateInput<'a> {
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub description_html: Option<&'a str>,
    pub description_binary: Option<&'a str>,
}

/// Port of `IssueCreateSerializer.validate()` (`issue.py:293-315`).
/// Dates compare as ISO-8601 strings (both are `DateField`s rendered
/// `YYYY-MM-DD`, so lexicographic order is chronological order); on success
/// the returned input carries the SANITIZED html (`:306-308`).
pub fn validate_create<'a>(
    input: &ValidatedCreateInput<'a>,
    html: &HtmlCheck<'a>,
    binary: &BinaryCheck,
) -> Result<ValidatedCreateInput<'a>, CreateValidationError> {
    if let (Some(start), Some(target)) = (input.start_date, input.target_date) {
        if start > target {
            return Err(CreateValidationError::StartExceedsTarget);
        }
    }
    let mut sanitized_html = input.description_html;
    if let Some(html_body) = input.description_html {
        if !html_body.is_empty() {
            if !html.is_valid {
                return Err(CreateValidationError::InvalidHtml);
            }
            if let Some(clean) = html.sanitized {
                sanitized_html = Some(clean);
            }
        }
    }
    if let Some(blob) = input.description_binary {
        if !blob.is_empty() && !binary.is_valid {
            return Err(CreateValidationError::InvalidBinary);
        }
    }
    Ok(ValidatedCreateInput {
        start_date: input.start_date,
        target_date: input.target_date,
        description_html: sanitized_html,
        description_binary: input.description_binary,
    })
}

/// Render `to_representation()` id lists (`issue.py:287-291`):
/// `data["assignees"] = [str(a.id) ...]`, `data["labels"] = [str(l.id)
/// ...]` — LIVE relation reads (BUG-stale-echo-inverted).
pub fn create_representation_ids<'a>(ids: &[&'a str]) -> Vec<&'a str> {
    ids.to_vec()
}

/// The `create()` write plan (`issue.py:317-374`): `assignees`/`labels`
/// are popped from the payload (`:318-319`); the issue row is created with
/// `project_id` from context (`:325`); `IssueAssignee`/`IssueLabel` rows
/// are bulk-created (`batch_size=10`, `:332-345`/`:359-372`) carrying the
/// issue's own `created_by`/`updated_by` (`:328-329`); an empty/`None`
/// assignee list falls back to `default_assignee_id` with NO membership
/// check (`:346-356`, BUG-default-assignee).
#[derive(Debug, Clone, PartialEq)]
pub struct CreatePlan<'a> {
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub assignee_ids: Option<Vec<&'a str>>,
    pub label_ids: Option<Vec<&'a str>>,
    pub default_assignee_id: Option<&'a str>,
    pub created_by_id: Option<&'a str>,
    pub updated_by_id: Option<&'a str>,
}

/// One `IssueAssignee` bulk row (`issue.py:332-345`).
#[derive(Debug, Clone, PartialEq)]
pub struct AssigneeBulkRow<'a> {
    pub assignee: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub created_by_id: Option<&'a str>,
    pub updated_by_id: Option<&'a str>,
}

/// One `IssueLabel` bulk row (`issue.py:359-372`).
#[derive(Debug, Clone, PartialEq)]
pub struct LabelBulkRow<'a> {
    pub label: &'a str,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub created_by_id: Option<&'a str>,
    pub updated_by_id: Option<&'a str>,
}

/// The resolved `create()` writes: bulk rows plus the unchecked
/// default-assignee fallback (`None` when the caller passed a non-empty
/// list, or when no default is configured).
#[derive(Debug, Clone, PartialEq)]
pub struct CreateWrites<'a> {
    pub assignee_rows: Vec<AssigneeBulkRow<'a>>,
    pub fallback_assignee: Option<AssigneeBulkRow<'a>>,
    pub label_rows: Vec<LabelBulkRow<'a>>,
}

/// Port of `IssueCreateSerializer.create()` (`issue.py:317-374`).
/// `assignees is not None and len(assignees)` (`:331`) gates the bulk
/// path — an explicit empty list takes the fallback branch, same as `None`.
pub fn plan_create<'a>(plan: &CreatePlan<'a>) -> CreateWrites<'a> {
    let has_assignees = plan
        .assignee_ids
        .as_ref()
        .is_some_and(|ids| !ids.is_empty());
    let mut writes = CreateWrites {
        assignee_rows: Vec::new(),
        fallback_assignee: None,
        label_rows: Vec::new(),
    };
    if has_assignees {
        writes.assignee_rows = plan
            .assignee_ids
            .as_ref()
            .expect("create plan checked non-empty assignees")
            .iter()
            .map(|user| AssigneeBulkRow {
                assignee: user,
                project_id: plan.project_id,
                workspace_id: plan.workspace_id,
                created_by_id: plan.created_by_id,
                updated_by_id: plan.updated_by_id,
            })
            .collect();
    } else if let Some(default) = plan.default_assignee_id {
        writes.fallback_assignee = Some(AssigneeBulkRow {
            assignee: default,
            project_id: plan.project_id,
            workspace_id: plan.workspace_id,
            created_by_id: plan.created_by_id,
            updated_by_id: plan.updated_by_id,
        });
    }
    if let Some(labels) = &plan.label_ids {
        writes.label_rows = labels
            .iter()
            .map(|label| LabelBulkRow {
                label,
                project_id: plan.project_id,
                workspace_id: plan.workspace_id,
                created_by_id: plan.created_by_id,
                updated_by_id: plan.updated_by_id,
            })
            .collect();
    }
    writes
}

/// The `update()` write plan (`issue.py:376-422`): `assignees`/`labels`
/// popped (`:377-378`); each non-`None` list deletes ALL existing rows then
/// bulk-recreates (an explicit empty list CLEARS, `:386-401`/`:403-418`);
/// `updated_at` is bumped to now even when only relations changed (`:421`).
#[derive(Debug, Clone, PartialEq)]
pub struct UpdatePlan<'a> {
    pub assignee_ids: Option<Vec<&'a str>>,
    pub label_ids: Option<Vec<&'a str>>,
    pub project_id: &'a str,
    pub workspace_id: &'a str,
    pub created_by_id: Option<&'a str>,
    pub updated_by_id: Option<&'a str>,
}

/// The resolved `update()` writes: `None` means "leave the relation alone";
/// `Some` (even empty) means delete-all then recreate these rows, plus the
/// mandatory `updated_at` bump.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateWrites<'a> {
    pub replace_assignees: Option<Vec<AssigneeBulkRow<'a>>>,
    pub replace_labels: Option<Vec<LabelBulkRow<'a>>>,
    pub bump_updated_at: bool,
}

/// Port of `IssueCreateSerializer.update()` (`issue.py:376-422`).
pub fn plan_update<'a>(plan: &UpdatePlan<'a>) -> UpdateWrites<'a> {
    UpdateWrites {
        replace_assignees: plan.assignee_ids.as_ref().map(|ids| {
            ids.iter()
                .map(|user| AssigneeBulkRow {
                    assignee: user,
                    project_id: plan.project_id,
                    workspace_id: plan.workspace_id,
                    created_by_id: plan.created_by_id,
                    updated_by_id: plan.updated_by_id,
                })
                .collect()
        }),
        replace_labels: plan.label_ids.as_ref().map(|ids| {
            ids.iter()
                .map(|label| LabelBulkRow {
                    label,
                    project_id: plan.project_id,
                    workspace_id: plan.workspace_id,
                    created_by_id: plan.created_by_id,
                    updated_by_id: plan.updated_by_id,
                })
                .collect()
        }),
        bump_updated_at: true,
    }
}

/// The `CommentReaction` `fields = "__all__"` wire keys
/// (`issue.py:425-429`), in live-DRF order (probed
/// `CommentReactionSerializer().fields`): `id`, the concrete columns
/// (`created_at`, `updated_at`, `deleted_at`, `reaction`), then the forward
/// relations trailing (`created_by`, `updated_by`, `project`, `workspace`,
/// `actor`, `comment`).
pub const COMMENT_REACTION_ALL_FIELDS: [&str; 11] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "reaction",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "actor",
    "comment",
];

/// A database row for `CommentReaction` rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentReactionRow<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub reaction: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub actor: &'a str,
    pub comment: &'a str,
}

/// `CommentReactionSerializer.to_representation` output (`issue.py:425-429`,
/// `fields = "__all__"`), in live-DRF wire order: no declared nests.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommentReactionView<'a> {
    pub id: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub reaction: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub actor: &'a str,
    pub comment: &'a str,
}

/// Port of `CommentReactionSerializer` (`issue.py:425-429`).
/// Field-for-field copy.
pub fn comment_reaction_to_representation<'a>(
    row: &'a CommentReactionRow<'a>,
) -> CommentReactionView<'a> {
    CommentReactionView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        reaction: row.reaction,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        actor: row.actor,
        comment: row.comment,
    }
}

/// `IssueVoteSerializer` field list (`issue.py:432-436`, `Meta.fields`
/// order — the wire order): every field read-only (`:436`, the whole
/// serializer is output-only). No `actor_detail` (contrast the app twin,
/// which adds one but is likewise all-read-only).
pub const ISSUE_VOTE_FIELDS: [&str; 5] = ["issue", "vote", "workspace", "project", "actor"];

/// A database row for `IssueVote` rendering (`db/models/issue.py:780-783`):
/// `vote` is `-1` (down) or `1` (up, the default).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueVoteRow<'a> {
    pub issue: &'a str,
    pub vote: i32,
    pub workspace: &'a str,
    pub project: &'a str,
    pub actor: &'a str,
}

/// `IssueVoteSerializer.to_representation` output (`issue.py:432-436`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueVoteView<'a> {
    pub issue: &'a str,
    pub vote: i32,
    pub workspace: &'a str,
    pub project: &'a str,
    pub actor: &'a str,
}

/// Port of `IssueVoteSerializer` (`issue.py:432-436`).
pub fn issue_vote_to_representation<'a>(row: &'a IssueVoteRow<'a>) -> IssueVoteView<'a> {
    IssueVoteView {
        issue: row.issue,
        vote: row.vote,
        workspace: row.workspace,
        project: row.project,
        actor: row.actor,
    }
}

/// `IssuePublicSerializer` field list (`issue.py:439-464`, `Meta.fields`
/// order — the wire order): every field read-only (`:464`). WIRE-
/// INCOMPATIBLE with the app twin of the same name (the app shape has
/// `description_html`/`state_detail`/`project_detail` and no id lists;
/// the space shape is the reverse) — port the space shape exactly.
pub const ISSUE_PUBLIC_FIELDS: [&str; 14] = [
    "id",
    "name",
    "sequence_id",
    "state",
    "project",
    "workspace",
    "priority",
    "target_date",
    "reactions",
    "votes",
    "module_ids",
    "created_by",
    "label_ids",
    "assignee_ids",
];

/// A database row for the public issue shape. `reactions` is the space
/// 5-key reaction nest (`source="issue_reactions"`, `:440`); `votes` the
/// space 5-key vote nest (`:441`); the id lists are the annotated
/// `Coalesce(ArrayAgg(distinct))` uuid arrays
/// (`space/views/issue.py:614-643`: `label_ids` filtered on live
/// `label_issue` rows, `assignee_ids` on active project members, all default
/// `[]`); `state`/`target_date`/`created_by` are nullable columns.
#[derive(Debug, Clone, PartialEq)]
pub struct IssuePublicRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub sequence_id: i32,
    pub state: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub priority: &'a str,
    pub target_date: Option<&'a str>,
    pub reactions: Vec<IssueReactionView<'a>>,
    pub votes: Vec<IssueVoteView<'a>>,
    pub module_ids: Vec<&'a str>,
    pub created_by: Option<&'a str>,
    pub label_ids: Vec<&'a str>,
    pub assignee_ids: Vec<&'a str>,
}

/// `IssuePublicSerializer.to_representation` output (`issue.py:439-464`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssuePublicView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub sequence_id: i32,
    pub state: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub priority: &'a str,
    pub target_date: Option<&'a str>,
    pub reactions: Vec<IssueReactionView<'a>>,
    pub votes: Vec<IssueVoteView<'a>>,
    pub module_ids: Vec<&'a str>,
    pub created_by: Option<&'a str>,
    pub label_ids: Vec<&'a str>,
    pub assignee_ids: Vec<&'a str>,
}

/// Port of `IssuePublicSerializer` (`issue.py:439-464`). Field-for-field
/// copy in `Meta.fields` order.
pub fn issue_public_to_representation<'a>(row: &'a IssuePublicRow<'a>) -> IssuePublicView<'a> {
    IssuePublicView {
        id: row.id,
        name: row.name,
        sequence_id: row.sequence_id,
        state: row.state,
        project: row.project,
        workspace: row.workspace,
        priority: row.priority,
        target_date: row.target_date,
        reactions: row.reactions.clone(),
        votes: row.votes.clone(),
        module_ids: row.module_ids.clone(),
        created_by: row.created_by,
        label_ids: row.label_ids.clone(),
        assignee_ids: row.assignee_ids.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn graph_golden() -> Value {
        let path = format!(
            "{}/../../fixtures/space/serializers/issue_graph.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn case<'a>(golden: &'a Value, serializer: &str) -> &'a Value {
        golden
            .get("cases")
            .and_then(Value::as_array)
            .expect("cases array")
            .iter()
            .find(|case| case.get("serializer").and_then(Value::as_str) == Some(serializer))
            .unwrap_or_else(|| panic!("golden lacks {serializer} case"))
    }

    fn str_list(value: &Value, key: &str) -> Vec<String> {
        value
            .get(key)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("golden case lacks array key {key}"))
            .iter()
            .map(|item| {
                item.as_str()
                    .unwrap_or_else(|| panic!("golden {key} entry is not a string: {item}"))
                    .to_owned()
            })
            .collect()
    }

    /// Top-level JSON key order of a view's serialization, read off the
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

    fn const_keys<const N: usize>(fields: &[&str; N]) -> Vec<String> {
        fields.iter().map(|key| key.to_string()).collect()
    }

    /// Wire order for an `__all__`/`exclude` view: `id`, the declared
    /// nests, then the model body after `id`.
    fn wire_order<const N: usize>(nests: &[&str], body: &[&str; N]) -> Vec<String> {
        let mut expected = vec!["id".to_owned()];
        expected.extend(nests.iter().map(|key| key.to_string()));
        expected.extend(body[1..].iter().map(|key| key.to_string()));
        expected
    }

    fn sample_project_detail<'a>(id: &'a str, icon: &'a Value) -> ProjectLiteView<'a> {
        ProjectLiteView {
            id,
            identifier: "WEB",
            name: "Web",
            cover_image: None,
            icon_prop: icon,
            emoji: Some("🚀"),
            description: "Ship it",
        }
    }

    fn sample_user() -> UserLiteView<'static> {
        UserLiteView {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Ada",
            last_name: "L",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "Ada L",
        }
    }

    fn sample_endpoint_row() -> IssueProjectLiteRow<'static> {
        IssueProjectLiteRow {
            id: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            project_detail: sample_project_detail(
                "33333333-3333-3333-3333-333333333333",
                Box::leak(Box::new(serde_json::json!({"color": "#fff"}))),
            ),
            name: "Graph issue",
            sequence_id: 7,
        }
    }

    fn sample_endpoint() -> IssueProjectLiteView<'static> {
        let row = sample_endpoint_row();
        // The row borrows only 'static data (leaked icon), so the view's
        // borrows outlive this frame.
        let row: &'static IssueProjectLiteRow<'static> = Box::leak(Box::new(row));
        issue_project_lite_to_representation(row)
    }

    #[test]
    fn state_flat_keys_match_golden() {
        // Fixture serializers/issue_graph.golden.json:
        // IssueStateFlatSerializer (issue.py:41-47).
        let golden = graph_golden();
        let expected = str_list(case(&golden, "IssueStateFlatSerializer"), "output_keys");
        assert_eq!(
            ISSUE_STATE_FLAT_FIELDS.map(str::to_owned).to_vec(),
            expected
        );
    }

    #[test]
    fn state_flat_renders_null_state_detail() {
        // `Issue.state` is nullable (db/models/issue.py:122-128); DRF
        // renders None for the null state_detail source.
        let icon = serde_json::json!({"color": "#fff"});
        let row = IssueStateFlatRow {
            id: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            sequence_id: 7,
            name: "Graph issue",
            state_detail: None,
            project_detail: sample_project_detail("33333333-3333-3333-3333-333333333333", &icon),
        };
        let view = issue_state_flat_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&ISSUE_STATE_FLAT_FIELDS));
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_eq!(produced.get("state_detail"), Some(&Value::Null));
    }

    #[test]
    fn project_lite_shape() {
        // issue.py:60-66: [id, project_detail, name, sequence_id], all
        // read-only.
        assert_eq!(
            ISSUE_PROJECT_LITE_FIELDS,
            ["id", "project_detail", "name", "sequence_id"]
        );
        let endpoint_row = sample_endpoint_row();
        let view = issue_project_lite_to_representation(&endpoint_row);
        assert_eq!(
            serialized_keys(&view),
            const_keys(&ISSUE_PROJECT_LITE_FIELDS)
        );
        assert_eq!(
            serialized_keys(&view.project_detail),
            vec![
                "id",
                "identifier",
                "name",
                "cover_image",
                "icon_prop",
                "emoji",
                "description"
            ]
        );
    }

    #[test]
    fn relation_keys_match_golden_and_mirror_endpoints() {
        // Fixture: IssueRelationSerializer vs RelatedIssueSerializer
        // (issue.py:69-84): same Meta keys; mirrored edge source
        // (related_issue outgoing vs issue incoming); raw FK ids emitted.
        let golden = graph_golden();
        let expected = str_list(
            case(&golden, "IssueRelationSerializer vs RelatedIssueSerializer"),
            "output_keys",
        );
        assert_eq!(ISSUE_RELATION_FIELDS.map(str::to_owned).to_vec(), expected);
        let row = IssueRelationRow {
            issue_detail: sample_endpoint(),
            relation_type: "blocked_by",
            related_issue: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            issue: "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
            id: "ffffffff-ffff-ffff-ffff-ffffffffffff",
        };
        let outgoing_view = issue_relation_to_representation(&row);
        let incoming_view = related_issue_to_representation(&row);
        assert_eq!(
            serialized_keys(&outgoing_view),
            const_keys(&ISSUE_RELATION_FIELDS)
        );
        let outgoing = serde_json::to_value(&outgoing_view).expect("ok");
        let incoming = serde_json::to_value(&incoming_view).expect("ok");
        assert_eq!(outgoing, incoming);
    }

    #[test]
    fn cycle_and_module_detail_carry_all_columns_plus_nest() {
        // Fixture: IssueCycleDetailSerializer / IssueModuleDetailSerializer
        // (issue.py:87-116, read_only audit + workspace/project).
        let empty = serde_json::json!({});
        let cycle_source = super::super::taxonomy::CycleRow {
            id: "55555555-5555-5555-5555-555555555555",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            name: "Sprint 3",
            description: "",
            start_date: None,
            end_date: None,
            owned_by: "99999999-9999-9999-9999-999999999999",
            view_props: &empty,
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            progress_snapshot: &empty,
            archived_at: None,
            logo_props: &empty,
            timezone: "UTC",
            version: 1,
        };
        let cycle_row = IssueCycleDetailRow {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            issue: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            cycle: "55555555-5555-5555-5555-555555555555",
            cycle_detail: super::super::taxonomy::cycle_to_representation(&cycle_source),
        };
        let cycle_view = issue_cycle_detail_to_representation(&cycle_row);
        assert_eq!(
            serialized_keys(&cycle_view),
            wire_order(&["cycle_detail"], &CYCLE_ISSUE_ALL_FIELDS)
        );
        let module_source = super::super::taxonomy::ModuleRow {
            id: "66666666-6666-6666-6666-666666666666",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            name: "Auth",
            description: "",
            description_text: None,
            description_html: None,
            start_date: None,
            target_date: None,
            status: "planned",
            lead: None,
            members: Vec::new(),
            view_props: &empty,
            sort_order: 65535.0,
            external_source: None,
            external_id: None,
            archived_at: None,
            logo_props: &empty,
        };
        let module_row = IssueModuleDetailRow {
            id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            module: "66666666-6666-6666-6666-666666666666",
            issue: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            module_detail: super::super::taxonomy::module_to_representation(&module_source),
        };
        let module_view = issue_module_detail_to_representation(&module_row);
        assert_eq!(
            serialized_keys(&module_view),
            wire_order(&["module_detail"], &MODULE_ISSUE_ALL_FIELDS)
        );
    }

    #[test]
    fn link_duplicate_guard_matches_golden_body() {
        // Fixture duplicate_error (issue.py:136-139): 400 with
        // {"error": "URL already exists for this Issue"}.
        let golden = graph_golden();
        let dup = &case(&golden, "IssueLinkSerializer")["duplicate_error"];
        assert_eq!(
            dup.get("status").and_then(Value::as_u64),
            Some(u64::from(ISSUE_LINK_DUPLICATE_STATUS))
        );
        assert_eq!(
            dup.get("body")
                .and_then(|body| body.get("error"))
                .and_then(Value::as_str),
            Some(ISSUE_LINK_DUPLICATE_MESSAGE)
        );
        assert_eq!(check_issue_link_duplicate(true), Err(DuplicateIssueLink));
        assert_eq!(check_issue_link_duplicate(false), Ok(()));
        assert_eq!(
            format!("{DuplicateIssueLink}"),
            "URL already exists for this Issue"
        );
    }

    #[test]
    fn link_and_attachment_carry_all_columns() {
        // Fixture: IssueLinkSerializer (12 IssueLink columns +
        // created_by_detail) and IssueAttachmentSerializer (24 FileAsset
        // columns, no asset_url extra).
        let meta = serde_json::json!({});
        let link_row = IssueLinkRow {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            created_at: None,
            updated_at: None,
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            title: None,
            url: "https://example.com/spec",
            issue: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            metadata: &meta,
            created_by_detail: sample_user(),
        };
        let link_view = issue_link_to_representation(&link_row);
        assert_eq!(
            serialized_keys(&link_view),
            wire_order(&["created_by_detail"], &ISSUE_LINK_ALL_FIELDS)
        );
        let attrs = serde_json::json!({});
        let attach_row = IssueAttachmentRow {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            attributes: &attrs,
            asset: "uploads/f.png",
            user: None,
            workspace: Some("22222222-2222-2222-2222-222222222222"),
            draft_issue: None,
            project: Some("33333333-3333-3333-3333-333333333333"),
            issue: Some("dddddddd-dddd-dddd-dddd-dddddddddddd"),
            comment: None,
            page: None,
            entity_type: Some("ISSUE_ATTACHMENT"),
            entity_identifier: None,
            is_deleted: false,
            is_archived: false,
            external_id: None,
            external_source: None,
            size: 12.0,
            is_uploaded: true,
            storage_metadata: None,
        };
        let attach_view = issue_attachment_to_representation(&attach_row);
        assert_eq!(
            serialized_keys(&attach_view),
            const_keys(&FILE_ASSET_ALL_FIELDS)
        );
        let produced = serde_json::to_value(&attach_view).expect("ok");
        assert_eq!(produced.get("asset_url"), None);
    }

    #[test]
    fn reaction_and_vote_five_key_shapes() {
        // Fixture: IssueReactionSerializer (space) and IssueVoteSerializer
        // (space) — 5 keys each, all read-only, no actor_detail, no id.
        let golden = graph_golden();
        assert_eq!(
            str_list(
                case(&golden, "IssueReactionSerializer (space)"),
                "output_keys"
            ),
            ISSUE_REACTION_FIELDS.map(str::to_owned).to_vec()
        );
        assert_eq!(
            str_list(case(&golden, "IssueVoteSerializer (space)"), "output_keys"),
            ISSUE_VOTE_FIELDS.map(str::to_owned).to_vec()
        );
        let reaction_row = IssueReactionRow {
            issue: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            reaction: "heart",
            workspace: "22222222-2222-2222-2222-222222222222",
            project: "33333333-3333-3333-3333-333333333333",
            actor: "11111111-1111-1111-1111-111111111111",
        };
        let reaction_view = issue_reaction_to_representation(&reaction_row);
        assert_eq!(
            serialized_keys(&reaction_view),
            const_keys(&ISSUE_REACTION_FIELDS)
        );
        let reaction = serde_json::to_value(&reaction_view).expect("ok");
        assert_eq!(
            reaction,
            serde_json::json!({
                "issue": "dddddddd-dddd-dddd-dddd-dddddddddddd",
                "reaction": "heart",
                "workspace": "22222222-2222-2222-2222-222222222222",
                "project": "33333333-3333-3333-3333-333333333333",
                "actor": "11111111-1111-1111-1111-111111111111",
            })
        );
        let vote_row = IssueVoteRow {
            issue: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            vote: 1,
            workspace: "22222222-2222-2222-2222-222222222222",
            project: "33333333-3333-3333-3333-333333333333",
            actor: "11111111-1111-1111-1111-111111111111",
        };
        let vote_view = issue_vote_to_representation(&vote_row);
        assert_eq!(serialized_keys(&vote_view), const_keys(&ISSUE_VOTE_FIELDS));
        let vote = serde_json::to_value(&vote_view).expect("ok");
        assert_eq!(vote.get("vote").and_then(Value::as_i64), Some(1));
        assert_eq!(vote.get("actor_detail"), None);
        assert_eq!(vote.get("id"), None);
    }

    #[test]
    fn full_issue_declared_nests_match_golden() {
        // Fixture declared_nests_in_order (issue.py:164-201): 14 nests.
        let golden = graph_golden();
        let expected = str_list(
            case(&golden, "IssueSerializer (space)"),
            "declared_nests_in_order",
        );
        let short: Vec<String> = expected
            .iter()
            .map(|entry| entry.split('(').next().unwrap_or(entry).to_owned())
            .collect();
        assert_eq!(ISSUE_DECLARED_NESTS.map(str::to_owned).to_vec(), short);
    }

    #[test]
    fn full_issue_excludes_workpad_and_renders_pod_shapes() {
        // issue.py:180-185 Meta.exclude=["workpad"]: the agent scratchpad
        // must never leak. get_assigned_pod_detail (issue.py:195-201):
        // None when assigned_pod_id is None, PodMini shape otherwise.
        let description = serde_json::json!({});
        let icon = serde_json::json!({"color": "#fff"});
        let row = sample_issue_row(&description, &icon, None);
        let view = issue_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            wire_order(&ISSUE_DECLARED_NESTS, &ISSUE_MODEL_FIELDS_NO_WORKPAD)
        );
        let produced = serde_json::to_value(&view).expect("ok");
        assert_eq!(produced.get("workpad"), None, "workpad MUST NOT render");
        assert_eq!(produced.get("assigned_pod_detail"), Some(&Value::Null));
        assert_eq!(
            produced.get("sub_issues_count").and_then(Value::as_i64),
            Some(2)
        );
        // Non-null pod renders the recorded 5-key PodMini shape.
        let row = sample_issue_row(
            &description,
            &icon,
            Some(PodMiniView {
                id: "99999999-9999-9999-9999-999999999999",
                name: "gpu-1",
                is_default: true,
                project: "33333333-3333-3333-3333-333333333333",
                project_identifier: "WEB",
            }),
        );
        let view = issue_to_representation(&row);
        assert_eq!(
            serialized_keys(view.assigned_pod_detail.as_ref().expect("pod detail")),
            const_keys(&POD_MINI_FIELDS)
        );
        let produced = serde_json::to_value(&view).expect("ok");
        assert_eq!(
            produced["assigned_pod_detail"]
                .get("project_identifier")
                .and_then(Value::as_str),
            Some("WEB")
        );
    }

    #[test]
    fn flat_ten_keys_without_complexity() {
        // Fixture: IssueFlatSerializer (issue.py:204-220) — NO
        // complexity_score (the app twin adds it).
        let golden = graph_golden();
        assert_eq!(
            str_list(case(&golden, "IssueFlatSerializer"), "output_keys"),
            ISSUE_FLAT_FIELDS.map(str::to_owned).to_vec()
        );
        let description = serde_json::json!({});
        let flat_row = IssueFlatRow {
            id: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            name: "Flat",
            description_json: &description,
            description_html: "<p>x</p>",
            priority: "high",
            start_date: None,
            target_date: None,
            sequence_id: 3,
            sort_order: 65535.0,
            is_draft: false,
        };
        let view = issue_flat_to_representation(&flat_row);
        assert_eq!(serialized_keys(&view), const_keys(&ISSUE_FLAT_FIELDS));
        let produced = serde_json::to_value(&view).expect("ok");
        assert_eq!(produced.get("complexity_score"), None);
        assert_eq!(produced.get("sequence_id").and_then(Value::as_i64), Some(3));
    }

    #[test]
    fn comment_shapes_and_system_actor_null() {
        // Fixture: CommentReactionLiteSerializer (issue.py:223-228) and
        // IssueCommentSerializer (issue.py:231-250): actor nullable
        // (system comments), issue_detail is the 10-key flat shape, no
        // is_synced (app twin only).
        let golden = graph_golden();
        assert_eq!(
            str_list(
                case(&golden, "CommentReactionLiteSerializer"),
                "output_keys"
            ),
            COMMENT_REACTION_LITE_FIELDS.map(str::to_owned).to_vec()
        );
        let lite_row = CommentReactionLiteRow {
            id: "cccccccc-cccc-cccc-cccc-cccccccccccc",
            reaction: "+1",
            comment: "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
            actor_detail: sample_user(),
        };
        let lite_view = comment_reaction_lite_to_representation(&lite_row);
        assert_eq!(
            serialized_keys(&lite_view),
            const_keys(&COMMENT_REACTION_LITE_FIELDS)
        );
        assert_eq!(
            serialized_keys(&lite_view.actor_detail),
            vec![
                "id",
                "first_name",
                "last_name",
                "avatar",
                "avatar_url",
                "is_bot",
                "display_name"
            ]
        );
        let row = sample_comment_row(None);
        let view = issue_comment_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            wire_order(
                &[
                    "actor_detail",
                    "issue_detail",
                    "project_detail",
                    "workspace_detail",
                    "comment_reactions",
                    "is_member",
                ],
                &ISSUE_COMMENT_ALL_FIELDS
            )
        );
        assert_eq!(
            serialized_keys(&view.issue_detail),
            const_keys(&ISSUE_FLAT_FIELDS)
        );
        let produced = serde_json::to_value(&view).expect("ok");
        assert_eq!(produced.get("actor"), Some(&Value::Null));
        assert_eq!(produced.get("actor_detail"), Some(&Value::Null));
        assert_eq!(produced.get("is_synced"), None);
    }

    #[test]
    fn create_validate_guards_match_python() {
        // Fixture validate list (issue.py:293-315): start>target, bad html,
        // bad binary — plus the ABSENCE of app-twin guards.
        let ok_html = HtmlCheck {
            is_valid: true,
            sanitized: None,
        };
        let ok_bin = BinaryCheck { is_valid: true };
        // start > target fires only when BOTH are present.
        let bad = ValidatedCreateInput {
            start_date: Some("2026-09-30"),
            target_date: Some("2026-09-01"),
            description_html: None,
            description_binary: None,
        };
        assert_eq!(
            validate_create(&bad, &ok_html, &ok_bin),
            Err(CreateValidationError::StartExceedsTarget)
        );
        assert_eq!(
            ISSUE_CREATE_DATE_MESSAGE,
            "Start date cannot exceed target date"
        );
        // One-sided dates skip the comparison.
        let one_sided = ValidatedCreateInput {
            start_date: Some("2026-09-30"),
            target_date: None,
            description_html: None,
            description_binary: None,
        };
        assert!(validate_create(&one_sided, &ok_html, &ok_bin).is_ok());
        // Bad html rejects; sanitized html substitutes.
        let html_in = ValidatedCreateInput {
            start_date: None,
            target_date: None,
            description_html: Some("<p>hi<script></script></p>"),
            description_binary: None,
        };
        assert_eq!(
            validate_create(
                &html_in,
                &HtmlCheck {
                    is_valid: false,
                    sanitized: None
                },
                &ok_bin
            ),
            Err(CreateValidationError::InvalidHtml)
        );
        assert_eq!(ISSUE_CREATE_HTML_MESSAGE, "html content is not valid");
        let out = validate_create(
            &html_in,
            &HtmlCheck {
                is_valid: true,
                sanitized: Some("<p>hi</p>"),
            },
            &ok_bin,
        )
        .expect("sanitized");
        assert_eq!(out.description_html, Some("<p>hi</p>"));
        // Empty html skips the check entirely.
        let empty_html = ValidatedCreateInput {
            start_date: None,
            target_date: None,
            description_html: Some(""),
            description_binary: None,
        };
        assert!(validate_create(
            &empty_html,
            &HtmlCheck {
                is_valid: false,
                sanitized: None
            },
            &ok_bin
        )
        .is_ok());
        // Bad binary rejects under its own key.
        let bin_in = ValidatedCreateInput {
            start_date: None,
            target_date: None,
            description_html: None,
            description_binary: Some("aGk="),
        };
        assert_eq!(
            validate_create(&bin_in, &ok_html, &BinaryCheck { is_valid: false }),
            Err(CreateValidationError::InvalidBinary)
        );
        assert_eq!(ISSUE_CREATE_BINARY_MESSAGE, "Invalid binary data");
        // Write keys are bare assignees/labels, not *_ids.
        assert_eq!(ISSUE_CREATE_WRITE_KEYS, ["assignees", "labels"]);
    }

    #[test]
    fn create_plans_match_bulk_and_fallback_semantics() {
        // issue.py:317-374: bulk rows carry issue audit ids; empty/None
        // assignees fall back to the default with NO membership check.
        let writes = plan_create(&CreatePlan {
            project_id: "33333333-3333-3333-3333-333333333333",
            workspace_id: "22222222-2222-2222-2222-222222222222",
            assignee_ids: Some(vec!["11111111-1111-1111-1111-111111111111"]),
            label_ids: Some(vec!["77777777-7777-7777-7777-777777777777"]),
            default_assignee_id: Some("22222222-2222-2222-2222-222222222222"),
            created_by_id: Some("11111111-1111-1111-1111-111111111111"),
            updated_by_id: None,
        });
        assert_eq!(writes.assignee_rows.len(), 1);
        assert_eq!(writes.fallback_assignee, None);
        assert_eq!(writes.label_rows.len(), 1);
        assert_eq!(
            writes.assignee_rows[0].created_by_id,
            Some("11111111-1111-1111-1111-111111111111")
        );
        // Explicit empty list falls back, same as None.
        for assignees in [None, Some(Vec::new())] {
            let writes = plan_create(&CreatePlan {
                project_id: "33333333-3333-3333-3333-333333333333",
                workspace_id: "22222222-2222-2222-2222-222222222222",
                assignee_ids: assignees,
                label_ids: None,
                default_assignee_id: Some("22222222-2222-2222-2222-222222222222"),
                created_by_id: None,
                updated_by_id: None,
            });
            assert!(writes.assignee_rows.is_empty());
            assert_eq!(
                writes.fallback_assignee.as_ref().map(|row| row.assignee),
                Some("22222222-2222-2222-2222-222222222222")
            );
        }
        // No default configured: no fallback row at all.
        let writes = plan_create(&CreatePlan {
            project_id: "33333333-3333-3333-3333-333333333333",
            workspace_id: "22222222-2222-2222-2222-222222222222",
            assignee_ids: None,
            label_ids: None,
            default_assignee_id: None,
            created_by_id: None,
            updated_by_id: None,
        });
        assert_eq!(writes.fallback_assignee, None);
        // to_representation str-list rendering.
        assert_eq!(
            create_representation_ids(&["11111111-1111-1111-1111-111111111111"]),
            vec!["11111111-1111-1111-1111-111111111111"]
        );
    }

    #[test]
    fn update_plan_clears_on_empty_and_bumps_updated_at() {
        // issue.py:376-422: Some (even empty) replaces; None leaves alone;
        // updated_at bumps even for relation-only changes.
        let writes = plan_update(&UpdatePlan {
            assignee_ids: Some(Vec::new()),
            label_ids: None,
            project_id: "33333333-3333-3333-3333-333333333333",
            workspace_id: "22222222-2222-2222-2222-222222222222",
            created_by_id: None,
            updated_by_id: None,
        });
        assert_eq!(writes.replace_assignees, Some(Vec::new()));
        assert_eq!(writes.replace_labels, None);
        assert!(writes.bump_updated_at);
    }

    #[test]
    fn comment_reaction_all_keys_and_public_shape() {
        // Fixture: CommentReactionSerializer (issue.py:425-429, 11 keys) and
        // IssuePublicSerializer (issue.py:439-464, 14 keys in Meta order).
        let golden = graph_golden();
        let reaction_row = CommentReactionRow {
            id: "cccccccc-cccc-cccc-cccc-cccccccccccc",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            actor: "11111111-1111-1111-1111-111111111111",
            comment: "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
            reaction: "heart",
        };
        let reaction_view = comment_reaction_to_representation(&reaction_row);
        assert_eq!(
            serialized_keys(&reaction_view),
            const_keys(&COMMENT_REACTION_ALL_FIELDS)
        );
        assert_eq!(
            str_list(
                case(&golden, "IssuePublicSerializer (space)"),
                "output_keys"
            ),
            ISSUE_PUBLIC_FIELDS.map(str::to_owned).to_vec()
        );
        let public_row = IssuePublicRow {
            id: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            name: "Public",
            sequence_id: 7,
            state: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            priority: "none",
            target_date: None,
            reactions: Vec::new(),
            votes: Vec::new(),
            module_ids: Vec::new(),
            created_by: None,
            label_ids: Vec::new(),
            assignee_ids: Vec::new(),
        };
        let public_view = issue_public_to_representation(&public_row);
        assert_eq!(
            serialized_keys(&public_view),
            const_keys(&ISSUE_PUBLIC_FIELDS)
        );
        let public = serde_json::to_value(&public_view).expect("ok");
        assert_eq!(public.get("description_html"), None);
        assert_eq!(public.get("state_detail"), None);
    }

    #[test]
    fn label_lite_resolves_to_taxonomy_shape() {
        // Fixture: LabelLiteSerializer (issue.py:467-470) — owned by
        // taxonomy.rs, re-exported here; pins the 3-key contract once.
        let golden = graph_golden();
        assert_eq!(
            str_list(case(&golden, "LabelLiteSerializer"), "output_keys"),
            vec!["id", "name", "color"]
        );
        let lite_row = LabelLiteRow {
            id: "77777777-7777-7777-7777-777777777777",
            name: "Bug",
            color: "#ff0000",
        };
        let view = label_lite_to_representation(&lite_row);
        assert_eq!(serialized_keys(&view), vec!["id", "name", "color"]);
        let produced = serde_json::to_value(&view).expect("ok");
        assert_eq!(
            produced,
            serde_json::json!({
                "id": "77777777-7777-7777-7777-777777777777",
                "name": "Bug",
                "color": "#ff0000",
            })
        );
    }

    // Shared representative full-issue row. JSON literals are borrowed from
    // the caller (the tests pass short-lived locals).
    fn sample_issue_row<'a>(
        description: &'a Value,
        icon: &'a Value,
        pod: Option<PodMiniView<'a>>,
    ) -> IssueRow<'a> {
        let assigned_pod = pod.as_ref().map(|view| view.id);
        IssueRow {
            id: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            parent: None,
            state: Some("44444444-4444-4444-4444-444444444444"),
            point: None,
            estimate_point: None,
            name: "Graph issue",
            description_json: description,
            description_html: "<p>why</p>",
            description_stripped: None,
            description_binary: None,
            priority: "none",
            complexity_score: 0,
            start_date: None,
            target_date: None,
            assignees: Vec::new(),
            sequence_id: 7,
            labels: Vec::new(),
            sort_order: 65535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            r#type: None,
            git_work_branch: "",
            created_via: None,
            assigned_pod,
            agent_executor: None,
            project_detail: sample_project_detail("33333333-3333-3333-3333-333333333333", icon),
            state_detail: None,
            parent_detail: None,
            label_details: Vec::new(),
            assignee_details: Vec::new(),
            related_issues: Vec::new(),
            issue_relations: Vec::new(),
            issue_cycle: None,
            issue_module: None,
            issue_link: Vec::new(),
            issue_attachment: Vec::new(),
            sub_issues_count: 2,
            issue_reactions: Vec::new(),
            assigned_pod_detail: pod,
        }
    }

    fn sample_comment_row(actor: Option<UserLiteView<'_>>) -> IssueCommentRow<'_> {
        fn leak(value: Value) -> &'static Value {
            Box::leak(Box::new(value))
        }
        let actor_id = actor.as_ref().map(|view| view.id);
        IssueCommentRow {
            id: "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
            created_at: None,
            updated_at: None,
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: "33333333-3333-3333-3333-333333333333",
            workspace: "22222222-2222-2222-2222-222222222222",
            comment_stripped: "hello",
            comment_json: leak(serde_json::json!({})),
            comment_html: "<p>hello</p>",
            description: None,
            attachments: Vec::new(),
            labels: Vec::new(),
            issue: "dddddddd-dddd-dddd-dddd-dddddddddddd",
            actor: actor_id,
            access: "EXTERNAL",
            external_source: None,
            external_id: None,
            speaker_type: "human",
            speaker_label: "",
            speaker_agent_run_id: None,
            edited_at: None,
            parent: None,
            actor_detail: actor,
            issue_detail: IssueFlatView {
                id: "dddddddd-dddd-dddd-dddd-dddddddddddd",
                name: "Graph issue",
                description_json: leak(serde_json::json!({})),
                description_html: "<p>why</p>",
                priority: "none",
                start_date: None,
                target_date: None,
                sequence_id: 7,
                sort_order: 65535.0,
                is_draft: false,
            },
            project_detail: sample_project_detail(
                "33333333-3333-3333-3333-333333333333",
                leak(serde_json::json!({"color": "#fff"})),
            ),
            workspace_detail: WorkspaceLiteView {
                name: "Acme",
                slug: "acme",
                id: "22222222-2222-2222-2222-222222222222",
            },
            comment_reactions: Vec::new(),
            is_member: true,
        }
    }
}
