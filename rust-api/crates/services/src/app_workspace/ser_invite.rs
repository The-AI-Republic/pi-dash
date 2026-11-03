#![forbid(unsafe_code)]

//! Workspace invite + join-request + theme/props serializers (D-24, SER-B).
//!
//! Port of `apps/api/pi_dash/app/serializers/workspace.py:110-194`:
//!
//! * `WorkSpaceMemberInviteSerializer` (`:110-131`, nested `workspace` lite
//!   + `invite_link` method field)
//! * `WorkspaceJoinRequestSerializer` (`:133-152`, nested
//!   `workspace`/`requester`)
//! * `UserWorkspaceJoinRequestSerializer` (`:154-180`, requester-facing
//!   8-key shape, `workspace` deliberately omitted)
//! * `WorkspaceThemeSerializer` (`:182-187`)
//! * `WorkspaceUserPropertiesSerializer` (`:189-194`)
//!
//! Pure read shapes in the `serializers_engage` style: each
//! `to_representation` takes a row borrowed from the caller and returns a
//! `serde::Serialize` view whose fields are the live DRF wire fields in DRF
//! order. Key order follows the confirmed `__all__` rule (declared `id`
//! from `BaseSerializer`, then the subclass declares, then non-relational
//! concrete columns in `_meta` order, then forward relations trailing),
//! verified field by field against a live in-venv probe (Django 4.2.30 /
//! DRF 3.15.2, in-memory instances, no DB); the explicit `Meta.fields`
//! list of the requester-facing serializer renders in list order. UUID and
//! FK primary keys render as strings (`PrimaryKeyRelatedField`); a null FK
//! renders `null`, and a nested serializer over a null FK renders a
//! present-but-null key (probed live). Datetimes cross this boundary
//! already rendered as DRF iso-8601 strings (formatting owns to the DB
//! edge), so rendering here is a byte-exact passthrough. JSON blobs
//! (`colors`, `filters`, `display_filters`, `display_properties`,
//! `rich_filters`) pass through by reference.
//!
//! Nested details reuse the merged app ports (call, don't copy):
//! `workspace` is D-25's app `WorkspaceLiteSerializer` port
//! (`app_project::ser_member`, `workspace.py:79-83`), `requester` is D-25's
//! app `UserLiteSerializer` port (`app_project::ser_shared`,
//! `user.py:141-154`). Rows hold the already-rendered nested views —
//! handlers compose by rendering the nests first. The canonical D-24
//! owners of those lite shapes (PIDASHCONV-600/603) reuse or absorb the
//! D-25 ports when they land; this module defines no lite twin.
//!
//! `invite_link` (`:114-115`) is a raw f-string interpolation with no
//! URL-encoding; [`invite_link`] formats the same template byte for byte.
//!
//! Write-path note (shape-only no-ops preserved as documentation, not
//! code): the invite / join-request / requester-facing serializers have no
//! `data=` call site — the invite and join-request views read through them
//! and write via `objects.create` / direct `save`
//! (`app/views/workspace/invite.py`, `join_request.py`) — so their
//! `read_only_fields` never filter an input on the wire. The two writes
//! through this file's serializers are the theme create
//! (`app/views/workspace/base.py:361`, `is_valid` then 400 on
//! `serializer.errors`) and the user-properties patch
//! (`app/views/workspace/user.py:263`, `partial=True`); both are
//! DRF-generic validation with no custom `validate_*` method, owned by the
//! handler issues (H-A PIDASHCONV-615, H-I PIDASHCONV-623) against the
//! F-W24-15 error goldens — the same division as the L3 estimate
//! `validate` precedent. The `*_READ_ONLY_FIELDS` consts below port the
//! `Meta.read_only_fields` lists exactly (the F-W24-02 oracle) so handlers
//! can replicate input filtering; none of the five units raises a
//! serializer-specific error body.
//!
//! Ported bugs and quirks (translate, don't redesign; also listed in the
//! PR):
//!
//! * BUG-remainder-invite (`workspace.py:120-130`): the invite
//!   `read_only_fields` leave `role` + `accepted` writable — and, live,
//!   also `deleted_at`/`created_by`/`updated_by`, which the 9-key list
//!   omits (the fixture's "only role + accepted" names the two meaningful
//!   ones). Ported exactly via [`INVITE_READ_ONLY_FIELDS`]; no view writes
//!   through this serializer, so the remainder has no wire effect.
//! * BUG-remainder-join (`workspace.py:140-151`): the join-request list
//!   leaves `message` writable — and, live, the same audit trio.
//!   Ported exactly via [`JOIN_REQUEST_READ_ONLY_FIELDS`]; likewise
//!   write-free on the wire.
//! * OMISSION-requester (`workspace.py:154-179`, docstring `:155-163`):
//!   the requester-facing shape deliberately omits `workspace` so a
//!   requester cannot enumerate admin emails; the pending state looks
//!   identical whether or not the email resolved. Ported as an absent key
//!   (not `null`) — [`USER_JOIN_REQUEST_WIRE_FIELDS`] has no `workspace`.
//! * Theme/props `Meta.read_only_fields` cover only the scope FKs
//!   (`[workspace, actor]` / `[workspace, user]`); the live-effective set
//!   additionally holds base-declared `id` and the auto-now
//!   `created_at`/`updated_at` (probed). The consts port the Meta lists;
//!   handlers add the framework-implied three when filtering input.

