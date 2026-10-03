//! User / profile / account / API-token / favorite table models (D-24, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/user.py:25-304` (`User`, `Profile`,
//! `Account`, the onboarding/tour default factories), `db/models/api.py:16-60`
//! (`APIToken`, the token/label generators) and `db/models/favorite.py:14-69`
//! (`UserFavorite`), adopting the Django-owned schema column-for-column;
//! migrations are not ported — Django stays schema owner until switchover.
//! Fixture source of truth: `rust-api/fixtures/app_workspace/models/`
//! `user_token.columns.json` (F-W24-08, recorded by PIDASHCONV-599) for the
//! first four units and `workspace_prefs.columns.json` (F-W24-07) for
//! `UserFavorite`; the `#[cfg(test)]` suite replays both section by section.
//!
//! Column order in each `DECLARED_COLUMNS` const follows its fixture: the
//! model fields in declaration order with Django attnames for FKs
//! (`avatar_asset_id`, `user_id`, `workspace_id`, `parent_id`, …).
//! Inherited columns live in a separate const per model (`AUDIT_COLUMNS` /
//! `AUTH_COLUMNS` / `INHERITED_COLUMNS`) matching the fixture's `inherited`
//! section: `User` carries its own `id`/`created_at`/`updated_at` plus the
//! `AbstractBaseUser` pair (`password`, `last_login`); `Profile`/`Account`
//! add `TimeAuditModel` (`mixins.py:16-24`); `APIToken` adds the full
//! `BaseModel` set (`base.py:17-21`, `mixins.py:16-82`); `UserFavorite` adds
//! the `BaseModel` set plus `WorkspaceBaseModel` (`workspace.py:185-195`).
//! Every application-level default below is Django-side (the live tables
//! carry no `column_default` in `information_schema`, as established for
//! D-01); Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped where the marker exists
//!
//! Only `APIToken` and `UserFavorite` inherit the soft-delete marker
//! (`deleted_at`, from `SoftDeleteModel` in `pi_dash/db/mixins.py:61-67`);
//! their default manager filters `deleted_at IS NULL` (`objects =
//! SoftDeletionManager`, `mixins.py:66`; `all_objects` is the plain unscoped
//! manager, `mixins.py:67`). `User` (own audit cols, no `deleted_at`),
//! `Profile` and `Account` (`TimeAuditModel` only) have no marker and no
//! scoping. Each submodule pins this in `HAS_DELETED_AT`; every read built
//! from the two marked tables must filter `deleted_at IS NULL`.
//!
//! # Writes
//!
//! `User.save` (`user.py:169-187`) is pure column-value logic plus the
//! `super().save()` flush: the email normalization, the token-regen rule,
//! the display-name fill and the superuser `is_staff` forcing are ported as
//! [`user`] helpers; the flush itself is handler SQL owned by the write
//! layer. `UserFavorite.save` (`favorite.py:52-65`) likewise splits into the
//! pure halves ported here ([`user_favorite::next_sequence`],
//! [`user_favorite::resolve_workspace`]) and the `MAX(sequence)` probe,
//! which is queries-layer SQL. `BaseModel.save` (`base.py:23-44`) resolves
//! `created_by`/`updated_by` from the request user (`crum.get_current_user`)
//! and `SoftDeleteModel.delete` (`mixins.py:72-82`) stamps `deleted_at` and
//! enqueues `soft_delete_related_objects` — both are write-path
//! orchestration over the request context / task queue, owned by the
//! handlers and tasks layers, not ported here. The `post_save`
//! `create_user_notification` receiver (`user.py:307-321`) is recorded in
//! [`user::POST_SAVE_NOTIFICATION_FLAGS`] for the create path to honor.
//!
//! # Ported quirks (translate as-is)
//!
//! `user.py:25-321`, `api.py:16-60` and `favorite.py:14-69` were read line
//! by line for this layer. The following behaviors mistranslate easily and
//! are ported exactly:
//!
//! * `User.save` lower-cases `email` unconditionally (`user.py:170`) but
//!   `email` allows null (`:61`) — saving with `email = None` raises
//!   `AttributeError`. Ported as an explicit error arm
//!   ([`user::SaveError::EmailNone`]), never a silent `None`.
//! * The token regen (`user.py:173-175`) fires on EVERY save once
//!   `token_updated_at` is non-null: it regenerates the token AND re-stamps
//!   `now()`, so the condition never settles. Ported as-is
//!   ([`user::should_regenerate_token`]).
//! * The `save()` display-name random-6 fallback (`user.py:181`) is DEAD:
//!   `len(email.split("@"))` is always `>= 1`, so the truthy branch always
//!   wins and a falsy `display_name` is ALWAYS `email.split("@")[0]`.
//!   Ported as-is ([`user::save_display_name`] has no random arm) — unlike
//!   [`user::get_display_name`], which checks `== 2` (`:195`) and CAN return
//!   random letters.
//! * `bot_type` (`user.py:116`) does NOT enforce `BotTypeEnum`
//!   (`WORKSPACE_SEED`, `:52-54`) as `choices` — free text up to 30 chars.
//!   Ported as a plain max-length const plus the unenforced value.
//! * `Profile.last_workspace_id` (`user.py:236`) is a PLAIN nullable
//!   `UUIDField`, not an FK — no referential integrity. Ported as a
//!   non-FK column (and there is NO `last_workspace_id` on `User` itself;
//!   the last-visited view's `AttributeError` → 500 is a pinned bug the
//!   handlers layer ports).
//! * `Account.provider` (`user.py:290`) declares `choices` with NO
//!   `max_length` (a Django E120 check error); the migration DDL is source
//!   of truth for the column width. Ported as [`account::PROVIDER_CHOICES`]
//!   with no length const.
//! * `APIToken.Meta.verbose_name_plural` is `"API Tokems"` (`api.py:55`),
//!   a typo. Ported verbatim.
//! * `UserFavorite.is_folder` (`favorite.py:23`) is Django-side
//!   `default=False` but the DB column is `NOT NULL` with no DB default —
//!   omitting it violates 23502 (the D-27 favorites 500, PIDASHCONV-509).
//!   Rust inserts must bind [`user_favorite::IS_FOLDER_DEFAULT`] explicitly.
//! * `UserFavorite.save` scopes the sequence `MAX` to the whole workspace,
//!   NOT per-user (`favorite.py:55-61`) — cross-user gaps. Ported as-is.
//! * `WorkspaceBaseModel.save` (`workspace.py:192-195`) overwrites
//!   `workspace` from `project.workspace` whenever `project` is set, even
//!   when `workspace` was explicitly assigned. Ported as-is
//!   ([`user_favorite::resolve_workspace`]).
//!
//! # Out of scope (documented, not ported)
//!
//! * `Session` — D-16 owns (`db/models/session.py:17-30`).
//! * `CLIDeviceCode` (`api.py:63-94`) — D-22 owns.
//! * `APIActivityLog` (`api.py:97-122`) — written by bgtasks only (D-08/D-09).
//! * `Sticky` — reuse `pidash_db::v1_assets::model::sticky::Sticky`
//!   (D-21, merged), the single source of truth; not re-ported here.
//! * `ChangePassword`/`ResetPasswordSerializer` (`app/serializers/user.py:173-199`)
//!   have no view usage — excluded per F-W24-04, noted in TRACE.md.

/// Django-level FK delete behavior (ORM-emulated; same shape as the D-26
/// `app_issues::models_core::OnDelete`, the D-05 `integrations::OnDelete`
/// and the D-22 `v1_cli_auth::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `users` table (`user.py:56-199`).
pub mod user {
    use super::OnDelete;

    /// Physical table (`Meta.db_table`, `user.py:136`).
    pub const TABLE: &str = "users";
    /// Default ordering (`Meta.ordering`, `user.py:137`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`user.py:134`).
    pub const VERBOSE_NAME: &str = "User";
    /// `verbose_name_plural` (`user.py:135`).
    pub const VERBOSE_NAME_PLURAL: &str = "Users";
    /// No soft-delete marker: `User` carries its own `id`/`created_at`/
    /// `updated_at` and does NOT inherit `SoftDeleteModel` — reads are
    /// unscoped. (There is deliberately NO `last_workspace_id` here; it
    /// lives on `Profile`, `user.py:236`.)
    pub const HAS_DELETED_AT: bool = false;

