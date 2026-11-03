//! User serializers: `User*` read shapes, name guards, me-settings (D-24).
//!
//! Port of `apps/api/pi_dash/app/serializers/user.py:15-171`:
//!
//! * `UserSerializer` (`:15-61`, `validate_first_name` /
//!   `validate_last_name` URL guards, dynamic fields minus `password`,
//!   24-key read-only list)
//! * `UserMeSerializer` (`:63-88`, exact 19-key list including the
//!   duplicated `is_email_verified`)
//! * `UserMeSettingsSerializer` (`:90-139`, `get_workspace` last-workspace
//!   vs fallback branches)
//! * `UserLiteSerializer` (`:141-154`, 7 keys)
//! * `UserAdminLiteSerializer` (`:156-171`, 9 keys)
//!
//! Pure kernels in the `ser_workspace` style: each `to_representation`
//! takes a row borrowed from the caller and returns a `serde::Serialize`
//! view whose fields are the live DRF wire fields in output order. UUID
//! primary keys render as strings (`PrimaryKeyRelatedField`, read-only;
//! `base.py:8-9`); nullable FKs render `null` when unset. Datetimes cross
//! this boundary already rendered as DRF iso-8601 strings (formatting
//! owns to the DB edge), so rendering here is a byte-exact passthrough.
//!
//! Single-owner notes (never fork a helper):
//!
//! * [`contains_url`] is reused from [`super::ser_workspace`] (the port
//!   of `pi_dash/utils/url.py:26-53`); the name guards delegate to it.
//! * `avatar_url` / `cover_image_url` arrive already resolved (the models
//!   layer owns the property port, `db/models/user.py:142-162`); this
//!   module only places the value (`None` renders `null`).
//! * The nested `member` slots of `ser_workspace`'s member views are
//!   generic over these lite renderers; handlers compose the two.
//! * `get_workspace` queries (invite count `:99`, profile lookup `:102`,
//!   membership exists-checks `:104-115`, earliest-created fallback
//!   `:127-131`) are owned by the handler/query layer; this module owns
//!   the branch predicate input (`Some` = last workspace set and an
//!   active member) and both output shapes.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-onboarded (`user.py:59-60`): `get_is_onboarded` is defined
//!   *inside* `class Meta`, so it is dead code — no `is_onboarded` field
//!   is declared and nothing renders. Ported by omitting it.
//! * BUG-dup (`user.py:79,83,87`): `UserMeSerializer.fields` lists
//!   `is_email_verified` twice and `read_only_fields = fields` keeps the
//!   duplicate. DRF builds the field map as a dict, so the wire keeps one
//!   copy at the first position (18 keys, verified live). The [`USER_ME_FIELDS`]
//!   const carries all 19 entries verbatim; the view renders 18.
//! * BUG-asymmetry (`user.py:117-138`): the `get_workspace` branches
//!   return different shapes — the last-workspace branch has
//!   `last_workspace_name` / `last_workspace_logo` plus `fallback_*`
//!   duplicating `last_*`, while the fallback branch has no name/logo
//!   keys and `last_workspace_* = null`. Ported exactly.
//! * BUG-dynamic (`user.py:29`): `UserSerializer.fields` is the dynamic
//!   rule "every `User._meta.fields` name except `password`" (concrete
//!   and forward-FK fields only — M2M `groups` / `user_permissions` are
//!   not in `_meta.fields`). Ported as the exclusion rule, documented on
//!   [`USER_WIRE_FIELDS`], not a frozen list.
//! * BUG-profile (`user.py:99,102`): invites are counted on every call
//!   and `Profile.objects.get(user=obj)` raises when the profile is
//!   missing. Handler/query-layer semantics; noted, not emulated here.
//!
//! Out of scope (documented, not ported): `ChangePasswordSerializer` /
//! `ResetPasswordSerializer` (`user.py:173-199`) — re-exported only, no
//! view usage in any D-24 route; the DRF `{"field": ["message"]}` error
//! envelope — the handler arm (this kernel returns the message via
//! `Display`).

use super::ser_workspace::contains_url;
use serde::Serialize;
use thiserror::Error as ThisError;

/// `UserSerializer` wire keys in output order (`user.py:15-61`):
/// `Meta.fields` verbatim — every `User._meta.fields` name except
/// `password` (`user.py:29`), verified against the live serializer.
/// (Declared `id` keeps its `Meta` position; M2M-only names never
/// appear because `_meta.fields` excludes them. See BUG-dynamic.)
pub const USER_WIRE_FIELDS: [&str; 39] = [
    "last_login",
    "id",
    "username",
    "mobile_number",
    "email",
    "display_name",
    "first_name",
    "last_name",
    "avatar",
    "avatar_asset",
    "cover_image",
    "cover_image_asset",
    "date_joined",
    "created_at",
    "updated_at",
    "last_location",
    "created_location",
    "is_superuser",
    "is_managed",
    "is_password_expired",
    "is_active",
    "is_staff",
    "is_email_verified",
    "is_password_autoset",
    "is_password_reset_required",
    "token",
    "last_active",
    "last_login_time",
    "last_logout_time",
    "last_login_ip",
    "last_logout_ip",
    "last_login_medium",
    "last_login_uagent",
    "token_updated_at",
    "is_bot",
    "bot_type",
    "user_timezone",
    "is_email_valid",
    "masked_at",
];

