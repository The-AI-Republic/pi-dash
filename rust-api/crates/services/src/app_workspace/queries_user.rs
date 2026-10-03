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
//!
//! Out of scope (owned by sibling issues): response envelopes and error
//! bodies (handlers E/F/J, PIDASHCONV-619/620/624); serializer shapes
//! (PIDASHCONV-603/604); model column lists (PIDASHCONV-607 —
//! referenced in doc comments, never forked); the me-activities query
//! (`user/base.py:392-403`, owned by QRY-C PIDASHCONV-610 as "same
//! shape"); permission gates, throttles and cache sites
//! (PIDASHCONV-613); Celery enqueues (PIDASHCONV-614); the timezone
//! table (static — handler only, PIDASHCONV-624, no query surface).

use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// `ROLE.ADMIN.value` (`app/permissions/base.py:14`) — the `role=20`
/// literal inside the deactivate `When` clauses.
pub const ROLE_ADMIN: i32 = 20;

// ---------------------------------------------------------------------------
// Unit 1a — settings serializer reads (serializers/user.py:90-139)
// ---------------------------------------------------------------------------

/// Invite count (`user.py:99`):
/// `WorkspaceMemberInvite.objects.filter(email=obj.email).count()`.
///
/// `COUNT(*)` under the soft-delete manager. Ported bug 1: there is NO
/// `accepted` filter — accepted and pending invites count alike.
pub fn invite_count_sql() -> String {
    "SELECT COUNT(*) FROM workspace_member_invites WHERE workspace_member_invites.email = :email AND workspace_member_invites.deleted_at IS NULL".to_owned()
}

/// Profile read (`user.py:102`): `Profile.objects.get(user=obj)`.
///
/// `profiles` carries no `deleted_at` (`TimeAuditModel` only,
/// `mixins.py:16-24`), so the lookup is unscoped. `.get()` fetches
/// with a `LIMIT 2` clone guard (unobservable in SQL semantics —
/// `MultipleObjectsReturned` is impossible on the `OneToOne`).
pub fn profile_by_user_sql() -> String {
    "SELECT profiles.* FROM profiles WHERE profiles.user_id = :user".to_owned()
}

/// Shared join for the last-workspace check, fetch, and fallback
/// (`user.py:105-109`, `:111-115`, `:128-130`): the reverse
/// `workspace_member__` traversal.
///
/// Ported bug 2 (probed live on Django 4.2): the joined
/// `workspace_members` rows carry NO `deleted_at IS NULL` guard —
/// cross-FK filters never apply the related model's manager. Only the
/// base `workspaces.deleted_at IS NULL` (from `Workspace.objects`)
/// applies. A soft-deleted membership therefore still satisfies every
/// branch below.
pub fn last_workspace_join_sql() -> String {
    "INNER JOIN workspace_members ON (workspaces.id = workspace_members.workspace_id)".to_owned()
}

/// Last-workspace membership check (`user.py:104-110`):
/// `.filter(pk=last_workspace_id, workspace_member__member=...,
/// workspace_member__is_active=True).exists()`.
///
/// Runs only when `profile.last_workspace_id is not None` (`:104`).
/// `.exists()` renders `SELECT 1 ... LIMIT 1`.
pub fn last_workspace_exists_sql() -> String {
    format!(
        "SELECT 1 FROM workspaces {} WHERE workspaces.deleted_at IS NULL AND workspaces.id = :ws AND workspace_members.member_id = :user AND workspace_members.is_active LIMIT 1",
        last_workspace_join_sql()
    )
}

/// Last-workspace fetch (`user.py:111-115`): the SAME filter as
/// [`last_workspace_exists_sql`] re-run with `.first()`.
///
/// Ported bug 3: the check row is not reused — this is a second
/// round-trip. `.first()` applies the default `-created_at` ordering
/// (`db/models/workspace.py:186`) with `LIMIT 1`.
pub fn last_workspace_fetch_sql() -> String {
    format!(
        "SELECT workspaces.* FROM workspaces {} WHERE workspaces.deleted_at IS NULL AND workspaces.id = :ws AND workspace_members.member_id = :user AND workspace_members.is_active ORDER BY workspaces.created_at DESC LIMIT 1",
        last_workspace_join_sql()
    )
}

