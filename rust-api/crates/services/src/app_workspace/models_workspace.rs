#![forbid(unsafe_code)]

//! Workspace core table models (D-24, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/workspace.py:19,114-320,348-369`
//! (`Workspace`, `WorkspaceMember`, `WorkspaceMemberInvite`,
//! `WorkspaceJoinRequest` + `Status`, `WorkspaceTheme`), adopting the
//! Django-owned schema column-for-column; migrations are not ported —
//! Django stays schema owner until switchover. Inherited audit columns
//! come from `BaseModel` (`db/models/base.py:17-18`) and `AuditModel`
//! (`TimeAuditModel` / `UserAuditModel` / `SoftDeleteModel` at
//! `db/mixins.py:16-82`).
//!
//! Column order in each `COLUMNS` const follows the F-W24-06 fixture
//! (`rust-api/fixtures/app_workspace/models/workspace_core.columns.json`,
//! recorded by PIDASHCONV-599): the 6 inherited audit columns first
//! (`id`, `created_at`, `updated_at`, `created_by_id`, `updated_by_id`,
//! `deleted_at`), then the model fields in declaration order. FK entries
//! use the Django attnames (`logo_asset_id`, `owner_id`, `workspace_id`,
//! `member_id`, `requester_id`, `responded_by_id`, `actor_id`). Every
//! application-level default below is Django-side (the live tables carry
//! no `column_default` in `information_schema`, as established for D-01);
//! Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All five tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:61-69`) and the default
//! manager filters `deleted_at IS NULL` (`objects =
//! SoftDeletionManager`, `mixins.py:56-58`; `all_objects` is the plain
//! unscoped manager). Every read built from these tables must add
//! `deleted_at IS NULL` (each module's `LIVE_SCOPE_WHERE`); the SQL
//! itself lives with the queries layer (PIDASHCONV-608/609). The partial
//! unique constraints stay as they are (tombstones are excluded by the
//! scope, so a deleted membership, invite, or theme name can be
//! re-created).
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * `Workspace.delete()` rewrites the slug with
//!   `save(update_fields=["slug"])` AFTER the soft delete
//!   (`workspace.py:170-174`), so `BaseModel.save` re-stamps
//!   `updated_by` on a deleted row. Kept: [`workspace::soft_deleted_slug`]
//!   is the pure half; the queries layer owns the stamp + save order.
//! * `WorkspaceMemberInvite.message` is `null=True` WITHOUT `blank=True`
//!   (`workspace.py:239`), while `WorkspaceJoinRequest.message` is
//!   `null=True, blank=True` (`:294`) — a form-validation asymmetry.
//!   Ported as written (both nullable in the structs).
//! * `RESTRICTED_WORKSPACE_SLUGS` (`pi_dash/utils/constants.py:5-70`)
//!   lists `config`, `mobile`, and `monitor` twice each. Reused verbatim
//!   (duplicates included) from the foundation list — never forked.
//!
//! # Referenced, never ported here
//!
//! `User` / `Profile` / `Account` / `APIToken` / `UserFavorite` are owned
//! by PIDASHCONV-607 (F-W24-08); `WorkspaceUserProperties` and friends by
//! PIDASHCONV-606 (F-W24-07); `FileAsset.asset_url` by the assets domain.
//! Only the FK target names, `on_delete` behaviors, and nullability Django
//! records on *this* side appear below. `Team`
//! (`workspace.py:323-347`) is dead (zero runtime references anywhere)
//! and is excluded here as in the fixture.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook.

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; same shape as the D-28
/// `app_modules::models::OnDelete` and the sibling domain ports).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `ROLE_CHOICES` (`db/models/workspace.py:19`): `(20, "Admin")`,
/// `(15, "Member")`, `(5, "Guest")`. Application-level only (Django
/// `choices=` is form-validation, no DB constraint); the column is a
/// plain smallint.
pub const ROLE_CHOICES: &[(i32, &str)] = &[(20, "Admin"), (15, "Member"), (5, "Guest")];

/// Role value for Admin (`workspace.py:19`).
pub const ROLE_ADMIN: i32 = 20;
/// Role value for Member (`workspace.py:19`).
pub const ROLE_MEMBER: i32 = 15;
/// Role value for Guest (`workspace.py:19`).
pub const ROLE_GUEST: i32 = 5;

/// Default `role` on members and invites (`workspace.py:205,241`).
/// Join requests default to [`ROLE_MEMBER`], not this.
pub const DEFAULT_ROLE: i32 = 5;