/// `UserSerializer.Meta.read_only_fields` (`user.py:31-56`), 24 keys.
pub const USER_READ_ONLY_FIELDS: [&str; 24] = [
    "id",
    "username",
    "mobile_number",
    "email",
    "token",
    "created_at",
    "updated_at",
    "is_superuser",
    "is_staff",
    "is_managed",
    "last_active",
    "last_login_time",
    "last_logout_time",
    "last_login_ip",
    "last_logout_ip",
    "last_login_uagent",
    "last_location",
    "last_login_medium",
    "created_location",
    "is_bot",
    "is_password_autoset",
    "is_email_verified",
    "is_active",
    "token_updated_at",
];

/// `UserMeSerializer.Meta.fields` (`user.py:66-86`) verbatim: 19 entries
/// *including* the duplicated `is_email_verified` (BUG-dup). The
/// rendered wire order is [`USER_ME_WIRE_FIELDS`].
pub const USER_ME_FIELDS: [&str; 19] = [
    "id",
    "avatar",
    "cover_image",
    "avatar_url",
    "cover_image_url",
    "date_joined",
    "display_name",
    "email",
    "first_name",
    "last_name",
    "is_active",
    "is_bot",
    "is_email_verified",
    "user_timezone",
    "username",
    "is_password_autoset",
    "is_email_verified",
    "last_login_medium",
    "last_login_time",
];

/// `UserMeSerializer.Meta.read_only_fields` (`user.py:87`): `= fields`,
/// duplicate included.
pub const USER_ME_READ_ONLY_FIELDS: [&str; 19] = USER_ME_FIELDS;

/// `UserMeSerializer` wire keys in output order (verified against the
/// live serializer): the duplicated `is_email_verified` collapses to one
/// copy at its first position, so 18 keys render (BUG-dup).
pub const USER_ME_WIRE_FIELDS: [&str; 18] = [
    "id",
    "avatar",
    "cover_image",
    "avatar_url",
    "cover_image_url",
    "date_joined",
    "display_name",
    "email",
    "first_name",
    "last_name",
    "is_active",
    "is_bot",
    "is_email_verified",
    "user_timezone",
    "username",
    "is_password_autoset",
    "last_login_medium",
    "last_login_time",
];

/// `UserMeSettingsSerializer.Meta.fields` (`user.py:95`), wire order.
pub const USER_ME_SETTINGS_FIELDS: [&str; 3] = ["id", "email", "workspace"];

/// `UserMeSettingsSerializer.Meta.read_only_fields` (`user.py:96`):
/// `= fields`.
pub const USER_ME_SETTINGS_READ_ONLY_FIELDS: [&str; 3] = USER_ME_SETTINGS_FIELDS;

/// `get_workspace` last-workspace branch keys (`user.py:117-125`), in
/// output order (BUG-asymmetry).
pub const ME_SETTINGS_WORKSPACE_FULL_KEYS: [&str; 7] = [
    "last_workspace_id",
    "last_workspace_slug",
    "last_workspace_name",
    "last_workspace_logo",
    "fallback_workspace_id",
    "fallback_workspace_slug",
    "invites",
];

/// `get_workspace` fallback branch keys (`user.py:132-138`), in output
/// order (BUG-asymmetry).
pub const ME_SETTINGS_WORKSPACE_FALLBACK_KEYS: [&str; 5] = [
    "last_workspace_id",
    "last_workspace_slug",
    "fallback_workspace_id",
    "fallback_workspace_slug",
    "invites",
];

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

/// `UserAdminLiteSerializer.Meta.fields` (`user.py:159-169`), wire order.
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

/// Shared read-only list of both lite serializers (`user.py:153` and
/// `:170` are identical).
pub const USER_LITE_READ_ONLY_FIELDS: [&str; 2] = ["id", "is_bot"];

/// `UserSerializer.validate_first_name` failure (`user.py:16-19`).
///
/// `Display` renders the exact DRF `ValidationError` message, which DRF
/// envelopes as `{"first_name": ["<message>"]}` in `serializer.errors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ThisError)]
pub enum FirstNameError {
    /// The first name contains a URL (`user.py:18`).
    #[error("First name cannot contain a URL.")]
    ContainsUrl,
}

/// Port of `UserSerializer.validate_first_name` (`user.py:16-19`).
pub fn validate_first_name(value: &str) -> Result<&str, FirstNameError> {
    if contains_url(value) {
        return Err(FirstNameError::ContainsUrl);
    }
    Ok(value)
}

/// `UserSerializer.validate_last_name` failure (`user.py:21-24`).
///
/// `Display` renders the exact DRF `ValidationError` message, which DRF
/// envelopes as `{"last_name": ["<message>"]}` in `serializer.errors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ThisError)]
pub enum LastNameError {
    /// The last name contains a URL (`user.py:23`).
    #[error("Last name cannot contain a URL.")]
    ContainsUrl,
}