    /// Declared columns in fixture F-W24-08 order (`user.py:57-126` in
    /// declaration order; FK entries use the Django attnames).
    pub const DECLARED_COLUMNS: &[&str] = &[
        "id",
        "username",
        "mobile_number",
        "email",
        "display_name",
        "first_name",
        "last_name",
        "avatar",
        "avatar_asset_id",
        "cover_image",
        "cover_image_asset_id",
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

    /// Inherited auth-framework columns (`AbstractBaseUser`: `password`
    /// `CharField(128)`, `last_login` `DateTime`; NOT in `user.py`).
    /// `is_superuser` is overridden locally (`user.py:94`) so it lives in
    /// [`DECLARED_COLUMNS`]; the `PermissionsMixin` M2Ms
    /// (`groups`, `user_permissions`) materialize through their own join
    /// tables and contribute no columns here.
    pub const AUTH_COLUMNS: &[&str] = &["password", "last_login"];

    /// `avatar_asset` FK: `SET_NULL`, nullable (`user.py:69-75`).
    pub const AVATAR_ASSET_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `cover_image_asset` FK: `SET_NULL`, nullable (`user.py:78-84`).
    pub const COVER_IMAGE_ASSET_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `USERNAME_FIELD` (`user.py:128`).
    pub const USERNAME_FIELD: &str = "email";
    /// `REQUIRED_FIELDS` (`user.py:129`).
    pub const REQUIRED_FIELDS: &[&str] = &["username"];

    /// `display_name` Django-side default (`CharField default=""`, `:64`).
    pub const DISPLAY_NAME_DEFAULT: &str = "";
    /// `last_login_medium` Django-side default (`:110`).
    pub const LAST_LOGIN_MEDIUM_DEFAULT: &str = "email";
    /// `user_timezone` Django-side default (`:120`; choices are the
    /// `pytz.common_timezones` tuple, `:119`).
    pub const USER_TIMEZONE_DEFAULT: &str = "UTC";
    /// `is_active` Django-side default (`:97`) — the only `True` `is_*`.
    pub const IS_ACTIVE_DEFAULT: bool = true;
    /// `bot_type` max length (`CharField max_length=30`, `:116`); the
    /// `BotTypeEnum` value is NOT enforced as `choices` (ported quirk).
    pub const BOT_TYPE_MAX_LENGTH: usize = 30;
    /// The unenforced `BotTypeEnum.WORKSPACE_SEED` value (`:52-54`).
    pub const BOT_TYPE_WORKSPACE_SEED: &str = "WORKSPACE_SEED";
    /// `token` regen shape (`:174`): two `uuid4().hex` concatenated —
    /// 64 lowercase hex chars.
    pub const REGENERATED_TOKEN_HEX_LEN: usize = 64;
    /// `get_display_name` / `save` random fallback alphabet
    /// (`string.ascii_letters`, `:181,:192,:196`) — letters only, no digits.
    pub const DISPLAY_NAME_RANDOM_ALPHABET: &[u8; 52] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    /// Length of the `get_display_name` random fallback (`:192,:196`).
    pub const DISPLAY_NAME_RANDOM_LEN: usize = 6;

    /// `post_save` `create_user_notification` flags (`user.py:307-321`): on
    /// create, and only when NOT `is_bot`, the receiver creates a
    /// `UserNotificationPreference` row with these five flags `True`. The
    /// create path must honor this; `(name, value)` in receiver-call order.
    pub const POST_SAVE_NOTIFICATION_FLAGS: [(&str, bool); 5] = [
        ("property_change", true),
        ("state_change", true),
        ("comment", true),
        ("mention", true),
        ("issue_completed", true),
    ];

    /// Resolve `avatar`-style image URLs (`user.py:142-162`): the asset URL
    /// wins when the FK is set, else the direct value when non-empty, else
    /// `None`. Both `avatar_url` and `cover_image_url` share this shape
    /// (mirroring `Workspace.logo_url`); the two public wrappers name the
    /// field so call sites read like the Python.
    fn resolve_image_url<'a>(
        asset_attached: bool,
        asset_url: Option<&'a str>,
        direct: &'a str,
    ) -> Option<&'a str> {
        // Python checks only the FK's presence (`if self.avatar_asset:`):
        // when attached the asset URL is returned as-is with NO
        // fall-through, even when it maps to no URL (same shape as the
        // merged `resolve_workspace_logo_url` precedent).
        if asset_attached {
            return asset_url;
        }
        if direct.is_empty() {
            return None;
        }
        Some(direct)
    }

    /// `User.avatar_url` (`user.py:142-151`): `avatar_asset.asset_url` when
    /// the FK is set (returned as-is, even if empty or `None` — Python
    /// checks only the FK's presence), else `avatar` when non-empty, else
    /// `None`. The caller resolves `asset_url` from the `FileAsset` row and
    /// passes FK presence separately.
    pub fn avatar_url<'a>(
        avatar_asset_attached: bool,
        avatar_asset_url: Option<&'a str>,
        avatar: &'a str,
    ) -> Option<&'a str> {
        resolve_image_url(avatar_asset_attached, avatar_asset_url, avatar)
    }

    /// `User.cover_image_url` (`user.py:153-162`): same shape as
    /// [`avatar_url`] over `cover_image_asset` / `cover_image`.
    pub fn cover_image_url<'a>(
        cover_image_asset_attached: bool,
        cover_image_asset_url: Option<&'a str>,
        cover_image: Option<&'a str>,
    ) -> Option<&'a str> {
        // `cover_image` is nullable (`:77`) while `avatar` is not (`:68`),
        // so the direct arm takes `Option`; `None` and `""` both fall to
        // `None`, matching `if self.cover_image:`.
        if cover_image_asset_attached {
            return cover_image_asset_url;
        }
        match cover_image {
            Some(value) if !value.is_empty() => Some(value),
            _ => None,
        }
    }

    /// `User.full_name` (`user.py:164-167`):
    /// `f"{first_name} {last_name}".strip()`.
    pub fn full_name(first_name: &str, last_name: &str) -> String {
        format!("{first_name} {last_name}").trim().to_string()
    }

    /// `User.save` email normalization (`user.py:170`):
    /// `email.lower().strip()`.
    ///
    /// `None` is an ERROR, not a silent `None`: `email` allows null
    /// (`:61`) but `save` calls `.lower()` unconditionally, so Python
    /// raises `AttributeError` — ported as [`SaveError::EmailNone`] (the
    /// write layer maps it to the same 500 Django produces).
    pub fn save_email(email: Option<&str>) -> Result<String, SaveError> {
        match email {
            None => Err(SaveError::EmailNone),
            Some(value) => Ok(value.to_lowercase().trim().to_string()),
        }
    }

    /// `User.save` token-regen condition (`user.py:173-175`):
    /// `token_updated_at is not None`. The regen re-stamps `now()`, so once
    /// set it fires on EVERY subsequent save, forever — ported as-is; the
    /// write layer regenerates via [`regenerated_token`] and re-stamps.
    pub fn should_regenerate_token(token_updated_at_is_set: bool) -> bool {
        token_updated_at_is_set
    }

    /// `User.save` token regen value (`user.py:174`):
    /// `uuid.uuid4().hex + uuid.uuid4().hex`.
    pub fn regenerated_token() -> String {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    }

    /// `User.save` display-name fill (`user.py:177-182`): when `display_name`
    /// is falsy it becomes `email.split("@")[0]` — ALWAYS. The `else`
    /// random-6 arm (`:181`) is DEAD (`len(...)` is always `>= 1`) and has
    /// no port; this differs deliberately from [`get_display_name`], whose
    /// `== 2` check (`:195`) CAN fall through to random.
    ///
    /// `email` here is the ALREADY-normalized value (`:170` runs first).
    /// `split("@")[0]` never fails (a split yields at least one item).
    pub fn save_display_name(display_name: &str, normalized_email: &str) -> String {
        if display_name.is_empty() {
            normalized_email.split('@').next().unwrap_or("").to_string()
        } else {
            display_name.to_string()
        }
    }

    /// `User.save` superuser rule (`user.py:184-185`): a superuser is always
    /// staff; otherwise `is_staff` is untouched.
    /// (`mobile_number` `:171` is a self-assign no-op — no port.)
    pub fn save_is_staff(is_superuser: bool, is_staff: bool) -> bool {
        if is_superuser {
            return true;
        }
        is_staff
    }

    /// `User.save` failure modes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SaveError {
        /// `email` was `None` (`:170`): Python raises `AttributeError`
        /// (500). The write layer must surface, never swallow, this arm.
        EmailNone,
    }

    /// `User.get_display_name` decision (`user.py:189-197`): the local part
    /// when `email` splits into exactly two `@` parts, else the random-6
    /// fallback (which also covers `None`/`""`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DisplayNameSource<'a> {
        /// `email.split("@")[0]` — the deterministic arm.
        LocalPart(&'a str),
        /// Six random `ascii_letters` — call [`random_display_name`].
        Random,
    }

    /// Classify `email` per `get_display_name` (`:190-197`) without drawing
    /// randomness, so the deterministic arm is test-pinned exactly.
    pub fn get_display_name_source(email: Option<&str>) -> DisplayNameSource<'_> {
        match email {
            None => DisplayNameSource::Random,
            Some(value) => {
                if value.is_empty() {
                    return DisplayNameSource::Random;
                }
                let parts: Vec<&str> = value.split('@').collect();
                if parts.len() == 2 {
                    DisplayNameSource::LocalPart(parts[0])
                } else {
                    DisplayNameSource::Random
                }
            }
        }
    }

    /// The `get_display_name` random fallback (`:192,:196`): six
    /// `random.choice(string.ascii_letters)`.
    pub fn random_display_name() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        (0..DISPLAY_NAME_RANDOM_LEN)
            .map(|_| {
                DISPLAY_NAME_RANDOM_ALPHABET
                    [rng.random_range(0..DISPLAY_NAME_RANDOM_ALPHABET.len())]
                    as char
            })
            .collect()
    }

    /// `User.get_display_name` classmethod (`user.py:189-197`), resolved:
    /// `""`/`None` → random 6 letters; `"a@b"` (exactly 2 parts) → `"a"`;
    /// anything else → random 6 letters.
    pub fn get_display_name(email: Option<&str>) -> String {
        match get_display_name_source(email) {
            DisplayNameSource::LocalPart(local) => local.to_string(),
            DisplayNameSource::Random => random_display_name(),
        }
    }
}