/// `workspaces` table (`db/models/workspace.py:119-182`).
pub mod workspace {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:181`).
    pub const TABLE: &str = "workspaces";
    /// Default ordering (`Meta.ordering`, `workspace.py:182`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:179`).
    pub const VERBOSE_NAME: &str = "Workspace";
    /// `verbose_name_plural` (`workspace.py:180`).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspaces";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace.py:122-139` in declaration order. FK columns use
    /// the Django attnames (`logo_asset_id`, `owner_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "name",
        "logo",
        "logo_asset_id",
        "owner_id",
        "slug",
        "organization_size",
        "timezone",
        "background_color",
    ];

    /// `name` bound (`workspace.py:122`, `max_length=80`).
    pub const NAME_MAX_LENGTH: usize = 80;
    /// `slug` bound (`workspace.py:136`, `max_length=48`).
    pub const SLUG_MAX_LENGTH: usize = 48;
    /// `organization_size` bound (`workspace.py:137`, `max_length=20`).
    pub const ORGANIZATION_SIZE_MAX_LENGTH: usize = 20;
    /// `timezone` bound (`workspace.py:138`, `max_length=255`).
    pub const TIMEZONE_MAX_LENGTH: usize = 255;
    /// `background_color` bound (`workspace.py:139`, `max_length=255`).
    pub const BACKGROUND_COLOR_MAX_LENGTH: usize = 255;

    /// `slug` carries a plain DB-level unique index (`workspace.py:136`,
    /// `unique=True, db_index=True`) — tombstones included, which is why
    /// [`soft_deleted_slug`] rewrites the slug on soft delete: the
    /// rewrite frees the plain slug for re-creation.
    pub const SLUG_UNIQUE: bool = true;

    /// Default `timezone` (`workspace.py:138`, `default="UTC"`).
    /// `choices=TIMEZONE_CHOICES` (`pytz.common_timezones`,
    /// `workspace.py:120`) is form-validation only — it creates no DB
    /// constraint — so the zone list is not enumerated here; the column
    /// accepts any value up to [`TIMEZONE_MAX_LENGTH`].
    pub const DEFAULT_TIMEZONE: &str = "UTC";

    /// `background_color` default (`workspace.py:139`,
    /// `default=get_random_color`, `pi_dash/utils/color.py:10-14`) is a
    /// per-row random value, so it has no const value: Rust inserts must
    /// generate `"#" + 6 × string.hexdigits` explicitly. The charset is
    /// `string.hexdigits` (`0123456789abcdefABCDEF`, mixed case —
    /// verified against CPython), e.g. `"#CA95b8"`.
    pub const BACKGROUND_COLOR_PREFIX: char = '#';
    /// Hex digits after [`BACKGROUND_COLOR_PREFIX`]
    /// (`color.py:14`, `k=6`).
    pub const BACKGROUND_COLOR_HEX_LEN: usize = 6;

    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `db/mixins.py:56-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `logo_asset` FK: `SET_NULL`, nullable (`workspace.py:124-130`).
    pub const LOGO_ASSET_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `logo_asset` reverse accessor (`workspace.py:127`).
    pub const LOGO_ASSET_RELATED_NAME: &str = "workspace_logo";
    /// `owner` FK: `CASCADE` (`workspace.py:131-135`).
    pub const OWNER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `owner` reverse accessor (`workspace.py:134`).
    pub const OWNER_RELATED_NAME: &str = "owner_workspace";
    /// `created_by` FK: `SET_NULL`, nullable (`db/mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`db/mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `slug_validator` (`workspace.py:114-116`): `True` when `value` is
    /// on the restricted list (`value in RESTRICTED_WORKSPACE_SLUGS`, a
    /// case-sensitive exact match), i.e. when Django raises
    /// `ValidationError("Slug is not valid")`. The list is reused from
    /// the foundation port — never forked. Model-level validation runs
    /// only on `full_clean()`, never on `save()`.
    pub fn slug_is_restricted(value: &str) -> bool {
        pidash_types::license::serializers_workspace::RESTRICTED_WORKSPACE_SLUGS.contains(&value)
    }

    /// Error message of `slug_validator` (`workspace.py:116`).
    pub const SLUG_VALIDATOR_ERROR: &str = "Slug is not valid";

    /// `logo_url` property (`workspace.py:145-154`) with the exact
    /// branching: the `logo_asset` arm is a None-check (a Django model
    /// instance is always truthy — `FileAsset` defines no `__bool__` or
    /// `__len__` — so its `asset_url` is returned verbatim, untested),
    /// while the `logo` arm is string truthiness (empty string falls
    /// through to `None`). The caller resolves `logo_asset_url` from the
    /// assets domain; `None` means "no `logo_asset` row".
    pub fn logo_url<'a>(logo_asset_url: Option<&'a str>, logo: Option<&'a str>) -> Option<&'a str> {
        if let Some(url) = logo_asset_url {
            return Some(url);
        }
        match logo {
            Some(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    }

    /// Pure half of `Workspace.delete(soft=True)`
    /// (`workspace.py:156-176`): after `super().delete()` stamps
    /// `deleted_at`, the slug becomes
    /// `f"{slug}__{int(deleted_at.timestamp())}"` and is saved with
    /// `save(update_fields=["slug"])` — which re-stamps `updated_by` on
    /// the deleted row (ported bug, kept). Hard deletes skip the slug
    /// logic entirely. `int()` truncates toward zero; `timestamp()`
    /// drops sub-second precision the same way.
    pub fn soft_deleted_slug(slug: &str, deleted_at: chrono::DateTime<chrono::Utc>) -> String {
        format!("{slug}__{}", deleted_at.timestamp())
    }

    /// `__str__` (`workspace.py:141-143`): returns the name.
    pub fn label(name: &str) -> &str {
        name
    }

    /// One workspace row. `logo` / `organization_size` are nullable
    /// (`workspace.py:123,137`); `timezone` / `background_color` carry
    /// Django-side defaults ([`DEFAULT_TIMEZONE`], per-row random).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Workspace {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub name: String,
        pub logo: Option<String>,
        pub logo_asset_id: Option<uuid::Uuid>,
        pub owner_id: uuid::Uuid,
        pub slug: String,
        pub organization_size: Option<String>,
        pub timezone: String,
        pub background_color: String,
    }

    impl std::fmt::Display for Workspace {
        /// `__str__` (`workspace.py:141-143`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", label(&self.name))
        }
    }
}

/// `workspace_members` table (`db/models/workspace.py:198-231`).
pub mod workspace_member {
    use super::{OnDelete, DEFAULT_ROLE};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:226`).
    pub const TABLE: &str = "workspace_members";
    /// Default ordering (`Meta.ordering`, `workspace.py:227`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:224`).
    pub const VERBOSE_NAME: &str = "Workspace Member";
    /// `verbose_name_plural` (`workspace.py:225`).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspace Members";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace.py:199-213` in declaration order. FK columns use
    /// the Django attnames (`workspace_id`, `member_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "member_id",
        "role",
        "company_role",
        "view_props",
        "default_props",
        "issue_props",
        "is_active",
        "getting_started_checklist",
        "tips",
        "explored_features",
    ];

    /// `role` Django-side default (`workspace.py:205`, `default=5`
    /// Guest).
    pub const DEFAULT_MEMBER_ROLE: i32 = DEFAULT_ROLE;
    /// `is_active` Django-side default (`workspace.py:210`).
    pub const DEFAULT_IS_ACTIVE: bool = true;
    /// `view_props` / `default_props` Django-side default
    /// (`workspace.py:207-208`, `default=get_default_props`,
    /// `:22-59`): the full nested dict, supplied explicitly on every
    /// Rust insert. `get_default_props` is a callable, so each row gets
    /// a fresh dict — parse a fresh value per insert, never share.
    pub const DEFAULT_VIEW_PROPS_JSON: &str = r#"{"filters":{"priority":null,"state":null,"state_group":null,"assignees":null,"created_by":null,"labels":null,"start_date":null,"target_date":null,"subscriber":null},"display_filters":{"group_by":null,"order_by":"-created_at","type":null,"sub_issue":true,"show_empty_groups":true,"layout":"list","calendar_date_range":""},"display_properties":{"assignee":true,"attachment_count":true,"created_on":true,"due_date":true,"estimate":true,"key":true,"labels":true,"link":true,"priority":true,"start_date":true,"state":true,"sub_issue_count":true,"updated_on":true}}"#;
    /// `issue_props` Django-side default (`workspace.py:209`,
    /// `default=get_issue_props`, `:110-111`).
    pub const DEFAULT_ISSUE_PROPS_JSON: &str =
        r#"{"subscribed":true,"assigned":true,"created":true,"all_issues":true}"#;
    /// `getting_started_checklist` / `tips` / `explored_features`
    /// Django-side defaults (`workspace.py:211-213`, `default=dict`):
    /// empty JSON object, supplied explicitly on every Rust insert.
    pub const EMPTY_DICT_JSON: &str = "{}";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `db/mixins.py:56-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `Meta.unique_together` (`workspace.py:216`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["workspace", "member", "deleted_at"];
    /// Partial unique constraint name (`workspace.py:221`).
    pub const UNIQUE_MEMBER_NAME: &str =
        "workspace_member_unique_workspace_member_when_deleted_at_null";
    /// Columns of [`UNIQUE_MEMBER_NAME`] (`workspace.py:219`), physical.
    pub const UNIQUE_MEMBER_COLUMNS: &[&str] = &["workspace_id", "member_id"];
    /// `WHERE` of [`UNIQUE_MEMBER_NAME`] (`workspace.py:220`,
    /// `deleted_at__isnull=True`): one live membership per
    /// (workspace, member); a soft-deleted row can be re-created.
    pub const UNIQUE_MEMBER_WHERE: &str = "deleted_at IS NULL";

    /// `workspace` FK: `CASCADE` (`workspace.py:199`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor (`workspace.py:199`).
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_member";
    /// `member` FK: `CASCADE` (`workspace.py:200-204`).
    pub const MEMBER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `member` reverse accessor (`workspace.py:203`).
    pub const MEMBER_RELATED_NAME: &str = "member_workspace";
    /// `created_by` FK: `SET_NULL`, nullable (`db/mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`db/mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `__str__` (`workspace.py:229-231`):
    /// `f"{self.member.email} <{self.workspace.name}>"`. Rust holds only
    /// the FKs, so the label takes the joined member email and workspace
    /// name; the joins are owned by the queries layer.
    pub fn label(member_email: &str, workspace_name: &str) -> String {
        format!("{member_email} <{workspace_name}>")
    }

    /// One workspace-membership row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceMember {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub member_id: uuid::Uuid,
        pub role: i32,
        pub company_role: Option<String>,
        pub view_props: serde_json::Value,
        pub default_props: serde_json::Value,
        pub issue_props: serde_json::Value,
        pub is_active: bool,
        pub getting_started_checklist: serde_json::Value,
        pub tips: serde_json::Value,
        pub explored_features: serde_json::Value,
    }
}

/// `workspace_member_invites` table (`db/models/workspace.py:234-258`).
pub mod workspace_member_invite {
    use super::{OnDelete, DEFAULT_ROLE};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:254`).
    pub const TABLE: &str = "workspace_member_invites";
    /// Default ordering (`Meta.ordering`, `workspace.py:255`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:252`).
    pub const VERBOSE_NAME: &str = "Workspace Member Invite";
    /// `verbose_name_plural` (`workspace.py:253`).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspace Member Invites";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace.py:235-241` in declaration order. FK columns use
    /// the Django attname (`workspace_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "email",
        "accepted",
        "token",
        "message",
        "responded_at",
        "role",
    ];

    /// `email` bound (`workspace.py:236`, `max_length=255`).
    pub const EMAIL_MAX_LENGTH: usize = 255;
    /// `token` bound (`workspace.py:238`, `max_length=255`).
    pub const TOKEN_MAX_LENGTH: usize = 255;
    /// `accepted` Django-side default (`workspace.py:237`).
    pub const DEFAULT_ACCEPTED: bool = false;
    /// `role` Django-side default (`workspace.py:241`, `default=5`
    /// Guest).
    pub const DEFAULT_INVITE_ROLE: i32 = DEFAULT_ROLE;
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `db/mixins.py:56-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `Meta.unique_together` (`workspace.py:244`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["email", "workspace", "deleted_at"];
    /// Partial unique constraint name (`workspace.py:249`).
    pub const UNIQUE_INVITE_NAME: &str =
        "workspace_member_invite_unique_email_workspace_when_deleted_at_null";
    /// Columns of [`UNIQUE_INVITE_NAME`] (`workspace.py:247`), physical.
    pub const UNIQUE_INVITE_COLUMNS: &[&str] = &["email", "workspace_id"];
    /// `WHERE` of [`UNIQUE_INVITE_NAME`] (`workspace.py:248`,
    /// `deleted_at__isnull=True`): one live invite per (email,
    /// workspace); a soft-deleted row can be re-created.
    pub const UNIQUE_INVITE_WHERE: &str = "deleted_at IS NULL";

    /// `workspace` FK: `CASCADE` (`workspace.py:235`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor (`workspace.py:235`).
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_member_invite";
    /// `created_by` FK: `SET_NULL`, nullable (`db/mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`db/mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `message` is `null=True` WITHOUT `blank=True`
    /// (`workspace.py:239`) — unlike `WorkspaceJoinRequest.message`
    /// (`:294`, `null=True, blank=True`). Form-validation asymmetry,
    /// ported as written; the column is nullable either way.
    pub const MESSAGE_ALLOWS_BLANK: bool = false;

    /// `__str__` (`workspace.py:257-258`):
    /// `f"{self.workspace.name} {self.email} {self.accepted}"`. Python
    /// renders bools as `True` / `False` (capitalized), not Rust's
    /// `true` / `false`. The workspace name is joined by the queries
    /// layer.
    pub fn label(workspace_name: &str, email: &str, accepted: bool) -> String {
        let accepted_py = if accepted { "True" } else { "False" };
        format!("{workspace_name} {email} {accepted_py}")
    }

    /// One invite row. `message` / `responded_at` are nullable
    /// (`workspace.py:239-240`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceMemberInvite {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub email: String,
        pub accepted: bool,
        pub token: String,
        pub message: Option<String>,
        pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
        pub role: i32,
    }

    // No `Display`: `__str__` needs the joined workspace name, so the
    // row alone cannot render it — see [`label`].
}

/// `workspace_join_requests` table (`db/models/workspace.py:261-320`).
pub mod workspace_join_request {
    use super::{OnDelete, ROLE_MEMBER};
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:316`).
    pub const TABLE: &str = "workspace_join_requests";
    /// Default ordering (`Meta.ordering`, `workspace.py:317`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:314`).
    pub const VERBOSE_NAME: &str = "Workspace Join Request";
    /// `verbose_name_plural` (`workspace.py:315`).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspace Join Requests";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace.py:281-304` in declaration order. FK columns use
    /// the Django attnames (`workspace_id`, `requester_id`,
    /// `responded_by_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "requester_id",
        "admin_email",
        "message",
        "role",
        "status",
        "responded_at",
        "responded_by_id",
    ];

    /// `admin_email` bound (`workspace.py:293`, `max_length=255`).
    pub const ADMIN_EMAIL_MAX_LENGTH: usize = 255;
    /// `status` bound (`workspace.py:296`, `max_length=20`).
    pub const STATUS_MAX_LENGTH: usize = 20;
    /// `role` Django-side default (`workspace.py:295`, `default=15`
    /// Member — NOT 5; join requests outrank plain invites).
    pub const DEFAULT_JOIN_REQUEST_ROLE: i32 = ROLE_MEMBER;
    /// `status` Django-side default (`workspace.py:296`,
    /// `default=Status.PENDING`).
    pub const DEFAULT_STATUS: &str = "PENDING";
    /// `status` choices in declaration order (`workspace.py:271-274`,
    /// mirrored by [`JoinRequestStatus::as_str`]).
    pub const STATUS_CHOICES: &[&str] = &["PENDING", "APPROVED", "DENIED"];
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `db/mixins.py:56-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// Partial unique constraint name (`workspace.py:311`). There is no
    /// legacy `unique_together` on this model.
    pub const UNIQUE_JOIN_REQUEST_NAME: &str =
        "workspace_join_request_unique_requester_workspace_when_pending";
    /// Columns of [`UNIQUE_JOIN_REQUEST_NAME`] (`workspace.py:309`),
    /// physical.
    pub const UNIQUE_JOIN_REQUEST_COLUMNS: &[&str] = &["requester_id", "workspace_id"];
    /// `WHERE` of [`UNIQUE_JOIN_REQUEST_NAME`] (`workspace.py:310`,
    /// `deleted_at__isnull=True, status="PENDING"`): one live PENDING
    /// request per (requester, workspace); decided or deleted rows free
    /// the pair. Note the `workspace` arm is nullable, and Postgres
    /// treats NULLs as distinct — two null-workspace pendings for one
    /// requester do NOT collide.
    pub const UNIQUE_JOIN_REQUEST_WHERE: &str = "deleted_at IS NULL AND status = 'PENDING'";

    /// `workspace` FK: `CASCADE` (`workspace.py:281-287`). Nullable ON
    /// PURPOSE (`:276-280`): when the typed admin email resolves to no
    /// workspace admin the request is still recorded with a null
    /// workspace, so the requester's onboarding "pending" state is
    /// identical either way — this avoids leaking which emails are
    /// workspace admins (enumeration).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor (`workspace.py:284`).
    pub const WORKSPACE_RELATED_NAME: &str = "workspace_join_request";
    /// `requester` FK: `CASCADE` (`workspace.py:288-292`).
    pub const REQUESTER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `requester` reverse accessor (`workspace.py:291`).
    pub const REQUESTER_RELATED_NAME: &str = "workspace_join_request";
    /// `responded_by` FK: `SET_NULL`, nullable (`workspace.py:298-304`).
    pub const RESPONDED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `responded_by` reverse accessor (`workspace.py:301`).
    pub const RESPONDED_BY_RELATED_NAME: &str = "responded_workspace_join_request";
    /// `created_by` FK: `SET_NULL`, nullable (`db/mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`db/mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `Status` choices (`workspace.py:271-274`, `models.TextChoices`).
    ///
    /// Django `choices=` is form-validation only — it creates no DB
    /// constraint — so the three wire values plus the `"PENDING"`
    /// default is the complete port. `CharField(max_length=20)` bound
    /// pinned by [`STATUS_MAX_LENGTH`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum JoinRequestStatus {
        /// `"PENDING"` — the field default (`workspace.py:296`).
        #[serde(rename = "PENDING")]
        Pending,
        /// `"APPROVED"`.
        #[serde(rename = "APPROVED")]
        Approved,
        /// `"DENIED"`.
        #[serde(rename = "DENIED")]
        Denied,
    }

    impl Default for JoinRequestStatus {
        /// `default=Status.PENDING` (`workspace.py:296`).
        fn default() -> Self {
            JoinRequestStatus::Pending
        }
    }

    impl JoinRequestStatus {
        /// Wire value of each choice (`workspace.py:272-274`).
        pub fn as_str(self) -> &'static str {
            match self {
                JoinRequestStatus::Pending => "PENDING",
                JoinRequestStatus::Approved => "APPROVED",
                JoinRequestStatus::Denied => "DENIED",
            }
        }
    }

    impl std::str::FromStr for JoinRequestStatus {
        type Err = ();

        /// Parse a wire value (`workspace.py:272-274`); anything else
        /// (including lowercase) is an error.
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "PENDING" => Ok(JoinRequestStatus::Pending),
                "APPROVED" => Ok(JoinRequestStatus::Approved),
                "DENIED" => Ok(JoinRequestStatus::Denied),
                _ => Err(()),
            }
        }
    }

    /// `__str__` (`workspace.py:319-320`):
    /// `f"{self.requester_id} -> {self.admin_email} ({self.status})"`.
    /// `requester_id` renders hyphenated lowercase in both Python
    /// `str(uuid)` and Rust `Uuid` display; `status` renders the wire
    /// value (`TextChoices.__str__` is `str(value)`).
    pub fn label(
        requester_id: &uuid::Uuid,
        admin_email: &str,
        status: JoinRequestStatus,
    ) -> String {
        format!("{requester_id} -> {admin_email} ({})", status.as_str())
    }

    /// One join-request row. `workspace_id` is nullable by design (see
    /// [`WORKSPACE_ON_DELETE`]); `message` / `responded_at` /
    /// `responded_by_id` are nullable (`workspace.py:294,297-304`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceJoinRequest {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: Option<uuid::Uuid>,
        pub requester_id: uuid::Uuid,
        pub admin_email: String,
        pub message: Option<String>,
        pub role: i32,
        pub status: JoinRequestStatus,
        pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
        pub responded_by_id: Option<uuid::Uuid>,
    }
}

/// `workspace_themes` table (`db/models/workspace.py:348-369`).
pub mod workspace_theme {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `workspace.py:368`).
    pub const TABLE: &str = "workspace_themes";
    /// Default ordering (`Meta.ordering`, `workspace.py:369`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`workspace.py:366`).
    pub const VERBOSE_NAME: &str = "Workspace Theme";
    /// `verbose_name_plural` (`workspace.py:367`).
    pub const VERBOSE_NAME_PLURAL: &str = "Workspace Themes";

    /// Physical columns in fixture order: 6 inherited audit columns,
    /// then `workspace.py:349-352` in declaration order. FK columns use
    /// the Django attnames (`workspace_id`, `actor_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "name",
        "actor_id",
        "colors",
    ];

    /// `name` bound (`workspace.py:350`, `max_length=300`).
    pub const NAME_MAX_LENGTH: usize = 300;
    /// `colors` Django-side default (`workspace.py:352`,
    /// `default=dict`): empty JSON object, supplied explicitly on every
    /// Rust insert.
    pub const EMPTY_COLORS_JSON: &str = "{}";
    /// Default-manager scope every read must apply
    /// (`SoftDeletionManager`, `db/mixins.py:56-58`).
    pub const LIVE_SCOPE_WHERE: &str = "deleted_at IS NULL";

    /// `Meta.unique_together` (`workspace.py:358`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["workspace", "name", "deleted_at"];
    /// Partial unique constraint name (`workspace.py:363`).
    pub const UNIQUE_THEME_NAME: &str =
        "workspace_theme_unique_workspace_name_when_deleted_at_null";
    /// Columns of [`UNIQUE_THEME_NAME`] (`workspace.py:361`), physical.
    pub const UNIQUE_THEME_COLUMNS: &[&str] = &["workspace_id", "name"];
    /// `WHERE` of [`UNIQUE_THEME_NAME`] (`workspace.py:362`,
    /// `deleted_at__isnull=True`): a name is unique among live rows of
    /// a workspace only, so a soft-deleted theme frees its name.
    pub const UNIQUE_THEME_WHERE: &str = "deleted_at IS NULL";

    /// `workspace` FK: `CASCADE` (`workspace.py:349`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` reverse accessor (`workspace.py:349`).
    pub const WORKSPACE_RELATED_NAME: &str = "themes";
    /// `actor` FK: `CASCADE` (`workspace.py:351`).
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `actor` reverse accessor (`workspace.py:351`).
    pub const ACTOR_RELATED_NAME: &str = "themes";
    /// `created_by` FK: `SET_NULL`, nullable (`db/mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`db/mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `__str__` (`workspace.py:354-355`):
    /// `str(self.name) + str(self.actor.email)` — concatenation with NO
    /// separator. The actor email is joined by the queries layer.
    pub fn label(name: &str, actor_email: &str) -> String {
        format!("{name}{actor_email}")
    }

    /// One theme row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceTheme {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub name: String,
        pub actor_id: uuid::Uuid,
        pub colors: serde_json::Value,
    }

    // No `Display`: `__str__` needs the joined actor email — see
    // [`label`].
}

