#![forbid(unsafe_code)]

//! User / account / profile / API-token query builders (D-24, stage 5).
//!
//! Ports the queryset and write shapes behind the user identity family
//! (`UserEndpoint`, session/onboard/tour, accounts, profile) and the
//! user-facing API-token endpoint as SQL text, following the D-24
//! precedent (`queries_membership.rs`, `queries_core.rs`): each builder
//! returns a fragment (or a representative statement) the caller splices
//! into the statement it executes. The services crate carries no
//! `sea-query`/`sqlx` dependency (foundation crates are read-only for
//! port agents), so placeholders stay symbolic — `:user`, `:email`,
//! `:pk`, `:pks`, `:ws`, `:instance`, `:now`, `:key`, `:hash`, `:ip`,
//! `:value`, `:step_json` — exactly the notation the fixtures use;
//! handlers bind them.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/serializers/user.py:90-139` — `UserMeSettingsSerializer`
//!   invite count, profile read, last-workspace check + fetch, fallback.
//! - `app/views/user/base.py:59-93` — `retrieve_instance_admin`
//!   (`Instance.first` + `InstanceAdmin` exists); `partial_update`
//!   reuses the full-row user save with the serializer-owned SET
//!   (PIDASHCONV-603), so it needs no builder of its own.
//! - `app/views/user/base.py:95-250` — `_validate_new_email`,
//!   `generate_email_verification_code`, `update_email`.
//! - `app/views/user/base.py:252-357` — `deactivate`.
//! - `app/views/user/base.py:359-371` — `UserSessionEndpoint` re-read.
//! - `app/views/user/base.py:373-389` — onboard / tour `update_fields`
//!   patches.
//! - `app/views/user/base.py:406-462` — accounts, profile get/patch.
//! - `app/views/api.py:20-72` — API-token create / list / detail /
//!   delete / patch.
//! - `db/models/user.py:56-321`, `db/models/api.py:35-61` — table and
//!   column facts (single source: PIDASHCONV-607 `models_user`).
//! - `db/mixins.py:48-82` — soft-delete manager (`deleted_at IS NULL`)
//!   and soft-by-default `.delete()`.
//!
//! Fixture oracle: F-W24-12, user part
//! (`fixtures/app_workspace/queries/extras_user.sql` R13-R14 +
//! `extras_user.rows.json` `user_account` / `api_tokens` cases). The
//! unit tests below pin the builders against those files so
//! transcription drift fails the build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. The invite count (`user.py:99`) has NO `accepted` filter —
//!    accepted invites count the same as pending ones.
//! 2. The `workspace_member__` traversal joins carry NO
//!    `workspace_members.deleted_at IS NULL` guard — cross-FK filters do
//!    not apply the related model's manager (probed live on Django
//!    4.2), so a soft-deleted membership still satisfies the
//!    last-workspace check and the fallback.
//! 3. The last-workspace branch re-runs the check filter as a second
//!    query (`:111-115`) instead of reusing the `.exists()` row.
//! 4. The deactivate sole-admin annotations (`base.py:266-275`,
//!    `:287-296`) group BY the row itself (probed: `GROUP BY` every
//!    column, no `ORDER BY`), so `other_admin_exists` counts
//!    `CASE ... ELSE 0` — `COUNT` sees no NULL and always yields 1 —
//!    and `total_members` is always 1. Both `400` "only admin" branches
//!    (`:282-285`, `:303-306`) are unreachable; every active membership
//!    is collected and deactivated.
//! 5. `bulk_update(..., ["is_active"], batch_size=100)` stamps NO
//!    `updated_at` (`auto_now` is a `save()` behavior) and carries NO
//!    `deleted_at` filter (pk-direct).
//! 6. Onboard / tour patches (`:376`, `:387`) bind the raw
//!    `request.data.get(flag, False)` value with NO serializer
//!    validation or coercion.
//! 7. Token GET-detail (`api.py:47`) omits `is_service=False` while
//!    DELETE (`:52`) and PATCH (`:63-65`) filter it — a service token
//!    is readable but not editable/deletable here.
//! 8. Token PATCH on a missing row returns a BARE 404 with an empty
//!    body (`:67`) — no `error`/`detail` key.
//! 9. `is_service` is PATCH-writable (probed by PIDASHCONV-604) — a
//!    user can flip their own token's service flag.
//! 10. Token POST defaults `label` from `uuid4().hex` only when the key
//!     is ABSENT (`:22`) — an explicit empty label is stored empty.
//! 11. `Session.user_id` is a `CharField` (`session.py:21`) — the UUID
//!     binds as text.
//! 12. Token PATCH accepts `deleted_at` / `created_by` / `updated_by`
//!     (none is in `read_only_fields` — probed live on DRF 3.15.2:
//!     all three validate); only `updated_by` is then overwritten by
//!     `BaseModel.save` with the request actor.
//! 13. The logo-asset follow (`user.py:116`) runs through the PLAIN
//!     `_base_manager`, not the soft-delete default manager
//!     (`related_descriptors.py`), so a soft-deleted asset is still
//!     followed and its URL served (probed live on Django 4.2).
//!
//! Out of scope (owned by sibling issues): response envelopes and error
//! bodies (handlers E/F/J, PIDASHCONV-619/620/624); serializer shapes
//! (PIDASHCONV-603/604); model column lists (PIDASHCONV-607 —
//! referenced in doc comments, never forked); the me-activities query
//! (`user/base.py:392-403`, owned by QRY-C PIDASHCONV-610 as "same
//! shape"); permission gates, throttles and cache sites
//! (PIDASHCONV-613); Celery enqueues (PIDASHCONV-614); the timezone
//! table (static — handler only, PIDASHCONV-624, no query surface).

