//! Member + invite serializers: `ProjectMember*` read shapes (D-25).
//!
//! Port of `apps/api/pi_dash/app/serializers/project.py:193-240`:
//!
//! * `ProjectMemberSerializer` (`:193-200`)
//! * `ProjectMemberPreferenceSerializer` (`:203-212`, `validate_preferences`
//!   merge)
//! * `ProjectMemberAdminSerializer` (`:215-222`)
//! * `ProjectMemberRoleSerializer` (`:225-231`, `original_role`,
//!   `DynamicBaseSerializer` `fields=`)
//! * `ProjectMemberInviteSerializer` (`:234-240`)
//!
//! Nested shapes consumed verbatim:
//!
//! * `WorkspaceLiteSerializer` (`app/serializers/workspace.py:79-83`)
//! * `UserLiteSerializer` / `UserAdminLiteSerializer`
//!   (`app/serializers/user.py:141-170`)
//! * `ProjectLiteSerializer` (`app/serializers/project.py:120-133`,
//!   nested under `member.project` and `invite.project`)
//!
//! These are pure kernels in the `v1_projects::ser_collab` style: each
//! `to_representation` takes a row borrowed from the caller and returns a
//! `serde::Serialize` view whose fields are the live DRF wire fields in
//! output order. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`.
//! Datetimes cross this boundary already rendered as DRF iso-8601 strings
//! (formatting owns to the DB edge), so rendering here is a byte-exact
//! passthrough. JSON blobs (`view_props`, `default_props`, `preferences`,
//! `logo_props`) pass through by reference.
//!
//! Single-owner notes (never fork a helper). The D-24 serializers that own
//! the app workspace/user shapes (PIDASHCONV-600/603) have not landed, and
//! neither has the L1 `ProjectLiteSerializer` port (PIDASHCONV-563), so the
//! nested renders below are defined here from FX-APROJ-02; review picks the
//! single owner and the loser is deleted, not forked. The one shared kernel
//! that already exists is reused, not redefined: `User.avatar_url`
//! resolution is [`crate::v1_projects::ser_collab::resolve_avatar_url`]
//! (same `db/models/user.py` property both surfaces read); handlers resolve
//! the app lite `avatar_url` through it.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-fields (`app/serializers/base.py:17-20`): `DynamicBaseSerializer`
//!   pops `fields` and then overwrites it with `expand`, so the `fields=`
//!   kwarg is silently ignored. The member list (`app/views/project/
//!   member.py:168`) and guest retrieve (`:201`) pass
//!   `fields=("id", "member", "role")` but render the full 6-key shape.
//!   [`member_role_fields_to_representation`] takes the same argument and
//!   ignores it. (TRACE.md bug 1; fixture
//!   `member_role_fields_kwarg_ignored`.)
//! * BUG-merge (`app/serializers/project.py:208-212`):
//!   `validate_preferences` calls `instance.preferences.update(value)`,
//!   mutating the instance in place during validation, and the merge is
//!   shallow — a nested dict in the patch REPLACES the whole sub-dict.
//!   [`merge_preferences`] takes `&mut` for the same reason.
//!
//! Out of scope (documented, not ported): `DynamicBaseSerializer` `expand=`
//! (`base.py:117-174`) — no D-25 call site passes `expand` to these
//! serializers and no fixture covers it; the member-serializer write
//! pipeline (`member.py:260` `partial=True` save) — handler layer
//! (FX-APROJ-09); the JSONB key reorder visible on DB re-read
//! (fixture `after_first_save`) — storage behaviour, not serializer output.

use serde::Serialize;

/// `ProjectMemberSerializer` wire keys in output order
/// (`project.py:193-200`, `fields = "__all__"`: declared `id`, `workspace`,
/// `project`, `member` first, then the remaining model fields).
pub const PROJECT_MEMBER_WIRE_FIELDS: [&str; 16] = [
    "id",
    "workspace",
    "project",
    "member",
    "created_at",
    "updated_at",
    "deleted_at",
    "comment",
    "role",
    "view_props",
    "default_props",
    "preferences",
    "sort_order",
    "is_active",
    "created_by",
    "updated_by",
];

/// `ProjectMemberInviteSerializer` wire keys in output order
/// (`project.py:234-240`).
pub const PROJECT_MEMBER_INVITE_WIRE_FIELDS: [&str; 14] = [
    "id",
    "project",
    "workspace",
    "created_at",
    "updated_at",
    "deleted_at",
    "email",
    "accepted",
    "token",
    "message",
    "responded_at",
    "role",
    "created_by",
    "updated_by",
];

/// `ProjectMemberRoleSerializer.Meta.fields` (`project.py:228`), wire order.
pub const PROJECT_MEMBER_ROLE_WIRE_FIELDS: [&str; 6] = [
    "id",
    "role",
    "member",
    "project",
    "original_role",
    "created_at",
];