/// Fallback earliest membership (`user.py:127-131`):
/// `.filter(workspace_member__member_id=..., ...is_active=True)`
/// `.order_by("created_at").first()`.
///
/// Same unscoped join as [`last_workspace_join_sql`], explicit
/// ascending order, `LIMIT 1`. Reached when `last_workspace_id` is
/// `None` OR the membership check above fails.
pub fn fallback_workspace_sql() -> String {
    format!(
        "SELECT workspaces.* FROM workspaces {} WHERE workspaces.deleted_at IS NULL AND workspace_members.member_id = :user AND workspace_members.is_active ORDER BY workspaces.created_at ASC LIMIT 1",
        last_workspace_join_sql()
    )
}

/// Logo-asset lazy follow (`user.py:116`):
/// `workspace.logo_asset.asset_url if workspace.logo_asset is not None`.
///
/// Forward-FK access through the default manager, so the soft-delete
/// scope applies (`FileAsset` extends `BaseModel`,
/// `db/models/asset.py:28`). One extra round-trip when
/// `logo_asset_id` is set and uncached; `asset_url` itself
/// (`asset.py:80-91`) is pure string formatting for workspace logos
/// (no further SQL).
pub fn logo_asset_lookup_sql() -> String {
    "SELECT file_assets.* FROM file_assets WHERE file_assets.id = :asset_id AND file_assets.deleted_at IS NULL".to_owned()
}

// ---------------------------------------------------------------------------
// Unit 1b — instance-admin + session reads (views/user/base.py:87-90, 359-371)
// ---------------------------------------------------------------------------

/// `Instance.objects.first()` (`base.py:88`): soft-delete scope
/// (`Instance` extends `BaseModel`, `license/models/instance.py:22`),
/// default `-created_at` ordering, `LIMIT 1`. May yield no row — the
/// caller binds `None` into [`instance_admin_exists_sql`].
pub fn instance_first_sql() -> String {
    "SELECT instances.* FROM instances WHERE instances.deleted_at IS NULL ORDER BY instances.created_at DESC LIMIT 1".to_owned()
}

/// `InstanceAdmin.objects.filter(instance=instance,
/// user=request.user).exists()` (`base.py:89`).
///
/// When `Instance.objects.first()` returned no row, `instance` binds
/// `None` and the predicate renders `instance_id IS NULL`.
pub fn instance_admin_exists_sql() -> String {
    "SELECT 1 FROM instance_admins WHERE instance_admins.instance_id = :instance AND instance_admins.user_id = :user AND instance_admins.deleted_at IS NULL LIMIT 1".to_owned()
}

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

/// Availability check (`:129`, re-checked finally at `:225`):
/// `User.objects.filter(email=new_email).exclude(id=user.id).exists()`.
///
/// `new_email` is pre-normalized (see [`email_update_cache_key`]);
/// `User.save()` lowercases again on write, a no-op). `users` has no
/// soft-delete scope. Django renders the `.exclude()` as `NOT (...)`.
pub fn email_availability_exists_sql() -> String {
    "SELECT 1 FROM users WHERE users.email = :email AND NOT (users.id = :user) LIMIT 1".to_owned()
}

/// Email save (`:232-235`): `user.email = new_email`,
/// `user.is_email_verified = False`, `user.save()`.
///
/// Django emits a full-row `UPDATE`; per the D-24 queries convention
/// the builder lists the semantically-changed columns plus the
/// `auto_now` stamp. `User.save()` (`db/models/user.py:154-177`)
/// additionally lowercases the email (no-op — already normalized),
/// backfills an empty `display_name` from the address, and rotates
/// `token` iff `token_updated_at` is set (model-owned, PIDASHCONV-607).
pub fn email_save_sql() -> String {
    "UPDATE users SET email = :email, is_email_verified = FALSE, updated_at = :now WHERE id = :pk"
        .to_owned()
}

/// Session invalidation for `logout(request)` (`:241`, also `:355`):
/// the session flush deletes the current session row by key
/// (`Session` extends Django's `AbstractBaseSession` — hard delete,
/// no soft-delete manager).
pub fn session_flush_sql() -> String {
    "DELETE FROM sessions WHERE session_key = :key".to_owned()
}

// ---------------------------------------------------------------------------
// Unit 3 — deactivate bundle (views/user/base.py:252-357)
// ---------------------------------------------------------------------------