// ---------------------------------------------------------------------------
// Unit 1a — settings serializer reads (serializers/user.py:90-139)
// ---------------------------------------------------------------------------

/// Profile read (`user.py:102`): `Profile.objects.get(user=obj)`.
///
/// `profiles` carries no `deleted_at` (`TimeAuditModel` only,
/// `mixins.py:16-24`), so the lookup is unscoped. `.get()` fetches
/// with a `LIMIT 21` clone guard (unobservable in SQL semantics —
/// `MultipleObjectsReturned` is impossible on the `OneToOne`).
pub fn profile_by_user_sql() -> String {
    "SELECT profiles.* FROM profiles WHERE profiles.user_id = :user".to_owned()
}

/// Logo-asset lazy follow (`user.py:116`):
/// `workspace.logo_asset.asset_url if workspace.logo_asset is not None`.
///
/// Ported bug 13: forward-FK access runs through the PLAIN
/// `_base_manager`, NOT the soft-delete default manager
/// (`related_descriptors.py`), so there is NO `deleted_at` scope —
/// a soft-deleted asset is still followed (probed live on Django
/// 4.2). One extra round-trip when `logo_asset_id` is set and
/// uncached; `asset_url` itself (`asset.py:80-91`) is pure string
/// formatting for workspace logos (no further SQL).
pub fn logo_asset_lookup_sql() -> String {
    "SELECT file_assets.* FROM file_assets WHERE file_assets.id = :asset_id".to_owned()
}

// ---------------------------------------------------------------------------
// Unit 1b — instance-admin + session reads (views/user/base.py:87-90, 359-371)
// ---------------------------------------------------------------------------

/// Session user re-read (`base.py:364`):
/// `User.objects.get(pk=request.user.id)`.
///
/// `users` carries no `deleted_at` (plain `AbstractBaseUser`), so the
/// lookup is unscoped. Runs only on the authenticated branch (`:363`);
/// the anonymous branch issues no SQL at all.
pub fn session_user_sql() -> String {
    "SELECT users.* FROM users WHERE users.id = :user".to_owned()
}