/// `ProjectMemberPreferenceSerializer.Meta.fields` (`project.py:206`), wire
/// order.
pub const PROJECT_MEMBER_PREFERENCE_WIRE_FIELDS: [&str; 4] =
    ["preferences", "project_id", "member_id", "workspace_id"];

/// `WorkspaceLiteSerializer.Meta.fields` (`workspace.py:82`), wire order.
pub const WORKSPACE_LITE_WIRE_FIELDS: [&str; 4] = ["name", "slug", "id", "logo_url"];

/// `UserLiteSerializer.Meta.fields` (`user.py:144-152`), wire order.
pub const USER_LITE_WIRE_FIELDS: [&str; 7] = [
    "id",
    "first_name",
    "last_name",
    "avatar",
    "avatar_url",
    "is_bot",
    "display_name",
];

/// `UserAdminLiteSerializer.Meta.fields` (`user.py:158-168`), wire order.
pub const USER_ADMIN_LITE_WIRE_FIELDS: [&str; 9] = [
    "id",
    "first_name",
    "last_name",
    "avatar",
    "avatar_url",
    "is_bot",
    "display_name",
    "email",
    "last_login_medium",
];

/// `ProjectLiteSerializer.Meta.fields` (`project.py:123-132`), wire order.
///
/// Nested copy for `member.project` / `invite.project`; the canonical port
/// is L1's (PIDASHCONV-563) — review picks the single owner.
pub const PROJECT_LITE_WIRE_FIELDS: [&str; 8] = [
    "id",
    "identifier",
    "name",
    "cover_image",
    "cover_image_url",
    "logo_props",
    "description",
    "is_default",
];

/// Port of `Workspace.logo_url` (`db/models/workspace.py:146-153`): the
/// logo asset URL as-is when an asset is attached (even when it maps to no
/// URL — no fall-through), else the `logo` text when non-empty, else `None`
/// (`if self.logo:` is falsy for both `None` and `""`).
///
/// Expected canonical home is D-24; defined here so member/invite rows
/// render without re-deriving the truthiness.
pub fn resolve_workspace_logo_url<'a>(
    logo_asset_attached: bool,
    logo_asset_url: Option<&'a str>,
    logo: Option<&'a str>,
) -> Option<&'a str> {
    if logo_asset_attached {
        return logo_asset_url;
    }
    match logo {
        Some(url) if !url.is_empty() => Some(url),
        _ => None,
    }
}

/// Port of `Project.cover_image_url` (`db/models/project.py:176-184`):
/// same precedence as [`resolve_workspace_logo_url`] over
/// `cover_image_asset` / `cover_image` (`TextField(null=True)`).
///
/// Expected canonical home is L1 (PIDASHCONV-563, `ProjectLiteSerializer`);
/// defined here so the nested project renders without re-deriving it.
pub fn resolve_project_cover_image_url<'a>(
    cover_image_asset_attached: bool,
    cover_image_asset_url: Option<&'a str>,
    cover_image: Option<&'a str>,
) -> Option<&'a str> {
    if cover_image_asset_attached {
        return cover_image_asset_url;
    }
    match cover_image {
        Some(url) if !url.is_empty() => Some(url),
        _ => None,
    }
}

/// A `Workspace` row for lite rendering (`workspace.py:79-83`): `name` /
/// `slug` are non-null (`db/models/workspace.py:122,134`); `logo_url` is
/// the resolved [`resolve_workspace_logo_url`] value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLiteRow<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub id: &'a str,
    pub logo_url: Option<&'a str>,
}

/// `WorkspaceLiteSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceLiteView<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub id: &'a str,
    pub logo_url: Option<&'a str>,
}

/// Port of `WorkspaceLiteSerializer` (`workspace.py:79-83`).
pub fn workspace_lite_to_representation<'a>(
    row: &'a WorkspaceLiteRow<'a>,
) -> WorkspaceLiteView<'a> {
    WorkspaceLiteView {
        name: row.name,
        slug: row.slug,
        id: row.id,
        logo_url: row.logo_url,
    }
}

/// A `User` row for lite rendering (`user.py:141-154`): `avatar` is
/// non-null text (`db/models/user.py:67`); `avatar_url` is the
/// `avatar_url` property resolved by the caller via
/// [`crate::v1_projects::ser_collab::resolve_avatar_url`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserLiteRow<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
}

/// `UserLiteSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserLiteView<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
}

/// Port of `UserLiteSerializer` (`user.py:141-154`).
pub fn user_lite_to_representation<'a>(row: &'a UserLiteRow<'a>) -> UserLiteView<'a> {
    UserLiteView {
        id: row.id,
        first_name: row.first_name,
        last_name: row.last_name,
        avatar: row.avatar,
        avatar_url: row.avatar_url,
        is_bot: row.is_bot,
        display_name: row.display_name,
    }
}