/// Instance-admin guard (`:257`):
/// `InstanceAdmin.objects.filter(user=user).exists()`.
///
/// NOTE the asymmetry with [`instance_admin_exists_sql`]: this guard
/// filters on the user ALONE — no `instance` predicate — so an admin
/// row on ANY (non-soft-deleted) instance blocks deactivation.
pub fn deactivate_instance_admin_guard_sql() -> String {
    "SELECT 1 FROM instance_admins WHERE instance_admins.user_id = :user AND instance_admins.deleted_at IS NULL LIMIT 1".to_owned()
}

/// `project_members` concrete columns in `_meta` order (abstract
/// parents first, then `id`, then per-level declared fields — probed
/// against Django 4.2 field ordering): the SELECT/GROUP BY column set
/// for [`project_deactivate_scan_sql`].
pub const PROJECT_DEACTIVATE_SCAN_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "project_id",
    "workspace_id",
    "member_id",
    "comment",
    "role",
    "view_props",
    "default_props",
    "preferences",
    "sort_order",
    "is_active",
];

/// `workspace_members` concrete columns in `_meta` order (same
/// convention): the SELECT/GROUP BY column set for
/// [`workspace_deactivate_scan_sql`].
pub const WORKSPACE_DEACTIVATE_SCAN_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
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

/// Shared `COUNT(CASE ...)` annotation expression
/// (`:267-273`, `:288-294`):
/// `Count(Case(When(Q(role=20, is_active=True) & ~Q(member=user),
/// then=1), default=0))`.
///
/// `nullable_member` selects the `IS NOT NULL` inside the negation:
/// `ProjectMember.member` is nullable (`project.py:333-338`) so Django
/// renders `NOT (member_id = :user AND member_id IS NOT NULL)`
/// (probed verbatim); `WorkspaceMember.member` is NOT NULL
/// (`workspace.py:199-203`) so the guard is omitted. Predicate order
/// (`is_active` before `role`) is the probed Django 4.2 render order.
fn sole_admin_case_sql(table: &str, nullable_member: bool) -> String {
    if nullable_member {
        format!(
            "COUNT(CASE WHEN ({table}.is_active AND {table}.role = {admin} AND NOT ({table}.member_id = :user AND {table}.member_id IS NOT NULL)) THEN 1 ELSE 0 END) AS other_admin_exists",
            admin = ROLE_ADMIN,
        )
    } else {
        format!(
            "COUNT(CASE WHEN ({table}.is_active AND {table}.role = {admin} AND NOT ({table}.member_id = :user)) THEN 1 ELSE 0 END) AS other_admin_exists",
            admin = ROLE_ADMIN,
        )
    }
}

/// Shared scan renderer: `SELECT <cols>, <case>, COUNT(<table>.id)
/// AS total_members FROM <table> WHERE member + active + manager
/// scope GROUP BY <cols>`.
///
/// Ported bug 4 (probed live on Django 4.2): the annotation groups BY
/// the row itself — `GROUP BY` every selected column — and Django
/// strips the default ordering when the annotation lands, so there is
/// NO `ORDER BY`. Consequence: `other_admin_exists` counts
/// `CASE ... ELSE 0`, whose `ELSE 0` is never NULL, so `COUNT`
/// yields 1 for every group of one; `total_members` is 1 too. The
/// caller's `other_admin_exists > 0 or total_members == 1` is
/// therefore always true and the `400` "only admin" branches are
/// unreachable — every active membership is collected.
fn deactivate_scan_sql(table: &str, columns: &[&str], nullable_member: bool) -> String {
    let mut select_cols = String::new();
    let mut group_cols = String::new();
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            select_cols.push_str(", ");
            group_cols.push_str(", ");
        }
        let _ = write!(select_cols, "{table}.{col}");
        let _ = write!(group_cols, "{table}.{col}");
    }
    format!(
        "SELECT {select_cols}, {case_expr}, COUNT({table}.id) AS total_members FROM {table} WHERE {table}.member_id = :user AND {table}.is_active AND {table}.deleted_at IS NULL GROUP BY {group_cols}",
        case_expr = sole_admin_case_sql(table, nullable_member),
    )
}

/// Project sole-admin annotation scan (`:266-275`):
/// `ProjectMember.objects.filter(member=user,
/// is_active=True).annotate(other_admin_exists=...,
/// total_members=Count("id"))`. See [`deactivate_scan_sql`] for the
/// vacuous-guard consequence (ported bug 4).
pub fn project_deactivate_scan_sql() -> String {
    deactivate_scan_sql("project_members", PROJECT_DEACTIVATE_SCAN_COLUMNS, true)
}