/// Port of `UserSerializer.validate_last_name` (`user.py:21-24`).
pub fn validate_last_name(value: &str) -> Result<&str, LastNameError> {
    if contains_url(value) {
        return Err(LastNameError::ContainsUrl);
    }
    Ok(value)
}

/// A `User` row for [`user_to_representation`]
/// (`db/models/user.py:56-126` plus `last_login` from
/// `AbstractBaseUser`): nullable columns (`CharField(null=True)`,
/// nullable FKs rendering as UUID strings, nullable datetimes) are
/// `Option` (`None` renders `null`); blank-only text renders `""`;
/// datetimes are pre-rendered DRF strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRow<'a> {
    pub last_login: Option<&'a str>,
    pub id: &'a str,
    pub username: &'a str,
    pub mobile_number: Option<&'a str>,
    pub email: Option<&'a str>,
    pub display_name: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_asset: Option<&'a str>,
    pub cover_image: Option<&'a str>,
    pub cover_image_asset: Option<&'a str>,
    pub date_joined: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub last_location: &'a str,
    pub created_location: &'a str,
    pub is_superuser: bool,
    pub is_managed: bool,
    pub is_password_expired: bool,
    pub is_active: bool,
    pub is_staff: bool,
    pub is_email_verified: bool,
    pub is_password_autoset: bool,
    pub is_password_reset_required: bool,
    pub token: &'a str,
    pub last_active: Option<&'a str>,
    pub last_login_time: Option<&'a str>,
    pub last_logout_time: Option<&'a str>,
    pub last_login_ip: &'a str,
    pub last_logout_ip: &'a str,
    pub last_login_medium: &'a str,
    pub last_login_uagent: &'a str,
    pub token_updated_at: Option<&'a str>,
    pub is_bot: bool,
    pub bot_type: Option<&'a str>,
    pub user_timezone: &'a str,
    pub is_email_valid: bool,
    pub masked_at: Option<&'a str>,
}

/// `UserSerializer.to_representation` output (`user.py:15-61`), in wire
/// order. No `password` key (BUG-dynamic) and no `is_onboarded` key
/// (BUG-onboarded).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserView<'a> {
    pub last_login: Option<&'a str>,
    pub id: &'a str,
    pub username: &'a str,
    pub mobile_number: Option<&'a str>,
    pub email: Option<&'a str>,
    pub display_name: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_asset: Option<&'a str>,
    pub cover_image: Option<&'a str>,
    pub cover_image_asset: Option<&'a str>,
    pub date_joined: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub last_location: &'a str,
    pub created_location: &'a str,
    pub is_superuser: bool,
    pub is_managed: bool,
    pub is_password_expired: bool,
    pub is_active: bool,
    pub is_staff: bool,
    pub is_email_verified: bool,
    pub is_password_autoset: bool,
    pub is_password_reset_required: bool,
    pub token: &'a str,
    pub last_active: Option<&'a str>,
    pub last_login_time: Option<&'a str>,
    pub last_logout_time: Option<&'a str>,
    pub last_login_ip: &'a str,
    pub last_logout_ip: &'a str,
    pub last_login_medium: &'a str,
    pub last_login_uagent: &'a str,
    pub token_updated_at: Option<&'a str>,
    pub is_bot: bool,
    pub bot_type: Option<&'a str>,
    pub user_timezone: &'a str,
    pub is_email_valid: bool,
    pub masked_at: Option<&'a str>,
}

/// Port of `UserSerializer` (`user.py:15-61`) read shape.
pub fn user_to_representation<'a>(row: &'a UserRow<'a>) -> UserView<'a> {
    UserView {
        last_login: row.last_login,
        id: row.id,
        username: row.username,
        mobile_number: row.mobile_number,
        email: row.email,
        display_name: row.display_name,
        first_name: row.first_name,
        last_name: row.last_name,
        avatar: row.avatar,
        avatar_asset: row.avatar_asset,
        cover_image: row.cover_image,
        cover_image_asset: row.cover_image_asset,
        date_joined: row.date_joined,
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_location: row.last_location,
        created_location: row.created_location,
        is_superuser: row.is_superuser,
        is_managed: row.is_managed,
        is_password_expired: row.is_password_expired,
        is_active: row.is_active,
        is_staff: row.is_staff,
        is_email_verified: row.is_email_verified,
        is_password_autoset: row.is_password_autoset,
        is_password_reset_required: row.is_password_reset_required,
        token: row.token,
        last_active: row.last_active,
        last_login_time: row.last_login_time,
        last_logout_time: row.last_logout_time,
        last_login_ip: row.last_login_ip,
        last_logout_ip: row.last_logout_ip,
        last_login_medium: row.last_login_medium,
        last_login_uagent: row.last_login_uagent,
        token_updated_at: row.token_updated_at,
        is_bot: row.is_bot,
        bot_type: row.bot_type,
        user_timezone: row.user_timezone,
        is_email_valid: row.is_email_valid,
        masked_at: row.masked_at,
    }
}

