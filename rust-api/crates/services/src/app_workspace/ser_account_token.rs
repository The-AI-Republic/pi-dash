//! Profile + account + API-token + favorite serializers (D-24).
//!
//! Port of `apps/api/pi_dash/app/serializers/`:
//!
//! * `user.py:201-211` (`ProfileSerializer`, `__all__`, read-only `user` +
//!   `settings`)
//! * `user.py:214-218` (`AccountSerializer`, `__all__`, read-only `user`)
//! * `api.py:11-25` (`APITokenSerializer`, `__all__`, 9-key read-only list)
//! * `api.py:28-38` (`APITokenReadSerializer`, `exclude = ("token",)` +
//!   computed `is_active`)
//! * `favorite.py:1-89` (`UserFavoriteSerializer` + `Project` / `Page` /
//!   `Cycle` / `Module` lite serializers + `ViewFavoriteSerializer` +
//!   `get_entity_model_and_serializer` dispatch)
//!
//! These are pure kernels in the [`crate::v1_projects::ser_collab`]
//! style: each `to_representation` takes a row borrowed from the caller and returns a
//! `serde::Serialize` view whose fields are the live DRF wire fields in
//! output order. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`.
//! Datetimes cross this boundary already rendered as DRF `iso-8601`
//! strings — formatting owns to the DB edge, so rendering here is a
//! byte-exact passthrough. JSON blobs (`theme`, `settings`, `metadata`,
//! `logo_props`, …) pass through by reference.
//!
//! DRF `__all__` order rule (verified against the live serializers,
//! Django 4.2.30 / DRF 3.15.2): declared fields first — `id`
//! (`BaseSerializer`, `base.py:8-9`), then subclass-declared fields
//! (`is_active` on the read serializer) — then non-relational concrete
//! columns in `_meta` order (abstract-base timestamps first), then forward
//! relations trailing (`user`, `workspace`, `created_by`, `updated_by`).
//! Explicit `Meta.fields` lists (the favorite family) render in list
//! order.
//!
//! Caller-resolved inputs (never fork a helper):
//!
//! * `is_active` on the read shape is computed by
//!   [`api_token_is_active`] from parsed datetimes and placed by the
//!   caller; the row only carries the pre-rendered `expired_at` string.
//! * `entity_data` is the already-rendered nested lite value composed by
//!   the handler via [`favorite_entity_kind`] (the `entity_model.objects
//!   .get(pk=…)` fetch is handler SQL; `None` renders `null` for the
//!   issue / folder / unknown / missing arms alike).
//! * `project_id` on the page lite is the `projects.first()` result
//!   (`favorite.py:23-25`) resolved by the caller; `None` renders `null`.
//!
//! Ported quirks (translate, don't redesign; also listed in the PR):
//!
//! * `settings` is read-only ON PURPOSE (`user.py:205-211` comment): the
//!   namespaced bag is validated and merged per namespace by
//!   `ProfileEndpoint` — whole-field writes stay disabled.
//! * `APITokenReadSerializer` is the only serializer using
//!   `Meta.exclude` (`api.py:33`) instead of `fields`; its declared
//!   `is_active` (`SerializerMethodField`, `:29`) overrides the model
//!   column and renders second, right after `id`.
//! * `UserFavoriteSerializer.read_only_fields` (`favorite.py:76`) names
//!   `workspace` / `created_by` / `updated_by`, but `Meta.fields`
//!   (`:64-75`) carries `workspace_id` / `project_id` — the `workspace`
//!   entry is silently ignored (probed: effective read-only keys are
//!   `entity_data`, `id`, `project_id`, `workspace_id`).
//! * The favorite dispatch maps `issue` to `(Issue, None)`
//!   (`favorite.py:49`), so `entity_data` is ALWAYS `None` for issues
//!   even when the row exists; `folder` maps to `(None, None)` (`:54`);
//!   a missing row maps to `None` via `DoesNotExist` (`:87-88`).
//! * The token value is returned by the POST-create and PATCH 200
//!   responses (`app/views/api.py:37-39, :68-71` — both serialize with
//!   `APITokenSerializer`); GET list/detail always use the read
//!   serializer, which excludes it. The token key is `read_only`
//!   (`api.py:15`), so it is model-default-generated, never
//!   input-writable.
//! * `expired_at` is `read_only` too, so a PATCH carrying it is silently
//!   accepted and the value ignored (probed: `{"expired_at":
//!   "not-a-date"}` validates clean); only `label`, `description`,
//!   `is_service` and `allowed_rate_limit` are PATCH-writable.
//! * `PageFavoriteLiteSerializer.get_project_id` uses `projects.first()`
//!   (`favorite.py:23-25`), differing from `PageRecentVisitSerializer`'s
//!   annotated-`project_id` preference — each ported as written.
//!
//! Out of scope (documented, not ported): `APIActivityLogSerializer`
//! (`api.py:41-44`) — unexported from `serializers/__init__.py` and
//! unused by every D-24 route (`app/views/api.py` uses only the two
//! token serializers); generic DRF field-error bodies (e.g. `{"label":
//! ["Ensure this field has no more than 255 characters."]}`, probed) —
//! the handler arm, owned by PIDASHCONV-620/622/624, like the sibling
//! handler-layer ports; the entity-fetch and `projects.first()` SQL —
//! the queries layer (PIDASHCONV-611).

use serde::Serialize;

/// `ProfileSerializer` wire keys in output order (`user.py:201-211`,
/// `fields = "__all__"`: declared `id`, then concrete columns
/// (`TimeAuditModel` timestamps first), then the `user` relation —
/// verified against the live serializer).
pub const PROFILE_WIRE_FIELDS: [&str; 30] = [
    "id",
    "created_at",
    "updated_at",
    "theme",
    "is_app_rail_docked",
    "is_tour_completed",
    "onboarding_step",
    "use_case",
    "role",
    "is_onboarded",
    "last_workspace_id",
    "billing_address_country",
    "billing_address",
    "has_billing_address",
    "company_name",
    "notification_view_mode",
    "is_smooth_cursor_enabled",
    "is_mobile_onboarded",
    "mobile_onboarding_step",
    "mobile_timezone_auto_set",
    "language",
    "start_of_the_week",
    "goals",
    "background_color",
    "is_navigation_tour_completed",
    "has_marketing_email_consent",
    "is_subscribed_to_changelog",
    "product_tour",
    "settings",
    "user",
];

/// `ProfileSerializer.Meta.read_only_fields` (`user.py:211`), verbatim.
/// `id` is additionally read-only via the `BaseSerializer` declaration
/// (`base.py:8-9`); `created_at` / `updated_at` via `auto_now_add` /
/// `auto_now` (probed effective set: `created_at`, `id`, `settings`,
/// `updated_at`, `user`).
pub const PROFILE_READ_ONLY_FIELDS: [&str; 2] = ["user", "settings"];

/// `AccountSerializer` wire keys in output order (`user.py:214-218`,
/// `fields = "__all__"` — verified against the live serializer).
pub const ACCOUNT_WIRE_FIELDS: [&str; 13] = [
    "id",
    "created_at",
    "updated_at",
    "provider_account_id",
    "provider",
    "access_token",
    "access_token_expired_at",
    "refresh_token",
    "refresh_token_expired_at",
    "last_connected_at",
    "id_token",
    "metadata",
    "user",
];

/// `AccountSerializer.Meta.read_only_fields` (`user.py:218`), verbatim
/// (probed effective set adds `created_at`, `id`, `updated_at`).
pub const ACCOUNT_READ_ONLY_FIELDS: [&str; 1] = ["user"];