/// Workspace sole-admin annotation scan (`:287-296`): same shape over
/// `workspace_members` (non-nullable `member_id`, so no `IS NOT NULL`
/// inside the negation). See [`deactivate_scan_sql`].
pub fn workspace_deactivate_scan_sql() -> String {
    deactivate_scan_sql(
        "workspace_members",
        WORKSPACE_DEACTIVATE_SCAN_COLUMNS,
        false,
    )
}

/// `bulk_update` batch size (`:308`, `:310`): `batch_size=100`.
pub const BULK_DEACTIVATE_BATCH_SIZE: usize = 100;

/// Bulk deactivation write (`:308`, `:310`):
/// `bulk_update(rows, ["is_active"], batch_size=100)`.
///
/// Ported bug 5: `bulk_update` bypasses `save()`, so `auto_now` does
/// NOT stamp `updated_at`, and the write is pk-direct with NO
/// `deleted_at` filter. The handler chunks `:pks` into batches of
/// [`BULK_DEACTIVATE_BATCH_SIZE`]; an empty collection issues NO
/// statement at all.
pub fn project_bulk_deactivate_sql() -> String {
    "UPDATE project_members SET is_active = FALSE WHERE project_members.id IN (:pks)".to_owned()
}

/// Workspace bulk deactivation write (`:310`): same shape as
/// [`project_bulk_deactivate_sql`].
pub fn workspace_bulk_deactivate_sql() -> String {
    "UPDATE workspace_members SET is_active = FALSE WHERE workspace_members.id IN (:pks)".to_owned()
}

/// Invite purge (`:313`):
/// `WorkspaceMemberInvite.objects.filter(email=user.email).delete()`.
///
/// `QuerySet.delete()` on a soft-delete model renders `UPDATE ...
/// SET deleted_at = :now` — a second `updated_at` stamp is NOT
/// applied (`QuerySet.update` bypasses `auto_now`). Manager scope on
/// the read side is preserved in the `WHERE`.
pub fn deactivate_invite_purge_sql() -> String {
    "UPDATE workspace_member_invites SET deleted_at = :now WHERE workspace_member_invites.email = :email AND workspace_member_invites.deleted_at IS NULL".to_owned()
}

/// Session purge (`:316`): `Session.objects.filter(user_id=...).delete()`.
///
/// HARD `DELETE` — `Session` has no soft-delete manager. Ported bug
/// 11: `user_id` is a `CharField` (`session.py:21`), so the UUID
/// binds as text.
pub fn deactivate_session_purge_sql() -> String {
    "DELETE FROM sessions WHERE sessions.user_id = :user".to_owned()
}

/// Onboarding-step reset value (`:325-330`), exact key order as
/// written in the view (JSON object insertion order is preserved on
/// the wire).
pub const ONBOARDING_RESET_JSON: &str = r#"{"workspace_join": false, "profile_complete": false, "workspace_create": false, "workspace_invite": false}"#;

/// Profile reset (`:319-339`): `Profile.objects.get(user=user)` (see
/// [`profile_by_user_sql`]) then `save(update_fields=[
/// "last_workspace_id", "is_tour_completed", "is_onboarded",
/// "onboarding_step", "updated_at"])`.
///
/// Instance `save()` addresses the row by pk. `last_workspace_id` is
/// a plain nullable `UUIDField`, not an FK (`user.py:236`).
pub fn deactivate_profile_reset_sql() -> String {
    "UPDATE profiles SET last_workspace_id = NULL, is_tour_completed = FALSE, is_onboarded = FALSE, onboarding_step = :step_json, updated_at = :now WHERE id = :pk".to_owned()
}

/// User deactivation save (`:342-349`): `is_password_autoset = True`,
/// `set_password(uuid4().hex)`, `is_active = False`,
/// `last_logout_ip/time` stamps, full `save()`.
///
/// Django emits a full-row `UPDATE`; per convention the builder lists
/// the semantically-changed columns plus `updated_at`. `:hash` is the
/// PBKDF2 rendering of a fresh `uuid4().hex` (Django `set_password`
/// format — the handler hashes, this module binds). `last_logout_ip`
/// comes from `user_ip(request)` (`:347`), `last_logout_time` from
/// `timezone.now()` (`:348`). (`user_deactivation_email.delay(...)`
/// at `:352` is owned by PIDASHCONV-614; `logout(request)` at `:355`
/// is [`session_flush_sql`].)
pub fn deactivate_user_save_sql() -> String {
    "UPDATE users SET password = :hash, is_password_autoset = TRUE, is_active = FALSE, last_logout_ip = :ip, last_logout_time = :now, updated_at = :now WHERE id = :pk".to_owned()
}

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