use serde::Serialize;

use crate::app_project::ser_member::WorkspaceLiteView;
use crate::app_project::ser_shared::UserLiteView;

/// `WorkSpaceMemberInviteSerializer` wire keys (`workspace.py:110-131`),
/// in live-DRF order: declared `id`/`workspace`/`invite_link`, concrete
/// `WorkspaceMemberInvite` columns (`db/models/workspace.py:236-243` over
/// `BaseModel`), then the `created_by`/`updated_by` forward relations.
pub const INVITE_WIRE_FIELDS: [&str; 14] = [
    "id",
    "workspace",
    "invite_link",
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

/// `WorkSpaceMemberInviteSerializer.Meta.read_only_fields`
/// (`workspace.py:120-130`), in declaration order.
pub const INVITE_READ_ONLY_FIELDS: [&str; 9] = [
    "id",
    "email",
    "token",
    "workspace",
    "message",
    "responded_at",
    "created_at",
    "updated_at",
    "invite_link",
];

/// `WorkspaceJoinRequestSerializer` wire keys (`workspace.py:133-152`), in
/// live-DRF order: declared `id`/`workspace`/`requester`, concrete
/// `WorkspaceJoinRequest` columns (`db/models/workspace.py:264-304`),
/// then forward relations (`created_by`/`updated_by` first, then the
/// model's own `responded_by`).
pub const JOIN_REQUEST_WIRE_FIELDS: [&str; 14] = [
    "id",
    "workspace",
    "requester",
    "created_at",
    "updated_at",
    "deleted_at",
    "admin_email",
    "message",
    "role",
    "status",
    "responded_at",
    "created_by",
    "updated_by",
    "responded_by",
];

/// `WorkspaceJoinRequestSerializer.Meta.read_only_fields`
/// (`workspace.py:140-151`), in declaration order.
pub const JOIN_REQUEST_READ_ONLY_FIELDS: [&str; 10] = [
    "id",
    "workspace",
    "requester",
    "admin_email",
    "role",
    "status",
    "responded_at",
    "responded_by",
    "created_at",
    "updated_at",
];

/// `UserWorkspaceJoinRequestSerializer.Meta.fields`
/// (`workspace.py:167-178`), in list order. `workspace` is deliberately
/// absent (anti-enumeration, `:155-163`).
pub const USER_JOIN_REQUEST_WIRE_FIELDS: [&str; 8] = [
    "id",
    "requester",
    "admin_email",
    "message",
    "status",
    "responded_at",
    "created_at",
    "updated_at",
];

/// `UserWorkspaceJoinRequestSerializer.Meta.read_only_fields`
/// (`workspace.py:179`): `read_only_fields = fields`, the whole shape.
pub const USER_JOIN_REQUEST_READ_ONLY_FIELDS: [&str; 8] = USER_JOIN_REQUEST_WIRE_FIELDS;

/// `WorkspaceThemeSerializer` wire keys (`workspace.py:182-187`), in
/// live-DRF order: declared `id`, concrete `WorkspaceTheme` columns
/// (`db/models/workspace.py:349-353`), then forward relations
/// (`created_by`/`updated_by`, then `workspace`/`actor` as PK strings).
pub const THEME_WIRE_FIELDS: [&str; 10] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "colors",
    "created_by",
    "updated_by",
    "workspace",
    "actor",
];