/// `APITokenSerializer` wire keys in output order (`api.py:11-25`,
/// `fields = "__all__"` — verified against the live serializer).
pub const API_TOKEN_WIRE_FIELDS: [&str; 17] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "label",
    "description",
    "is_active",
    "last_used",
    "token",
    "user_type",
    "expired_at",
    "is_service",
    "allowed_rate_limit",
    "created_by",
    "updated_by",
    "user",
    "workspace",
];

/// `APITokenSerializer.Meta.read_only_fields` (`api.py:14-24`), verbatim
/// (probed effective set adds `id`).
pub const API_TOKEN_READ_ONLY_FIELDS: [&str; 9] = [
    "token",
    "expired_at",
    "created_at",
    "updated_at",
    "workspace",
    "user",
    "is_active",
    "last_used",
    "user_type",
];

/// `APITokenReadSerializer` wire keys in output order (`api.py:28-38`):
/// declared `id` / `is_active`, then every `APIToken` column except
/// `token` (`Meta.exclude`, `:33`) — verified against the live
/// serializer.
pub const API_TOKEN_READ_WIRE_FIELDS: [&str; 16] = [
    "id",
    "is_active",
    "created_at",
    "updated_at",
    "deleted_at",
    "label",
    "description",
    "last_used",
    "user_type",
    "expired_at",
    "is_service",
    "allowed_rate_limit",
    "created_by",
    "updated_by",
    "user",
    "workspace",
];

/// `UserFavoriteSerializer.Meta.fields` (`favorite.py:64-75`), wire order.
/// Explicit list, so order is the list (verified live).
pub const USER_FAVORITE_WIRE_FIELDS: [&str; 10] = [
    "id",
    "entity_type",
    "entity_identifier",
    "entity_data",
    "name",
    "is_folder",
    "sequence",
    "parent",
    "workspace_id",
    "project_id",
];

/// `UserFavoriteSerializer.Meta.read_only_fields` (`favorite.py:76`),
/// verbatim — the `workspace` entry names a field that is not in
/// `Meta.fields` and is silently ignored (probed effective read-only
/// keys: `entity_data`, `id`, `project_id`, `workspace_id`).
pub const USER_FAVORITE_READ_ONLY_FIELDS: [&str; 3] = ["workspace", "created_by", "updated_by"];

/// `ProjectFavoriteLiteSerializer.Meta.fields` (`favorite.py:13`).
pub const PROJECT_FAVORITE_WIRE_FIELDS: [&str; 3] = ["id", "name", "logo_props"];

/// `PageFavoriteLiteSerializer.Meta.fields` (`favorite.py:21`).
pub const PAGE_FAVORITE_WIRE_FIELDS: [&str; 4] = ["id", "name", "logo_props", "project_id"];

/// `CycleFavoriteLiteSerializer.Meta.fields` (`favorite.py:31`).
pub const CYCLE_FAVORITE_WIRE_FIELDS: [&str; 4] = ["id", "name", "logo_props", "project_id"];

/// `ModuleFavoriteLiteSerializer.Meta.fields` (`favorite.py:37`).
pub const MODULE_FAVORITE_WIRE_FIELDS: [&str; 4] = ["id", "name", "logo_props", "project_id"];

/// `ViewFavoriteSerializer.Meta.fields` (`favorite.py:43`).
pub const VIEW_FAVORITE_WIRE_FIELDS: [&str; 4] = ["id", "name", "logo_props", "project_id"];

/// A `Profile` row for [`profile_to_representation`]
/// (`db/models/user.py:200-277`): `use_case` / `role` / `billing_address`
/// (`null=True`) and `last_workspace_id` (`UUIDField(null=True)`) render
/// `null` when unset; `start_of_the_week` is a `PositiveSmallIntegerField`;
/// `user` is a non-nullable one-to-one rendering as a UUID string;
/// datetimes are pre-rendered DRF strings.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub theme: &'a serde_json::Value,
    pub is_app_rail_docked: bool,
    pub is_tour_completed: bool,
    pub onboarding_step: &'a serde_json::Value,
    pub use_case: Option<&'a str>,
    pub role: Option<&'a str>,
    pub is_onboarded: bool,
    pub last_workspace_id: Option<&'a str>,
    pub billing_address_country: &'a str,
    pub billing_address: Option<&'a serde_json::Value>,
    pub has_billing_address: bool,
    pub company_name: &'a str,
    pub notification_view_mode: &'a str,
    pub is_smooth_cursor_enabled: bool,
    pub is_mobile_onboarded: bool,
    pub mobile_onboarding_step: &'a serde_json::Value,
    pub mobile_timezone_auto_set: bool,
    pub language: &'a str,
    pub start_of_the_week: i32,
    pub goals: &'a serde_json::Value,
    pub background_color: &'a str,
    pub is_navigation_tour_completed: bool,
    pub has_marketing_email_consent: bool,
    pub is_subscribed_to_changelog: bool,
    pub product_tour: &'a serde_json::Value,
    pub settings: &'a serde_json::Value,
    pub user: &'a str,
}

/// `ProfileSerializer.to_representation` output (`user.py:201-211`), in
/// wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub theme: &'a serde_json::Value,
    pub is_app_rail_docked: bool,
    pub is_tour_completed: bool,
    pub onboarding_step: &'a serde_json::Value,
    pub use_case: Option<&'a str>,
    pub role: Option<&'a str>,
    pub is_onboarded: bool,
    pub last_workspace_id: Option<&'a str>,
    pub billing_address_country: &'a str,
    pub billing_address: Option<&'a serde_json::Value>,
    pub has_billing_address: bool,
    pub company_name: &'a str,
    pub notification_view_mode: &'a str,
    pub is_smooth_cursor_enabled: bool,
    pub is_mobile_onboarded: bool,
    pub mobile_onboarding_step: &'a serde_json::Value,
    pub mobile_timezone_auto_set: bool,
    pub language: &'a str,
    pub start_of_the_week: i32,
    pub goals: &'a serde_json::Value,
    pub background_color: &'a str,
    pub is_navigation_tour_completed: bool,
    pub has_marketing_email_consent: bool,
    pub is_subscribed_to_changelog: bool,
    pub product_tour: &'a serde_json::Value,
    pub settings: &'a serde_json::Value,
    pub user: &'a str,
}

/// Port of `ProfileSerializer` (`user.py:201-211`) read shape.
pub fn profile_to_representation<'a>(row: &'a ProfileRow<'a>) -> ProfileView<'a> {
    ProfileView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        theme: row.theme,
        is_app_rail_docked: row.is_app_rail_docked,
        is_tour_completed: row.is_tour_completed,
        onboarding_step: row.onboarding_step,
        use_case: row.use_case,
        role: row.role,
        is_onboarded: row.is_onboarded,
        last_workspace_id: row.last_workspace_id,
        billing_address_country: row.billing_address_country,
        billing_address: row.billing_address,
        has_billing_address: row.has_billing_address,
        company_name: row.company_name,
        notification_view_mode: row.notification_view_mode,
        is_smooth_cursor_enabled: row.is_smooth_cursor_enabled,
        is_mobile_onboarded: row.is_mobile_onboarded,
        mobile_onboarding_step: row.mobile_onboarding_step,
        mobile_timezone_auto_set: row.mobile_timezone_auto_set,
        language: row.language,
        start_of_the_week: row.start_of_the_week,
        goals: row.goals,
        background_color: row.background_color,
        is_navigation_tour_completed: row.is_navigation_tour_completed,
        has_marketing_email_consent: row.has_marketing_email_consent,
        is_subscribed_to_changelog: row.is_subscribed_to_changelog,
        product_tour: row.product_tour,
        settings: row.settings,
        user: row.user,
    }
}