/// `profiles` table (`user.py:200-279`).
pub mod profile {
    use super::OnDelete;

    /// Physical table (`Meta.db_table`, `user.py:276`).
    pub const TABLE: &str = "profiles";
    /// Default ordering (`Meta.ordering`, `user.py:277`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`user.py:274`).
    pub const VERBOSE_NAME: &str = "Profile";
    /// `verbose_name_plural` (`user.py:275`).
    pub const VERBOSE_NAME_PLURAL: &str = "Profiles";
    /// No soft-delete marker: `Profile` extends `TimeAuditModel` only.
    pub const HAS_DELETED_AT: bool = false;

    /// Declared columns in fixture F-W24-08 order (`user.py:223-271` in
    /// declaration order; the `user` OneToOne uses the Django attname).
    /// `last_workspace_id` (`:236`) is a PLAIN nullable `UUIDField`, not an
    /// FK — no attname mapping, no referential integrity (ported quirk).
    pub const DECLARED_COLUMNS: &[&str] = &[
        "id",
        "user_id",
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
    ];

    /// Inherited audit columns (`TimeAuditModel`, `mixins.py:16-24`).
    pub const AUDIT_COLUMNS: &[&str] = &["created_at", "updated_at"];

    /// `user` OneToOne: `CASCADE` (`user.py:225`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `onboarding_step` keys in factory order
    /// (`get_default_onboarding`, `user.py:25-31`); all `False`.
    pub const ONBOARDING_KEYS: [&str; 4] = [
        "profile_complete",
        "workspace_create",
        "workspace_invite",
        "workspace_join",
    ];
    /// `mobile_onboarding_step` keys in factory order
    /// (`get_mobile_default_onboarding`, `user.py:34-39`); all `False`.
    /// Note the mobile variant LACKS `workspace_invite`.
    pub const MOBILE_ONBOARDING_KEYS: [&str; 3] =
        ["profile_complete", "workspace_create", "workspace_join"];
    /// `product_tour` keys in factory order (`get_default_product_tour`,
    /// `user.py:42-49`); all `False`.
    pub const PRODUCT_TOUR_KEYS: [&str; 5] = ["work_items", "cycles", "modules", "intake", "pages"];

    /// `NotificationViewMode.FULL` (`user.py:209-211`).
    pub const NOTIFICATION_VIEW_MODE_FULL: &str = "full";
    /// `NotificationViewMode.COMPACT` (`user.py:209-211`).
    pub const NOTIFICATION_VIEW_MODE_COMPACT: &str = "compact";
    /// `notification_view_mode` Django-side default (`:242-244`).
    pub const NOTIFICATION_VIEW_MODE_DEFAULT: &str = NOTIFICATION_VIEW_MODE_FULL;

    /// `start_of_the_week` choices, Sunday-first in declaration order
    /// (`START_OF_THE_WEEK_CHOICES`, `user.py:201-221`): `(value, label)`.
    pub const START_OF_THE_WEEK_CHOICES: [(i16, &str); 7] = [
        (0, "Sunday"),
        (1, "Monday"),
        (2, "Tuesday"),
        (3, "Wednesday"),
        (4, "Thursday"),
        (5, "Friday"),
        (6, "Saturday"),
    ];
    /// `start_of_the_week` Django-side default: `SUNDAY` (`:252`).
    pub const START_OF_THE_WEEK_DEFAULT: i16 = 0;

    /// `billing_address_country` Django-side default (`:238`).
    pub const BILLING_ADDRESS_COUNTRY_DEFAULT: &str = "INDIA";
    /// `language` Django-side default (`:251`).
    pub const LANGUAGE_DEFAULT: &str = "en";
    /// `is_app_rail_docked` Django-side default (`:228`).
    pub const IS_APP_RAIL_DOCKED_DEFAULT: bool = true;
    /// `background_color` random shape (`get_random_color`,
    /// `pi_dash/utils/color.py`): `"#"` + 6 `random.choices` from
    /// `string.hexdigits` — MIXED-case `0-9a-fA-F`, with replacement.
    pub const BACKGROUND_COLOR_PREFIX: &str = "#";
    /// `string.hexdigits` verbatim (lowercase run then uppercase run).
    pub const BACKGROUND_COLOR_ALPHABET: &[u8; 22] = b"0123456789abcdefABCDEF";
    /// Hex chars after the `"#"` (`k=6`).
    pub const BACKGROUND_COLOR_HEX_LEN: usize = 6;

    /// `settings` shape note (`user.py:264-271`): `{namespace: {key:
    /// value}}` owned by whatever build is running; recognized namespaces
    /// and defaults come from `pi_dash.ee.settings.user_settings` (empty in
    /// CE); `ProfileEndpoint` rejects anything undeclared. Read through
    /// `get_setting`, never by direct indexing. Django-side default `{}`.
    pub const SETTINGS_DEFAULT_IS_EMPTY_DICT: bool = true;

    /// `onboarding_step` Django-side default
    /// (`get_default_onboarding`, `user.py:25-31`).
    pub fn default_onboarding() -> serde_json::Value {
        serde_json::json!({
            "profile_complete": false,
            "workspace_create": false,
            "workspace_invite": false,
            "workspace_join": false,
        })
    }