/// `WorkspaceThemeSerializer.Meta.read_only_fields`
/// (`workspace.py:186`).
pub const THEME_READ_ONLY_FIELDS: [&str; 2] = ["workspace", "actor"];

/// `WorkspaceUserPropertiesSerializer` wire keys
/// (`workspace.py:189-194`), in live-DRF order: declared `id`, concrete
/// `WorkspaceUserProperties` columns (`db/models/workspace.py:372-397`),
/// then forward relations (`created_by`/`updated_by`, then
/// `workspace`/`user` as PK strings).
pub const USER_PROPERTIES_WIRE_FIELDS: [&str; 14] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "filters",
    "display_filters",
    "display_properties",
    "rich_filters",
    "navigation_project_limit",
    "navigation_control_preference",
    "created_by",
    "updated_by",
    "workspace",
    "user",
];

/// `WorkspaceUserPropertiesSerializer.Meta.read_only_fields`
/// (`workspace.py:193`).
pub const USER_PROPERTIES_READ_ONLY_FIELDS: [&str; 2] = ["workspace", "user"];

/// Port of `get_invite_link` (`workspace.py:114-115`): the raw f-string
/// template. Values interpolate unquoted, exactly as Python renders them
/// (`str()` of the UUID / slug / token); no URL-encoding is applied.
pub fn invite_link(invitation_id: &str, slug: &str, token: &str) -> String {
    format!("/workspace-invitations/?invitation_id={invitation_id}&slug={slug}&token={token}")
}

/// A `WorkspaceMemberInvite` row for [`invite_to_representation`]
/// (`db/models/workspace.py:236-243`): the non-nullable `workspace` FK
/// renders nested (the caller supplies the rendered lite view);
/// `invite_link` is derived from `id` + `workspace.slug` + `token`, so it
/// takes no separate input. `message` (`TextField(null=True)`) and
/// `responded_at` (nullable datetime) render `null` when unset; `role` is
/// the `ROLE_CHOICES` small int (20/15/5); `created_by`/`updated_by` are
/// nullable FK PK strings; datetimes are pre-rendered DRF strings.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberInviteRow<'a> {
    pub id: &'a str,
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

/// `WorkSpaceMemberInviteSerializer.to_representation` output
/// (`workspace.py:110-131`), in [`INVITE_WIRE_FIELDS`] order. Every field
/// borrows the row except the derived [`invite_link`], which is owned.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberInviteView<'a> {
    pub id: &'a str,
    pub workspace: WorkspaceLiteView<'a>,
    pub invite_link: String,
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