/// An `Account` row for [`account_to_representation`]
/// (`db/models/user.py:280-304`): the `*_expired_at` datetimes and
/// `refresh_token` (`null=True`) render `null` when unset; `user` is a
/// non-nullable FK rendering as a UUID string; datetimes are pre-rendered
/// DRF strings.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub provider_account_id: &'a str,
    pub provider: &'a str,
    pub access_token: &'a str,
    pub access_token_expired_at: Option<&'a str>,
    pub refresh_token: Option<&'a str>,
    pub refresh_token_expired_at: Option<&'a str>,
    pub last_connected_at: &'a str,
    pub id_token: &'a str,
    pub metadata: &'a serde_json::Value,
    pub user: &'a str,
}

/// `AccountSerializer.to_representation` output (`user.py:214-218`), in
/// wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AccountView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub provider_account_id: &'a str,
    pub provider: &'a str,
    pub access_token: &'a str,
    pub access_token_expired_at: Option<&'a str>,
    pub refresh_token: Option<&'a str>,
    pub refresh_token_expired_at: Option<&'a str>,
    pub last_connected_at: &'a str,
    pub id_token: &'a str,
    pub metadata: &'a serde_json::Value,
    pub user: &'a str,
}

/// Port of `AccountSerializer` (`user.py:214-218`) read shape.
pub fn account_to_representation<'a>(row: &'a AccountRow<'a>) -> AccountView<'a> {
    AccountView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        provider_account_id: row.provider_account_id,
        provider: row.provider,
        access_token: row.access_token,
        access_token_expired_at: row.access_token_expired_at,
        refresh_token: row.refresh_token,
        refresh_token_expired_at: row.refresh_token_expired_at,
        last_connected_at: row.last_connected_at,
        id_token: row.id_token,
        metadata: row.metadata,
        user: row.user,
    }
}

/// An `APIToken` row for [`api_token_to_representation`]
/// (`db/models/api.py:35-60`): `last_used` / `expired_at`
/// (`DateTimeField(null=True)`), `workspace` (`ForeignKey(null=True)`)
/// and the nullable `created_by` / `updated_by` / `deleted_at` audit
/// columns render `null` when unset; `user_type` is a
/// `PositiveSmallIntegerField`; `user` is a non-nullable FK rendering as
/// a UUID string; datetimes are pre-rendered DRF strings.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiTokenRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub label: &'a str,
    pub description: &'a str,
    pub is_active: bool,
    pub last_used: Option<&'a str>,
    pub token: &'a str,
    pub user_type: i32,
    pub expired_at: Option<&'a str>,
    pub is_service: bool,
    pub allowed_rate_limit: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: &'a str,
    pub workspace: Option<&'a str>,
}

/// `APITokenSerializer.to_representation` output (`api.py:11-25`), in wire
/// order — the POST-create shape that carries the `token` value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApiTokenView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub label: &'a str,
    pub description: &'a str,
    pub is_active: bool,
    pub last_used: Option<&'a str>,
    pub token: &'a str,
    pub user_type: i32,
    pub expired_at: Option<&'a str>,
    pub is_service: bool,
    pub allowed_rate_limit: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: &'a str,
    pub workspace: Option<&'a str>,
}

/// Port of `APITokenSerializer` (`api.py:11-25`) read shape.
pub fn api_token_to_representation<'a>(row: &'a ApiTokenRow<'a>) -> ApiTokenView<'a> {
    ApiTokenView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        label: row.label,
        description: row.description,
        is_active: row.is_active,
        last_used: row.last_used,
        token: row.token,
        user_type: row.user_type,
        expired_at: row.expired_at,
        is_service: row.is_service,
        allowed_rate_limit: row.allowed_rate_limit,
        created_by: row.created_by,
        updated_by: row.updated_by,
        user: row.user,
        workspace: row.workspace,
    }
}

/// Port of `APITokenReadSerializer.get_is_active` (`api.py:35-38`): no
/// expiry means active, otherwise strictly `now < expired_at` (aware
/// datetimes on both sides in Python; `Utc` here). The caller computes
/// this from parsed datetimes and passes it to
/// [`api_token_read_to_representation`].
pub fn api_token_is_active(
    expired_at: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    expired_at.is_none_or(|expiry| now < expiry)
}

/// An `APIToken` row for [`api_token_read_to_representation`]: the same
/// columns as [`ApiTokenRow`] minus the `token` secret (`Meta.exclude`,
/// `api.py:33`). `is_active` is computed, not stored — see
/// [`api_token_is_active`].
#[derive(Debug, Clone, PartialEq)]
pub struct ApiTokenReadRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub label: &'a str,
    pub description: &'a str,
    pub last_used: Option<&'a str>,
    pub user_type: i32,
    pub expired_at: Option<&'a str>,
    pub is_service: bool,
    pub allowed_rate_limit: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: &'a str,
    pub workspace: Option<&'a str>,
}

/// `APITokenReadSerializer.to_representation` output (`api.py:28-38`), in
/// wire order — the GET shape without the `token` value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApiTokenReadView<'a> {
    pub id: &'a str,
    pub is_active: bool,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub label: &'a str,
    pub description: &'a str,
    pub last_used: Option<&'a str>,
    pub user_type: i32,
    pub expired_at: Option<&'a str>,
    pub is_service: bool,
    pub allowed_rate_limit: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: &'a str,
    pub workspace: Option<&'a str>,
}

/// Port of `APITokenReadSerializer` (`api.py:28-38`) read shape.
/// `is_active` is the [`api_token_is_active`] value for this row.
pub fn api_token_read_to_representation<'a>(
    row: &'a ApiTokenReadRow<'a>,
    is_active: bool,
) -> ApiTokenReadView<'a> {
    ApiTokenReadView {
        id: row.id,
        is_active,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        label: row.label,
        description: row.description,
        last_used: row.last_used,
        user_type: row.user_type,
        expired_at: row.expired_at,
        is_service: row.is_service,
        allowed_rate_limit: row.allowed_rate_limit,
        created_by: row.created_by,
        updated_by: row.updated_by,
        user: row.user,
        workspace: row.workspace,
    }
}

/// The entity kinds whose favorites render a nested lite object: the
/// serializer half of `get_entity_model_and_serializer`
/// (`favorite.py:46-56`). `issue` maps to `(Issue, None)`, `folder` to
/// `(None, None)`, and unknown types to `(None, None)` — all render
/// `entity_data: null`, as does a `DoesNotExist` fetch (`:87-88`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FavoriteEntityKind {
    Cycle,
    Module,
    View,
    Page,
    Project,
}

/// Port of `get_entity_model_and_serializer` (`favorite.py:46-56`),
/// reduced to its observable half: `Some(kind)` when the entity type has
/// a lite serializer (the handler fetches the row and renders it),
/// `None` when `entity_data` is unconditionally `null` (`issue`,
/// `folder`, unknown). The model half is unobservable — it is only ever
/// used for the `.get(pk=…)` fetch that a `None` serializer skips.
pub fn favorite_entity_kind(entity_type: &str) -> Option<FavoriteEntityKind> {
    match entity_type {
        "cycle" => Some(FavoriteEntityKind::Cycle),
        "module" => Some(FavoriteEntityKind::Module),
        "view" => Some(FavoriteEntityKind::View),
        "page" => Some(FavoriteEntityKind::Page),
        "project" => Some(FavoriteEntityKind::Project),
        _ => None,
    }
}