/// A `User` row for admin-lite rendering (`user.py:157-168`): the lite
/// columns plus nullable `email` (`CharField(null=True)`,
/// `db/models/user.py:61`) and `last_login_medium` (non-null,
/// `db/models/user.py:110`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAdminLiteRow<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
    pub email: Option<&'a str>,
    pub last_login_medium: &'a str,
}

/// `UserAdminLiteSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserAdminLiteView<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
    pub email: Option<&'a str>,
    pub last_login_medium: &'a str,
}

/// Port of `UserAdminLiteSerializer` (`user.py:157-168`).
pub fn user_admin_lite_to_representation<'a>(
    row: &'a UserAdminLiteRow<'a>,
) -> UserAdminLiteView<'a> {
    UserAdminLiteView {
        id: row.id,
        first_name: row.first_name,
        last_name: row.last_name,
        avatar: row.avatar,
        avatar_url: row.avatar_url,
        is_bot: row.is_bot,
        display_name: row.display_name,
        email: row.email,
        last_login_medium: row.last_login_medium,
    }
}

/// A `Project` row for lite rendering (`project.py:120-133`):
/// `cover_image` is nullable (`db/models/project.py:107`), `description`
/// is non-null blankable text (`:75`), `logo_props` is a JSON object
/// (`JSONField(default=dict)`, `:118`), and `cover_image_url` is the
/// resolved [`resolve_project_cover_image_url`] value.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectLiteRow<'a> {
    pub id: &'a str,
    pub identifier: &'a str,
    pub name: &'a str,
    pub cover_image: Option<&'a str>,
    pub cover_image_url: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub description: &'a str,
    pub is_default: bool,
}

/// `ProjectLiteSerializer.to_representation` output, in wire order (nested
/// copy; canonical port is L1's — see [`PROJECT_LITE_WIRE_FIELDS`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectLiteView<'a> {
    pub id: &'a str,
    pub identifier: &'a str,
    pub name: &'a str,
    pub cover_image: Option<&'a str>,
    pub cover_image_url: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub description: &'a str,
    pub is_default: bool,
}

/// Port of `ProjectLiteSerializer` (`project.py:120-133`) as consumed
/// nested by the member/invite serializers.
pub fn project_lite_to_representation<'a>(row: &'a ProjectLiteRow<'a>) -> ProjectLiteView<'a> {
    ProjectLiteView {
        id: row.id,
        identifier: row.identifier,
        name: row.name,
        cover_image: row.cover_image,
        cover_image_url: row.cover_image_url,
        logo_props: row.logo_props,
        description: row.description,
        is_default: row.is_default,
    }
}

/// The non-nested `ProjectMember` columns shared by the member and admin
/// shapes (`db/models/project.py:332-346`): nullable `comment`
/// (`TextField(null=True)`), `role` (`PositiveSmallIntegerField`, 20/15/5),
/// JSON `view_props` / `default_props` / `preferences`, `sort_order`
/// (`FloatField`, default 65535), `is_active`, nullable `created_by` /
/// `updated_by` FKs (render as UUID strings, `null` when unset), and the
/// `created_at` / `updated_at` / nullable `deleted_at` datetimes
/// (pre-rendered DRF strings, byte-exact passthrough).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberCore<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub comment: Option<&'a str>,
    pub role: i64,
    pub view_props: &'a serde_json::Value,
    pub default_props: &'a serde_json::Value,
    pub preferences: &'a serde_json::Value,
    pub sort_order: f64,
    pub is_active: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// A `ProjectMember` row for [`member_to_representation`]:
/// non-nullable `workspace` / `project` FKs render nested lite objects;
/// the nullable `member` FK (`db/models/project.py:334-339`, `null=True`)
/// renders `null` when unset (DRF skips a nested serializer whose
/// attribute is `None`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberRow<'a> {
    pub core: ProjectMemberCore<'a>,
    pub workspace: WorkspaceLiteRow<'a>,
    pub project: ProjectLiteRow<'a>,
    pub member: Option<UserLiteRow<'a>>,
}

/// `ProjectMemberSerializer.to_representation` output
/// (`project.py:193-200`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectMemberView<'a> {
    pub id: &'a str,
    pub workspace: WorkspaceLiteView<'a>,
    pub project: ProjectLiteView<'a>,
    pub member: Option<UserLiteView<'a>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub comment: Option<&'a str>,
    pub role: i64,
    pub view_props: &'a serde_json::Value,
    pub default_props: &'a serde_json::Value,
    pub preferences: &'a serde_json::Value,
    pub sort_order: f64,
    pub is_active: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// Port of `ProjectMemberSerializer` (`project.py:193-200`).