/// A `User` row for [`user_me_to_representation`] (`user.py:63-88`):
/// `avatar_url` / `cover_image_url` are the resolved model properties
/// (`None` renders `null`); `cover_image` / `email` are nullable
/// columns; `last_login_time` is a nullable datetime (pre-rendered DRF
/// string when set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMeRow<'a> {
    pub id: &'a str,
    pub avatar: &'a str,
    pub cover_image: Option<&'a str>,
    pub avatar_url: Option<&'a str>,
    pub cover_image_url: Option<&'a str>,
    pub date_joined: &'a str,
    pub display_name: &'a str,
    pub email: Option<&'a str>,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub is_active: bool,
    pub is_bot: bool,
    pub is_email_verified: bool,
    pub user_timezone: &'a str,
    pub username: &'a str,
    pub is_password_autoset: bool,
    pub last_login_medium: &'a str,
    pub last_login_time: Option<&'a str>,
}

/// `UserMeSerializer.to_representation` output, in wire order: 18 keys —
/// the duplicated `is_email_verified` renders once (BUG-dup).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserMeView<'a> {
    pub id: &'a str,
    pub avatar: &'a str,
    pub cover_image: Option<&'a str>,
    pub avatar_url: Option<&'a str>,
    pub cover_image_url: Option<&'a str>,
    pub date_joined: &'a str,
    pub display_name: &'a str,
    pub email: Option<&'a str>,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub is_active: bool,
    pub is_bot: bool,
    pub is_email_verified: bool,
    pub user_timezone: &'a str,
    pub username: &'a str,
    pub is_password_autoset: bool,
    pub last_login_medium: &'a str,
    pub last_login_time: Option<&'a str>,
}

/// Port of `UserMeSerializer` (`user.py:63-88`).
pub fn user_me_to_representation<'a>(row: &'a UserMeRow<'a>) -> UserMeView<'a> {
    UserMeView {
        id: row.id,
        avatar: row.avatar,
        cover_image: row.cover_image,
        avatar_url: row.avatar_url,
        cover_image_url: row.cover_image_url,
        date_joined: row.date_joined,
        display_name: row.display_name,
        email: row.email,
        first_name: row.first_name,
        last_name: row.last_name,
        is_active: row.is_active,
        is_bot: row.is_bot,
        is_email_verified: row.is_email_verified,
        user_timezone: row.user_timezone,
        username: row.username,
        is_password_autoset: row.is_password_autoset,
        last_login_medium: row.last_login_medium,
        last_login_time: row.last_login_time,
    }
}

/// The last-workspace side of `get_workspace` (`user.py:103-125`):
/// `profile.last_workspace_id` plus the re-fetched workspace's slug /
/// name. `logo` is the resolved asset URL — `""` when the workspace has
/// no logo asset (`user.py:116`); the models layer owns the resolution.
/// The `if workspace is not None else ""` guards on slug/name are dead
/// (the `exists()` check just passed), so the handler always passes the
/// resolved values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeSettingsLastWorkspace<'a> {
    pub id: &'a str,
    pub slug: &'a str,
    pub name: &'a str,
    pub logo: &'a str,
}

/// The earliest-created active membership side of `get_workspace`
/// (`user.py:127-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeSettingsFallbackWorkspace<'a> {
    pub id: &'a str,
    pub slug: &'a str,
}

/// A `User` row for [`me_settings_to_representation`]
/// (`user.py:90-139`): `last_workspace` is `Some` exactly when
/// `profile.last_workspace_id` is set and the user is an active member
/// of that workspace (`user.py:103-110`, checked by the handler);
/// `fallback_workspace` is the earliest-created active membership if
/// any (`user.py:127-131`); `invites` is the invite count for the
/// user's email (`user.py:99`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserMeSettingsRow<'a> {
    pub id: &'a str,
    pub email: Option<&'a str>,
    pub last_workspace: Option<MeSettingsLastWorkspace<'a>>,
    pub fallback_workspace: Option<MeSettingsFallbackWorkspace<'a>>,
    pub invites: i64,
}

/// `get_workspace` last-workspace branch output (`user.py:117-125`), in
/// output order: `fallback_*` duplicate `last_*` (BUG-asymmetry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeSettingsWorkspaceFullView<'a> {
    pub last_workspace_id: &'a str,
    pub last_workspace_slug: &'a str,
    pub last_workspace_name: &'a str,
    pub last_workspace_logo: &'a str,
    pub fallback_workspace_id: &'a str,
    pub fallback_workspace_slug: &'a str,
    pub invites: i64,
}

/// `get_workspace` fallback branch output (`user.py:132-138`), in output
/// order: no name/logo keys; `last_workspace_*` are always `null`
/// (BUG-asymmetry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeSettingsWorkspaceFallbackView<'a> {
    pub last_workspace_id: Option<&'a str>,
    pub last_workspace_slug: Option<&'a str>,
    pub fallback_workspace_id: Option<&'a str>,
    pub fallback_workspace_slug: Option<&'a str>,
    pub invites: i64,
}