/// A `Project` row for [`project_favorite_lite_to_representation`]
/// (`db/models/project.py:72-119`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectFavoriteLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
}

/// `ProjectFavoriteLiteSerializer.to_representation` output
/// (`favorite.py:10-13`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectFavoriteLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
}

/// Port of `ProjectFavoriteLiteSerializer` (`favorite.py:10-13`).
pub fn project_favorite_lite_to_representation<'a>(
    row: &'a ProjectFavoriteLiteRow<'a>,
) -> ProjectFavoriteLiteView<'a> {
    ProjectFavoriteLiteView {
        id: row.id,
        name: row.name,
        logo_props: row.logo_props,
    }
}

/// A `Page` row for [`page_favorite_lite_to_representation`]:
/// `project_id` is the caller-resolved `projects.first()` result
/// (`favorite.py:23-25`), `None` when the page has no project.
#[derive(Debug, Clone, PartialEq)]
pub struct PageFavoriteLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
    pub project_id: Option<&'a str>,
}

/// `PageFavoriteLiteSerializer.to_representation` output
/// (`favorite.py:16-25`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PageFavoriteLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
    pub project_id: Option<&'a str>,
}

/// Port of `PageFavoriteLiteSerializer` (`favorite.py:16-25`).
pub fn page_favorite_lite_to_representation<'a>(
    row: &'a PageFavoriteLiteRow<'a>,
) -> PageFavoriteLiteView<'a> {
    PageFavoriteLiteView {
        id: row.id,
        name: row.name,
        logo_props: row.logo_props,
        project_id: row.project_id,
    }
}

/// A `Cycle` row for [`cycle_favorite_lite_to_representation`]
/// (`db/models/cycle.py:60-76`; `ProjectBaseModel.project` is
/// non-nullable, `db/models/project.py:302-311`, but the attname renders
/// `null` when unset, as on every lite).
#[derive(Debug, Clone, PartialEq)]
pub struct CycleFavoriteLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
    pub project_id: Option<&'a str>,
}

/// `CycleFavoriteLiteSerializer.to_representation` output
/// (`favorite.py:28-31`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CycleFavoriteLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
    pub project_id: Option<&'a str>,
}

/// Port of `CycleFavoriteLiteSerializer` (`favorite.py:28-31`).
pub fn cycle_favorite_lite_to_representation<'a>(
    row: &'a CycleFavoriteLiteRow<'a>,
) -> CycleFavoriteLiteView<'a> {
    CycleFavoriteLiteView {
        id: row.id,
        name: row.name,
        logo_props: row.logo_props,
        project_id: row.project_id,
    }
}

/// A `Module` row for [`module_favorite_lite_to_representation`]
/// (`db/models/module.py:67-99`; same non-nullable `project` note as
/// [`CycleFavoriteLiteRow`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleFavoriteLiteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
    pub project_id: Option<&'a str>,
}

/// `ModuleFavoriteLiteSerializer.to_representation` output
/// (`favorite.py:34-37`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModuleFavoriteLiteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
    pub project_id: Option<&'a str>,
}

/// Port of `ModuleFavoriteLiteSerializer` (`favorite.py:34-37`).
pub fn module_favorite_lite_to_representation<'a>(
    row: &'a ModuleFavoriteLiteRow<'a>,
) -> ModuleFavoriteLiteView<'a> {
    ModuleFavoriteLiteView {
        id: row.id,
        name: row.name,
        logo_props: row.logo_props,
        project_id: row.project_id,
    }
}

/// An `IssueView` row for [`view_favorite_to_representation`]
/// (`db/models/view.py:58-71`; `WorkspaceBaseModel.project` is nullable).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewFavoriteRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
    pub project_id: Option<&'a str>,
}

/// `ViewFavoriteSerializer.to_representation` output
/// (`favorite.py:40-43`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewFavoriteView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a serde_json::Value,
    pub project_id: Option<&'a str>,
}

/// Port of `ViewFavoriteSerializer` (`favorite.py:40-43`).
pub fn view_favorite_to_representation<'a>(row: &'a ViewFavoriteRow<'a>) -> ViewFavoriteView<'a> {
    ViewFavoriteView {
        id: row.id,
        name: row.name,
        logo_props: row.logo_props,
        project_id: row.project_id,
    }
}

/// A `UserFavorite` row for [`user_favorite_to_representation`]
/// (`db/models/favorite.py:14-31`, `WorkspaceBaseModel`)
/// (`db/models/workspace.py:185-189`): `entity_identifier`
/// (`UUIDField(null=True)`), `name` (`CharField(null=True)`), `parent`
/// (self-FK `null=True`) and `project_id` render `null` when unset;
/// `sequence` is a `FloatField` whose wire values are whole magnitudes
/// (`65535 + 10000k`, `db/models/favorite.py:52-65`), where `serde_json`
/// and Python `repr` agree byte for byte (pinned by the replay tests);
/// `workspace_id` is non-nullable;
/// `entity_data` is the already-rendered nested lite value composed by
/// the handler (`None` renders `null` for the issue / folder / unknown /
/// missing arms).
#[derive(Debug, Clone, PartialEq)]
pub struct UserFavoriteRow<'a, E> {
    pub id: &'a str,
    pub entity_type: &'a str,
    pub entity_identifier: Option<&'a str>,
    pub entity_data: Option<E>,
    pub name: Option<&'a str>,
    pub is_folder: bool,
    pub sequence: f64,
    pub parent: Option<&'a str>,
    pub workspace_id: &'a str,
    pub project_id: Option<&'a str>,
}

/// `UserFavoriteSerializer.to_representation` output
/// (`favorite.py:59-89`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserFavoriteView<'a, E> {
    pub id: &'a str,
    pub entity_type: &'a str,
    pub entity_identifier: Option<&'a str>,
    pub entity_data: Option<&'a E>,
    pub name: Option<&'a str>,
    pub is_folder: bool,
    pub sequence: f64,
    pub parent: Option<&'a str>,
    pub workspace_id: &'a str,
    pub project_id: Option<&'a str>,
}