    /// `mobile_onboarding_step` Django-side default
    /// (`get_mobile_default_onboarding`, `user.py:34-39`).
    pub fn default_mobile_onboarding() -> serde_json::Value {
        serde_json::json!({
            "profile_complete": false,
            "workspace_create": false,
            "workspace_join": false,
        })
    }

    /// `product_tour` Django-side default (`get_default_product_tour`,
    /// `user.py:42-49`).
    pub fn default_product_tour() -> serde_json::Value {
        serde_json::json!({
            "work_items": false,
            "cycles": false,
            "modules": false,
            "intake": false,
            "pages": false,
        })
    }

    /// `background_color` Django-side default (`get_random_color`,
    /// `pi_dash/utils/color.py`): `"#" + 6 × hexdigits`.
    pub fn random_background_color() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let mut out = String::with_capacity(1 + BACKGROUND_COLOR_HEX_LEN);
        out.push('#');
        for _ in 0..BACKGROUND_COLOR_HEX_LEN {
            out.push(
                BACKGROUND_COLOR_ALPHABET[rng.random_range(0..BACKGROUND_COLOR_ALPHABET.len())]
                    as char,
            );
        }
        out
    }
}

/// `accounts` table (`user.py:280-304`).
pub mod account {
    use super::OnDelete;

    /// Physical table (`Meta.db_table`, `user.py:303`).
    pub const TABLE: &str = "accounts";
    /// Default ordering (`Meta.ordering`, `user.py:304`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`user.py:301`).
    pub const VERBOSE_NAME: &str = "Account";
    /// `verbose_name_plural` (`user.py:302`).
    pub const VERBOSE_NAME_PLURAL: &str = "Accounts";
    /// `unique_together` (`user.py:300`).
    pub const UNIQUE_TOGETHER: [&str; 2] = ["provider", "provider_account_id"];
    /// No soft-delete marker: `Account` extends `TimeAuditModel` only.
    pub const HAS_DELETED_AT: bool = false;

    /// Declared columns in fixture F-W24-08 order (`user.py:287-297` in
    /// declaration order; the `user` FK uses the Django attname).
    pub const DECLARED_COLUMNS: &[&str] = &[
        "id",
        "user_id",
        "provider_account_id",
        "provider",
        "access_token",
        "access_token_expired_at",
        "refresh_token",
        "refresh_token_expired_at",
        "last_connected_at",
        "id_token",
        "metadata",
    ];

    /// Inherited audit columns (`TimeAuditModel`, `mixins.py:16-24`).
    pub const AUDIT_COLUMNS: &[&str] = &["created_at", "updated_at"];

    /// `user` FK: `CASCADE` (`user.py:288`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `PROVIDER_CHOICES` values in declaration order
    /// (`user.py:281-285`): `(value, label)`.
    pub const PROVIDER_CHOICES: [(&str, &str); 3] = [
        ("google", "Google"),
        ("github", "Github"),
        ("gitlab", "GitLab"),
    ];

    /// `provider` (`user.py:290`) declares `choices=PROVIDER_CHOICES` with
    /// NO `max_length` — a Django E120 check error; there is deliberately no
    /// length const here. The migration DDL is source of truth for the
    /// column width. Flagged for the write layer: do not invent a limit.
    pub const PROVIDER_HAS_NO_MAX_LENGTH: bool = true;
}

/// `api_tokens` table (`api.py:35-61`).
pub mod api_token {
    use super::OnDelete;

    /// Physical table (`Meta.db_table`, `api.py:56`).
    pub const TABLE: &str = "api_tokens";
    /// Default ordering (`Meta.ordering`, `api.py:57`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`api.py:54`).
    pub const VERBOSE_NAME: &str = "API Token";
    /// `verbose_name_plural` (`api.py:55`) — `"API Tokems"`, a typo in the
    /// Python, ported verbatim (translation only).
    pub const VERBOSE_NAME_PLURAL: &str = "API Tokems";
    /// Soft-delete marker inherited from `BaseModel`: reads are scoped to
    /// `deleted_at IS NULL`.
    pub const HAS_DELETED_AT: bool = true;

    /// Declared columns in fixture F-W24-08 order (`api.py:37-51` in
    /// declaration order; FK entries use the Django attnames).
    pub const DECLARED_COLUMNS: &[&str] = &[
        "label",
        "description",
        "is_active",
        "last_used",
        "token",
        "user_id",
        "user_type",
        "workspace_id",
        "expired_at",
        "is_service",
        "allowed_rate_limit",
    ];

    /// Inherited audit columns (`BaseModel`, `base.py:17-21` over
    /// `AuditModel`, `mixins.py:85-89`): the UUID pk, the
    /// `TimeAuditModel` pair, the `UserAuditModel` pair (attnames), then
    /// the `SoftDeleteModel` marker.
    pub const AUDIT_COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
    ];

    /// `user` FK: `CASCADE` (`api.py:46`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, nullable (`api.py:48`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `token` value prefix (`generate_token`, `api.py:20-21`).
    pub const TOKEN_PREFIX: &str = "pi_dash_api_";
    /// Hex chars after the prefix: one `uuid4().hex`.
    pub const TOKEN_HEX_LEN: usize = 32;
    /// `label` default shape (`generate_label_token`, `api.py:16-17`): one
    /// `uuid4().hex`.
    pub const LABEL_HEX_LEN: usize = 32;

    /// `user_type` Human value (`api.py:47`).
    pub const USER_TYPE_HUMAN: i16 = 0;
    /// `user_type` Bot value (`api.py:47`).
    pub const USER_TYPE_BOT: i16 = 1;
    /// `user_type` Django-side default (`:47`).
    pub const USER_TYPE_DEFAULT: i16 = USER_TYPE_HUMAN;
    /// `is_active` Django-side default (`:39`).
    pub const IS_ACTIVE_DEFAULT: bool = true;
    /// `is_service` Django-side default (`:50`).
    pub const IS_SERVICE_DEFAULT: bool = false;
    /// `allowed_rate_limit` Django-side default (`:51`).
    pub const ALLOWED_RATE_LIMIT_DEFAULT: &str = "60/min";
    /// `description` Django-side default (`TextField blank=True`, `:38`):
    /// `""` (empty strings allowed, not null).
    pub const DESCRIPTION_DEFAULT: &str = "";

    /// `token` Django-side default (`generate_token`, `api.py:20-21`):
    /// `"pi_dash_api_" + uuid4().hex`.
    pub fn generate_token() -> String {
        format!("{TOKEN_PREFIX}{}", uuid::Uuid::new_v4().simple())
    }

    /// `label` Django-side default (`generate_label_token`, `api.py:16-17`):
    /// `uuid4().hex`.
    pub fn generate_label_token() -> String {
        uuid::Uuid::new_v4().simple().to_string()
    }
}

/// `user_favorites` table (`favorite.py:14-69`).
pub mod user_favorite {
    use super::OnDelete;

    /// Physical table (`Meta.db_table`, `favorite.py:44`).
    pub const TABLE: &str = "user_favorites";
    /// Default ordering (`Meta.ordering`, `favorite.py:45`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`favorite.py:42`).
    pub const VERBOSE_NAME: &str = "User Favorite";
    /// `verbose_name_plural` (`favorite.py:43`).
    pub const VERBOSE_NAME_PLURAL: &str = "User Favorites";
    /// `unique_together` (`favorite.py:34`).
    pub const UNIQUE_TOGETHER: [&str; 4] =
        ["entity_type", "user", "entity_identifier", "deleted_at"];
    /// Partial unique constraint name (`favorite.py:35-41`): on
    /// `(entity_type, entity_identifier, user)` where `deleted_at IS NULL`.
    pub const PARTIAL_UNIQUE_NAME: &str =
        "user_favorite_unique_entity_type_entity_identifier_user_when_deleted_at_null";
    /// Partial unique constraint fields, in declaration order (`:37`).
    pub const PARTIAL_UNIQUE_FIELDS: [&str; 3] = ["entity_type", "entity_identifier", "user"];
    /// `Meta.indexes` names in declaration order (`favorite.py:46-50`).
    pub const INDEX_NAMES: [&str; 3] = [
        "fav_entity_type_idx",
        "fav_entity_identifier_idx",
        "fav_entity_idx",
    ];
    /// Soft-delete marker inherited from `BaseModel` (via
    /// `WorkspaceBaseModel`): reads are scoped to `deleted_at IS NULL`.
    pub const HAS_DELETED_AT: bool = true;