/// `get_workspace` output (`user.py:98-138`): untagged, so each branch
/// serializes as its bare shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum MeSettingsWorkspaceView<'a> {
    Full(MeSettingsWorkspaceFullView<'a>),
    Fallback(MeSettingsWorkspaceFallbackView<'a>),
}

/// Port of `UserMeSettingsSerializer.get_workspace` (`user.py:98-138`).
pub fn me_settings_workspace_to_representation<'a>(
    last: Option<MeSettingsLastWorkspace<'a>>,
    fallback: Option<MeSettingsFallbackWorkspace<'a>>,
    invites: i64,
) -> MeSettingsWorkspaceView<'a> {
    match last {
        Some(last) => MeSettingsWorkspaceView::Full(MeSettingsWorkspaceFullView {
            last_workspace_id: last.id,
            last_workspace_slug: last.slug,
            last_workspace_name: last.name,
            last_workspace_logo: last.logo,
            fallback_workspace_id: last.id,
            fallback_workspace_slug: last.slug,
            invites,
        }),
        None => {
            let (fallback_id, fallback_slug) = match fallback {
                Some(fallback) => (Some(fallback.id), Some(fallback.slug)),
                None => (None, None),
            };
            MeSettingsWorkspaceView::Fallback(MeSettingsWorkspaceFallbackView {
                last_workspace_id: None,
                last_workspace_slug: None,
                fallback_workspace_id: fallback_id,
                fallback_workspace_slug: fallback_slug,
                invites,
            })
        }
    }
}

/// `UserMeSettingsSerializer.to_representation` output (`user.py:90-97`),
/// in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserMeSettingsView<'a> {
    pub id: &'a str,
    pub email: Option<&'a str>,
    pub workspace: MeSettingsWorkspaceView<'a>,
}

/// Port of `UserMeSettingsSerializer` (`user.py:90-139`).
pub fn me_settings_to_representation<'a>(row: &'a UserMeSettingsRow<'a>) -> UserMeSettingsView<'a> {
    UserMeSettingsView {
        id: row.id,
        email: row.email,
        workspace: me_settings_workspace_to_representation(
            row.last_workspace,
            row.fallback_workspace,
            row.invites,
        ),
    }
}

/// A `User` row for [`user_lite_to_representation`] (`user.py:141-154`):
/// `avatar_url` is the resolved model property (`None` renders
/// `null`).
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

/// A `User` row for [`user_admin_lite_to_representation`]
/// (`user.py:156-171`): the lite columns plus nullable `email` and
/// `last_login_medium`.
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