// ---------------------------------------------------------------------------
// Unit 2 — email change: validation query + cache + save
// (views/user/base.py:95-250)
// ---------------------------------------------------------------------------

/// Cache-key prefix for the email-update magic code (`:155`, `:199`).
/// The full key binds the code to both the user and the target
/// address: `magic_email_update_{user.id}_{new_email}`.
pub const EMAIL_UPDATE_CACHE_KEY_PREFIX: &str = "magic_email_update_";

/// Render the email-update cache key (`:155`, `:199`).
///
/// `new_email` arrives already `.strip().lower()`-normalized by the
/// caller (`:144`, `:183`) — the key is built from the normalized
/// form on both the store and the verify path.
pub fn email_update_cache_key(user_id: &str, new_email: &str) -> String {
    format!("{EMAIL_UPDATE_CACHE_KEY_PREFIX}{user_id}_{new_email}")
}

/// Cache TTL in seconds (`:160`): `cache.set(key, data, timeout=600)`
/// — 10 minutes.
pub const EMAIL_UPDATE_CODE_TTL_SECS: u32 = 600;

/// Magic-code range (`:157`): `secrets.randbelow(900000) + 100000`,
/// i.e. six digits, `100000..=999999`, always zero-free at the front.
pub const EMAIL_UPDATE_CODE_MIN: u32 = 100000;
/// See [`EMAIL_UPDATE_CODE_MIN`].
pub const EMAIL_UPDATE_CODE_MAX: u32 = 999999;