    /// Declared columns in fixture F-W24-07 order (`favorite.py:19-31` in
    /// declaration order; FK entries use the Django attnames).
    pub const DECLARED_COLUMNS: &[&str] = &[
        "user_id",
        "entity_type",
        "entity_identifier",
        "name",
        "is_folder",
        "sequence",
        "parent_id",
    ];

    /// Inherited columns: the `BaseModel` audit set (`base.py:17-21`,
    /// `mixins.py:16-82`) then `WorkspaceBaseModel` (`workspace.py:185-195`,
    /// which declares `workspace` before `project`).
    pub const INHERITED_COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "project_id",
    ];

    /// `user` FK: `CASCADE` (`favorite.py:19`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `parent` self-FK: `CASCADE`, nullable (`favorite.py:25-31`).
    pub const PARENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK (from `WorkspaceBaseModel`, `workspace.py:186`):
    /// `CASCADE`, required.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK (from `WorkspaceBaseModel`, `workspace.py:187`):
    /// `CASCADE`, nullable (null=True, NO blank=True).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `is_folder` Django-side default (`BooleanField default=False`,
    /// `favorite.py:23`). The DB column is `NOT NULL` with NO DB default:
    /// Rust inserts must bind this literal explicitly — omitting it
    /// violates 23502 and the handler maps it to 500 (the D-27 favorites
    /// 500, PIDASHCONV-509).
    pub const IS_FOLDER_DEFAULT: bool = false;
    /// `sequence` Django-side default (`FloatField default=65535`, `:24`).
    pub const SEQUENCE_DEFAULT: f64 = 65535.0;
    /// `save()` sequence step (`:63`): `largest + 10000`.
    pub const SEQUENCE_STEP: f64 = 10000.0;

    /// `UserFavorite.save` sequence rule (`favorite.py:52-65`): on create
    /// only (`_state.adding`), `sequence` becomes the workspace `MAX` plus
    /// [`SEQUENCE_STEP`]; when no rows exist (`largest is None`) the
    /// `default=65535` survives. The `MAX` probe is queries-layer SQL over
    /// the whole workspace — NOT per-user (ported quirk) — resolved via
    /// `project.workspace` when `project` is set, else `self.workspace`;
    /// concurrent adds can collide (no lock — ported quirk).
    pub fn next_sequence(largest: Option<f64>) -> f64 {
        match largest {
            None => SEQUENCE_DEFAULT,
            Some(value) => value + SEQUENCE_STEP,
        }
    }

    /// `WorkspaceBaseModel.save` workspace rule (`workspace.py:192-195`):
    /// `if self.project: self.workspace = self.project.workspace` — the
    /// backfill runs on EVERY save and overwrites an explicitly assigned
    /// `workspace` whenever `project` is set (ported quirk); with no
    /// project the explicit value survives.
    pub fn resolve_workspace(
        project_workspace_id: Option<uuid::Uuid>,
        explicit_workspace_id: uuid::Uuid,
    ) -> uuid::Uuid {
        project_workspace_id.unwrap_or(explicit_workspace_id)
    }
}