pub fn member_to_representation<'a>(row: &'a ProjectMemberRow<'a>) -> ProjectMemberView<'a> {
    let core = &row.core;
    ProjectMemberView {
        id: core.id,
        workspace: workspace_lite_to_representation(&row.workspace),
        project: project_lite_to_representation(&row.project),
        member: row.member.as_ref().map(user_lite_to_representation),
        created_at: core.created_at,
        updated_at: core.updated_at,
        deleted_at: core.deleted_at,
        comment: core.comment,
        role: core.role,
        view_props: core.view_props,
        default_props: core.default_props,
        preferences: core.preferences,
        sort_order: core.sort_order,
        is_active: core.is_active,
        created_by: core.created_by,
        updated_by: core.updated_by,
    }
}

/// A `ProjectMember` row for [`member_admin_to_representation`]: same as
/// [`ProjectMemberRow`] with the admin-lite member shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberAdminRow<'a> {
    pub core: ProjectMemberCore<'a>,
    pub workspace: WorkspaceLiteRow<'a>,
    pub project: ProjectLiteRow<'a>,
    pub member: Option<UserAdminLiteRow<'a>>,
}

/// `ProjectMemberAdminSerializer.to_representation` output
/// (`project.py:215-222`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectMemberAdminView<'a> {
    pub id: &'a str,
    pub workspace: WorkspaceLiteView<'a>,
    pub project: ProjectLiteView<'a>,
    pub member: Option<UserAdminLiteView<'a>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub comment: Option<&'a str>,
    pub role: i64,
    pub view_props: &'a serde_json::Value,
    pub default_props: &'a serde_json::Value,
    pub preferences: &'a serde_json::Value,
    pub sort_order: f64,
    pub is_active: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// Port of `ProjectMemberAdminSerializer` (`project.py:215-222`).
pub fn member_admin_to_representation<'a>(
    row: &'a ProjectMemberAdminRow<'a>,
) -> ProjectMemberAdminView<'a> {
    let core = &row.core;
    ProjectMemberAdminView {
        id: core.id,
        workspace: workspace_lite_to_representation(&row.workspace),
        project: project_lite_to_representation(&row.project),
        member: row.member.as_ref().map(user_admin_lite_to_representation),
        created_at: core.created_at,
        updated_at: core.updated_at,
        deleted_at: core.deleted_at,
        comment: core.comment,
        role: core.role,
        view_props: core.view_props,
        default_props: core.default_props,
        preferences: core.preferences,
        sort_order: core.sort_order,
        is_active: core.is_active,
        created_by: core.created_by,
        updated_by: core.updated_by,
    }
}

/// A `ProjectMember` row for [`member_role_to_representation`]:
/// `member` / `project` render as PK strings (`PrimaryKeyRelatedField`);
/// `original_role` re-reads `role` (`source="role"`, `project.py:226`) so
/// it takes no separate input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMemberRoleRow<'a> {
    pub id: &'a str,
    pub role: i64,
    pub member: Option<&'a str>,
    pub project: &'a str,
    pub created_at: &'a str,
}

/// `ProjectMemberRoleSerializer.to_representation` output
/// (`project.py:225-231`), in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectMemberRoleView<'a> {
    pub id: &'a str,
    pub role: i64,
    pub member: Option<&'a str>,
    pub project: &'a str,
    pub original_role: i64,
    pub created_at: &'a str,
}

/// Port of `ProjectMemberRoleSerializer` (`project.py:225-231`).
pub fn member_role_to_representation<'a>(
    row: &'a ProjectMemberRoleRow<'a>,
) -> ProjectMemberRoleView<'a> {
    ProjectMemberRoleView {
        id: row.id,
        role: row.role,
        member: row.member,
        project: row.project,
        original_role: row.role,
        created_at: row.created_at,
    }
}

/// Port of the `fields=` call sites (`app/views/project/member.py:168,201`):
/// the views pass `fields=("id", "member", "role")`, but
/// `DynamicBaseSerializer.__init__` overwrites `fields` with `expand`
/// (`base.py:17-20`), so the argument is silently ignored and the full
/// 6-key shape renders. This kernel takes the same argument and ignores
/// it — BUG-fields, ported as-is.
pub fn member_role_fields_to_representation<'a>(
    row: &'a ProjectMemberRoleRow<'a>,
    _fields: &[&str],
) -> ProjectMemberRoleView<'a> {
    member_role_to_representation(row)
}

/// A `ProjectMember` row for [`preference_to_representation`]:
/// `project_id` / `member_id` / `workspace_id` are the raw FK attnames
/// (read-only UUID strings); `member_id` is nullable with the `member` FK.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberPreferenceRow<'a> {
    pub preferences: &'a serde_json::Value,
    pub project_id: &'a str,
    pub member_id: Option<&'a str>,
    pub workspace_id: &'a str,
}