/// Account delete (`:419`): instance `.delete()` on a model WITHOUT
/// the soft-delete mixin — a HARD `DELETE` addressed by pk, with no
/// related-object task.
pub fn account_hard_delete_sql() -> String {
    "DELETE FROM accounts WHERE accounts.id = :pk".to_owned()
}

/// Onboard-patch `update_fields` (`:380`).
pub const ONBOARD_UPDATE_FIELDS: &[&str] = &["is_onboarded", "updated_at"];

/// Onboard patch (`:375-380`): `Profile.objects.get(user_id=...)`
/// (see [`profile_by_user_sql`]) then
/// `save(update_fields=["is_onboarded", "updated_at"])`.
///
/// Ported bug 6: the value is the raw
/// `request.data.get("is_onboarded", False)` (`:376`) — no serializer
/// validation or coercion runs; whatever JSON value the client sent
/// binds at `:value` (defaulting to `False` only when the key is
/// absent).
pub fn onboard_patch_sql() -> String {
    "UPDATE profiles SET is_onboarded = :value, updated_at = :now WHERE id = :pk".to_owned()
}

/// Tour-patch `update_fields` (`:388`).
pub const TOUR_UPDATE_FIELDS: &[&str] = &["is_tour_completed", "updated_at"];

/// Tour patch (`:386-388`): same shape as [`onboard_patch_sql`] for
/// `is_tour_completed` (`request.data.get("is_tour_completed",
/// False)`, `:387` — likewise unvalidated, ported bug 6).
pub fn tour_patch_sql() -> String {
    "UPDATE profiles SET is_tour_completed = :value, updated_at = :now WHERE id = :pk".to_owned()
}

/// Profile locked read (`:455`):
/// `Profile.objects.select_for_update().get(user=request.user)`
/// inside `transaction.atomic()` (`:454`).
///
/// No `deleted_at` exists on `profiles`, so the lock read is
/// unscoped. Profile GET (`:427`) reuses [`profile_by_user_sql`].
pub fn profile_lock_sql() -> String {
    "SELECT profiles.* FROM profiles WHERE profiles.user_id = :user FOR UPDATE".to_owned()
}

/// `profiles` concrete columns in `_meta` order minus the pk:
/// the full-row `UPDATE` SET set for [`profile_full_save_sql`]
/// (`user.py:223-271` declared order over the `TimeAuditModel` pair —
/// abstract parents first, then declared fields).
pub const PROFILE_SAVE_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
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

/// Profile patch save (`:456-461`): `ProfileSerializer.save()` issues
/// a full-row `UPDATE` — `settings` included, since the instance
/// carries it whether or not the serializer may write it (`:448-449`).
///
/// The `settings` merge (`user_settings.merge_settings`, `:460`) runs
/// BEFORE this write and is handler-owned (PIDASHCONV-620); by the
/// time this statement executes, `:settings` already holds the merged
/// bag. Every column renders as `col = :col`; the row is addressed
/// by pk.
pub fn profile_full_save_sql() -> String {
    let mut set_clause = String::new();
    for (i, col) in PROFILE_SAVE_COLUMNS.iter().enumerate() {
        if i > 0 {
            set_clause.push_str(", ");
        }
        let _ = write!(set_clause, "{col} = :{col}");
    }
    format!("UPDATE profiles SET {set_clause} WHERE id = :pk")
}

// ---------------------------------------------------------------------------
// Unit 4b — API tokens (views/api.py:20-72)
// ---------------------------------------------------------------------------

/// `api_tokens` concrete columns in `_meta` order (abstract parents
/// first, then `id`, then `api.py:37-51` declared order): the
/// `INSERT` column set for [`api_token_insert_sql`].
pub const API_TOKEN_INSERT_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
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