#[cfg(test)]
mod tests {
    use super::user::{self, DisplayNameSource, SaveError};
    use super::{account, api_token, profile, user_favorite, OnDelete};
    use serde_json::Value;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_workspace/models")
    }

    fn fixture(name: &str) -> Value {
        let path = fixtures_dir().join(name);
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    fn user_token() -> Value {
        fixture("user_token.columns.json")
    }

    fn prefs() -> Value {
        fixture("workspace_prefs.columns.json")
    }

    fn core() -> Value {
        fixture("workspace_core.columns.json")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// `models.<Model>.columns[].name` in fixture order.
    fn model_column_names(value: &Value, model: &str) -> Vec<String> {
        value["models"][model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .map(|c| {
                c["name"]
                    .as_str()
                    .expect("column entry has name")
                    .to_string()
            })
            .collect()
    }

    /// `inherited.columns[].name` in fixture order.
    fn inherited_names(value: &Value) -> Vec<String> {
        value["inherited"]["columns"]
            .as_array()
            .expect("inherited has columns array")
            .iter()
            .map(|c| {
                c["name"]
                    .as_str()
                    .expect("inherited entry has name")
                    .to_string()
            })
            .collect()
    }

    /// `models.<Model>.behavior` joined, for default/contract assertions.
    fn behavior(value: &Value, model: &str) -> String {
        value["models"][model]["behavior"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has behavior"))
            .iter()
            .map(|b| b.as_str().expect("behavior is str"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `models.<Model>.bugs` joined, for ported-quirk assertions.
    fn bugs(value: &Value, model: &str) -> String {
        value["models"][model]["bugs"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has bugs"))
            .iter()
            .map(|b| b.as_str().expect("bug is str"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Django field name → DB attname, written literally from the Python
    /// field declarations (independent of the module consts under test).
    /// Only true FK/OneToOne fields map; notably `Profile.last_workspace_id`
    /// is already a plain column name (not an FK) and passes through.
    fn attname(model: &str, field: &str) -> String {
        let is_fk = matches!(
            (model, field),
            ("User", "avatar_asset")
                | ("User", "cover_image_asset")
                | ("Profile", "user")
                | ("Account", "user")
                | ("APIToken", "user")
                | ("APIToken", "workspace")
                | ("UserFavorite", "user")
                | ("UserFavorite", "parent")
                | ("audit", "created_by")
                | ("audit", "updated_by")
                | ("workspace_base", "workspace")
                | ("workspace_base", "project")
        );
        if is_fk {
            format!("{field}_id")
        } else {
            field.to_string()
        }
    }

    fn meta_str(value: &Value, model: &str, key: &str) -> String {
        value["models"][model]["meta"][key]
            .as_str()
            .unwrap_or_else(|| panic!("{model}.meta.{key} is str"))
            .to_string()
    }

    fn meta_str_list(value: &Value, model: &str, key: &str) -> Vec<String> {
        value["models"][model]["meta"][key]
            .as_array()
            .unwrap_or_else(|| panic!("{model}.meta.{key} is list"))
            .iter()
            .map(|e| e.as_str().expect("meta entry is str").to_string())
            .collect()
    }

    #[test]
    fn user_columns_match_fixture() {
        let value = user_token();
        let expected: Vec<String> = model_column_names(&value, "User")
            .iter()
            .map(|f| attname("User", f))
            .collect();
        assert_eq!(expected.len(), 38, "F-W24-08 User has 38 columns");
        assert_eq!(owned(user::DECLARED_COLUMNS), expected);
    }

    #[test]
    fn user_auth_columns_match_fixture() {
        // The AbstractBaseUser pair is documented under User.behavior (NOT
        // in the columns array), with the M2M join-table note.
        assert_eq!(owned(user::AUTH_COLUMNS), vec!["password", "last_login"]);
        let text = behavior(&user_token(), "User");
        assert!(text.contains("password"), "fixture records password");
        assert!(text.contains("last_login"), "fixture records last_login");
        assert!(
            text.contains("groups") && text.contains("user_permissions"),
            "fixture records the M2M join tables"
        );
    }

    #[test]
    fn user_meta_matches_fixture() {
        let value = user_token();
        assert_eq!(user::TABLE, meta_str(&value, "User", "db_table"));
        assert_eq!(
            vec![user::ORDERING.to_string()],
            meta_str_list(&value, "User", "ordering")
        );
        assert_eq!(user::VERBOSE_NAME, meta_str(&value, "User", "verbose_name"));
        assert_eq!(
            user::VERBOSE_NAME_PLURAL,
            meta_str(&value, "User", "verbose_name_plural")
        );
        let text = behavior(&value, "User");
        assert!(
            text.contains("USERNAME_FIELD=email"),
            "fixture pins USERNAME_FIELD"
        );
        assert_eq!(user::USERNAME_FIELD, "email");
        assert_eq!(user::REQUIRED_FIELDS, &["username"]);
    }

    #[test]
    fn user_avatar_url_goldens() {
        // Asset arm wins and is returned as-is (Python checks only the FK):
        // no fall-through, even when the URL is empty or missing.
        assert_eq!(
            user::avatar_url(true, Some("https://cdn/a.png"), "https://x/b.png"),
            Some("https://cdn/a.png")
        );
        assert_eq!(
            user::avatar_url(true, Some(""), "https://x/b.png"),
            Some("")
        );
        assert_eq!(user::avatar_url(true, None, "https://x/b.png"), None);
        // Direct arm: non-empty passes through, empty falls to None.
        assert_eq!(
            user::avatar_url(false, None, "https://x/b.png"),
            Some("https://x/b.png")
        );
        assert_eq!(user::avatar_url(false, None, ""), None);
    }

    #[test]
    fn user_cover_image_url_goldens() {
        assert_eq!(
            user::cover_image_url(true, Some("https://cdn/c.png"), Some("https://x/d.png")),
            Some("https://cdn/c.png")
        );
        // Attached with no URL: no fall-through (Python returns None).
        assert_eq!(
            user::cover_image_url(true, None, Some("https://x/d.png")),
            None
        );
        assert_eq!(
            user::cover_image_url(false, None, Some("https://x/d.png")),
            Some("https://x/d.png")
        );
        // cover_image is nullable: None and "" both fall to None.
        assert_eq!(user::cover_image_url(false, None, Some("")), None);
        assert_eq!(user::cover_image_url(false, None, None), None);
        assert_eq!(
            user::cover_image_url(true, Some("https://cdn/c.png"), None),
            Some("https://cdn/c.png")
        );
    }

    #[test]
    fn user_full_name_goldens() {
        assert_eq!(user::full_name("Ada", "Lovelace"), "Ada Lovelace");
        assert_eq!(user::full_name("", ""), "");
        assert_eq!(user::full_name(" Ada ", ""), "Ada");
        assert_eq!(user::full_name("", "x"), "x");
    }

    #[test]
    fn user_save_email_goldens() {
        // The email=None AttributeError is an explicit error arm.
        assert_eq!(user::save_email(None), Err(SaveError::EmailNone));
        assert_eq!(
            user::save_email(Some("  ALI@X.com  ")).expect("normalizes"),
            "ali@x.com"
        );
        let text = bugs(&user_token(), "User");
        assert!(
            text.contains("AttributeError"),
            "fixture pins the email=None bug"
        );
    }

    #[test]
    fn user_token_regen_rules() {
        // Condition: any non-null token_updated_at regenerates (and the
        // re-stamp means it fires on EVERY later save — ported as-is).
        assert!(user::should_regenerate_token(true));
        assert!(!user::should_regenerate_token(false));
        let text = bugs(&user_token(), "User");
        assert!(
            text.contains("EVERY subsequent save"),
            "fixture pins the never-settles bug"
        );
        // Value: uuid hex × 2 → 64 lowercase hex chars.
        let token = user::regenerated_token();
        assert_eq!(token.len(), user::REGENERATED_TOKEN_HEX_LEN);
        assert_eq!(token.len(), 64);
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "regen token is hex"
        );
        assert_eq!(token.to_lowercase(), token, "regen token is lowercase");
    }

    #[test]
    fn user_save_display_name_goldens() {
        // Set display_name survives untouched.
        assert_eq!(user::save_display_name("Ada", "a@b"), "Ada");
        // Falsy display_name is ALWAYS email.split("@")[0] — the random-6
        // else arm is DEAD, so multi-@ input is deterministic, not random.
        assert_eq!(user::save_display_name("", "a@b"), "a");
        assert_eq!(user::save_display_name("", "no-at-sign"), "no-at-sign");
        assert_eq!(user::save_display_name("", "a@b@c"), "a");
        let text = bugs(&user_token(), "User");
        assert!(text.contains("DEAD"), "fixture pins the dead branch");
    }

    #[test]
    fn user_save_is_staff_goldens() {
        assert!(user::save_is_staff(true, false));
        assert!(user::save_is_staff(true, true));
        assert!(!user::save_is_staff(false, false));
        assert!(user::save_is_staff(false, true));
    }

    #[test]
    fn user_get_display_name_goldens() {
        // Deterministic arm: exactly two @ parts → local part.
        assert_eq!(
            user::get_display_name_source(Some("a@b")),
            DisplayNameSource::LocalPart("a")
        );
        assert_eq!(user::get_display_name(Some("a@b")), "a");
        // Random arms: None, "", one part, three parts.
        assert_eq!(
            user::get_display_name_source(None),
            DisplayNameSource::Random
        );
        assert_eq!(
            user::get_display_name_source(Some("")),
            DisplayNameSource::Random
        );
        assert_eq!(
            user::get_display_name_source(Some("nope")),
            DisplayNameSource::Random
        );
        assert_eq!(
            user::get_display_name_source(Some("a@b@c")),
            DisplayNameSource::Random
        );
        // Random shape: 6 ascii_letters (letters only, no digits).
        let random = user::random_display_name();
        assert_eq!(random.len(), user::DISPLAY_NAME_RANDOM_LEN);
        assert_eq!(random.len(), 6);
        assert!(
            random.chars().all(|c| c.is_ascii_alphabetic()),
            "random display name is ascii letters"
        );
        assert_eq!(user::DISPLAY_NAME_RANDOM_ALPHABET.len(), 52);
    }

    #[test]
    fn user_save_defaults_and_receiver() {
        assert_eq!(user::DISPLAY_NAME_DEFAULT, "");
        assert_eq!(user::LAST_LOGIN_MEDIUM_DEFAULT, "email");
        assert_eq!(user::USER_TIMEZONE_DEFAULT, "UTC");
        assert_eq!(user::BOT_TYPE_MAX_LENGTH, 30);
        assert_eq!(user::BOT_TYPE_WORKSPACE_SEED, "WORKSPACE_SEED");
        let text = bugs(&user_token(), "User");
        assert!(
            text.contains("does NOT enforce BotTypeEnum"),
            "fixture pins the free-text bot_type"
        );
        // post_save create_user_notification: five True flags, in order.
        let flags: Vec<(String, bool)> = user::POST_SAVE_NOTIFICATION_FLAGS
            .iter()
            .map(|(name, value)| ((*name).to_string(), *value))
            .collect();
        assert_eq!(
            flags,
            vec![
                ("property_change".to_string(), true),
                ("state_change".to_string(), true),
                ("comment".to_string(), true),
                ("mention".to_string(), true),
                ("issue_completed".to_string(), true),
            ]
        );
        let behavior_text = behavior(&user_token(), "User");
        assert!(
            behavior_text.contains("create_user_notification"),
            "fixture records the receiver"
        );
    }

    #[test]
    fn profile_columns_match_fixture() {
        let value = user_token();
        let expected: Vec<String> = model_column_names(&value, "Profile")
            .iter()
            .map(|f| attname("Profile", f))
            .collect();
        assert_eq!(expected.len(), 28, "F-W24-08 Profile has 28 columns");
        assert_eq!(owned(profile::DECLARED_COLUMNS), expected);
        // last_workspace_id passes through unmapped: plain UUID, not an FK.
        assert!(owned(profile::DECLARED_COLUMNS).contains(&"last_workspace_id".to_string()));
        assert!(!owned(profile::DECLARED_COLUMNS).contains(&"last_workspace_id_id".to_string()));
        let text = bugs(&value, "Profile");
        assert!(
            text.contains("NOT an FK"),
            "fixture pins the plain-UUID quirk"
        );
    }

    #[test]
    fn profile_audit_columns_match_fixture() {
        // TimeAuditModel pair, from the F-W24-08 inherited section.
        assert_eq!(
            inherited_names(&user_token()),
            vec!["created_at", "updated_at"]
        );
        assert_eq!(
            owned(profile::AUDIT_COLUMNS),
            vec!["created_at", "updated_at"]
        );
        assert_eq!(
            owned(account::AUDIT_COLUMNS),
            vec!["created_at", "updated_at"]
        );
    }

    #[test]
    fn profile_meta_matches_fixture() {
        let value = user_token();
        assert_eq!(profile::TABLE, meta_str(&value, "Profile", "db_table"));
        assert_eq!(
            vec![profile::ORDERING.to_string()],
            meta_str_list(&value, "Profile", "ordering")
        );
        assert_eq!(
            profile::VERBOSE_NAME,
            meta_str(&value, "Profile", "verbose_name")
        );
        assert_eq!(
            profile::VERBOSE_NAME_PLURAL,
            meta_str(&value, "Profile", "verbose_name_plural")
        );
    }

    #[test]
    fn profile_onboarding_defaults_match_fixture() {
        let text = behavior(&user_token(), "Profile");
        assert!(
            text.contains("get_default_onboarding"),
            "fixture pins onboarding"
        );
        assert!(
            text.contains("lacks workspace_invite"),
            "fixture pins the mobile gap"
        );
        assert_eq!(
            profile::default_onboarding(),
            serde_json::json!({
                "profile_complete": false,
                "workspace_create": false,
                "workspace_invite": false,
                "workspace_join": false,
            })
        );
        assert_eq!(
            profile::default_mobile_onboarding(),
            serde_json::json!({
                "profile_complete": false,
                "workspace_create": false,
                "workspace_join": false,
            })
        );
        assert_eq!(
            profile::default_product_tour(),
            serde_json::json!({
                "work_items": false,
                "cycles": false,
                "modules": false,
                "intake": false,
                "pages": false,
            })
        );
        // Key consts cover exactly the default objects' keys.
        for (keys, default) in [
            (&profile::ONBOARDING_KEYS[..], profile::default_onboarding()),
            (
                &profile::MOBILE_ONBOARDING_KEYS[..],
                profile::default_mobile_onboarding(),
            ),
            (
                &profile::PRODUCT_TOUR_KEYS[..],
                profile::default_product_tour(),
            ),
        ] {
            let mut from_const: Vec<String> = keys.iter().map(|k| (*k).to_string()).collect();
            from_const.sort();
            let mut from_default: Vec<String> = default
                .as_object()
                .expect("default is object")
                .keys()
                .cloned()
                .collect();
            from_default.sort();
            assert_eq!(from_const, from_default);
        }
    }

    #[test]
    fn profile_enums_and_scalar_defaults() {
        assert_eq!(profile::NOTIFICATION_VIEW_MODE_FULL, "full");
        assert_eq!(profile::NOTIFICATION_VIEW_MODE_COMPACT, "compact");
        assert_eq!(profile::NOTIFICATION_VIEW_MODE_DEFAULT, "full");
        let choices: Vec<(i16, String)> = profile::START_OF_THE_WEEK_CHOICES
            .iter()
            .map(|(value, label)| (*value, (*label).to_string()))
            .collect();
        assert_eq!(
            choices,
            vec![
                (0, "Sunday".to_string()),
                (1, "Monday".to_string()),
                (2, "Tuesday".to_string()),
                (3, "Wednesday".to_string()),
                (4, "Thursday".to_string()),
                (5, "Friday".to_string()),
                (6, "Saturday".to_string()),
            ]
        );
        assert_eq!(profile::START_OF_THE_WEEK_DEFAULT, 0);
        assert_eq!(profile::BILLING_ADDRESS_COUNTRY_DEFAULT, "INDIA");
        assert_eq!(profile::LANGUAGE_DEFAULT, "en");
        // background_color: "#" + 6 mixed-case hexdigits.
        let color = profile::random_background_color();
        assert_eq!(profile::BACKGROUND_COLOR_PREFIX, "#");
        assert!(color.starts_with('#'), "color has # prefix");
        let tail = &color[1..];
        assert_eq!(tail.len(), profile::BACKGROUND_COLOR_HEX_LEN);
        assert!(
            tail.bytes()
                .all(|b| profile::BACKGROUND_COLOR_ALPHABET.contains(&b)),
            "color tail is hexdigits"
        );
        assert_eq!(profile::BACKGROUND_COLOR_ALPHABET.len(), 22);
    }

    #[test]
    fn account_columns_and_meta_match_fixture() {
        let value = user_token();
        let expected: Vec<String> = model_column_names(&value, "Account")
            .iter()
            .map(|f| attname("Account", f))
            .collect();
        assert_eq!(expected.len(), 11, "F-W24-08 Account has 11 columns");
        assert_eq!(owned(account::DECLARED_COLUMNS), expected);
        assert_eq!(account::TABLE, meta_str(&value, "Account", "db_table"));
        assert_eq!(
            vec![account::ORDERING.to_string()],
            meta_str_list(&value, "Account", "ordering")
        );
        assert_eq!(
            account::VERBOSE_NAME,
            meta_str(&value, "Account", "verbose_name")
        );
        assert_eq!(
            account::VERBOSE_NAME_PLURAL,
            meta_str(&value, "Account", "verbose_name_plural")
        );
        assert_eq!(
            account::UNIQUE_TOGETHER.as_slice(),
            meta_str_list(&value, "Account", "unique_together").as_slice()
        );
    }

    #[test]
    fn account_provider_choices_and_length_quirk() {
        let choices: Vec<(String, String)> = account::PROVIDER_CHOICES
            .iter()
            .map(|(value, label)| ((*value).to_string(), (*label).to_string()))
            .collect();
        assert_eq!(
            choices,
            vec![
                ("google".to_string(), "Google".to_string()),
                ("github".to_string(), "Github".to_string()),
                ("gitlab".to_string(), "GitLab".to_string()),
            ]
        );
        let text = bugs(&user_token(), "Account");
        assert!(
            text.contains("NO max_length"),
            "fixture pins the missing max_length"
        );
    }

    #[test]
    fn api_token_columns_match_fixtures() {
        let value = user_token();
        let expected: Vec<String> = model_column_names(&value, "APIToken")
            .iter()
            .map(|f| attname("APIToken", f))
            .collect();
        assert_eq!(expected.len(), 11, "F-W24-08 APIToken has 11 columns");
        assert_eq!(owned(api_token::DECLARED_COLUMNS), expected);
        // BaseModel audit set, from the F-W24-06 inherited section (the
        // F-W24-08 note defers to it: "full BaseModel set per F-W24-06").
        let audit: Vec<String> = inherited_names(&core())
            .iter()
            .map(|f| attname("audit", f))
            .collect();
        assert_eq!(audit.len(), 6, "BaseModel contributes 6 columns");
        assert_eq!(owned(api_token::AUDIT_COLUMNS), audit);
    }

    #[test]
    fn api_token_meta_matches_fixture() {
        let value = user_token();
        assert_eq!(api_token::TABLE, meta_str(&value, "APIToken", "db_table"));
        assert_eq!(
            vec![api_token::ORDERING.to_string()],
            meta_str_list(&value, "APIToken", "ordering")
        );
        assert_eq!(
            api_token::VERBOSE_NAME,
            meta_str(&value, "APIToken", "verbose_name")
        );
        // The "API Tokems" typo is ported verbatim.
        assert_eq!(
            api_token::VERBOSE_NAME_PLURAL,
            meta_str(&value, "APIToken", "verbose_name_plural")
        );
        assert_eq!(api_token::VERBOSE_NAME_PLURAL, "API Tokems");
        let text = bugs(&value, "APIToken");
        assert!(text.contains("API Tokems"), "fixture pins the typo");
    }

    #[test]
    fn api_token_generators_and_defaults() {
        let text = behavior(&user_token(), "APIToken");
        assert!(
            text.contains("pi_dash_api_"),
            "fixture pins the token prefix"
        );
        let token = api_token::generate_token();
        assert!(token.starts_with(api_token::TOKEN_PREFIX));
        assert_eq!(api_token::TOKEN_PREFIX, "pi_dash_api_");
        let tail = &token[api_token::TOKEN_PREFIX.len()..];
        assert_eq!(tail.len(), api_token::TOKEN_HEX_LEN);
        assert_eq!(tail.len(), 32);
        assert!(
            tail.chars().all(|c| c.is_ascii_hexdigit()),
            "token tail is hex"
        );
        assert_eq!(tail.to_lowercase(), tail, "token tail is lowercase");
        let label = api_token::generate_label_token();
        assert_eq!(label.len(), api_token::LABEL_HEX_LEN);
        assert_eq!(label.len(), 32);
        assert!(label.chars().all(|c| c.is_ascii_hexdigit()), "label is hex");
        assert_eq!(api_token::USER_TYPE_HUMAN, 0);
        assert_eq!(api_token::USER_TYPE_BOT, 1);
        assert_eq!(api_token::USER_TYPE_DEFAULT, 0);
        assert_eq!(api_token::ALLOWED_RATE_LIMIT_DEFAULT, "60/min");
        assert_eq!(api_token::DESCRIPTION_DEFAULT, "");
    }

    #[test]
    fn favorite_columns_match_fixtures() {
        let value = prefs();
        let expected: Vec<String> = model_column_names(&value, "UserFavorite")
            .iter()
            .map(|f| attname("UserFavorite", f))
            .collect();
        assert_eq!(expected.len(), 7, "F-W24-07 UserFavorite has 7 columns");
        assert_eq!(owned(user_favorite::DECLARED_COLUMNS), expected);
        // Inherited: BaseModel audit 6 (F-W24-06) + WorkspaceBaseModel 2
        // (F-W24-07 inherited section, workspace before project).
        let audit: Vec<String> = inherited_names(&core())
            .iter()
            .map(|f| attname("audit", f))
            .collect();
        let base: Vec<String> = inherited_names(&value)
            .iter()
            .map(|f| attname("workspace_base", f))
            .collect();
        assert_eq!(base, vec!["workspace_id", "project_id"]);
        let expected_inherited: Vec<String> = audit.into_iter().chain(base).collect();
        assert_eq!(expected_inherited.len(), 8);
        assert_eq!(owned(user_favorite::INHERITED_COLUMNS), expected_inherited);
    }

    #[test]
    fn favorite_meta_matches_fixture() {
        let value = prefs();
        assert_eq!(
            user_favorite::TABLE,
            meta_str(&value, "UserFavorite", "db_table")
        );
        assert_eq!(
            vec![user_favorite::ORDERING.to_string()],
            meta_str_list(&value, "UserFavorite", "ordering")
        );
        assert_eq!(
            user_favorite::VERBOSE_NAME,
            meta_str(&value, "UserFavorite", "verbose_name")
        );
        assert_eq!(
            user_favorite::VERBOSE_NAME_PLURAL,
            meta_str(&value, "UserFavorite", "verbose_name_plural")
        );
        assert_eq!(
            user_favorite::UNIQUE_TOGETHER.as_slice(),
            meta_str_list(&value, "UserFavorite", "unique_together").as_slice()
        );
        let constraints = meta_str_list(&value, "UserFavorite", "constraints");
        assert_eq!(constraints.len(), 1);
        assert!(
            constraints[0].contains(user_favorite::PARTIAL_UNIQUE_NAME),
            "constraint pins the partial-unique name"
        );
        assert_eq!(
            user_favorite::PARTIAL_UNIQUE_NAME,
            "user_favorite_unique_entity_type_entity_identifier_user_when_deleted_at_null"
        );
        assert_eq!(
            user_favorite::PARTIAL_UNIQUE_FIELDS,
            ["entity_type", "entity_identifier", "user"]
        );
        let indexes = meta_str_list(&value, "UserFavorite", "indexes");
        assert_eq!(indexes.len(), 3);
        for name in user_favorite::INDEX_NAMES {
            assert!(
                indexes.iter().any(|entry| entry.contains(name)),
                "fixture pins index {name}"
            );
        }
    }

    #[test]
    fn favorite_save_rules() {
        // Sequence: None keeps the 65535 default; else largest + 10000.
        assert_eq!(user_favorite::next_sequence(None), 65535.0);
        assert_eq!(user_favorite::SEQUENCE_DEFAULT, 65535.0);
        assert_eq!(user_favorite::SEQUENCE_STEP, 10000.0);
        assert_eq!(user_favorite::next_sequence(Some(65535.0)), 75535.0);
        assert_eq!(user_favorite::next_sequence(Some(0.0)), 10000.0);
        let text = bugs(&prefs(), "UserFavorite");
        assert!(
            text.contains("NOT per-user"),
            "fixture pins the workspace-wide scope"
        );
        // Workspace backfill: project wins even when explicit; else explicit.
        let project_ws = uuid::Uuid::new_v4();
        let explicit_ws = uuid::Uuid::new_v4();
        assert_eq!(
            user_favorite::resolve_workspace(Some(project_ws), explicit_ws),
            project_ws
        );
        assert_eq!(
            user_favorite::resolve_workspace(None, explicit_ws),
            explicit_ws
        );
    }

    #[test]
    fn soft_delete_scopes() {
        // Only the BaseModel children carry the deleted_at marker (fixture
        // inherited sections + model bases); their reads filter
        // `deleted_at IS NULL`. Compared as runtime pairs so the assertion
        // is not on constants.
        let actual: Vec<(&str, bool)> = vec![
            ("User", user::HAS_DELETED_AT),
            ("Profile", profile::HAS_DELETED_AT),
            ("Account", account::HAS_DELETED_AT),
            ("APIToken", api_token::HAS_DELETED_AT),
            ("UserFavorite", user_favorite::HAS_DELETED_AT),
        ];
        assert_eq!(
            actual,
            vec![
                ("User", false),
                ("Profile", false),
                ("Account", false),
                ("APIToken", true),
                ("UserFavorite", true),
            ]
        );
    }

    #[test]
    fn scalar_bool_flags() {
        // Boolean defaults/quirks as runtime pairs: the only True `is_*`
        // user default; the docked-rail default; the empty-dict settings
        // default; the provider no-max_length E120 flag; the token
        // active/service defaults; is_folder Django-side False (DB NOT
        // NULL without a DB default — inserts must bind the literal,
        // D-27 #916).
        let actual: Vec<(&str, bool)> = vec![
            ("user.is_active", user::IS_ACTIVE_DEFAULT),
            (
                "profile.is_app_rail_docked",
                profile::IS_APP_RAIL_DOCKED_DEFAULT,
            ),
            (
                "profile.settings_empty_dict",
                profile::SETTINGS_DEFAULT_IS_EMPTY_DICT,
            ),
            (
                "account.provider_no_max_length",
                account::PROVIDER_HAS_NO_MAX_LENGTH,
            ),
            ("api_token.is_active", api_token::IS_ACTIVE_DEFAULT),
            ("api_token.is_service", api_token::IS_SERVICE_DEFAULT),
            ("user_favorite.is_folder", user_favorite::IS_FOLDER_DEFAULT),
        ];
        assert_eq!(
            actual,
            vec![
                ("user.is_active", true),
                ("profile.is_app_rail_docked", true),
                ("profile.settings_empty_dict", true),
                ("account.provider_no_max_length", true),
                ("api_token.is_active", true),
                ("api_token.is_service", false),
                ("user_favorite.is_folder", false),
            ]
        );
    }

    #[test]
    fn on_delete_contracts() {
        // SET_NULL arms (nullable FKs) vs CASCADE arms, per the Python.
        assert_eq!(user::AVATAR_ASSET_ON_DELETE, OnDelete::SetNull);
        assert_eq!(user::COVER_IMAGE_ASSET_ON_DELETE, OnDelete::SetNull);
        assert_eq!(profile::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(account::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(api_token::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(api_token::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(user_favorite::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(user_favorite::PARENT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(user_favorite::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(user_favorite::PROJECT_ON_DELETE, OnDelete::Cascade);
    }
}