/// Port of `WorkSpaceMemberInviteSerializer` (`workspace.py:110-131`).
pub fn invite_to_representation<'a>(row: &'a MemberInviteRow<'a>) -> MemberInviteView<'a> {
    MemberInviteView {
        id: row.id,
        workspace: row.workspace.clone(),
        invite_link: invite_link(row.id, row.workspace.slug, row.token),
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

/// A `WorkspaceJoinRequest` row for [`join_request_to_representation`]
/// (`db/models/workspace.py:264-304`): the nullable `workspace` FK
/// (`null=True`, `:283-289` — null when the typed admin email resolved to
/// no admin) renders a present `null` nest when unset; `requester` is
/// non-null and renders nested. `message` (`TextField(null=True,
/// blank=True)`), `responded_at` (nullable datetime) and `responded_by`
/// (nullable FK, `:296-302`) render `null` when unset; `status` is the
/// `PENDING`/`APPROVED`/`DENIED` text choice; `role` defaults to 15.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinRequestRow<'a> {
    pub id: &'a str,
    pub workspace: Option<WorkspaceLiteView<'a>>,
    pub requester: UserLiteView<'a>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub admin_email: &'a str,
    pub message: Option<&'a str>,
    pub role: i64,
    pub status: &'a str,
    pub responded_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub responded_by: Option<&'a str>,
}

/// `WorkspaceJoinRequestSerializer.to_representation` output
/// (`workspace.py:133-152`), in [`JOIN_REQUEST_WIRE_FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JoinRequestView<'a> {
    pub id: &'a str,
    pub workspace: Option<WorkspaceLiteView<'a>>,
    pub requester: UserLiteView<'a>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub admin_email: &'a str,
    pub message: Option<&'a str>,
    pub role: i64,
    pub status: &'a str,
    pub responded_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub responded_by: Option<&'a str>,
}

/// Port of `WorkspaceJoinRequestSerializer` (`workspace.py:133-152`).
pub fn join_request_to_representation<'a>(row: &'a JoinRequestRow<'a>) -> JoinRequestView<'a> {
    JoinRequestView {
        id: row.id,
        workspace: row.workspace.clone(),
        requester: row.requester.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        admin_email: row.admin_email,
        message: row.message,
        role: row.role,
        status: row.status,
        responded_at: row.responded_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        responded_by: row.responded_by,
    }
}

/// A `WorkspaceJoinRequest` row for
/// [`user_join_request_to_representation`]: the requester-facing 8-key
/// shape carries no `workspace` input at all (anti-enumeration,
/// `workspace.py:155-163`) — there is nothing for a caller to supply.
#[derive(Debug, Clone, PartialEq)]
pub struct UserJoinRequestRow<'a> {
    pub id: &'a str,
    pub requester: UserLiteView<'a>,
    pub admin_email: &'a str,
    pub message: Option<&'a str>,
    pub status: &'a str,
    pub responded_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// `UserWorkspaceJoinRequestSerializer.to_representation` output
/// (`workspace.py:154-180`), in [`USER_JOIN_REQUEST_WIRE_FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserJoinRequestView<'a> {
    pub id: &'a str,
    pub requester: UserLiteView<'a>,
    pub admin_email: &'a str,
    pub message: Option<&'a str>,
    pub status: &'a str,
    pub responded_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// Port of `UserWorkspaceJoinRequestSerializer`
/// (`workspace.py:154-180`).
pub fn user_join_request_to_representation<'a>(
    row: &'a UserJoinRequestRow<'a>,
) -> UserJoinRequestView<'a> {
    UserJoinRequestView {
        id: row.id,
        requester: row.requester.clone(),
        admin_email: row.admin_email,
        message: row.message,
        status: row.status,
        responded_at: row.responded_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// A `WorkspaceTheme` row for [`theme_to_representation`]
/// (`db/models/workspace.py:349-353`): `name` is non-null text,
/// `colors` is a JSON object (`JSONField(default=dict)`), and the
/// non-nullable `workspace`/`actor` FKs render as PK strings.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub colors: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub actor: &'a str,
}

/// `WorkspaceThemeSerializer.to_representation` output
/// (`workspace.py:182-187`), in [`THEME_WIRE_FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub colors: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub actor: &'a str,
}