/// Cache value shape (`:159`): `json.dumps({"token": token})` — the
/// code travels as a JSON string under the `token` key.
pub fn email_update_cache_value(code: &str) -> String {
    format!(r#"{{"token": "{code}"}}"#)
}

// ---------------------------------------------------------------------------
// Unit 3 — deactivate bundle (views/user/base.py:252-357)
// ---------------------------------------------------------------------------

/// `bulk_update` batch size (`:308`, `:310`): `batch_size=100`.
pub const BULK_DEACTIVATE_BATCH_SIZE: usize = 100;

/// Onboarding-step reset value (`:325-330`), exact key order as
/// written in the view (JSON object insertion order is preserved on
/// the wire).
pub const ONBOARDING_RESET_JSON: &str = r#"{"workspace_join": false, "profile_complete": false, "workspace_create": false, "workspace_invite": false}"#;

// ---------------------------------------------------------------------------
// Unit 4a — accounts + profile (views/user/base.py:373-462)
// ---------------------------------------------------------------------------

/// Account list (`:413`): `Account.objects.filter(user=request.user)`.
///
/// `accounts` carries no `deleted_at` (`TimeAuditModel` only), so the
/// read is unscoped. Default `-created_at` ordering
/// (`user.py:301`) applies to the collection read.
pub fn account_list_sql() -> String {
    "SELECT accounts.* FROM accounts WHERE accounts.user_id = :user ORDER BY accounts.created_at DESC".to_owned()
}

/// Account detail / delete lookup (`:409`, `:418`):
/// `Account.objects.get(pk=pk, user=request.user)`.
///
/// `.get()` adds no `ORDER BY`. A miss raises `DoesNotExist`, which
/// the base view maps to `404 {"error": "The required object does
/// not exist."}` (PIDASHCONV-699 — handlers own the body).
pub fn account_detail_sql() -> String {
    "SELECT accounts.* FROM accounts WHERE accounts.id = :pk AND accounts.user_id = :user"
        .to_owned()
}

pub use crate::domain::identity::account_hard_delete_sql;

/// Onboard-patch `update_fields` (`:380`).
pub const ONBOARD_UPDATE_FIELDS: &[&str] = &["is_onboarded", "updated_at"];

/// Tour-patch `update_fields` (`:388`).
pub const TOUR_UPDATE_FIELDS: &[&str] = &["is_tour_completed", "updated_at"];

/// Profile locked read (`:455`):
/// `Profile.objects.select_for_update().get(user=request.user)`
/// inside `transaction.atomic()` (`:454`).
///
/// No `deleted_at` exists on `profiles`, so the lock read is
/// unscoped. Profile GET (`:427`) reuses [`profile_by_user_sql`].
pub fn profile_lock_sql() -> String {
    "SELECT profiles.* FROM profiles WHERE profiles.user_id = :user FOR UPDATE".to_owned()
}

pub use crate::domain::identity::profile_full_save_sql;
pub use crate::domain::identity::PROFILE_SAVE_COLUMNS;

// ---------------------------------------------------------------------------
// Unit 4b — API tokens (views/api.py:20-72)
// ---------------------------------------------------------------------------

// NOTE: the token create builder lived here; the statement moved to
// `crate::domain::tokens::issue_token` (PIDASHCONV-1520), whose tests
// carry the pins (including ported bug 10's label fallback, resolved
// by the handler before the call).

/// Token list (`:43`):
/// `APIToken.objects.filter(user=request.user, is_service=False)`.
///
/// Soft-delete scope applies (`BaseModel`). Default `-created_at`
/// ordering (`api.py:57`).
pub fn api_token_list_sql() -> String {
    "SELECT api_tokens.* FROM api_tokens WHERE api_tokens.user_id = :user AND api_tokens.is_service = FALSE AND api_tokens.deleted_at IS NULL ORDER BY api_tokens.created_at DESC".to_owned()
}

/// Token GET-detail (`:47`):
/// `APIToken.objects.get(user=request.user, pk=pk)`.
///
/// Ported bug 7: NO `is_service` filter — unlike DELETE (`:52`) and
/// PATCH (`:63-65`), the detail read returns service tokens too.
/// Soft-delete scope still applies.
pub fn api_token_detail_sql() -> String {
    "SELECT api_tokens.* FROM api_tokens WHERE api_tokens.user_id = :user AND api_tokens.id = :pk AND api_tokens.deleted_at IS NULL".to_owned()
}

/// Token delete lookup (`:52`):
/// `APIToken.objects.get(user=request.user, pk=pk, is_service=False)`.
///
/// Same as [`api_token_detail_sql`] plus the service guard.
pub fn api_token_delete_lookup_sql() -> String {
    "SELECT api_tokens.* FROM api_tokens WHERE api_tokens.user_id = :user AND api_tokens.id = :pk AND api_tokens.is_service = FALSE AND api_tokens.deleted_at IS NULL".to_owned()
}

// NOTE: the token soft-delete builder lived here; the statement moved
// to `crate::domain::tokens::soft_delete_token` (PIDASHCONV-1520),
// whose tests carry the pin (the `soft_delete_related_objects`
// enqueue stays tasks-owned, PIDASHCONV-614).

/// Token PATCH lookup (`:63-65`):
/// `APIToken.objects.filter(user=request.user, pk=pk,
/// is_service=False).first()`.
///
/// `.first()` applies the default `-created_at` ordering with
/// `LIMIT 1`. A miss yields `None` and the view returns a BARE 404
/// with an empty body (`:67`, ported bug 8 — handlers own the body).
pub fn api_token_patch_lookup_sql() -> String {
    "SELECT api_tokens.* FROM api_tokens WHERE api_tokens.user_id = :user AND api_tokens.id = :pk AND api_tokens.is_service = FALSE AND api_tokens.deleted_at IS NULL ORDER BY api_tokens.created_at DESC LIMIT 1".to_owned()
}

/// Token PATCH client-effective writable fields (probed live against
/// `APITokenSerializer`, `api.py:11-24`, on DRF 3.15.2): `label`,
/// `description`, `is_service`, `allowed_rate_limit` — plus, ported
/// bug 12, `deleted_at` and `created_by`, which are NOT in
/// `read_only_fields` and validate. (`updated_by` validates too but
/// `BaseModel.save` overwrites it with the request actor on update,
/// `db/models/base.py:23-44` — handler sequencing, like the sibling
/// `updated_by` notes — so no client value persists.) Everything
/// else in `fields = "__all__"` is `read_only` (plus DRF-default
/// `id`) and silently ignored on input.
///
/// Ported bug 9: `is_service` is writable — a PATCH can flip a
/// token's own service flag.
pub const API_TOKEN_PATCH_WRITABLE: &[&str] = &[
    "label",
    "description",
    "is_service",
    "allowed_rate_limit",
    "deleted_at",
    "created_by",
];

// NOTE: the token PATCH-save builder lived here; the statement moved
// to `crate::domain::tokens::update_token` (PIDASHCONV-1520), whose
// tests carry the pin.

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SQL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/queries/extras_user.sql"
    );
    const FIXTURE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/queries/extras_user.rows.json"
    );

    fn rows_fixture() -> serde_json::Value {
        let raw = std::fs::read_to_string(FIXTURE_ROWS).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    fn sql_fixture() -> String {
        std::fs::read_to_string(FIXTURE_SQL).expect("fixture exists")
    }

    fn case<'a>(fixture: &'a serde_json::Value, q: &str) -> &'a serde_json::Value {
        fixture["cases"]
            .as_array()
            .expect("cases array")
            .iter()
            .find(|c| c["q"] == q)
            .unwrap_or_else(|| panic!("case {q} exists"))
    }

    #[test]
    fn every_soft_delete_read_carries_deleted_at_scope() {
        // Issue rule: soft-delete scoping on every read. users /
        // profiles / accounts / sessions carry no deleted_at (not
        // BaseModels); every other read does.
        for sql in [
            api_token_list_sql(),
            api_token_detail_sql(),
            api_token_delete_lookup_sql(),
            api_token_patch_lookup_sql(),
        ] {
            assert!(
                sql.contains("deleted_at IS NULL"),
                "missing soft-delete scope: {sql}"
            );
        }
        // The logo-asset follow is deliberately UNSCOPED even though
        // the table has the column: forward-FK access uses the plain
        // `_base_manager` (ported bug 13).
        assert!(
            !logo_asset_lookup_sql().contains("deleted_at"),
            "FK follow must stay unscoped"
        );
        // users / profiles / accounts / sessions have no deleted_at
        // column — assert absence, not scope.
        for sql in [
            profile_by_user_sql(),
            session_user_sql(),
            account_list_sql(),
            account_detail_sql(),
            profile_lock_sql(),
        ] {
            assert!(!sql.contains("deleted_at"), "no such column: {sql}");
        }
    }

    #[test]
    fn unit2_cache_round_trip() {
        // Key (base.py:155,199): prefix + user + normalized email.
        assert_eq!(
            email_update_cache_key("u-1", "new@example.com"),
            "magic_email_update_u-1_new@example.com"
        );
        assert_eq!(EMAIL_UPDATE_CACHE_KEY_PREFIX, "magic_email_update_");
        // TTL 600s (:160); six-digit range (:157).
        assert_eq!(EMAIL_UPDATE_CODE_TTL_SECS, 600);
        assert_eq!(
            (EMAIL_UPDATE_CODE_MIN, EMAIL_UPDATE_CODE_MAX),
            (100000, 999999)
        );
        // Value (:159): {"token": "<code>"}.
        assert_eq!(email_update_cache_value("482910"), r#"{"token": "482910"}"#);
    }

    #[test]
    fn unit4_accounts_and_profile() {
        // List: user-scoped, default order. Detail: pk + user, no order.
        assert!(account_list_sql().contains("ORDER BY accounts.created_at DESC"));
        assert!(!account_detail_sql().contains("ORDER BY"));
        assert!(account_detail_sql().contains("accounts.id = :pk"));
        // Delete: hard, pk-addressed.
        assert_eq!(
            account_hard_delete_sql(),
            "DELETE FROM accounts WHERE accounts.id = :pk"
        );
        // Onboard / tour: single-flag update_fields writes.
        assert_eq!(ONBOARD_UPDATE_FIELDS, &["is_onboarded", "updated_at"]);
        assert_eq!(TOUR_UPDATE_FIELDS, &["is_tour_completed", "updated_at"]);
        // Locked read: FOR UPDATE, unscoped.
        assert!(profile_lock_sql().ends_with("FOR UPDATE"));
        // Full save: every column incl. settings, pk-addressed.
        assert_eq!(PROFILE_SAVE_COLUMNS.len(), 29);
        assert!(PROFILE_SAVE_COLUMNS.contains(&"settings"));
        assert!(!PROFILE_SAVE_COLUMNS.contains(&"id"));
        let save = profile_full_save_sql();
        assert!(save.starts_with("UPDATE profiles SET "));
        assert!(save.contains("settings = :settings"));
        assert!(save.ends_with("WHERE id = :pk"));
    }

    #[test]
    fn unit4_api_tokens() {
        // (The INSERT/soft-delete/PATCH-save pins moved with the
        // statements to `crate::domain::tokens`.)
        // List carries is_service=False; detail omits it (ported bug 7).
        assert!(api_token_list_sql().contains("api_tokens.is_service = FALSE"));
        assert!(!api_token_detail_sql().contains("is_service"));
        assert!(api_token_detail_sql().contains("api_tokens.id = :pk"));
        // Delete lookup re-adds the guard.
        assert!(api_token_delete_lookup_sql().contains("api_tokens.is_service = FALSE"));
        // PATCH lookup: scoped .first(); writable set per SER-E probe.
        let lookup = api_token_patch_lookup_sql();
        assert!(lookup.contains("api_tokens.is_service = FALSE"));
        assert!(lookup.ends_with("ORDER BY api_tokens.created_at DESC LIMIT 1"));
        assert_eq!(
            API_TOKEN_PATCH_WRITABLE,
            &[
                "label",
                "description",
                "is_service",
                "allowed_rate_limit",
                "deleted_at",
                "created_by"
            ]
        );
    }

    #[test]
    fn fixture_sql_names_every_unit() {
        // F-W24-12 R13 (user_account) + R14 (api_tokens) trace markers.
        let sql = sql_fixture();
        for marker in [
            "R13 user account (views/user/base.py)",
            "R14 api tokens (views/api.py)",
            "InstanceAdmin.exists(user)",
            "bulk_update is_active batch 100",
            "Profile reset",
            "user autoset-pw uuid4hex",
            "select_for_update get",
            "merge_settings + save",
            "no is_service filter",
            "BARE 404 empty body",
            "R15 timezone",
            "NO DB",
        ] {
            assert!(sql.contains(marker), "fixture missing {marker}");
        }
    }

    #[test]
    fn fixture_rows_match_builders() {
        // The user_account + api_tokens cases pin this module's sources
        // and row shapes; the bugs array pins the ported bugs.
        let fixture = rows_fixture();
        let user = case(&fixture, "user_account");
        assert_eq!(user["source"], "views/user/base.py:64-462");
        for key in [
            "instance_admin",
            "deactivate",
            "session",
            "activity",
            "accounts",
            "onboard_tour",
            "profile_patch",
        ] {
            assert!(user["row"].get(key).is_some(), "row missing {key}");
        }
        let tokens = case(&fixture, "api_tokens");
        assert_eq!(tokens["source"], "views/api.py:21-72");
        for key in ["post", "get_list", "get_detail", "delete", "patch"] {
            assert!(tokens["row"].get(key).is_some(), "row missing {key}");
        }
        let bugs = fixture["bugs"].as_array().expect("bugs array");
        let joined = bugs
            .iter()
            .map(|b| b.as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        for marker in [
            "api token GET-detail omits is_service=False (api.py:47)",
            "token PATCH missing/service -> bare 404 empty body (api.py:67)",
        ] {
            assert!(joined.contains(marker), "bugs missing {marker}");
        }
    }
}