#[cfg(test)]
mod tests {
    use super::workspace;
    use super::workspace_join_request::{self, JoinRequestStatus};
    use super::workspace_member;
    use super::workspace_member_invite;
    use super::workspace_theme;
    use super::{OnDelete, ROLE_CHOICES, ROLE_GUEST};

    fn fixture() -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/app_workspace/models/workspace_core.columns.json");
        let body = std::fs::read_to_string(&path).expect("read workspace_core.columns.json");
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    fn model<'a>(v: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        &v["models"][name]
    }

    /// Fixture `columns` entries are `{name, ddl}` objects using Django
    /// field names (`logo_asset`, not `logo_asset_id`).
    fn column_names(value: &serde_json::Value) -> Vec<String> {
        value["columns"]
            .as_array()
            .expect("columns array")
            .iter()
            .map(|c| c["name"].as_str().expect("name is str").to_string())
            .collect()
    }

    fn ddl_of<'a>(value: &'a serde_json::Value, name: &str) -> &'a str {
        value["columns"]
            .as_array()
            .expect("columns array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("fixture has column {name}"))["ddl"]
            .as_str()
            .expect("ddl is str")
    }

    /// FK Django field names that gain `_id` as physical columns.
    fn attname(field: &str) -> String {
        match field {
            "created_by" | "updated_by" | "logo_asset" | "owner" | "workspace" | "member"
            | "requester" | "responded_by" | "actor" => format!("{field}_id"),
            other => other.to_string(),
        }
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    #[test]
    fn inherited_prefix_matches_fixture() {
        let v = fixture();
        assert_eq!(
            column_names(&v["inherited"]),
            vec![
                "id",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "deleted_at"
            ]
        );
        assert!(v["inherited"]["from"]
            .as_str()
            .unwrap()
            .contains("db/mixins.py"));
        // Every module's COLUMNS starts with the same 6-column physical
        // prefix (FK attnames).
        let prefix = vec![
            "id",
            "created_at",
            "updated_at",
            "created_by_id",
            "updated_by_id",
            "deleted_at",
        ];
        for cols in [
            workspace::COLUMNS,
            workspace_member::COLUMNS,
            workspace_member_invite::COLUMNS,
            workspace_join_request::COLUMNS,
            workspace_theme::COLUMNS,
        ] {
            assert_eq!(owned(&cols[..6]), prefix);
        }
        // The fixture's inherited behaviors pin the save/delete rules
        // the queries layer must implement.
        let behaviors = v["inherited"]["behavior"].as_array().unwrap();
        assert!(behaviors
            .iter()
            .any(|b| b.as_str().unwrap().contains("auto-sets")));
        assert!(behaviors
            .iter()
            .any(|b| b.as_str().unwrap().contains("soft_delete_related_objects")));
    }

    #[test]
    fn workspace_columns_match_fixture() {
        let v = fixture();
        let m = model(&v, "Workspace");
        let own: Vec<String> = column_names(m).iter().map(|n| attname(n)).collect();
        assert_eq!(owned(&workspace::COLUMNS[6..]), own);
        assert_eq!(workspace::COLUMNS.len(), 14);
        assert_eq!(
            owned(&workspace::COLUMNS[6..]),
            vec![
                "name",
                "logo",
                "logo_asset_id",
                "owner_id",
                "slug",
                "organization_size",
                "timezone",
                "background_color",
            ]
        );
        let table: &str = workspace::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "workspaces");
        let ordering: &str = workspace::ORDERING;
        assert_eq!(ordering, m["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(
            workspace::VERBOSE_NAME,
            m["meta"]["verbose_name"].as_str().unwrap()
        );
        assert_eq!(
            workspace::VERBOSE_NAME_PLURAL,
            m["meta"]["verbose_name_plural"].as_str().unwrap()
        );
        // Workspace has neither partial constraints nor unique_together
        // — the slug frees via the delete() rewrite instead.
        assert!(m["meta"].get("constraints").is_none());
        assert!(m["meta"].get("unique_together").is_none());
        // Column semantics.
        assert!(ddl_of(m, "slug").contains("unique"));
        const { assert!(workspace::SLUG_UNIQUE) }
        assert_eq!(workspace::NAME_MAX_LENGTH, 80);
        assert!(ddl_of(m, "name").contains("max_length=80"));
        assert_eq!(workspace::SLUG_MAX_LENGTH, 48);
        assert_eq!(workspace::ORGANIZATION_SIZE_MAX_LENGTH, 20);
        assert_eq!(workspace::TIMEZONE_MAX_LENGTH, 255);
        assert_eq!(workspace::BACKGROUND_COLOR_MAX_LENGTH, 255);
        assert!(ddl_of(m, "timezone").contains("default='UTC'"));
        assert_eq!(workspace::DEFAULT_TIMEZONE, "UTC");
        assert!(ddl_of(m, "timezone").contains("TIMEZONE_CHOICES"));
        assert!(ddl_of(m, "background_color").contains("get_random_color"));
        assert!(ddl_of(m, "logo_asset").contains("SET_NULL"));
        assert!(ddl_of(m, "logo_asset").contains("workspace_logo"));
        assert_eq!(workspace::LOGO_ASSET_ON_DELETE, OnDelete::SetNull);
        assert_eq!(workspace::LOGO_ASSET_RELATED_NAME, "workspace_logo");
        assert!(ddl_of(m, "owner").contains("CASCADE"));
        assert_eq!(workspace::OWNER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(workspace::OWNER_RELATED_NAME, "owner_workspace");
        assert_eq!(workspace::LIVE_SCOPE_WHERE, "deleted_at IS NULL");
        // Behaviors + ported bug recorded in the fixture.
        let behaviors: Vec<&str> = m["behavior"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap())
            .collect();
        assert!(behaviors.iter().any(|b| b.contains("logo_url")));
        assert!(behaviors.iter().any(|b| b.contains("slug=f'{slug}")));
        assert!(behaviors.iter().any(|b| b.contains("slug_validator")));
        assert_eq!(m["bugs"].as_array().unwrap().len(), 1);
        assert!(m["bugs"][0]
            .as_str()
            .unwrap()
            .contains("re-stamps updated_by"));
    }

    #[test]
    fn member_columns_match_fixture() {
        let v = fixture();
        let m = model(&v, "WorkspaceMember");
        let own: Vec<String> = column_names(m).iter().map(|n| attname(n)).collect();
        assert_eq!(owned(&workspace_member::COLUMNS[6..]), own);
        assert_eq!(workspace_member::COLUMNS.len(), 17);
        let table: &str = workspace_member::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "workspace_members");
        let ordering: &str = workspace_member::ORDERING;
        assert_eq!(ordering, m["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(
            workspace_member::VERBOSE_NAME,
            m["meta"]["verbose_name"].as_str().unwrap()
        );
        assert_eq!(
            workspace_member::VERBOSE_NAME_PLURAL,
            m["meta"]["verbose_name_plural"].as_str().unwrap()
        );
        assert_eq!(workspace_member::DEFAULT_MEMBER_ROLE, 5);
        assert!(ddl_of(m, "role").contains("default=5"));
        const { assert!(workspace_member::DEFAULT_IS_ACTIVE) }
        assert!(ddl_of(m, "is_active").contains("default=True"));
        assert!(ddl_of(m, "view_props").contains("get_default_props"));
        assert!(ddl_of(m, "default_props").contains("get_default_props"));
        assert!(ddl_of(m, "issue_props").contains("get_issue_props"));
        assert!(ddl_of(m, "getting_started_checklist").contains("default=dict"));
        // Constraint + legacy unique_together.
        assert_eq!(m["meta"]["constraints"].as_array().unwrap().len(), 1);
        let constraint = m["meta"]["constraints"][0].as_str().unwrap();
        assert!(constraint.contains(workspace_member::UNIQUE_MEMBER_NAME));
        assert!(constraint.contains("deleted_at__isnull"));
        assert_eq!(
            owned(workspace_member::UNIQUE_MEMBER_COLUMNS),
            vec!["workspace_id", "member_id"]
        );
        assert_eq!(workspace_member::UNIQUE_MEMBER_WHERE, "deleted_at IS NULL");
        assert_eq!(
            owned(
                &m["meta"]["unique_together"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|u| u.as_str().unwrap())
                    .collect::<Vec<_>>()
            ),
            owned(workspace_member::UNIQUE_TOGETHER)
        );
        assert_eq!(workspace_member::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(workspace_member::WORKSPACE_RELATED_NAME, "workspace_member");
        assert_eq!(workspace_member::MEMBER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(workspace_member::MEMBER_RELATED_NAME, "member_workspace");
        assert_eq!(workspace_member::LIVE_SCOPE_WHERE, "deleted_at IS NULL");
        assert!(m["behavior"][0].as_str().unwrap().contains("ROLE_CHOICES"));
    }

    #[test]
    fn invite_columns_match_fixture() {
        let v = fixture();
        let m = model(&v, "WorkspaceMemberInvite");
        let own: Vec<String> = column_names(m).iter().map(|n| attname(n)).collect();
        assert_eq!(owned(&workspace_member_invite::COLUMNS[6..]), own);
        assert_eq!(workspace_member_invite::COLUMNS.len(), 13);
        let table: &str = workspace_member_invite::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "workspace_member_invites");
        let ordering: &str = workspace_member_invite::ORDERING;
        assert_eq!(ordering, m["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(
            workspace_member_invite::VERBOSE_NAME,
            m["meta"]["verbose_name"].as_str().unwrap()
        );
        assert_eq!(
            workspace_member_invite::VERBOSE_NAME_PLURAL,
            m["meta"]["verbose_name_plural"].as_str().unwrap()
        );
        assert_eq!(workspace_member_invite::EMAIL_MAX_LENGTH, 255);
        assert_eq!(workspace_member_invite::TOKEN_MAX_LENGTH, 255);
        const { assert!(!workspace_member_invite::DEFAULT_ACCEPTED) }
        assert_eq!(workspace_member_invite::DEFAULT_INVITE_ROLE, 5);
        // The form-validation asymmetry bug: invite message is null=True
        // WITHOUT blank=True.
        let message_ddl = ddl_of(m, "message");
        assert!(message_ddl.contains("null=True"), "{message_ddl}");
        assert!(!message_ddl.contains("blank"), "{message_ddl}");
        const { assert!(!workspace_member_invite::MESSAGE_ALLOWS_BLANK) }
        assert_eq!(m["bugs"].as_array().unwrap().len(), 1);
        assert!(m["bugs"][0]
            .as_str()
            .unwrap()
            .contains("WITHOUT blank=True"));
        assert_eq!(m["meta"]["constraints"].as_array().unwrap().len(), 1);
        let constraint = m["meta"]["constraints"][0].as_str().unwrap();
        assert!(constraint.contains(workspace_member_invite::UNIQUE_INVITE_NAME));
        assert_eq!(
            owned(workspace_member_invite::UNIQUE_INVITE_COLUMNS),
            vec!["email", "workspace_id"]
        );
        assert_eq!(
            workspace_member_invite::UNIQUE_INVITE_WHERE,
            "deleted_at IS NULL"
        );
        assert_eq!(
            owned(workspace_member_invite::UNIQUE_TOGETHER),
            vec!["email", "workspace", "deleted_at"]
        );
        assert_eq!(
            workspace_member_invite::WORKSPACE_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            workspace_member_invite::WORKSPACE_RELATED_NAME,
            "workspace_member_invite"
        );
        assert_eq!(
            workspace_member_invite::LIVE_SCOPE_WHERE,
            "deleted_at IS NULL"
        );
    }

    #[test]
    fn join_request_columns_match_fixture() {
        let v = fixture();
        let m = model(&v, "WorkspaceJoinRequest");
        let own: Vec<String> = column_names(m).iter().map(|n| attname(n)).collect();
        assert_eq!(owned(&workspace_join_request::COLUMNS[6..]), own);
        assert_eq!(workspace_join_request::COLUMNS.len(), 14);
        let table: &str = workspace_join_request::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "workspace_join_requests");
        let ordering: &str = workspace_join_request::ORDERING;
        assert_eq!(ordering, m["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(
            workspace_join_request::VERBOSE_NAME,
            m["meta"]["verbose_name"].as_str().unwrap()
        );
        assert_eq!(
            workspace_join_request::VERBOSE_NAME_PLURAL,
            m["meta"]["verbose_name_plural"].as_str().unwrap()
        );
        assert_eq!(workspace_join_request::ADMIN_EMAIL_MAX_LENGTH, 255);
        assert_eq!(workspace_join_request::STATUS_MAX_LENGTH, 20);
        // Join requests default to Member (15), NOT Guest (5).
        assert!(
            ddl_of(m, "role").contains("default=15"),
            "{}",
            ddl_of(m, "role")
        );
        assert_eq!(workspace_join_request::DEFAULT_JOIN_REQUEST_ROLE, 15);
        assert!(ddl_of(m, "status").contains("default=PENDING"));
        assert_eq!(workspace_join_request::DEFAULT_STATUS, "PENDING");
        // Nullable workspace is the deliberate anti-enumeration design.
        assert!(ddl_of(m, "workspace").contains("null=True"));
        assert!(m["behavior"][1]
            .as_str()
            .unwrap()
            .contains("anti-enumeration"));
        // Join-request message HAS blank=True (the invite asymmetry arm).
        assert!(ddl_of(m, "message").contains("blank=True"));
        // Partial unique on (requester, workspace) among PENDING rows;
        // no legacy unique_together on this model.
        assert!(m["meta"].get("unique_together").is_none());
        assert_eq!(m["meta"]["constraints"].as_array().unwrap().len(), 1);
        let constraint = m["meta"]["constraints"][0].as_str().unwrap();
        assert!(constraint.contains(workspace_join_request::UNIQUE_JOIN_REQUEST_NAME));
        assert!(constraint.contains("status='PENDING'"), "{constraint}");
        assert_eq!(
            owned(workspace_join_request::UNIQUE_JOIN_REQUEST_COLUMNS),
            vec!["requester_id", "workspace_id"]
        );
        assert_eq!(
            workspace_join_request::UNIQUE_JOIN_REQUEST_WHERE,
            "deleted_at IS NULL AND status = 'PENDING'"
        );
        assert_eq!(
            workspace_join_request::WORKSPACE_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            workspace_join_request::WORKSPACE_RELATED_NAME,
            "workspace_join_request"
        );
        assert_eq!(
            workspace_join_request::REQUESTER_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(
            workspace_join_request::REQUESTER_RELATED_NAME,
            "workspace_join_request"
        );
        assert_eq!(
            workspace_join_request::RESPONDED_BY_ON_DELETE,
            OnDelete::SetNull
        );
        assert_eq!(
            workspace_join_request::RESPONDED_BY_RELATED_NAME,
            "responded_workspace_join_request"
        );
        assert_eq!(
            workspace_join_request::LIVE_SCOPE_WHERE,
            "deleted_at IS NULL"
        );
    }

    #[test]
    fn theme_columns_match_fixture() {
        let v = fixture();
        let m = model(&v, "WorkspaceTheme");
        let own: Vec<String> = column_names(m).iter().map(|n| attname(n)).collect();
        assert_eq!(owned(&workspace_theme::COLUMNS[6..]), own);
        assert_eq!(workspace_theme::COLUMNS.len(), 10);
        let table: &str = workspace_theme::TABLE;
        assert_eq!(table, m["meta"]["db_table"].as_str().unwrap());
        assert_eq!(table, "workspace_themes");
        let ordering: &str = workspace_theme::ORDERING;
        assert_eq!(ordering, m["meta"]["ordering"][0].as_str().unwrap());
        assert_eq!(ordering, "-created_at");
        assert_eq!(
            workspace_theme::VERBOSE_NAME,
            m["meta"]["verbose_name"].as_str().unwrap()
        );
        assert_eq!(
            workspace_theme::VERBOSE_NAME_PLURAL,
            m["meta"]["verbose_name_plural"].as_str().unwrap()
        );
        assert_eq!(workspace_theme::NAME_MAX_LENGTH, 300);
        assert!(ddl_of(m, "name").contains("max_length=300"));
        assert!(ddl_of(m, "colors").contains("default=dict"));
        assert_eq!(m["meta"]["constraints"].as_array().unwrap().len(), 1);
        let constraint = m["meta"]["constraints"][0].as_str().unwrap();
        assert!(constraint.contains(workspace_theme::UNIQUE_THEME_NAME));
        assert_eq!(
            owned(workspace_theme::UNIQUE_THEME_COLUMNS),
            vec!["workspace_id", "name"]
        );
        assert_eq!(workspace_theme::UNIQUE_THEME_WHERE, "deleted_at IS NULL");
        assert_eq!(
            owned(workspace_theme::UNIQUE_TOGETHER),
            vec!["workspace", "name", "deleted_at"]
        );
        assert_eq!(workspace_theme::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(workspace_theme::WORKSPACE_RELATED_NAME, "themes");
        assert_eq!(workspace_theme::ACTOR_ON_DELETE, OnDelete::Cascade);
        assert_eq!(workspace_theme::ACTOR_RELATED_NAME, "themes");
        assert_eq!(workspace_theme::LIVE_SCOPE_WHERE, "deleted_at IS NULL");
    }

    #[test]
    fn role_choices_and_defaults() {
        assert_eq!(ROLE_CHOICES, &[(20, "Admin"), (15, "Member"), (5, "Guest")]);
        assert_eq!(ROLE_GUEST, 5);
        // Members and invites default to Guest; join requests to Member.
        assert_eq!(workspace_member::DEFAULT_MEMBER_ROLE, ROLE_GUEST);
        assert_eq!(workspace_member_invite::DEFAULT_INVITE_ROLE, ROLE_GUEST);
        assert_eq!(workspace_join_request::DEFAULT_JOIN_REQUEST_ROLE, 15);
        assert_ne!(
            workspace_join_request::DEFAULT_JOIN_REQUEST_ROLE,
            workspace_member::DEFAULT_MEMBER_ROLE
        );
    }

    #[test]
    fn logo_url_branching() {
        // Asset arm wins whenever a logo_asset row exists — its URL is
        // returned verbatim, even an empty one (None-check, not
        // string truthiness).
        assert_eq!(
            workspace::logo_url(Some("/api/assets/v2/static/1/"), Some("https://x/logo.png")),
            Some("/api/assets/v2/static/1/")
        );
        assert_eq!(
            workspace::logo_url(Some(""), Some("https://x/logo.png")),
            Some("")
        );
        assert_eq!(workspace::logo_url(Some("/a/"), None), Some("/a/"));
        // Without an asset row, a non-empty logo wins; empty and null
        // both fall through to None (Python `if self.logo:`).
        assert_eq!(
            workspace::logo_url(None, Some("https://x/logo.png")),
            Some("https://x/logo.png")
        );
        assert_eq!(workspace::logo_url(None, Some("")), None);
        assert_eq!(workspace::logo_url(None, None), None);
    }

    #[test]
    fn soft_deleted_slug_appends_epoch() {
        // `f"{slug}__{int(deleted_at.timestamp())}"`
        // (`workspace.py:173`).
        let at = chrono::DateTime::from_timestamp(1_767_000_000, 0).unwrap();
        assert_eq!(workspace::soft_deleted_slug("acme", at), "acme__1767000000");
        // Sub-second precision is dropped, like Python `int(ts)`.
        let sub = chrono::DateTime::from_timestamp(100, 999_000_000).unwrap();
        assert_eq!(workspace::soft_deleted_slug("acme", sub), "acme__100");
        // Hard deletes skip the slug logic — no rewrite helper applies.
    }

    #[test]
    fn slug_validator_matrix() {
        // Restricted slugs raise (`workspace.py:114-116`); the check is
        // a case-sensitive exact match against the foundation list.
        assert!(workspace::slug_is_restricted("api"));
        assert!(workspace::slug_is_restricted("admin"));
        assert!(workspace::slug_is_restricted("config"));
        assert!(workspace::slug_is_restricted("404"));
        assert!(workspace::slug_is_restricted("monitor"));
        assert!(!workspace::slug_is_restricted("acme"));
        assert!(!workspace::slug_is_restricted("API"));
        assert!(!workspace::slug_is_restricted("apiary"));
        assert!(!workspace::slug_is_restricted(""));
        assert_eq!(workspace::SLUG_VALIDATOR_ERROR, "Slug is not valid");
    }

    #[test]
    fn join_request_status_roundtrip() {
        assert_eq!(
            owned(workspace_join_request::STATUS_CHOICES),
            vec!["PENDING", "APPROVED", "DENIED"]
        );
        assert_eq!(JoinRequestStatus::default(), JoinRequestStatus::Pending);
        assert_eq!(JoinRequestStatus::Pending.as_str(), "PENDING");
        assert_eq!(JoinRequestStatus::Approved.as_str(), "APPROVED");
        assert_eq!(JoinRequestStatus::Denied.as_str(), "DENIED");
        use std::str::FromStr;
        assert_eq!(
            JoinRequestStatus::from_str("PENDING"),
            Ok(JoinRequestStatus::Pending)
        );
        assert_eq!(
            JoinRequestStatus::from_str("APPROVED"),
            Ok(JoinRequestStatus::Approved)
        );
        assert_eq!(
            JoinRequestStatus::from_str("DENIED"),
            Ok(JoinRequestStatus::Denied)
        );
        assert_eq!(JoinRequestStatus::from_str("pending"), Err(()));
        assert_eq!(JoinRequestStatus::from_str(""), Err(()));
        // Serde renders the wire values, not the variant names.
        assert_eq!(
            serde_json::to_string(&JoinRequestStatus::Approved).unwrap(),
            "\"APPROVED\""
        );
        assert_eq!(
            serde_json::from_str::<JoinRequestStatus>("\"DENIED\"").unwrap(),
            JoinRequestStatus::Denied
        );
    }

    #[test]
    fn member_json_defaults_match_python() {
        // `get_default_props` (`workspace.py:22-59`), exact structure.
        let props: serde_json::Value =
            serde_json::from_str(workspace_member::DEFAULT_VIEW_PROPS_JSON).unwrap();
        assert_eq!(
            props,
            serde_json::json!({
                "filters": {
                    "priority": null, "state": null, "state_group": null,
                    "assignees": null, "created_by": null, "labels": null,
                    "start_date": null, "target_date": null, "subscriber": null
                },
                "display_filters": {
                    "group_by": null, "order_by": "-created_at", "type": null,
                    "sub_issue": true, "show_empty_groups": true,
                    "layout": "list", "calendar_date_range": ""
                },
                "display_properties": {
                    "assignee": true, "attachment_count": true, "created_on": true,
                    "due_date": true, "estimate": true, "key": true,
                    "labels": true, "link": true, "priority": true,
                    "start_date": true, "state": true, "sub_issue_count": true,
                    "updated_on": true
                }
            })
        );
        // `get_issue_props` (`workspace.py:110-111`).
        let issue: serde_json::Value =
            serde_json::from_str(workspace_member::DEFAULT_ISSUE_PROPS_JSON).unwrap();
        assert_eq!(
            issue,
            serde_json::json!({
                "subscribed": true, "assigned": true,
                "created": true, "all_issues": true
            })
        );
        // `default=dict` arms.
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(workspace_member::EMPTY_DICT_JSON).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(workspace_theme::EMPTY_COLORS_JSON).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn str_labels_match_python() {
        assert_eq!(workspace::label("Acme"), "Acme");
        assert_eq!(
            workspace::Workspace {
                id: uuid::Uuid::nil(),
                created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
                updated_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
                created_by_id: None,
                updated_by_id: None,
                deleted_at: None,
                name: "Acme".to_string(),
                logo: None,
                logo_asset_id: None,
                owner_id: uuid::Uuid::nil(),
                slug: "acme".to_string(),
                organization_size: None,
                timezone: "UTC".to_string(),
                background_color: "#ffffff".to_string(),
            }
            .to_string(),
            "Acme"
        );
        assert_eq!(
            workspace_member::label("a@example.com", "Acme"),
            "a@example.com <Acme>"
        );
        // Python bool spelling is capitalized.
        assert_eq!(
            workspace_member_invite::label("Acme", "a@example.com", true),
            "Acme a@example.com True"
        );
        assert_eq!(
            workspace_member_invite::label("Acme", "a@example.com", false),
            "Acme a@example.com False"
        );
        let requester = uuid::Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        assert_eq!(
            workspace_join_request::label(
                &requester,
                "admin@example.com",
                JoinRequestStatus::Pending
            ),
            "12345678-1234-5678-1234-567812345678 -> admin@example.com (PENDING)"
        );
        // Theme concatenates with NO separator.
        assert_eq!(
            workspace_theme::label("Ocean", "a@example.com"),
            "Oceana@example.com"
        );
    }

    #[test]
    fn background_color_default_shape() {
        // `get_random_color` (`color.py:10-14`): `"#" + 6 ×
        // string.hexdigits` (mixed case, verified against CPython).
        assert_eq!(workspace::BACKGROUND_COLOR_PREFIX, '#');
        assert_eq!(workspace::BACKGROUND_COLOR_HEX_LEN, 6);
        let hexdigits = "0123456789abcdefABCDEF";
        assert!(hexdigits.contains('a'));
        assert!(hexdigits.contains('A'));
        for sample in ["#CA95b8", "#000000", "#FFFFFF", "#abcdef"] {
            let mut chars = sample.chars();
            assert_eq!(chars.next(), Some(workspace::BACKGROUND_COLOR_PREFIX));
            let rest: String = chars.collect();
            assert_eq!(rest.len(), workspace::BACKGROUND_COLOR_HEX_LEN);
            assert!(rest.chars().all(|c| hexdigits.contains(c)));
        }
    }

    #[test]
    fn team_excluded_and_source_noted() {
        let v = fixture();
        // Team is dead — excluded from the port and the fixture alike.
        assert_eq!(
            v["excluded"]["Team"]["reason"].as_str().unwrap(),
            "dead model"
        );
        assert!(v["_note"].as_str().unwrap().contains("Team"));
        assert!(v["source"]
            .as_str()
            .unwrap()
            .contains("db/models/workspace.py"));
        assert!(v["source"].as_str().unwrap().contains("db/mixins.py"));
    }
}