/// Port of `WorkspaceThemeSerializer` (`workspace.py:182-187`).
pub fn theme_to_representation<'a>(row: &'a ThemeRow<'a>) -> ThemeView<'a> {
    ThemeView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        colors: row.colors,
        created_by: row.created_by,
        updated_by: row.updated_by,
        workspace: row.workspace,
        actor: row.actor,
    }
}

/// A `WorkspaceUserProperties` row for
/// [`user_properties_to_representation`]
/// (`db/models/workspace.py:372-397`): four JSON columns (`filters`,
/// `display_filters`, `display_properties`, `rich_filters`),
/// `navigation_project_limit` (`IntegerField`, default 10),
/// `navigation_control_preference` (the `ACCORDION`/`TABBED` text choice),
/// and the non-nullable `workspace`/`user` FKs as PK strings.
#[derive(Debug, Clone, PartialEq)]
pub struct UserPropertiesRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub filters: &'a serde_json::Value,
    pub display_filters: &'a serde_json::Value,
    pub display_properties: &'a serde_json::Value,
    pub rich_filters: &'a serde_json::Value,
    pub navigation_project_limit: i32,
    pub navigation_control_preference: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub user: &'a str,
}

/// `WorkspaceUserPropertiesSerializer.to_representation` output
/// (`workspace.py:189-194`), in [`USER_PROPERTIES_WIRE_FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserPropertiesView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub filters: &'a serde_json::Value,
    pub display_filters: &'a serde_json::Value,
    pub display_properties: &'a serde_json::Value,
    pub rich_filters: &'a serde_json::Value,
    pub navigation_project_limit: i32,
    pub navigation_control_preference: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub user: &'a str,
}