/// Port of `UserFavoriteSerializer` (`favorite.py:59-89`) read shape.
pub fn user_favorite_to_representation<'a, E>(
    row: &'a UserFavoriteRow<'a, E>,
) -> UserFavoriteView<'a, E> {
    UserFavoriteView {
        id: row.id,
        entity_type: row.entity_type,
        entity_identifier: row.entity_identifier,
        entity_data: row.entity_data.as_ref(),
        name: row.name,
        is_folder: row.is_folder,
        sequence: row.sequence,
        parent: row.parent,
        workspace_id: row.workspace_id,
        project_id: row.project_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_workspace/serializers/account_token_fav.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn str_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("string list")
            .iter()
            .map(|entry| entry.as_str().expect("str entry").to_string())
            .collect()
    }

    fn find_case<'a>(golden: &'a Value, name: &str) -> &'a Value {
        golden["cases"]
            .as_array()
            .expect("cases array")
            .iter()
            .find(|case| case["name"] == name)
            .unwrap_or_else(|| panic!("fixture lacks case {name}"))
    }

    fn utc(moment: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(moment)
            .expect("rfc3339 parses")
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn fixture_lists_match_consts() {
        // Every field/read_only/exclude list in F-W24-05 passes against
        // this module's consts.
        let golden = fixture();
        let serializers = &golden["serializers"];
        assert_eq!(
            str_list(&serializers["ProfileSerializer"]["read_only"]),
            PROFILE_READ_ONLY_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            str_list(&serializers["AccountSerializer"]["read_only"]),
            ACCOUNT_READ_ONLY_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            str_list(&serializers["APITokenSerializer"]["read_only"]),
            API_TOKEN_READ_ONLY_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            str_list(&serializers["APITokenReadSerializer"]["exclude"]),
            vec!["token".to_string()],
            "the only Meta.exclude in the family (api.py:33)"
        );
        assert_eq!(
            str_list(&serializers["UserFavoriteSerializer"]["fields"]),
            USER_FAVORITE_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            str_list(&serializers["UserFavoriteSerializer"]["read_only"]),
            USER_FAVORITE_READ_ONLY_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            "verbatim incl. the silently-ignored 'workspace' (favorite.py:76)"
        );
        for (name, wire) in [
            (
                "ProjectFavoriteLiteSerializer",
                PROJECT_FAVORITE_WIRE_FIELDS.to_vec(),
            ),
            (
                "PageFavoriteLiteSerializer",
                PAGE_FAVORITE_WIRE_FIELDS.to_vec(),
            ),
            (
                "CycleFavoriteLiteSerializer",
                CYCLE_FAVORITE_WIRE_FIELDS.to_vec(),
            ),
            (
                "ModuleFavoriteLiteSerializer",
                MODULE_FAVORITE_WIRE_FIELDS.to_vec(),
            ),
            ("ViewFavoriteSerializer", VIEW_FAVORITE_WIRE_FIELDS.to_vec()),
        ] {
            assert_eq!(
                str_list(&serializers[name]["fields"]),
                wire.iter().map(|key| key.to_string()).collect::<Vec<_>>(),
                "{name} fields"
            );
        }
    }

    #[test]
    fn fixture_cases_pass() {
        // The `cases` array of F-W24-05, executed against the kernels.
        let golden = fixture();
        let now = utc("2026-06-01T00:00:00Z");
        assert_eq!(
            find_case(&golden, "is_active null expiry")["output"],
            Value::Bool(true)
        );
        assert!(api_token_is_active(None, now));
        assert_eq!(
            find_case(&golden, "is_active past expiry")["output"],
            Value::Bool(false)
        );
        assert!(!api_token_is_active(Some(utc("2000-01-01T00:00:00Z")), now));
        let dispatch = &find_case(&golden, "fav dispatch")["output_map"];
        assert_eq!(dispatch["cycle"], "(Cycle,CycleFavoriteLiteSerializer)");
        assert_eq!(dispatch["issue"], "(Issue,None)->None");
        assert_eq!(dispatch["module"], "(Module,ModuleFavoriteLiteSerializer)");
        assert_eq!(dispatch["view"], "(IssueView,ViewFavoriteSerializer)");
        assert_eq!(dispatch["page"], "(Page,PageFavoriteLiteSerializer)");
        assert_eq!(
            dispatch["project"],
            "(Project,ProjectFavoriteLiteSerializer)"
        );
        assert_eq!(dispatch["folder"], "(None,None)->None");
        assert_eq!(
            favorite_entity_kind("cycle"),
            Some(FavoriteEntityKind::Cycle)
        );
        assert_eq!(favorite_entity_kind("issue"), None);
        assert_eq!(
            favorite_entity_kind("module"),
            Some(FavoriteEntityKind::Module)
        );
        assert_eq!(favorite_entity_kind("view"), Some(FavoriteEntityKind::View));
        assert_eq!(favorite_entity_kind("page"), Some(FavoriteEntityKind::Page));
        assert_eq!(
            favorite_entity_kind("project"),
            Some(FavoriteEntityKind::Project)
        );
        assert_eq!(favorite_entity_kind("folder"), None);
        assert_eq!(favorite_entity_kind("nope"), None);
    }

    #[test]
    fn wire_fields_match_live_serializer_order() {
        // Pinned against `list(Serializer().fields)` from the live Django
        // serializers (repo probe, Django 4.2.30 / DRF 3.15.2).
        assert_eq!(
            PROFILE_WIRE_FIELDS.to_vec(),
            [
                "id",
                "created_at",
                "updated_at",
                "theme",
                "is_app_rail_docked",
                "is_tour_completed",
                "onboarding_step",
                "use_case",
                "role",
                "is_onboarded",
                "last_workspace_id",
                "billing_address_country",
                "billing_address",
                "has_billing_address",
                "company_name",
                "notification_view_mode",
                "is_smooth_cursor_enabled",
                "is_mobile_onboarded",
                "mobile_onboarding_step",
                "mobile_timezone_auto_set",
                "language",
                "start_of_the_week",
                "goals",
                "background_color",
                "is_navigation_tour_completed",
                "has_marketing_email_consent",
                "is_subscribed_to_changelog",
                "product_tour",
                "settings",
                "user",
            ]
        );
        assert_eq!(
            ACCOUNT_WIRE_FIELDS.to_vec(),
            [
                "id",
                "created_at",
                "updated_at",
                "provider_account_id",
                "provider",
                "access_token",
                "access_token_expired_at",
                "refresh_token",
                "refresh_token_expired_at",
                "last_connected_at",
                "id_token",
                "metadata",
                "user",
            ]
        );
        assert_eq!(
            API_TOKEN_WIRE_FIELDS.to_vec(),
            [
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "label",
                "description",
                "is_active",
                "last_used",
                "token",
                "user_type",
                "expired_at",
                "is_service",
                "allowed_rate_limit",
                "created_by",
                "updated_by",
                "user",
                "workspace",
            ]
        );
        assert_eq!(
            API_TOKEN_READ_WIRE_FIELDS.to_vec(),
            [
                "id",
                "is_active",
                "created_at",
                "updated_at",
                "deleted_at",
                "label",
                "description",
                "last_used",
                "user_type",
                "expired_at",
                "is_service",
                "allowed_rate_limit",
                "created_by",
                "updated_by",
                "user",
                "workspace",
            ]
        );
    }

    #[test]
    fn is_active_edges_follow_strict_less_than() {
        // `get_is_active` (api.py:35-38) is a strict `now < expired_at`:
        // equal instants are expired (one line of Python, pinned here).
        let now = utc("2026-06-01T00:00:00Z");
        assert!(api_token_is_active(None, now));
        assert!(api_token_is_active(Some(utc("2100-01-01T00:00:00Z")), now));
        assert!(!api_token_is_active(Some(utc("2000-01-01T00:00:00Z")), now));
        assert!(!api_token_is_active(Some(now), now));
    }

    /// Live DRF bytes below: captured from the real serializers via
    /// DRF's `JSONRenderer` (repo probe, Django 4.2.30 / DRF 3.15.2,
    /// unsaved model instances, no DB — `{"a": [1, "x\"y"]}` and
    /// `väl`/`död` stress escaping/unicode; the ORM-touching arms —
    /// `projects.first()`, the `entity_model.objects.get(pk=…)` fetch —
    /// run against `mock.patch`ed fetches with the composition
    /// `entity_serializer(entity).data` executing for real).
    const PROFILE_FULL: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000001","created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","theme":{"mode":"dark"},"is_app_rail_docked":true,"is_tour_completed":false,"onboarding_step":{"step":3},"use_case":"plan väl","role":"Lead \"x\"","is_onboarded":true,"last_workspace_id":"cccccccc-0000-4000-8000-000000000001","billing_address_country":"INDIA","billing_address":{"city":"Blr"},"has_billing_address":true,"company_name":"Acme","notification_view_mode":"compact","is_smooth_cursor_enabled":false,"is_mobile_onboarded":true,"mobile_onboarding_step":{"m":1},"mobile_timezone_auto_set":false,"language":"en","start_of_the_week":1,"goals":{"g":["a","x\"y"]},"background_color":"#FF0000","is_navigation_tour_completed":false,"has_marketing_email_consent":true,"is_subscribed_to_changelog":false,"product_tour":{"t":true},"settings":{"ns":{"k":"v"}},"user":"bbbbbbbb-0000-4000-8000-000000000001"}"##;
    const PROFILE_NULL: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000001","created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","theme":{"mode":"dark"},"is_app_rail_docked":true,"is_tour_completed":false,"onboarding_step":{"step":3},"use_case":null,"role":null,"is_onboarded":true,"last_workspace_id":null,"billing_address_country":"INDIA","billing_address":null,"has_billing_address":true,"company_name":"Acme","notification_view_mode":"compact","is_smooth_cursor_enabled":false,"is_mobile_onboarded":true,"mobile_onboarding_step":{"m":1},"mobile_timezone_auto_set":false,"language":"en","start_of_the_week":1,"goals":{"g":["a","x\"y"]},"background_color":"#FF0000","is_navigation_tour_completed":false,"has_marketing_email_consent":true,"is_subscribed_to_changelog":false,"product_tour":{"t":true},"settings":{"ns":{"k":"v"}},"user":"bbbbbbbb-0000-4000-8000-000000000001"}"##;
    const ACCOUNT_FULL: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000002","created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","provider_account_id":"gh-123","provider":"github","access_token":"tok-abc","access_token_expired_at":"2027-05-01T00:00:00Z","refresh_token":"ref-abc","refresh_token_expired_at":"2027-06-01T09:30:00Z","last_connected_at":"2026-01-10T03:04:05Z","id_token":"idt","metadata":{"a":[1,"x\"y"]},"user":"bbbbbbbb-0000-4000-8000-000000000001"}"##;
    const ACCOUNT_NULL: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000002","created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","provider_account_id":"gh-123","provider":"github","access_token":"tok-abc","access_token_expired_at":null,"refresh_token":null,"refresh_token_expired_at":null,"last_connected_at":"2026-01-10T03:04:05Z","id_token":"idt","metadata":{"a":[1,"x\"y"]},"user":"bbbbbbbb-0000-4000-8000-000000000001"}"##;
    const TOKEN_FULL: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000003","created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"label":"ci-token","description":"död","is_active":true,"last_used":"2026-02-01T10:00:00Z","token":"pi_dash_api_secret","user_type":1,"expired_at":"2100-01-01T00:00:00Z","is_service":false,"allowed_rate_limit":"60/min","created_by":null,"updated_by":"dddddddd-0000-4000-8000-000000000001","user":"bbbbbbbb-0000-4000-8000-000000000001","workspace":"cccccccc-0000-4000-8000-000000000001"}"##;
    const TOKEN_RD_FUTURE: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000003","is_active":true,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"label":"ci-token","description":"död","last_used":"2026-02-01T10:00:00Z","user_type":1,"expired_at":"2100-01-01T00:00:00Z","is_service":false,"allowed_rate_limit":"60/min","created_by":null,"updated_by":"dddddddd-0000-4000-8000-000000000001","user":"bbbbbbbb-0000-4000-8000-000000000001","workspace":"cccccccc-0000-4000-8000-000000000001"}"##;
    const TOKEN_RD_NULL: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000003","is_active":true,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"label":"ci-token","description":"död","last_used":"2026-02-01T10:00:00Z","user_type":1,"expired_at":null,"is_service":false,"allowed_rate_limit":"60/min","created_by":null,"updated_by":"dddddddd-0000-4000-8000-000000000001","user":"bbbbbbbb-0000-4000-8000-000000000001","workspace":"cccccccc-0000-4000-8000-000000000001"}"##;
    const TOKEN_RD_PAST: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000003","is_active":false,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"label":"ci-token","description":"död","last_used":"2026-02-01T10:00:00Z","user_type":1,"expired_at":"2000-01-01T00:00:00Z","is_service":false,"allowed_rate_limit":"60/min","created_by":null,"updated_by":"dddddddd-0000-4000-8000-000000000001","user":"bbbbbbbb-0000-4000-8000-000000000001","workspace":"cccccccc-0000-4000-8000-000000000001"}"##;
    const TOKEN_RD_PAST_NULL: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000003","is_active":false,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"label":"ci-token","description":"död","last_used":null,"user_type":1,"expired_at":"2000-01-01T00:00:00Z","is_service":false,"allowed_rate_limit":"60/min","created_by":null,"updated_by":"dddddddd-0000-4000-8000-000000000001","user":"bbbbbbbb-0000-4000-8000-000000000001","workspace":null}"##;
    const FAV_ISSUE: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000004","entity_type":"issue","entity_identifier":"eeeeeeee-0000-4000-8000-000000000001","entity_data":null,"name":"my fav","is_folder":false,"sequence":75535.0,"parent":null,"workspace_id":"cccccccc-0000-4000-8000-000000000001","project_id":"ffffffff-0000-4000-8000-000000000001"}"##;
    const FAV_FOLDER: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000005","entity_type":"folder","entity_identifier":null,"entity_data":null,"name":null,"is_folder":true,"sequence":65535.0,"parent":"aaaaaaaa-0000-4000-8000-000000000004","workspace_id":"cccccccc-0000-4000-8000-000000000001","project_id":null}"##;
    const LITE_PRJ: &str = r##"{"id":"ffffffff-0000-4000-8000-000000000001","name":"Proj väl","logo_props":{"i":"x\"y"}}"##;
    const LITE_CYC: &str = r##"{"id":"11111111-0000-4000-8000-000000000001","name":"C1","logo_props":{},"project_id":"ffffffff-0000-4000-8000-000000000001"}"##;
    const LITE_MOD_NULLPRJ: &str = r##"{"id":"22222222-0000-4000-8000-000000000001","name":"M1","logo_props":{"a":1},"project_id":null}"##;
    const LITE_VIW: &str = r##"{"id":"33333333-0000-4000-8000-000000000001","name":"V1","logo_props":{},"project_id":"ffffffff-0000-4000-8000-000000000001"}"##;
    const LITE_PAG_SOME: &str = r##"{"id":"44444444-0000-4000-8000-000000000001","name":"P1","logo_props":{"p":true},"project_id":"ffffffff-0000-4000-8000-000000000001"}"##;
    const LITE_PAG_NONE: &str = r##"{"id":"44444444-0000-4000-8000-000000000001","name":"P1","logo_props":{"p":true},"project_id":null}"##;
    const FAV_PROJECT: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000010","entity_type":"project","entity_identifier":"ffffffff-0000-4000-8000-000000000001","entity_data":{"id":"ffffffff-0000-4000-8000-000000000001","name":"Proj väl","logo_props":{"i":"x\"y"}},"name":"pf","is_folder":false,"sequence":85535.0,"parent":null,"workspace_id":"cccccccc-0000-4000-8000-000000000001","project_id":"ffffffff-0000-4000-8000-000000000001"}"##;
    const FAV_MISSING: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000010","entity_type":"cycle","entity_identifier":"99999999-0000-4000-8000-000000000001","entity_data":null,"name":"pf","is_folder":false,"sequence":85535.0,"parent":null,"workspace_id":"cccccccc-0000-4000-8000-000000000001","project_id":"ffffffff-0000-4000-8000-000000000001"}"##;
    const FAV_PAGE: &str = r##"{"id":"aaaaaaaa-0000-4000-8000-000000000010","entity_type":"page","entity_identifier":"44444444-0000-4000-8000-000000000001","entity_data":{"id":"44444444-0000-4000-8000-000000000001","name":"P1","logo_props":{"p":true},"project_id":"ffffffff-0000-4000-8000-000000000001"},"name":"pf","is_folder":false,"sequence":85535.0,"parent":null,"workspace_id":"cccccccc-0000-4000-8000-000000000001","project_id":"ffffffff-0000-4000-8000-000000000001"}"##;

    fn profile_row<'a>(
        theme: &'a Value,
        onboarding_step: &'a Value,
        billing_address: &'a Value,
        mobile_onboarding_step: &'a Value,
        goals: &'a Value,
        product_tour: &'a Value,
        settings: &'a Value,
    ) -> ProfileRow<'a> {
        ProfileRow {
            id: "aaaaaaaa-0000-4000-8000-000000000001",
            created_at: "2026-01-15T12:30:45.123456Z",
            updated_at: "2026-01-16T08:05:04Z",
            theme,
            is_app_rail_docked: true,
            is_tour_completed: false,
            onboarding_step,
            use_case: Some("plan väl"),
            role: Some("Lead \"x\""),
            is_onboarded: true,
            last_workspace_id: Some("cccccccc-0000-4000-8000-000000000001"),
            billing_address_country: "INDIA",
            billing_address: Some(billing_address),
            has_billing_address: true,
            company_name: "Acme",
            notification_view_mode: "compact",
            is_smooth_cursor_enabled: false,
            is_mobile_onboarded: true,
            mobile_onboarding_step,
            mobile_timezone_auto_set: false,
            language: "en",
            start_of_the_week: 1,
            goals,
            background_color: "#FF0000",
            is_navigation_tour_completed: false,
            has_marketing_email_consent: true,
            is_subscribed_to_changelog: false,
            product_tour,
            settings,
            user: "bbbbbbbb-0000-4000-8000-000000000001",
        }
    }

    #[test]
    fn profile_replays_live_bytes() {
        let theme = json!({"mode": "dark"});
        let onboarding_step = json!({"step": 3});
        let billing_address = json!({"city": "Blr"});
        let mobile_onboarding_step = json!({"m": 1});
        let goals = json!({"g": ["a", "x\"y"]});
        let product_tour = json!({"t": true});
        let settings = json!({"ns": {"k": "v"}});
        let row = profile_row(
            &theme,
            &onboarding_step,
            &billing_address,
            &mobile_onboarding_step,
            &goals,
            &product_tour,
            &settings,
        );
        assert_eq!(
            serde_json::to_string(&profile_to_representation(&row)).expect("serializes"),
            PROFILE_FULL,
        );
        let mut nulls = row.clone();
        nulls.use_case = None;
        nulls.role = None;
        nulls.last_workspace_id = None;
        nulls.billing_address = None;
        assert_eq!(
            serde_json::to_string(&profile_to_representation(&nulls)).expect("serializes"),
            PROFILE_NULL,
        );
    }

    #[test]
    fn account_replays_live_bytes() {
        let metadata = json!({"a": [1, "x\"y"]});
        let row = AccountRow {
            id: "aaaaaaaa-0000-4000-8000-000000000002",
            created_at: "2026-01-15T12:30:45.123456Z",
            updated_at: "2026-01-16T08:05:04Z",
            provider_account_id: "gh-123",
            provider: "github",
            access_token: "tok-abc",
            access_token_expired_at: Some("2027-05-01T00:00:00Z"),
            refresh_token: Some("ref-abc"),
            refresh_token_expired_at: Some("2027-06-01T09:30:00Z"),
            last_connected_at: "2026-01-10T03:04:05Z",
            id_token: "idt",
            metadata: &metadata,
            user: "bbbbbbbb-0000-4000-8000-000000000001",
        };
        assert_eq!(
            serde_json::to_string(&account_to_representation(&row)).expect("serializes"),
            ACCOUNT_FULL,
        );
        let mut nulls = row.clone();
        nulls.access_token_expired_at = None;
        nulls.refresh_token = None;
        nulls.refresh_token_expired_at = None;
        assert_eq!(
            serde_json::to_string(&account_to_representation(&nulls)).expect("serializes"),
            ACCOUNT_NULL,
        );
    }

    fn token_row() -> ApiTokenRow<'static> {
        ApiTokenRow {
            id: "aaaaaaaa-0000-4000-8000-000000000003",
            created_at: "2026-01-15T12:30:45.123456Z",
            updated_at: "2026-01-16T08:05:04Z",
            deleted_at: None,
            label: "ci-token",
            description: "död",
            is_active: true,
            last_used: Some("2026-02-01T10:00:00Z"),
            token: "pi_dash_api_secret",
            user_type: 1,
            expired_at: Some("2100-01-01T00:00:00Z"),
            is_service: false,
            allowed_rate_limit: "60/min",
            created_by: None,
            updated_by: Some("dddddddd-0000-4000-8000-000000000001"),
            user: "bbbbbbbb-0000-4000-8000-000000000001",
            workspace: Some("cccccccc-0000-4000-8000-000000000001"),
        }
    }

    fn token_read_row<'a>(row: &'a ApiTokenRow<'a>) -> ApiTokenReadRow<'a> {
        ApiTokenReadRow {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
            label: row.label,
            description: row.description,
            last_used: row.last_used,
            user_type: row.user_type,
            expired_at: row.expired_at,
            is_service: row.is_service,
            allowed_rate_limit: row.allowed_rate_limit,
            created_by: row.created_by,
            updated_by: row.updated_by,
            user: row.user,
            workspace: row.workspace,
        }
    }

    #[test]
    fn token_create_shape_replays_live_bytes() {
        // The POST-create shape carries the secret (views/api.py:37-39).
        let row = token_row();
        assert_eq!(
            serde_json::to_string(&api_token_to_representation(&row)).expect("serializes"),
            TOKEN_FULL,
        );
    }

    #[test]
    fn token_read_shape_replays_live_bytes() {
        // `is_active` runs through the kernel from the same instants the
        // row strings denote, so kernel and capture agree by construction.
        let now = utc("2026-06-01T00:00:00Z");
        let row = token_row();
        let future = utc("2100-01-01T00:00:00Z");
        let read = token_read_row(&row);
        assert_eq!(
            serde_json::to_string(&api_token_read_to_representation(
                &read,
                api_token_is_active(Some(future), now)
            ))
            .expect("serializes"),
            TOKEN_RD_FUTURE,
        );
        let mut null_expiry = row.clone();
        null_expiry.expired_at = None;
        let read = token_read_row(&null_expiry);
        assert_eq!(
            serde_json::to_string(&api_token_read_to_representation(
                &read,
                api_token_is_active(None, now)
            ))
            .expect("serializes"),
            TOKEN_RD_NULL,
        );
        let mut past_expiry = row.clone();
        past_expiry.expired_at = Some("2000-01-01T00:00:00Z");
        let past = utc("2000-01-01T00:00:00Z");
        let read = token_read_row(&past_expiry);
        assert_eq!(
            serde_json::to_string(&api_token_read_to_representation(
                &read,
                api_token_is_active(Some(past), now)
            ))
            .expect("serializes"),
            TOKEN_RD_PAST,
        );
        past_expiry.last_used = None;
        past_expiry.workspace = None;
        let read = token_read_row(&past_expiry);
        assert_eq!(
            serde_json::to_string(&api_token_read_to_representation(
                &read,
                api_token_is_active(Some(past), now)
            ))
            .expect("serializes"),
            TOKEN_RD_PAST_NULL,
        );
    }

    #[test]
    fn favorite_lites_replay_live_bytes() {
        let prj_logo = json!({"i": "x\"y"});
        let prj_row = ProjectFavoriteLiteRow {
            id: "ffffffff-0000-4000-8000-000000000001",
            name: "Proj väl",
            logo_props: &prj_logo,
        };
        assert_eq!(
            serde_json::to_string(&project_favorite_lite_to_representation(&prj_row))
                .expect("serializes"),
            LITE_PRJ,
        );
        let empty = json!({});
        let cyc_row = CycleFavoriteLiteRow {
            id: "11111111-0000-4000-8000-000000000001",
            name: "C1",
            logo_props: &empty,
            project_id: Some("ffffffff-0000-4000-8000-000000000001"),
        };
        assert_eq!(
            serde_json::to_string(&cycle_favorite_lite_to_representation(&cyc_row))
                .expect("serializes"),
            LITE_CYC,
        );
        let mod_logo = json!({"a": 1});
        let mod_row = ModuleFavoriteLiteRow {
            id: "22222222-0000-4000-8000-000000000001",
            name: "M1",
            logo_props: &mod_logo,
            project_id: None,
        };
        assert_eq!(
            serde_json::to_string(&module_favorite_lite_to_representation(&mod_row))
                .expect("serializes"),
            LITE_MOD_NULLPRJ,
        );
        let viw_row = ViewFavoriteRow {
            id: "33333333-0000-4000-8000-000000000001",
            name: "V1",
            logo_props: &empty,
            project_id: Some("ffffffff-0000-4000-8000-000000000001"),
        };
        assert_eq!(
            serde_json::to_string(&view_favorite_to_representation(&viw_row)).expect("serializes"),
            LITE_VIW,
        );
        let pag_logo = json!({"p": true});
        let pag_row = PageFavoriteLiteRow {
            id: "44444444-0000-4000-8000-000000000001",
            name: "P1",
            logo_props: &pag_logo,
            project_id: Some("ffffffff-0000-4000-8000-000000000001"),
        };
        assert_eq!(
            serde_json::to_string(&page_favorite_lite_to_representation(&pag_row))
                .expect("serializes"),
            LITE_PAG_SOME,
        );
        let pag_none = PageFavoriteLiteRow {
            project_id: None,
            ..pag_row
        };
        assert_eq!(
            serde_json::to_string(&page_favorite_lite_to_representation(&pag_none))
                .expect("serializes"),
            LITE_PAG_NONE,
        );
    }

    #[test]
    fn favorite_null_arms_replay_live_bytes() {
        // `issue` always renders null (dispatch `(Issue, None)`), as do
        // `folder`, unknown types, and missing rows — no DB involved.
        let row: UserFavoriteRow<'_, Value> = UserFavoriteRow {
            id: "aaaaaaaa-0000-4000-8000-000000000004",
            entity_type: "issue",
            entity_identifier: Some("eeeeeeee-0000-4000-8000-000000000001"),
            entity_data: None,
            name: Some("my fav"),
            is_folder: false,
            sequence: 75535.0,
            parent: None,
            workspace_id: "cccccccc-0000-4000-8000-000000000001",
            project_id: Some("ffffffff-0000-4000-8000-000000000001"),
        };
        assert_eq!(
            serde_json::to_string(&user_favorite_to_representation(&row)).expect("serializes"),
            FAV_ISSUE,
        );
        let folder: UserFavoriteRow<'_, Value> = UserFavoriteRow {
            id: "aaaaaaaa-0000-4000-8000-000000000005",
            entity_type: "folder",
            entity_identifier: None,
            entity_data: None,
            name: None,
            is_folder: true,
            sequence: 65535.0,
            parent: Some("aaaaaaaa-0000-4000-8000-000000000004"),
            workspace_id: "cccccccc-0000-4000-8000-000000000001",
            project_id: None,
        };
        assert_eq!(
            serde_json::to_string(&user_favorite_to_representation(&folder)).expect("serializes"),
            FAV_FOLDER,
        );
        let missing: UserFavoriteRow<'_, CycleFavoriteLiteView<'_>> = UserFavoriteRow {
            id: "aaaaaaaa-0000-4000-8000-000000000010",
            entity_type: "cycle",
            entity_identifier: Some("99999999-0000-4000-8000-000000000001"),
            entity_data: None,
            name: Some("pf"),
            is_folder: false,
            sequence: 85535.0,
            parent: None,
            workspace_id: "cccccccc-0000-4000-8000-000000000001",
            project_id: Some("ffffffff-0000-4000-8000-000000000001"),
        };
        assert_eq!(
            serde_json::to_string(&user_favorite_to_representation(&missing)).expect("serializes"),
            FAV_MISSING,
        );
    }

    #[test]
    fn favorite_nested_arms_replay_live_bytes() {
        // The composition `entity_serializer(entity).data`
        // (favorite.py:86) with handler-composed lite values.
        let prj_logo = json!({"i": "x\"y"});
        let prj_row = ProjectFavoriteLiteRow {
            id: "ffffffff-0000-4000-8000-000000000001",
            name: "Proj väl",
            logo_props: &prj_logo,
        };
        let lite = project_favorite_lite_to_representation(&prj_row);
        let row = UserFavoriteRow {
            id: "aaaaaaaa-0000-4000-8000-000000000010",
            entity_type: "project",
            entity_identifier: Some("ffffffff-0000-4000-8000-000000000001"),
            entity_data: Some(lite),
            name: Some("pf"),
            is_folder: false,
            sequence: 85535.0,
            parent: None,
            workspace_id: "cccccccc-0000-4000-8000-000000000001",
            project_id: Some("ffffffff-0000-4000-8000-000000000001"),
        };
        assert_eq!(
            serde_json::to_string(&user_favorite_to_representation(&row)).expect("serializes"),
            FAV_PROJECT,
        );
        let pag_logo = json!({"p": true});
        let pag_row = PageFavoriteLiteRow {
            id: "44444444-0000-4000-8000-000000000001",
            name: "P1",
            logo_props: &pag_logo,
            project_id: Some("ffffffff-0000-4000-8000-000000000001"),
        };
        let page_lite = page_favorite_lite_to_representation(&pag_row);
        let page_row = UserFavoriteRow {
            id: "aaaaaaaa-0000-4000-8000-000000000010",
            entity_type: "page",
            entity_identifier: Some("44444444-0000-4000-8000-000000000001"),
            entity_data: Some(page_lite),
            name: Some("pf"),
            is_folder: false,
            sequence: 85535.0,
            parent: None,
            workspace_id: "cccccccc-0000-4000-8000-000000000001",
            project_id: Some("ffffffff-0000-4000-8000-000000000001"),
        };
        assert_eq!(
            serde_json::to_string(&user_favorite_to_representation(&page_row)).expect("serializes"),
            FAV_PAGE,
        );
    }
}