/// `ProjectMemberPreferenceSerializer.to_representation` output
/// (`project.py:203-212`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectMemberPreferenceView<'a> {
    pub preferences: &'a serde_json::Value,
    pub project_id: &'a str,
    pub member_id: Option<&'a str>,
    pub workspace_id: &'a str,
}

/// Port of `ProjectMemberPreferenceSerializer` (`project.py:203-212`)
/// read shape.
pub fn preference_to_representation<'a>(
    row: &'a ProjectMemberPreferenceRow<'a>,
) -> ProjectMemberPreferenceView<'a> {
    ProjectMemberPreferenceView {
        preferences: row.preferences,
        project_id: row.project_id,
        member_id: row.member_id,
        workspace_id: row.workspace_id,
    }
}

/// Port of `ProjectMemberPreferenceSerializer.validate_preferences`
/// (`project.py:208-212`): `instance.preferences.update(value)`.
///
/// The merge is `dict.update` — shallow: every top-level patch key
/// replaces the stored value wholesale, so a nested dict in the patch
/// REPLACES the whole sub-dict. It mutates the instance in place during
/// validation (hence `&mut`) and returns the same mapping. A non-dict
/// patch never reaches this kernel: DRF's `JSONField` stage runs first,
/// and a non-dict that clears it dies with `TypeError` inside `.update`
/// (a Django 500), which is the handler's arm, not this kernel's.
pub fn merge_preferences<'a>(
    current: &'a mut serde_json::Map<String, serde_json::Value>,
    patch: &serde_json::Map<String, serde_json::Value>,
) -> &'a mut serde_json::Map<String, serde_json::Value> {
    for (key, value) in patch {
        current.insert(key.clone(), value.clone());
    }
    current
}

/// A `ProjectMemberInvite` row for [`invite_to_representation`]
/// (`db/models/project.py:314-330`): non-nullable `project` / `workspace`
/// FKs render nested lite objects; `message` (`TextField(null=True)`) and
/// `responded_at` (nullable datetime) render `null` when unset.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberInviteRow<'a> {
    pub id: &'a str,
    pub project: ProjectLiteRow<'a>,
    pub workspace: WorkspaceLiteRow<'a>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub email: &'a str,
    pub accepted: bool,
    pub token: &'a str,
    pub message: Option<&'a str>,
    pub responded_at: Option<&'a str>,
    pub role: i64,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// `ProjectMemberInviteSerializer.to_representation` output
/// (`project.py:234-240`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectMemberInviteView<'a> {
    pub id: &'a str,
    pub project: ProjectLiteView<'a>,
    pub workspace: WorkspaceLiteView<'a>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub email: &'a str,
    pub accepted: bool,
    pub token: &'a str,
    pub message: Option<&'a str>,
    pub responded_at: Option<&'a str>,
    pub role: i64,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// Port of `ProjectMemberInviteSerializer` (`project.py:234-240`).