/// Token create (`:29-35`):
/// `APIToken.objects.create(label=..., description=...,
/// user=..., user_type=..., expired_at=...)`.
///
/// Python-side defaults resolved BEFORE the `INSERT` (`:22-27`):
/// `label` falls back to `uuid4().hex` only when the key is absent
/// (ported bug 10 — an explicit empty label stores empty);
/// `description` defaults to `""`; `expired_at` defaults to `None`;
/// `user_type` is `1` iff `request.user.is_bot`, else `0`.
/// Model-side: `token` is `generate_token()` (`"pi_dash_api_" +
/// `uuid4().hex`, `api.py:20-21`); `is_active` is `TRUE`;
/// `is_service` is `FALSE`; `workspace_id` is `NULL`;
/// `allowed_rate_limit` is `"60/min"`; `created_by_id` is stamped
/// from the request user by `BaseModel.save()` (`None` when
/// anonymous); `updated_by_id` stays `NULL` on create.
pub fn api_token_insert_sql() -> String {
    "INSERT INTO api_tokens (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, label, description, is_active, last_used, token, user_id, user_type, workspace_id, expired_at, is_service, allowed_rate_limit) VALUES (:now, :now, :actor, NULL, NULL, :id, :label, :description, TRUE, NULL, :token, :user, :user_type, NULL, :expired_at, FALSE, '60/min')".to_owned()
}

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

/// Token delete write (`:53`): instance `.delete()` on a soft-delete
/// model — `deleted_at` is stamped and the instance is SAVED, so
/// `updated_at` moves too (same full-row-save shape as the sibling
/// `my_invite_soft_delete_sql`). The write also enqueues
/// `soft_delete_related_objects` (tasks-owned, PIDASHCONV-614).
pub fn api_token_soft_delete_sql() -> String {
    "UPDATE api_tokens SET deleted_at = :now, updated_at = :now WHERE id = :pk".to_owned()
}

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

/// Token PATCH writable fields (probed live by PIDASHCONV-604 against
/// `APITokenSerializer`, `api.py:11-24`): everything else in
/// `fields = "__all__"` is `read_only` (plus DRF-default `id`) and
/// silently ignored on input.
///
/// Ported bug 9: `is_service` is writable — a PATCH can flip a
/// token's own service flag.
pub const API_TOKEN_PATCH_WRITABLE: &[&str] =
    &["label", "description", "is_service", "allowed_rate_limit"];