/// Port of `WorkspaceUserPropertiesSerializer`
/// (`workspace.py:189-194`).
pub fn user_properties_to_representation<'a>(
    row: &'a UserPropertiesRow<'a>,
) -> UserPropertiesView<'a> {
    UserPropertiesView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        filters: row.filters,
        display_filters: row.display_filters,
        display_properties: row.display_properties,
        rich_filters: row.rich_filters,
        navigation_project_limit: row.navigation_project_limit,
        navigation_control_preference: row.navigation_control_preference,
        created_by: row.created_by,
        updated_by: row.updated_by,
        workspace: row.workspace,
        user: row.user,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_workspace/serializers/invites.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn str_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("string array")
            .iter()
            .map(|key| key.as_str().expect("key string").to_string())
            .collect()
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

    fn const_list<const N: usize>(fields: [&str; N]) -> Vec<String> {
        fields.iter().map(|name| name.to_string()).collect()
    }

    fn lite_workspace() -> WorkspaceLiteView<'static> {
        WorkspaceLiteView {
            name: "WS",
            slug: "ws",
            id: "22222222-2222-2222-2222-222222222222",
            logo_url: Some("http://logo/x.png"),
        }
    }

    fn lite_user() -> UserLiteView<'static> {
        UserLiteView {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Fix",
            last_name: "Ture",
            avatar: "http://av/a.png",
            avatar_url: Some("http://av/a.png"),
            is_bot: false,
            display_name: "fx-user",
        }
    }

    #[test]
    fn read_only_consts_match_fixture() {
        // Every read_only port equals the fixture's recorded
        // Meta.read_only_fields list for that serializer.
        let golden = fixture();
        let serializers = &golden["serializers"];
        assert_eq!(
            const_list(INVITE_READ_ONLY_FIELDS),
            str_list(&serializers["WorkSpaceMemberInviteSerializer"]["read_only"]),
            "invite read_only"
        );
        assert_eq!(
            const_list(JOIN_REQUEST_READ_ONLY_FIELDS),
            str_list(&serializers["WorkspaceJoinRequestSerializer"]["read_only"]),
            "join-request read_only"
        );
        // The requester-facing serializer records read_only as "= fields".
        assert_eq!(
            serializers["UserWorkspaceJoinRequestSerializer"]["read_only"]
                .as_str()
                .expect("read_only note"),
            "= fields (:179)",
        );
        assert_eq!(
            const_list(USER_JOIN_REQUEST_READ_ONLY_FIELDS),
            str_list(&serializers["UserWorkspaceJoinRequestSerializer"]["fields"]),
            "requester-facing read_only = fields"
        );
        assert_eq!(
            const_list(THEME_READ_ONLY_FIELDS),
            str_list(&serializers["WorkspaceThemeSerializer"]["read_only"]),
            "theme read_only"
        );
        assert_eq!(
            const_list(USER_PROPERTIES_READ_ONLY_FIELDS),
            str_list(&serializers["WorkspaceUserPropertiesSerializer"]["read_only"]),
            "props read_only"
        );
    }

    #[test]
    fn requester_facing_shape_matches_fixture_case() {
        // Fixture case "requester-facing omits workspace": the 8-key
        // present list is the wire order and "workspace" is absent.
        let golden = fixture();
        let case = golden["cases"]
            .as_array()
            .expect("cases array")
            .iter()
            .find(|case| case["name"] == "requester-facing omits workspace")
            .expect("omission case");
        assert_eq!(
            const_list(USER_JOIN_REQUEST_WIRE_FIELDS),
            str_list(&case["fields_present"]),
        );
        assert_eq!(str_list(&case["fields_absent"]), vec!["workspace"]);
        let row = UserJoinRequestRow {
            id: "88888888-8888-8888-8888-888888888888",
            requester: lite_user(),
            admin_email: "ad@w.io",
            message: Some("m2"),
            status: "APPROVED",
            responded_at: Some("2026-05-06T07:08:09Z"),
            created_at: "2026-01-02T03:04:05.123456Z",
            updated_at: "2026-05-06T07:08:09Z",
        };
        let view = user_join_request_to_representation(&row);
        let keys = serialized_keys(&view);
        assert_eq!(keys, const_list(USER_JOIN_REQUEST_WIRE_FIELDS));
        assert!(
            !keys.contains(&"workspace".to_string()),
            "workspace key must be absent, not null"
        );
    }

    #[test]
    fn invite_link_matches_fixture_template() {
        // Fixture case "invite_link shape": the kernel renders the pinned
        // template with the three interpolations substituted.
        let golden = fixture();
        let template = golden["cases"]
            .as_array()
            .expect("cases array")
            .iter()
            .find(|case| case["name"] == "invite_link shape")
            .expect("invite_link case")["output"]
            .as_str()
            .expect("template string")
            .to_string();
        assert_eq!(
            invite_link("<obj.id>", "<obj.workspace.slug>", "<obj.token>"),
            template,
        );
        assert_eq!(
            invite_link("55555555-5555-5555-5555-555555555555", "ws", "tok123"),
            "/workspace-invitations/?invitation_id=55555555-5555-5555-5555-555555555555&slug=ws&token=tok123",
        );
    }

    #[test]
    fn wire_orders_match_live_django() {
        // Live Django 4.2.30 / DRF 3.15.2 `get_fields()` order per
        // serializer (in-venv probe; in-memory instances, no DB).
        assert_eq!(
            INVITE_WIRE_FIELDS.to_vec(),
            vec![
                "id",
                "workspace",
                "invite_link",
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
            ],
        );
        assert_eq!(
            JOIN_REQUEST_WIRE_FIELDS.to_vec(),
            vec![
                "id",
                "workspace",
                "requester",
                "created_at",
                "updated_at",
                "deleted_at",
                "admin_email",
                "message",
                "role",
                "status",
                "responded_at",
                "created_by",
                "updated_by",
                "responded_by",
            ],
        );
        assert_eq!(
            THEME_WIRE_FIELDS.to_vec(),
            vec![
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "colors",
                "created_by",
                "updated_by",
                "workspace",
                "actor",
            ],
        );
        assert_eq!(
            USER_PROPERTIES_WIRE_FIELDS.to_vec(),
            vec![
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "filters",
                "display_filters",
                "display_properties",
                "rich_filters",
                "navigation_project_limit",
                "navigation_control_preference",
                "created_by",
                "updated_by",
                "workspace",
                "user",
            ],
        );
    }

    #[test]
    fn invite_full_replays_wire_bytes() {
        let row = MemberInviteRow {
            id: "55555555-5555-5555-5555-555555555555",
            workspace: lite_workspace(),
            created_at: "2026-01-02T03:04:05.123456Z",
            updated_at: "2026-05-06T07:08:09Z",
            deleted_at: None,
            email: "a@b.io",
            accepted: false,
            token: "tok123",
            message: Some("hi"),
            responded_at: None,
            role: 15,
            created_by: None,
            updated_by: None,
        };
        let view = invite_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_list(INVITE_WIRE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"55555555-5555-5555-5555-555555555555","workspace":{"name":"WS","slug":"ws","id":"22222222-2222-2222-2222-222222222222","logo_url":"http://logo/x.png"},"invite_link":"/workspace-invitations/?invitation_id=55555555-5555-5555-5555-555555555555&slug=ws&token=tok123","created_at":"2026-01-02T03:04:05.123456Z","updated_at":"2026-05-06T07:08:09Z","deleted_at":null,"email":"a@b.io","accepted":false,"token":"tok123","message":"hi","responded_at":null,"role":15,"created_by":null,"updated_by":null}"#,
        );
    }

    #[test]
    fn join_request_full_replays_wire_bytes() {
        let row = JoinRequestRow {
            id: "66666666-6666-6666-6666-666666666666",
            workspace: Some(lite_workspace()),
            requester: lite_user(),
            created_at: "2026-01-02T03:04:05.123456Z",
            updated_at: "2026-05-06T07:08:09Z",
            deleted_at: None,
            admin_email: "ad@w.io",
            message: None,
            role: 15,
            status: "PENDING",
            responded_at: None,
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            updated_by: None,
            responded_by: None,
        };
        let view = join_request_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_list(JOIN_REQUEST_WIRE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"66666666-6666-6666-6666-666666666666","workspace":{"name":"WS","slug":"ws","id":"22222222-2222-2222-2222-222222222222","logo_url":"http://logo/x.png"},"requester":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Fix","last_name":"Ture","avatar":"http://av/a.png","avatar_url":"http://av/a.png","is_bot":false,"display_name":"fx-user"},"created_at":"2026-01-02T03:04:05.123456Z","updated_at":"2026-05-06T07:08:09Z","deleted_at":null,"admin_email":"ad@w.io","message":null,"role":15,"status":"PENDING","responded_at":null,"created_by":"11111111-1111-1111-1111-111111111111","updated_by":null,"responded_by":null}"#,
        );
    }

    #[test]
    fn join_request_null_workspace_renders_present_null() {
        // The nullable workspace FK (unresolved admin email) renders a
        // present-but-null nest, exactly as DRF short-circuits it.
        let row = JoinRequestRow {
            id: "77777777-7777-7777-7777-777777777777",
            workspace: None,
            requester: lite_user(),
            created_at: "2026-01-02T03:04:05.123456Z",
            updated_at: "2026-05-06T07:08:09Z",
            deleted_at: None,
            admin_email: "zz@w.io",
            message: Some("m"),
            role: 5,
            status: "PENDING",
            responded_at: None,
            created_by: None,
            updated_by: None,
            responded_by: None,
        };
        let view = join_request_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_list(JOIN_REQUEST_WIRE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"77777777-7777-7777-7777-777777777777","workspace":null,"requester":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Fix","last_name":"Ture","avatar":"http://av/a.png","avatar_url":"http://av/a.png","is_bot":false,"display_name":"fx-user"},"created_at":"2026-01-02T03:04:05.123456Z","updated_at":"2026-05-06T07:08:09Z","deleted_at":null,"admin_email":"zz@w.io","message":"m","role":5,"status":"PENDING","responded_at":null,"created_by":null,"updated_by":null,"responded_by":null}"#,
        );
    }

    #[test]
    fn user_join_request_full_replays_wire_bytes() {
        let row = UserJoinRequestRow {
            id: "88888888-8888-8888-8888-888888888888",
            requester: lite_user(),
            admin_email: "ad@w.io",
            message: Some("m2"),
            status: "APPROVED",
            responded_at: Some("2026-05-06T07:08:09Z"),
            created_at: "2026-01-02T03:04:05.123456Z",
            updated_at: "2026-05-06T07:08:09Z",
        };
        let view = user_join_request_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            const_list(USER_JOIN_REQUEST_WIRE_FIELDS)
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"88888888-8888-8888-8888-888888888888","requester":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Fix","last_name":"Ture","avatar":"http://av/a.png","avatar_url":"http://av/a.png","is_bot":false,"display_name":"fx-user"},"admin_email":"ad@w.io","message":"m2","status":"APPROVED","responded_at":"2026-05-06T07:08:09Z","created_at":"2026-01-02T03:04:05.123456Z","updated_at":"2026-05-06T07:08:09Z"}"#,
        );
    }

    #[test]
    fn theme_full_replays_wire_bytes() {
        let colors = serde_json::json!({"bg": "#000"});
        let row = ThemeRow {
            id: "99999999-9999-9999-9999-999999999999",
            created_at: "2026-01-02T03:04:05.123456Z",
            updated_at: "2026-05-06T07:08:09Z",
            deleted_at: None,
            name: "dark",
            colors: &colors,
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            updated_by: Some("11111111-1111-1111-1111-111111111111"),
            workspace: "22222222-2222-2222-2222-222222222222",
            actor: "11111111-1111-1111-1111-111111111111",
        };
        let view = theme_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_list(THEME_WIRE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r##"{"id":"99999999-9999-9999-9999-999999999999","created_at":"2026-01-02T03:04:05.123456Z","updated_at":"2026-05-06T07:08:09Z","deleted_at":null,"name":"dark","colors":{"bg":"#000"},"created_by":"11111111-1111-1111-1111-111111111111","updated_by":"11111111-1111-1111-1111-111111111111","workspace":"22222222-2222-2222-2222-222222222222","actor":"11111111-1111-1111-1111-111111111111"}"##,
        );
    }

    #[test]
    fn user_properties_full_replays_wire_bytes() {
        let filters = serde_json::json!({"a": 1});
        let display_filters = serde_json::json!({});
        let display_properties = serde_json::json!({"p": true});
        let rich_filters = serde_json::json!({});
        let row = UserPropertiesRow {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            created_at: "2026-01-02T03:04:05.123456Z",
            updated_at: "2026-05-06T07:08:09Z",
            deleted_at: None,
            filters: &filters,
            display_filters: &display_filters,
            display_properties: &display_properties,
            rich_filters: &rich_filters,
            navigation_project_limit: 10,
            navigation_control_preference: "TABBED",
            created_by: None,
            updated_by: None,
            workspace: "22222222-2222-2222-2222-222222222222",
            user: "11111111-1111-1111-1111-111111111111",
        };
        let view = user_properties_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            const_list(USER_PROPERTIES_WIRE_FIELDS)
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","created_at":"2026-01-02T03:04:05.123456Z","updated_at":"2026-05-06T07:08:09Z","deleted_at":null,"filters":{"a":1},"display_filters":{},"display_properties":{"p":true},"rich_filters":{},"navigation_project_limit":10,"navigation_control_preference":"TABBED","created_by":null,"updated_by":null,"workspace":"22222222-2222-2222-2222-222222222222","user":"11111111-1111-1111-1111-111111111111"}"#,
        );
    }
}