/// Port of `UserAdminLiteSerializer` (`user.py:156-171`).
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// F-W24-04 golden
    /// (`rust-api/fixtures/app_workspace/serializers/user.golden.json`),
    /// the Done-when oracle for this module.
    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_workspace/serializers/user.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn case<'a>(fixture: &'a Value, name: &str) -> &'a Value {
        fixture["cases"]
            .as_array()
            .expect("cases array")
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("fixture lacks case {name}"))
    }

    fn str_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("string array")
            .iter()
            .map(|s| s.as_str().expect("string").to_string())
            .collect()
    }

    fn const_list<const N: usize>(fields: &[&str; N]) -> Vec<String> {
        fields.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fixture_cases_drive_validators() {
        // Every validator case in F-W24-04 passes against this module.
        let golden = fixture();
        let first_case = case(&golden, "first-name URL guard");
        let err = validate_first_name(
            first_case["input"]["first_name"]
                .as_str()
                .expect("input first_name"),
        )
        .expect_err("first name with URL is rejected");
        assert_eq!(
            err.to_string(),
            first_case["error"].as_str().expect("error")
        );

        let last_case = case(&golden, "last-name URL guard");
        let err = validate_last_name(
            last_case["input"]["last_name"]
                .as_str()
                .expect("input last_name"),
        )
        .expect_err("last name with URL is rejected");
        assert_eq!(err.to_string(), last_case["error"].as_str().expect("error"));

        // Plain names pass through untouched.
        assert_eq!(validate_first_name("Ada"), Ok("Ada"));
        assert_eq!(validate_last_name("Lovelace"), Ok("Lovelace"));
        assert_eq!(
            FirstNameError::ContainsUrl.to_string(),
            "First name cannot contain a URL."
        );
        assert_eq!(
            LastNameError::ContainsUrl.to_string(),
            "Last name cannot contain a URL."
        );
    }

    #[test]
    fn fields_and_read_only_match_fixture() {
        let golden = fixture();
        let serializers = &golden["serializers"];
        assert_eq!(
            const_list(&USER_READ_ONLY_FIELDS),
            str_list(&serializers["UserSerializer"]["read_only"]),
            "UserSerializer read_only"
        );
        assert_eq!(
            serializers["UserSerializer"]["fields"].as_str(),
            Some("[f.name for f in User._meta.fields if f != 'password'] (:29)"),
            "UserSerializer fields rule"
        );
        assert_eq!(
            const_list(&USER_ME_FIELDS),
            str_list(&serializers["UserMeSerializer"]["fields"]),
            "UserMeSerializer fields (19, dup included)"
        );
        assert_eq!(
            serializers["UserMeSerializer"]["read_only"].as_str(),
            Some("= fields (:87)"),
            "UserMeSerializer read_only"
        );
        assert_eq!(
            USER_ME_READ_ONLY_FIELDS, USER_ME_FIELDS,
            "read_only_fields = fields"
        );
        assert_eq!(
            const_list(&USER_ME_SETTINGS_FIELDS),
            str_list(&serializers["UserMeSettingsSerializer"]["fields"]),
            "UserMeSettingsSerializer fields"
        );
        assert_eq!(
            serializers["UserMeSettingsSerializer"]["read_only"].as_str(),
            Some("= fields (:96)"),
            "UserMeSettingsSerializer read_only"
        );
        assert_eq!(
            USER_ME_SETTINGS_READ_ONLY_FIELDS, USER_ME_SETTINGS_FIELDS,
            "read_only_fields = fields"
        );
        assert_eq!(
            const_list(&USER_LITE_WIRE_FIELDS),
            str_list(&serializers["UserLiteSerializer"]["fields"]),
            "UserLiteSerializer fields"
        );
        assert_eq!(
            const_list(&USER_LITE_READ_ONLY_FIELDS),
            str_list(&serializers["UserLiteSerializer"]["read_only"]),
            "UserLiteSerializer read_only"
        );
        assert_eq!(
            const_list(&USER_ADMIN_LITE_WIRE_FIELDS),
            str_list(&serializers["UserAdminLiteSerializer"]["fields"]),
            "UserAdminLiteSerializer fields"
        );
        assert_eq!(
            const_list(&USER_LITE_READ_ONLY_FIELDS),
            str_list(&serializers["UserAdminLiteSerializer"]["read_only"]),
            "UserAdminLiteSerializer read_only"
        );
    }

    #[test]
    fn workspace_branch_keys_match_fixture() {
        let golden = fixture();
        let full = case(&golden, "get_workspace last-workspace branch");
        assert_eq!(
            const_list(&ME_SETTINGS_WORKSPACE_FULL_KEYS),
            str_list(&full["output_keys"]),
            "last-workspace branch keys"
        );
        let fallback = case(&golden, "get_workspace fallback branch");
        assert_eq!(
            const_list(&ME_SETTINGS_WORKSPACE_FALLBACK_KEYS),
            str_list(&fallback["output_keys"]),
            "fallback branch keys"
        );
    }

    #[test]
    fn wire_fields_match_live_serializer_order() {
        // Pinned against `list(Serializer().fields)` from the live Django
        // serializers (repo venv probe, /tmp/probe603_bytes.py).
        assert_eq!(
            USER_WIRE_FIELDS.to_vec(),
            [
                "last_login",
                "id",
                "username",
                "mobile_number",
                "email",
                "display_name",
                "first_name",
                "last_name",
                "avatar",
                "avatar_asset",
                "cover_image",
                "cover_image_asset",
                "date_joined",
                "created_at",
                "updated_at",
                "last_location",
                "created_location",
                "is_superuser",
                "is_managed",
                "is_password_expired",
                "is_active",
                "is_staff",
                "is_email_verified",
                "is_password_autoset",
                "is_password_reset_required",
                "token",
                "last_active",
                "last_login_time",
                "last_logout_time",
                "last_login_ip",
                "last_logout_ip",
                "last_login_medium",
                "last_login_uagent",
                "token_updated_at",
                "is_bot",
                "bot_type",
                "user_timezone",
                "is_email_valid",
                "masked_at",
            ]
        );
        assert_eq!(
            USER_ME_WIRE_FIELDS.to_vec(),
            [
                "id",
                "avatar",
                "cover_image",
                "avatar_url",
                "cover_image_url",
                "date_joined",
                "display_name",
                "email",
                "first_name",
                "last_name",
                "is_active",
                "is_bot",
                "is_email_verified",
                "user_timezone",
                "username",
                "is_password_autoset",
                "last_login_medium",
                "last_login_time",
            ]
        );
        assert_eq!(
            USER_LITE_WIRE_FIELDS.to_vec(),
            [
                "id",
                "first_name",
                "last_name",
                "avatar",
                "avatar_url",
                "is_bot",
                "display_name",
            ]
        );
        assert_eq!(
            USER_ADMIN_LITE_WIRE_FIELDS.to_vec(),
            [
                "id",
                "first_name",
                "last_name",
                "avatar",
                "avatar_url",
                "is_bot",
                "display_name",
                "email",
                "last_login_medium",
            ]
        );
    }

    #[test]
    fn user_me_duplicate_collapses_on_wire() {
        // BUG-dup (user.py:79,83,87): the 19-entry fields list carries
        // `is_email_verified` twice; the live wire renders one copy at
        // the first position.
        assert_eq!(USER_ME_FIELDS.len(), 19);
        assert_eq!(USER_ME_FIELDS[12], "is_email_verified");
        assert_eq!(USER_ME_FIELDS[16], "is_email_verified");
        assert_eq!(USER_ME_WIRE_FIELDS.len(), 18);
        assert_eq!(
            USER_ME_WIRE_FIELDS
                .iter()
                .filter(|k| **k == "is_email_verified")
                .count(),
            1
        );
        // The wire is the fields list minus the second occurrence.
        let deduped: Vec<&str> = USER_ME_FIELDS
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 16)
            .map(|(_, k)| *k)
            .collect();
        assert_eq!(deduped, USER_ME_WIRE_FIELDS.to_vec());
    }

    #[test]
    fn dynamic_rule_and_dead_code_hold() {
        // BUG-dynamic: `password` is excluded from the User wire.
        assert!(!USER_WIRE_FIELDS.contains(&"password"));
        // BUG-onboarded: `get_is_onboarded` is dead Meta code — no key.
        assert!(!USER_WIRE_FIELDS.contains(&"is_onboarded"));
        // M2M names are not in `_meta.fields`, so they never render.
        assert!(!USER_WIRE_FIELDS.contains(&"groups"));
        assert!(!USER_WIRE_FIELDS.contains(&"user_permissions"));
    }

    /// Live DRF bytes below: captured from the real serializers via
    /// DRF's `JSONRenderer` (repo venv, unsaved model instance, no DB —
    /// `Mozilla/5.0 (x) "q"` stresses escaping). Datetimes cross this
    /// boundary pre-rendered, so the oracles use the production `Z` form
    /// (the probe's minimal settings render `-06:00` for the same
    /// instants).
    const USER: &str = r##"{"last_login":null,"id":"11111111-1111-1111-1111-111111111111","username":"ada_lovelace","mobile_number":null,"email":"ada@acme.test","display_name":"","first_name":"Ada","last_name":"Lovelace","avatar":"avatars/a.png","avatar_asset":null,"cover_image":null,"cover_image_asset":null,"date_joined":"2026-01-15T12:30:45.123456Z","created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","last_location":"","created_location":"","is_superuser":false,"is_managed":false,"is_password_expired":false,"is_active":true,"is_staff":false,"is_email_verified":true,"is_password_autoset":false,"is_password_reset_required":false,"token":"tok_abc","last_active":"2026-01-16T08:05:04Z","last_login_time":"2026-01-16T08:05:04Z","last_logout_time":null,"last_login_ip":"1.2.3.4","last_logout_ip":"","last_login_medium":"password","last_login_uagent":"Mozilla/5.0 (x) \"q\"","token_updated_at":null,"is_bot":false,"bot_type":null,"user_timezone":"UTC","is_email_valid":true,"masked_at":null}"##;
    const ME: &str = r##"{"id":"11111111-1111-1111-1111-111111111111","avatar":"avatars/a.png","cover_image":null,"avatar_url":"avatars/a.png","cover_image_url":null,"date_joined":"2026-01-15T12:30:45.123456Z","display_name":"","email":"ada@acme.test","first_name":"Ada","last_name":"Lovelace","is_active":true,"is_bot":false,"is_email_verified":true,"user_timezone":"UTC","username":"ada_lovelace","is_password_autoset":false,"last_login_medium":"password","last_login_time":"2026-01-16T08:05:04Z"}"##;
    const LITE: &str = r##"{"id":"11111111-1111-1111-1111-111111111111","first_name":"Ada","last_name":"Lovelace","avatar":"avatars/a.png","avatar_url":"avatars/a.png","is_bot":false,"display_name":""}"##;
    const ADMIN_LITE: &str = r##"{"id":"11111111-1111-1111-1111-111111111111","first_name":"Ada","last_name":"Lovelace","avatar":"avatars/a.png","avatar_url":"avatars/a.png","is_bot":false,"display_name":"","email":"ada@acme.test","last_login_medium":"password"}"##;
    /// `get_workspace` is DB-backed (invites, profile, memberships), so
    /// these oracles are constructed from the source key order
    /// (`user.py:117-138`) plus the F-W24-04 branch key lists; `Wörks`
    /// stresses unicode (DRF renders `ensure_ascii=False`, as in the
    /// sibling's live-captured oracles).
    const ME_SETTINGS_FULL: &str = r##"{"id":"11111111-1111-1111-1111-111111111111","email":"ada@acme.test","workspace":{"last_workspace_id":"55555555-5555-5555-5555-555555555555","last_workspace_slug":"acme-works","last_workspace_name":"Acme Wörks","last_workspace_logo":"https://cdn.test/logo.png","fallback_workspace_id":"55555555-5555-5555-5555-555555555555","fallback_workspace_slug":"acme-works","invites":2}}"##;
    const ME_SETTINGS_FALLBACK: &str = r##"{"id":"11111111-1111-1111-1111-111111111111","email":"ada@acme.test","workspace":{"last_workspace_id":null,"last_workspace_slug":null,"fallback_workspace_id":"66666666-6666-6666-6666-666666666666","fallback_workspace_slug":"other-ws","invites":0}}"##;
    const ME_SETTINGS_EMPTY: &str = r##"{"id":"11111111-1111-1111-1111-111111111111","email":"ada@acme.test","workspace":{"last_workspace_id":null,"last_workspace_slug":null,"fallback_workspace_id":null,"fallback_workspace_slug":null,"invites":1}}"##;

    fn user_row() -> UserRow<'static> {
        UserRow {
            last_login: None,
            id: "11111111-1111-1111-1111-111111111111",
            username: "ada_lovelace",
            mobile_number: None,
            email: Some("ada@acme.test"),
            display_name: "",
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "avatars/a.png",
            avatar_asset: None,
            cover_image: None,
            cover_image_asset: None,
            date_joined: "2026-01-15T12:30:45.123456Z",
            created_at: "2026-01-15T12:30:45.123456Z",
            updated_at: "2026-01-16T08:05:04Z",
            last_location: "",
            created_location: "",
            is_superuser: false,
            is_managed: false,
            is_password_expired: false,
            is_active: true,
            is_staff: false,
            is_email_verified: true,
            is_password_autoset: false,
            is_password_reset_required: false,
            token: "tok_abc",
            last_active: Some("2026-01-16T08:05:04Z"),
            last_login_time: Some("2026-01-16T08:05:04Z"),
            last_logout_time: None,
            last_login_ip: "1.2.3.4",
            last_logout_ip: "",
            last_login_medium: "password",
            last_login_uagent: "Mozilla/5.0 (x) \"q\"",
            token_updated_at: None,
            is_bot: false,
            bot_type: None,
            user_timezone: "UTC",
            is_email_valid: true,
            masked_at: None,
        }
    }

    fn user_me_row() -> UserMeRow<'static> {
        UserMeRow {
            id: "11111111-1111-1111-1111-111111111111",
            avatar: "avatars/a.png",
            cover_image: None,
            avatar_url: Some("avatars/a.png"),
            cover_image_url: None,
            date_joined: "2026-01-15T12:30:45.123456Z",
            display_name: "",
            email: Some("ada@acme.test"),
            first_name: "Ada",
            last_name: "Lovelace",
            is_active: true,
            is_bot: false,
            is_email_verified: true,
            user_timezone: "UTC",
            username: "ada_lovelace",
            is_password_autoset: false,
            last_login_medium: "password",
            last_login_time: Some("2026-01-16T08:05:04Z"),
        }
    }

    #[test]
    fn user_serializers_replay_live_bytes() {
        let rendered =
            serde_json::to_string(&user_to_representation(&user_row())).expect("serializes");
        assert_eq!(rendered, USER);

        let rendered =
            serde_json::to_string(&user_me_to_representation(&user_me_row())).expect("serializes");
        assert_eq!(rendered, ME);

        let lite = UserLiteRow {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "avatars/a.png",
            avatar_url: Some("avatars/a.png"),
            is_bot: false,
            display_name: "",
        };
        let rendered =
            serde_json::to_string(&user_lite_to_representation(&lite)).expect("serializes");
        assert_eq!(rendered, LITE);

        let admin_lite = UserAdminLiteRow {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "avatars/a.png",
            avatar_url: Some("avatars/a.png"),
            is_bot: false,
            display_name: "",
            email: Some("ada@acme.test"),
            last_login_medium: "password",
        };
        let rendered = serde_json::to_string(&user_admin_lite_to_representation(&admin_lite))
            .expect("serializes");
        assert_eq!(rendered, ADMIN_LITE);
    }

    #[test]
    fn me_settings_branches_replay_shapes() {
        // Last-workspace branch: fallback_* duplicate last_*.
        let full = UserMeSettingsRow {
            id: "11111111-1111-1111-1111-111111111111",
            email: Some("ada@acme.test"),
            last_workspace: Some(MeSettingsLastWorkspace {
                id: "55555555-5555-5555-5555-555555555555",
                slug: "acme-works",
                name: "Acme Wörks",
                logo: "https://cdn.test/logo.png",
            }),
            fallback_workspace: Some(MeSettingsFallbackWorkspace {
                id: "66666666-6666-6666-6666-666666666666",
                slug: "other-ws",
            }),
            invites: 2,
        };
        let rendered =
            serde_json::to_string(&me_settings_to_representation(&full)).expect("serializes");
        assert_eq!(rendered, ME_SETTINGS_FULL);

        // Fallback branch: last_* null, earliest membership fills fallback_*.
        let fallback = UserMeSettingsRow {
            id: "11111111-1111-1111-1111-111111111111",
            email: Some("ada@acme.test"),
            last_workspace: None,
            fallback_workspace: Some(MeSettingsFallbackWorkspace {
                id: "66666666-6666-6666-6666-666666666666",
                slug: "other-ws",
            }),
            invites: 0,
        };
        let rendered =
            serde_json::to_string(&me_settings_to_representation(&fallback)).expect("serializes");
        assert_eq!(rendered, ME_SETTINGS_FALLBACK);

        // No memberships at all: everything null except invites.
        let empty = UserMeSettingsRow {
            id: "11111111-1111-1111-1111-111111111111",
            email: Some("ada@acme.test"),
            last_workspace: None,
            fallback_workspace: None,
            invites: 1,
        };
        let rendered =
            serde_json::to_string(&me_settings_to_representation(&empty)).expect("serializes");
        assert_eq!(rendered, ME_SETTINGS_EMPTY);
    }
}