/// Token PATCH save (`:68-71`): `serializer.save()` on the partial
/// serializer issues a full-row `UPDATE`; per convention the builder
/// lists the writable columns ([`API_TOKEN_PATCH_WRITABLE`]) plus the
/// `auto_now` stamp.
pub fn api_token_patch_save_sql() -> String {
    "UPDATE api_tokens SET label = :label, description = :description, is_service = :is_service, allowed_rate_limit = :rate, updated_at = :now WHERE id = :pk".to_owned()
}

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
            invite_count_sql(),
            logo_asset_lookup_sql(),
            instance_first_sql(),
            instance_admin_exists_sql(),
            deactivate_instance_admin_guard_sql(),
            project_deactivate_scan_sql(),
            workspace_deactivate_scan_sql(),
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
        // The last-workspace statements scope the BASE table while the
        // joined member table stays deliberately unscoped (ported bug 2).
        for sql in [
            last_workspace_exists_sql(),
            last_workspace_fetch_sql(),
            fallback_workspace_sql(),
        ] {
            assert!(
                sql.contains("workspaces.deleted_at IS NULL"),
                "missing base scope: {sql}"
            );
            assert!(
                !sql.contains("workspace_members.deleted_at"),
                "joined member table must stay unscoped: {sql}"
            );
        }
        // Writes: the invite purge preserves the manager scope in its
        // WHERE; bulk deactivations are pk-direct (ported bug 5);
        // session/account deletes are hard (no soft-delete mixin).
        assert!(deactivate_invite_purge_sql().contains("deleted_at IS NULL"));
        assert!(!project_bulk_deactivate_sql().contains("deleted_at"));
        assert!(!workspace_bulk_deactivate_sql().contains("deleted_at"));
        // users / profiles / accounts / sessions have no deleted_at
        // column — assert absence, not scope.
        for sql in [
            profile_by_user_sql(),
            session_user_sql(),
            email_availability_exists_sql(),
            account_list_sql(),
            account_detail_sql(),
            profile_lock_sql(),
        ] {
            assert!(!sql.contains("deleted_at"), "no such column: {sql}");
        }
    }

    #[test]
    fn unit1_invite_count_has_no_accepted_filter() {
        // Ported bug 1 (serializers/user.py:99).
        let sql = invite_count_sql();
        assert!(sql.contains("COUNT(*)"));
        assert!(sql.contains("workspace_member_invites.email = :email"));
        assert!(!sql.contains("accepted"));
    }

    #[test]
    fn unit1_last_workspace_branches() {
        // Check (user.py:104-110): pk + member + active, SELECT 1 LIMIT 1.
        let exists = last_workspace_exists_sql();
        assert!(exists.starts_with("SELECT 1 FROM workspaces "));
        assert!(exists.contains("workspaces.id = :ws"));
        assert!(exists.contains("workspace_members.member_id = :user"));
        assert!(exists.contains("workspace_members.is_active"));
        assert!(exists.ends_with("LIMIT 1"));
        assert!(!exists.contains("ORDER BY"));
        // Fetch (user.py:111-115): same filter re-run with .first() —
        // ported bug 3 — default -created_at order, LIMIT 1.
        let fetch = last_workspace_fetch_sql();
        assert!(fetch.starts_with("SELECT workspaces.* FROM workspaces "));
        assert!(fetch.contains("workspaces.id = :ws"));
        assert!(fetch.contains("ORDER BY workspaces.created_at DESC LIMIT 1"));
        // Fallback (user.py:127-131): no pk, explicit ASC, LIMIT 1.
        let fallback = fallback_workspace_sql();
        assert!(!fallback.contains("workspaces.id = :ws"));
        assert!(fallback.contains("ORDER BY workspaces.created_at ASC LIMIT 1"));
    }

    #[test]
    fn unit1_instance_and_session_reads() {
        let first = instance_first_sql();
        assert!(first.contains("FROM instances"));
        assert!(first.contains("ORDER BY instances.created_at DESC LIMIT 1"));
        let admin = instance_admin_exists_sql();
        assert!(admin.contains("instance_admins.instance_id = :instance"));
        assert!(admin.contains("instance_admins.user_id = :user"));
        // Deactivate guard filters on the user ALONE (no instance
        // predicate) — the documented asymmetry.
        let guard = deactivate_instance_admin_guard_sql();
        assert!(!guard.contains("instance_id = :instance"));
        assert!(guard.contains("instance_admins.user_id = :user"));
        let session = session_user_sql();
        assert_eq!(session, "SELECT users.* FROM users WHERE users.id = :user");
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
        // Availability (:129, :225): email match minus self, no scope.
        let avail = email_availability_exists_sql();
        assert!(avail.contains("users.email = :email"));
        assert!(avail.contains("NOT (users.id = :user)"));
        // Save (:232-235): email + unverified + stamp.
        let save = email_save_sql();
        assert!(save.contains("email = :email"));
        assert!(save.contains("is_email_verified = FALSE"));
        assert!(save.contains("updated_at = :now"));
        // Logout flush (:241, :355): hard delete by session key.
        assert_eq!(
            session_flush_sql(),
            "DELETE FROM sessions WHERE session_key = :key"
        );
    }

    #[test]
    fn unit3_deactivate_scans_group_by_the_row() {
        // Ported bug 4 (base.py:266-296, probed live on Django 4.2).
        assert_eq!(ROLE_ADMIN, 20);
        assert_eq!(PROJECT_DEACTIVATE_SCAN_COLUMNS.len(), 16);
        assert_eq!(WORKSPACE_DEACTIVATE_SCAN_COLUMNS.len(), 17);
        for (sql, table, nullable) in [
            (project_deactivate_scan_sql(), "project_members", true),
            (workspace_deactivate_scan_sql(), "workspace_members", false),
        ] {
            // WHERE: member + active + manager scope.
            assert!(sql.contains(&format!("{table}.member_id = :user")));
            assert!(sql.contains(&format!("{table}.is_active")));
            // COUNT(CASE ... ELSE 0) — ELSE 0 is never NULL so COUNT
            // always yields 1 per group of one (vacuous guard).
            assert!(sql.contains("COUNT(CASE WHEN ("));
            assert!(sql.contains("THEN 1 ELSE 0 END) AS other_admin_exists"));
            assert!(sql.contains(&format!("COUNT({table}.id) AS total_members")));
            // GROUP BY every selected column; NO ORDER BY.
            let group_by = sql.split("GROUP BY ").nth(1).expect("GROUP BY present");
            assert_eq!(group_by.split(", ").count(), if nullable { 16 } else { 17 });
            assert!(!sql.contains("ORDER BY"));
        }
        // Nullable-FK negation guard only on the project variant.
        assert!(project_deactivate_scan_sql().contains(
            "NOT (project_members.member_id = :user AND project_members.member_id IS NOT NULL)"
        ));
        assert!(
            workspace_deactivate_scan_sql().contains("NOT (workspace_members.member_id = :user)")
        );
        assert!(!workspace_deactivate_scan_sql().contains("IS NOT NULL"));
    }

    #[test]
    fn unit3_deactivate_writes() {
        // bulk_update batch 100, is_active only, no stamp, no scope.
        assert_eq!(BULK_DEACTIVATE_BATCH_SIZE, 100);
        assert_eq!(
            project_bulk_deactivate_sql(),
            "UPDATE project_members SET is_active = FALSE WHERE project_members.id IN (:pks)"
        );
        assert_eq!(
            workspace_bulk_deactivate_sql(),
            "UPDATE workspace_members SET is_active = FALSE WHERE workspace_members.id IN (:pks)"
        );
        // Invite purge: soft, no updated_at stamp.
        let purge = deactivate_invite_purge_sql();
        assert!(purge.contains("SET deleted_at = :now WHERE"));
        assert!(!purge.contains("updated_at"));
        assert!(purge.contains("workspace_member_invites.email = :email"));
        // Session purge: hard delete, text user_id.
        assert_eq!(
            deactivate_session_purge_sql(),
            "DELETE FROM sessions WHERE sessions.user_id = :user"
        );
        // Profile reset: exact update_fields + reset JSON key order.
        assert_eq!(
            ONBOARDING_RESET_JSON,
            r#"{"workspace_join": false, "profile_complete": false, "workspace_create": false, "workspace_invite": false}"#
        );
        assert_eq!(
            deactivate_profile_reset_sql(),
            "UPDATE profiles SET last_workspace_id = NULL, is_tour_completed = FALSE, is_onboarded = FALSE, onboarding_step = :step_json, updated_at = :now WHERE id = :pk"
        );
        // User save: hash + autoset + deactivate + logout stamps.
        let save = deactivate_user_save_sql();
        for frag in [
            "password = :hash",
            "is_password_autoset = TRUE",
            "is_active = FALSE",
            "last_logout_ip = :ip",
            "last_logout_time = :now",
            "updated_at = :now",
            "WHERE id = :pk",
        ] {
            assert!(save.contains(frag), "missing {frag}");
        }
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
        assert_eq!(
            onboard_patch_sql(),
            "UPDATE profiles SET is_onboarded = :value, updated_at = :now WHERE id = :pk"
        );
        assert_eq!(
            tour_patch_sql(),
            "UPDATE profiles SET is_tour_completed = :value, updated_at = :now WHERE id = :pk"
        );
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
        // INSERT column set: audit parents first, then id, then declared.
        assert_eq!(API_TOKEN_INSERT_COLUMNS.len(), 17);
        let insert = api_token_insert_sql();
        assert!(insert.contains("INSERT INTO api_tokens (created_at, updated_at"));
        assert!(insert.contains(":token, :user, :user_type, NULL, :expired_at, FALSE, '60/min'"));
        // List carries is_service=False; detail omits it (ported bug 7).
        assert!(api_token_list_sql().contains("api_tokens.is_service = FALSE"));
        assert!(!api_token_detail_sql().contains("is_service"));
        assert!(api_token_detail_sql().contains("api_tokens.id = :pk"));
        // Delete lookup re-adds the guard; write is a stamp-and-save.
        assert!(api_token_delete_lookup_sql().contains("api_tokens.is_service = FALSE"));
        assert_eq!(
            api_token_soft_delete_sql(),
            "UPDATE api_tokens SET deleted_at = :now, updated_at = :now WHERE id = :pk"
        );
        // PATCH lookup: scoped .first(); writable set per SER-E probe.
        let lookup = api_token_patch_lookup_sql();
        assert!(lookup.contains("api_tokens.is_service = FALSE"));
        assert!(lookup.ends_with("ORDER BY api_tokens.created_at DESC LIMIT 1"));
        assert_eq!(
            API_TOKEN_PATCH_WRITABLE,
            &["label", "description", "is_service", "allowed_rate_limit"]
        );
        let save = api_token_patch_save_sql();
        assert!(save.contains("is_service = :is_service"));
        assert!(!save.contains("expired_at"));
        assert!(!save.contains("user_type"));
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