pub fn invite_to_representation<'a>(
    row: &'a ProjectMemberInviteRow<'a>,
) -> ProjectMemberInviteView<'a> {
    ProjectMemberInviteView {
        id: row.id,
        project: project_lite_to_representation(&row.project),
        workspace: workspace_lite_to_representation(&row.workspace),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        email: row.email,
        accepted: row.accepted,
        token: row.token,
        message: row.message,
        responded_at: row.responded_at,
        role: row.role,
        created_by: row.created_by,
        updated_by: row.updated_by,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_project/FX-APROJ-02.serializers_member_invite.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn section<'a>(fixture: &'a Value, name: &str) -> &'a Value {
        fixture
            .get(name)
            .unwrap_or_else(|| panic!("fixture lacks {name}"))
    }

    fn fixture_keys(section: &Value) -> Vec<String> {
        section["keys"]
            .as_array()
            .expect("keys array")
            .iter()
            .map(|key| key.as_str().expect("key string").to_string())
            .collect()
    }

    /// Top-level JSON key order of a struct's serialization, read off the
    /// serialized string: struct serialization always emits declaration
    /// order.
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

    /// Byte-identical replay: `serde_json` builds with `preserve_order`,
    /// so the parsed golden keeps document order and the compact forms
    /// must match exactly.
    fn assert_byte_replay<T: serde::Serialize>(produced: &T, expected: &Value) {
        assert_eq!(
            serde_json::to_string(produced).expect("serializes"),
            serde_json::to_string(expected).expect("serializes"),
            "byte-identical replay mismatch"
        );
    }

    fn req_str<'a>(value: &'a Value, key: &str) -> &'a str {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{key} is a string"))
    }

    fn workspace_row<'a>(value: &'a Value) -> WorkspaceLiteRow<'a> {
        WorkspaceLiteRow {
            name: req_str(value, "name"),
            slug: req_str(value, "slug"),
            id: req_str(value, "id"),
            logo_url: value["logo_url"].as_str(),
        }
    }

    fn user_row<'a>(value: &'a Value) -> UserLiteRow<'a> {
        UserLiteRow {
            id: req_str(value, "id"),
            first_name: req_str(value, "first_name"),
            last_name: req_str(value, "last_name"),
            avatar: req_str(value, "avatar"),
            avatar_url: value["avatar_url"].as_str(),
            is_bot: value["is_bot"].as_bool().expect("is_bot bool"),
            display_name: req_str(value, "display_name"),
        }
    }

    fn user_admin_row<'a>(value: &'a Value) -> UserAdminLiteRow<'a> {
        UserAdminLiteRow {
            id: req_str(value, "id"),
            first_name: req_str(value, "first_name"),
            last_name: req_str(value, "last_name"),
            avatar: req_str(value, "avatar"),
            avatar_url: value["avatar_url"].as_str(),
            is_bot: value["is_bot"].as_bool().expect("is_bot bool"),
            display_name: req_str(value, "display_name"),
            email: value["email"].as_str(),
            last_login_medium: req_str(value, "last_login_medium"),
        }
    }

    fn project_row<'a>(value: &'a Value) -> ProjectLiteRow<'a> {
        ProjectLiteRow {
            id: req_str(value, "id"),
            identifier: req_str(value, "identifier"),
            name: req_str(value, "name"),
            cover_image: value["cover_image"].as_str(),
            cover_image_url: value["cover_image_url"].as_str(),
            logo_props: &value["logo_props"],
            description: req_str(value, "description"),
            is_default: value["is_default"].as_bool().expect("is_default bool"),
        }
    }

    fn member_core<'a>(value: &'a Value) -> ProjectMemberCore<'a> {
        ProjectMemberCore {
            id: req_str(value, "id"),
            created_at: req_str(value, "created_at"),
            updated_at: req_str(value, "updated_at"),
            deleted_at: value["deleted_at"].as_str(),
            comment: value["comment"].as_str(),
            role: value["role"].as_i64().expect("role int"),
            view_props: &value["view_props"],
            default_props: &value["default_props"],
            preferences: &value["preferences"],
            sort_order: value["sort_order"].as_f64().expect("sort_order float"),
            is_active: value["is_active"].as_bool().expect("is_active bool"),
            created_by: value["created_by"].as_str(),
            updated_by: value["updated_by"].as_str(),
        }
    }

    #[test]
    fn wire_fields_match_fixture_key_order() {
        // Every WIRE_FIELDS const equals the fixture's recorded DRF output
        // order for that serializer.
        let golden = fixture();
        let cases: &[(&str, &[&str])] = &[
            ("member", &PROJECT_MEMBER_WIRE_FIELDS),
            ("member_admin", &PROJECT_MEMBER_WIRE_FIELDS),
            ("member_role", &PROJECT_MEMBER_ROLE_WIRE_FIELDS),
            (
                "member_role_fields_kwarg_ignored",
                &PROJECT_MEMBER_ROLE_WIRE_FIELDS,
            ),
            ("preference", &PROJECT_MEMBER_PREFERENCE_WIRE_FIELDS),
            ("invite", &PROJECT_MEMBER_INVITE_WIRE_FIELDS),
            ("nested_user_lite", &USER_LITE_WIRE_FIELDS),
            ("nested_user_admin_lite", &USER_ADMIN_LITE_WIRE_FIELDS),
            ("nested_workspace_lite", &WORKSPACE_LITE_WIRE_FIELDS),
        ];
        for (name, fields) in cases {
            assert_eq!(
                fields
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<_>>(),
                fixture_keys(section(&golden, name)),
                "{name} wire order"
            );
        }
        // The nested project has no standalone keys array; its order is
        // pinned here against Meta.fields (project.py:123-132).
        assert_eq!(
            PROJECT_LITE_WIRE_FIELDS.to_vec(),
            [
                "id",
                "identifier",
                "name",
                "cover_image",
                "cover_image_url",
                "logo_props",
                "description",
                "is_default",
            ]
        );
    }

    #[test]
    fn member_replays_golden() {
        // Fixture member: full 16-key shape with nested lite objects.
        let golden = fixture();
        let case = section(&golden, "member");
        let data = &case["data"];
        let row = ProjectMemberRow {
            core: member_core(data),
            workspace: workspace_row(&data["workspace"]),
            project: project_row(&data["project"]),
            member: Some(user_row(&data["member"])),
        };
        let view = member_to_representation(&row);
        assert_eq!(serialized_keys(&view), fixture_keys(case));
        assert_eq!(
            serialized_keys(&view.workspace),
            fixture_keys(section(&golden, "nested_workspace_lite")),
        );
        assert_eq!(
            serialized_keys(&view.project),
            PROJECT_LITE_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            serialized_keys(view.member.as_ref().expect("member present")),
            fixture_keys(section(&golden, "nested_user_lite")),
        );
        assert_byte_replay(&view, &case["data"]);
    }

    #[test]
    fn member_admin_replays_golden() {
        // Fixture member_admin: same 16 keys, admin-lite member shape.
        let golden = fixture();
        let case = section(&golden, "member_admin");
        let data = &case["data"];
        let row = ProjectMemberAdminRow {
            core: member_core(data),
            workspace: workspace_row(&data["workspace"]),
            project: project_row(&data["project"]),
            member: Some(user_admin_row(&data["member"])),
        };
        let view = member_admin_to_representation(&row);
        assert_eq!(serialized_keys(&view), fixture_keys(case));
        assert_eq!(
            serialized_keys(view.member.as_ref().expect("member present")),
            fixture_keys(section(&golden, "nested_user_admin_lite")),
        );
        assert_byte_replay(&view, &case["data"]);
    }

    #[test]
    fn member_null_member_renders_null() {
        // Nullable member FK (db/models/project.py:334-339): DRF renders
        // a nested serializer with a None attribute as null. No fixture
        // row covers it; the golden pins the rest of the shape.
        let golden = fixture();
        let data = &section(&golden, "member")["data"];
        let row = ProjectMemberRow {
            core: member_core(data),
            workspace: workspace_row(&data["workspace"]),
            project: project_row(&data["project"]),
            member: None,
        };
        let produced = serde_json::to_value(member_to_representation(&row)).expect("serializes");
        assert_eq!(produced["member"], Value::Null);
        assert_eq!(produced["id"], data["id"]);
        let admin = ProjectMemberAdminRow {
            core: member_core(data),
            workspace: workspace_row(&data["workspace"]),
            project: project_row(&data["project"]),
            member: None,
        };
        let produced =
            serde_json::to_value(member_admin_to_representation(&admin)).expect("serializes");
        assert_eq!(produced["member"], Value::Null);
    }

    #[test]
    fn member_role_replays_golden() {
        // Fixture member_role: (id, role, member, project, original_role,
        // created_at); original_role re-reads role (source="role").
        let golden = fixture();
        let case = section(&golden, "member_role");
        let data = &case["data"];
        let row = ProjectMemberRoleRow {
            id: req_str(data, "id"),
            role: data["role"].as_i64().expect("role int"),
            member: data["member"].as_str(),
            project: req_str(data, "project"),
            created_at: req_str(data, "created_at"),
        };
        let view = member_role_to_representation(&row);
        assert_eq!(view.original_role, view.role);
        assert_eq!(serialized_keys(&view), fixture_keys(case));
        assert_byte_replay(&view, &case["data"]);
    }

    #[test]
    fn member_role_fields_kwarg_ignored_replays_golden() {
        // BUG-fields: the views pass fields=("id", "member", "role")
        // (member.py:168,201) but base.py:17-20 drops the kwarg, so the
        // full 6-key shape renders.
        let golden = fixture();
        let case = section(&golden, "member_role_fields_kwarg_ignored");
        let data = &case["data"];
        let row = ProjectMemberRoleRow {
            id: req_str(data, "id"),
            role: data["role"].as_i64().expect("role int"),
            member: data["member"].as_str(),
            project: req_str(data, "project"),
            created_at: req_str(data, "created_at"),
        };
        let view = member_role_fields_to_representation(&row, &["id", "member", "role"]);
        assert_eq!(serialized_keys(&view), fixture_keys(case));
        assert_eq!(serialized_keys(&view).len(), 6);
        assert_byte_replay(&view, &case["data"]);
    }

    #[test]
    fn preference_replays_golden() {
        // Fixture preference: (preferences, project_id, member_id,
        // workspace_id).
        let golden = fixture();
        let case = section(&golden, "preference");
        let data = &case["data"];
        let row = ProjectMemberPreferenceRow {
            preferences: &data["preferences"],
            project_id: req_str(data, "project_id"),
            member_id: data["member_id"].as_str(),
            workspace_id: req_str(data, "workspace_id"),
        };
        let view = preference_to_representation(&row);
        assert_eq!(serialized_keys(&view), fixture_keys(case));
        assert_byte_replay(&view, &case["data"]);
    }

    #[test]
    fn preference_merge_replays_golden() {
        // Fixture preference_merge: dict.update is shallow — a top-level
        // key is added as-is, a nested dict patch REPLACES the sub-dict.
        // The patches below are the only ones consistent with the
        // recorded before/after states under shallow update.
        let golden = fixture();
        let case = section(&golden, "preference_merge");
        let mut current = case["initial"].as_object().expect("initial object").clone();
        let first_patch = serde_json::json!({"theme": "dark"});
        {
            let merged: &mut serde_json::Map<String, Value> =
                merge_preferences(&mut current, first_patch.as_object().expect("patch object"));
            assert_eq!(
                Value::Object(merged.clone()),
                case["after_first_validate"],
                "first merge adds the top-level key"
            );
        }
        assert_eq!(
            Value::Object(current.clone()),
            case["after_first_validate"],
            "merge mutates the mapping in place, like dict.update"
        );
        let second_patch = serde_json::json!({"pages": {"block_display": false}});
        merge_preferences(
            &mut current,
            second_patch.as_object().expect("patch object"),
        );
        assert_eq!(
            Value::Object(current),
            case["after_second_validate_nested_replace"],
            "nested dict patch replaces the whole sub-dict"
        );
    }

    #[test]
    fn invite_replays_golden() {
        // Fixture invite: full 14-key shape with nested project/workspace
        // lite objects.
        let golden = fixture();
        let case = section(&golden, "invite");
        let data = &case["data"];
        let row = ProjectMemberInviteRow {
            id: req_str(data, "id"),
            project: project_row(&data["project"]),
            workspace: workspace_row(&data["workspace"]),
            created_at: req_str(data, "created_at"),
            updated_at: req_str(data, "updated_at"),
            deleted_at: data["deleted_at"].as_str(),
            email: req_str(data, "email"),
            accepted: data["accepted"].as_bool().expect("accepted bool"),
            token: req_str(data, "token"),
            message: data["message"].as_str(),
            responded_at: data["responded_at"].as_str(),
            role: data["role"].as_i64().expect("role int"),
            created_by: data["created_by"].as_str(),
            updated_by: data["updated_by"].as_str(),
        };
        let view = invite_to_representation(&row);
        assert_eq!(serialized_keys(&view), fixture_keys(case));
        assert_byte_replay(&view, &case["data"]);
    }

    #[test]
    fn nested_lites_replay_goldens() {
        // Standalone nested sections: user lite, admin lite, workspace lite.
        let golden = fixture();
        let user = section(&golden, "nested_user_lite");
        let user_binding = user_row(&user["data"]);
        let view = user_lite_to_representation(&user_binding);
        assert_eq!(serialized_keys(&view), fixture_keys(user));
        assert_byte_replay(&view, &user["data"]);

        let admin = section(&golden, "nested_user_admin_lite");
        let admin_binding = user_admin_row(&admin["data"]);
        let view = user_admin_lite_to_representation(&admin_binding);
        assert_eq!(serialized_keys(&view), fixture_keys(admin));
        assert_byte_replay(&view, &admin["data"]);

        let workspace = section(&golden, "nested_workspace_lite");
        let workspace_binding = workspace_row(&workspace["data"]);
        let view = workspace_lite_to_representation(&workspace_binding);
        assert_eq!(serialized_keys(&view), fixture_keys(workspace));
        assert_byte_replay(&view, &workspace["data"]);
    }

    #[test]
    fn url_resolvers_match_python_properties() {
        // workspace.py:146-153 / project.py:176-184 / user.py:143-151:
        // attached asset wins (even a missing URL — no fall-through),
        // else the text column when truthy, else None.
        use crate::v1_projects::ser_collab::resolve_avatar_url;
        assert_eq!(resolve_workspace_logo_url(false, None, None), None);
        assert_eq!(resolve_workspace_logo_url(false, None, Some("")), None);
        assert_eq!(
            resolve_workspace_logo_url(false, None, Some("https://logo")),
            Some("https://logo")
        );
        assert_eq!(
            resolve_workspace_logo_url(true, Some("https://asset"), Some("https://logo")),
            Some("https://asset")
        );
        assert_eq!(
            resolve_workspace_logo_url(true, None, Some("https://logo")),
            None,
            "attached asset with no URL does not fall through"
        );
        assert_eq!(resolve_project_cover_image_url(false, None, None), None);
        assert_eq!(resolve_project_cover_image_url(false, None, Some("")), None);
        assert_eq!(
            resolve_project_cover_image_url(false, None, Some("https://cover")),
            Some("https://cover")
        );
        assert_eq!(
            resolve_project_cover_image_url(true, None, Some("https://cover")),
            None,
            "attached asset with no URL does not fall through"
        );
        // The shared avatar kernel resolves the app golden's inputs
        // (no asset, empty avatar text) to the recorded null.
        assert_eq!(resolve_avatar_url(false, None, ""), None);
        assert_eq!(
            resolve_avatar_url(false, None, "https://avatar"),
            Some("https://avatar")
        );
    }
}
