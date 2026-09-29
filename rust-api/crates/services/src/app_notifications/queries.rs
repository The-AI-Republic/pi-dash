#![forbid(unsafe_code)]

//! App notification queries (D-34, stage 5).
//!
//! Port of the four query units in
//! `apps/api/pi_dash/app/views/notification/base.py`:
//!
//! * `base_scope` — `NotificationViewSet.get_queryset` (`:36-45`):
//!   `workspace__slug` + `receiver_id` scoping with
//!   `select_related("workspace", "project", "triggered_by", "receiver")`.
//! * `list_query` — `NotificationViewSet.list` queryset (`:57-137`).
//! * `unread_counts` — `UnreadNotificationEndpoint.get` (`:202-221`).
//! * `mark_all_read_filter` + bulk `read_at` update —
//!   `MarkAllReadNotificationViewSet.create` (`:239-287`).
//!
//! Recorded in `rust-api/fixtures/app_notifications/` (filed by
//! PIDASHCONV-297; trace: `rust-api/fixtures/app_notifications/TRACE.md`):
//! FX-NOTIF-01 (column list), FX-NOTIF-06 (list SQL + rows), FX-NOTIF-07
//! (unread SQL + rows), FX-NOTIF-08 (mark-all-read SQL + before/after).
//! The `#[cfg(test)]` suite replays those fixtures.
//!
//! Conventions (same as the merged `auth_session::queries` and D-29
//! `app_views_search` kernels): everything here is pure over injected
//! inputs. Builders take bind placeholders (`$1`, …) as parameters and
//! return `String`; no database handle is held, nothing executes here.
//! Execution belongs to the handlers layer, which binds the documented
//! `$N` params in order. Django spells placeholders `%s` / `%(name)s`;
//! `$N` is the driver-level translation, the predicates are unchanged.
//!
//! Explicit request context on every path: every read builder requires
//! *both* scope placeholders (workspace slug and receiver id) — there is
//! no builder that reads without both conjuncts, so handlers can never
//! obtain an unscoped statement. [`NotificationScope`] is the pair the
//! handler threads through, and [`NotificationScope::bind_order`] pins the
//! conventional `$1` = slug, `$2` = user binding the builders document.
//!
//! # Ported bugs and quirks (translate, don't redesign — also listed in the PR)
//!
//! * BUG-snoozed-true (`base.py:81`, fixture `list.sql`): the
//!   `snoozed=true` branch is `snoozed_till < now OR snoozed_till IS NOT
//!   NULL`, which matches every row whose `snoozed_till` is set — past
//!   *and* future — while excluding NULL-snooze rows. [`snoozed_clause`]
//!   ports it verbatim. The mark-all-read path (`:247`) repeats the same
//!   OR.
//! * BUG-double-exists (`base.py:66-67`): `is_inbox_issue` and
//!   `is_intake_issue` annotate the *same* intake `Exists` subquery.
//!   [`list_select_sql`] emits both columns from one shared
//!   [`intake_exists_sql`].
//! * QUIRK-mentioned-truthiness (`base.py:54,100`): the list `mentioned`
//!   param defaults to boolean `False`, but any *present* query value —
//!   including the string `"false"` — is truthy in Python, so only an
//!   absent param takes the `exclude` branch. [`mentioned_clause`] ports
//!   presence (not value) as the branch signal.
//! * QUIRK-snoozed-archived-keyerror (`base.py:80-92`): `snoozed` /
//!   `archived` look their filter up by dict key, so any value other than
//!   `"true"` / `"false"` raises `KeyError` (500 via `handle_exception`).
//!   [`snoozed_clause`] / [`archived_clause`] return
//!   [`ParamError::UnknownValue`] for those inputs; the handler maps it to
//!   the Django 500.
//! * QUIRK-read-silent (`base.py:52,94-98`): `read` defaults to `None` and
//!   only the exact strings `"true"` / `"false"` add a clause; anything
//!   else is silently ignored. [`read_clause`] ports that.
//! * QUIRK-created-none (`base.py:126-129`, `:273-276`): with
//!   `type=created` (list) / `type="created"` (mark-all-read), a workspace
//!   member whose role is sub-15 sees *nothing* — the queryset becomes
//!   `.none()`, which short-circuits to an empty list without hitting the
//!   database. [`TypeFilter::Empty`] ports the short-circuit; the handler
//!   must return `[]` without querying when the [`created_member_guard_sql`]
//!   check hits.
//! * QUIRK-type-spelling (`base.py:105-135` vs `:258-281`): the list path
//!   comma-splits `type` and matches `subscribed/assigned/created`, while
//!   mark-all-read exact-matches `watching/assigned/created` — and its
//!   `watching` branch is a plain subscriber list without the list path's
//!   created/assigned exclusions. Both spellings are ported as-is.
//! * QUIRK-no-entity-guard (`base.py:202-221,239-243`): unlike the list
//!   path's `entity_name="issue"` guard (`:65`), neither the unread counts
//!   nor the mark-all-read base filter it. Ported as-is.
//! * QUIRK-per-row-now (`base.py:283-287`): the loop calls
//!   `timezone.now()` per row, so touched rows get distinct
//!   microseconds-apart `read_at` values — not one shared timestamp — and
//!   `bulk_update(["read_at"], batch_size=100)` neither runs signals nor
//!   refreshes `updated_at`. [`read_stamps`] ports the per-row call;
//!   [`BULK_UPDATE_BATCH_SIZE`] pins the batch size.

use serde_json::Value;

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// Physical table (`Meta.db_table`, `db/models/notification.py:38`).
pub const NOTIFICATION_TABLE: &str = "notifications";
/// Joined workspace table.
pub const WORKSPACE_TABLE: &str = "workspaces";
/// Intake join tables for the `Exists` annotation (`base.py:57-61`).
pub const ISSUE_TABLE: &str = "issues";
/// Intake status table (`issue_intake`, joined `issue_id`).
pub const ISSUE_INTAKE_TABLE: &str = "issue_intake";
/// Subscriber link table (list `:108-114`, mark-all-read `:259-261`).
pub const ISSUE_SUBSCRIBER_TABLE: &str = "issue_subscribers";
/// Assignee link table (list `:111,119-121`, mark-all-read `:266-268`).
pub const ISSUE_ASSIGNEE_TABLE: &str = "issue_assignees";
/// Membership table for the `created` guard (`:126-128`, `:273-275`).
pub const WORKSPACE_MEMBER_TABLE: &str = "workspace_members";

/// Explicit request context carried by every builder in this module
/// (Porting guide: handlers never get an unscoped handle).
///
/// `workspace_slug` is the URL `slug` kwarg (`workspace__slug`);
/// `user_id` is `request.user.id` (`receiver_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationScope<'a> {
    /// Workspace slug from the URL (`workspace__slug`).
    pub workspace_slug: &'a str,
    /// Requesting user id (`receiver_id`).
    pub user_id: &'a str,
}

impl<'a> NotificationScope<'a> {
    /// Conventional bind order for the `$1` / `$2` scope placeholders every
    /// builder in this module documents: slug first, user second. The
    /// handler binds these values to `$1` / `$2` in this order.
    pub fn bind_order(&self) -> [&'a str; 2] {
        [self.workspace_slug, self.user_id]
    }
}

// ---------------------------------------------------------------------------
// Unit 1 — base_scope (base.py:36-45)
// ---------------------------------------------------------------------------

/// FX-NOTIF-01 — every `Notification` column in Django's `str(query)`
/// render order: the audit block first (`id`, `created_at`, `updated_at`,
/// `created_by_id`, `updated_by_id`, `deleted_at` — `db/models/base.py:18`,
/// `db/mixins.py:16-20,:26-42,:56-67`), then definition order
/// (`db/models/notification.py:14-33`).
///
/// Names are physical (`workspace_id`, not the `workspace` model-field
/// spelling the serializer shape uses). The `#[cfg(test)]` suite asserts
/// this set equals the FX-NOTIF-01 `columns` + `audit_columns` sets.
pub const NOTIFICATION_SELECT_COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "workspace_id",
    "project_id",
    "data",
    "entity_identifier",
    "entity_name",
    "title",
    "message",
    "message_html",
    "message_stripped",
    "sender",
    "triggered_by_id",
    "receiver_id",
    "read_at",
    "snoozed_till",
    "archived_at",
];

/// The four `select_related` joins (`base.py:44`), in call order. They
/// change no SQL fanout — one row per notification — but the executed
/// statement carries each table's full column list (needed for the FK ids
/// and the nested `triggered_by_details` the serializer renders).
pub const SELECT_RELATED_TABLES: &[&str] = &["workspaces", "projects", "users", "users"];

/// Render `"alias"."c1", "alias"."c2", …` in
/// [`NOTIFICATION_SELECT_COLUMNS`] order.
pub fn select_list(alias: &str) -> String {
    NOTIFICATION_SELECT_COLUMNS
        .iter()
        .map(|c| format!("\"{alias}\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `get_queryset` scope (`base.py:36-45`): `super().get_queryset()` is
/// `self.model.objects.all()` (`app/views/base.py:103-108`) — the
/// `SoftDeletionManager` (`db/mixins.py:56-58`), hence the `deleted_at`
/// conjunct — filtered by `workspace__slug` + `receiver_id`.
///
/// `$slug` / `$user` are the caller's bind placeholders for
/// `scope.workspace_slug` / `scope.user_id` (conventionally `$1` / `$2`).
pub fn base_where(notif_alias: &str, slug_param: &str, user_param: &str) -> String {
    format!(
        "\"{WORKSPACE_TABLE}\".\"slug\" = {slug_param} \
         AND \"{notif_alias}\".\"receiver_id\" = {user_param} \
         AND \"{notif_alias}\".\"deleted_at\" IS NULL"
    )
}

// ---------------------------------------------------------------------------
// Unit 2 — list_query (base.py:57-137)
// ---------------------------------------------------------------------------

/// The intake `Exists` subquery (`base.py:57-61`): issues whose intake row
/// has `status IN (0, 2, -2)` in this workspace, correlated on
/// `entity_identifier`.
///
/// `$slug` is the caller's workspace-slug placeholder. Both
/// `is_inbox_issue` and `is_intake_issue` reuse this one text
/// (BUG-double-exists, `:66-67`).
pub fn intake_exists_sql(notif_alias: &str, slug_param: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM \"{ISSUE_TABLE}\" U0 \
         INNER JOIN \"{ISSUE_INTAKE_TABLE}\" U1 ON (U0.\"id\" = U1.\"issue_id\") \
         INNER JOIN \"{WORKSPACE_TABLE}\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
         WHERE (U0.\"id\" = \"{notif_alias}\".\"entity_identifier\" \
         AND U1.\"status\" IN (0, 2, -2) \
         AND U2.\"slug\" = {slug_param}))"
    )
}

/// The `is_mentioned_notification` annotation (`base.py:68-74`):
/// `Case(When(sender__icontains="mentioned", then=True), default=False)`
/// — `icontains` renders `ILIKE '%mentioned%'` (Semantic traps).
pub fn mentioned_annotation_sql(notif_alias: &str) -> String {
    format!(
        "CASE WHEN \"{notif_alias}\".\"sender\" ILIKE '%mentioned%' THEN true ELSE false END \
         AS \"is_mentioned_notification\""
    )
}

/// Base list statement (`base.py:63-77`): the scope plus the
/// `entity_name="issue"` guard (`:65`), the double `Exists` annotations
/// (`:66-67`), the mentioned `Case/When` (`:68-74`), and
/// `order_by("snoozed_till", "-created_at")` (`:76` — `snoozed_till` ASC,
/// Postgres `NULLS LAST` by default, then `created_at` DESC).
///
/// `$slug` / `$user` bind the scope (`$1` / `$2` by convention). The
/// executed statement additionally selects the four
/// [`SELECT_RELATED_TABLES`] column lists (row-neutral; see unit 1).
pub fn list_select_sql(scope_slug: &str, scope_user: &str) -> String {
    let n = NOTIFICATION_TABLE;
    let exists = intake_exists_sql(n, scope_slug);
    let mentioned = mentioned_annotation_sql(n);
    format!(
        "SELECT {cols}, {exists} AS \"is_inbox_issue\", {exists} AS \"is_intake_issue\", {mentioned} \
         FROM \"{n}\" \
         INNER JOIN \"{WORKSPACE_TABLE}\" ON (\"{n}\".\"workspace_id\" = \"{WORKSPACE_TABLE}\".\"id\") \
         WHERE (\"{WORKSPACE_TABLE}\".\"slug\" = {scope_slug} \
         AND \"{n}\".\"receiver_id\" = {scope_user} \
         AND \"{n}\".\"entity_name\" = 'issue' \
         AND \"{n}\".\"deleted_at\" IS NULL) \
         ORDER BY \"{n}\".\"snoozed_till\" ASC, \"{n}\".\"created_at\" DESC",
        cols = select_list(n),
    )
}

/// Bad `snoozed` / `archived` param value: Django raises `KeyError`
/// (QUIRK-snoozed-archived-keyerror). The handler maps this to the Django
/// 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamError {
    /// The offending query param (`"snoozed"` or `"archived"`).
    pub param: &'static str,
    /// The value Django failed to look up.
    pub value: String,
}

/// `snoozed` filter (`base.py:80-85`; default `"false"`). `$now` binds
/// `timezone.now()` at evaluation time.
///
/// The `"true"` branch is BUG-snoozed-true (`snoozed_till < now OR
/// snoozed_till IS NOT NULL` — matches every set `snoozed_till`, past and
/// future); ported verbatim.
pub fn snoozed_clause(param: &str, now_param: &str) -> Result<String, ParamError> {
    let n = NOTIFICATION_TABLE;
    match param {
        "true" => Ok(format!(
            "(\"{n}\".\"snoozed_till\" < {now_param} OR \"{n}\".\"snoozed_till\" IS NOT NULL)"
        )),
        "false" => Ok(format!(
            "(\"{n}\".\"snoozed_till\" >= {now_param} OR \"{n}\".\"snoozed_till\" IS NULL)"
        )),
        other => Err(ParamError {
            param: "snoozed",
            value: other.to_string(),
        }),
    }
}

/// `archived` filter (`base.py:87-92`; default `"false"`).
pub fn archived_clause(param: &str) -> Result<&'static str, ParamError> {
    match param {
        "true" => Ok("\"notifications\".\"archived_at\" IS NOT NULL"),
        "false" => Ok("\"notifications\".\"archived_at\" IS NULL"),
        other => Err(ParamError {
            param: "archived",
            value: other.to_string(),
        }),
    }
}

/// `read` filter (`base.py:52,94-98`; default `None` = no clause).
/// Only the exact strings `"true"` / `"false"` filter; anything else —
/// including a present-but-odd value — is silently ignored
/// (QUIRK-read-silent).
pub fn read_clause(param: Option<&str>) -> Option<&'static str> {
    match param {
        Some("false") => Some("\"notifications\".\"read_at\" IS NULL"),
        Some("true") => Some("\"notifications\".\"read_at\" IS NOT NULL"),
        _ => None,
    }
}

/// `mentioned` filter (`base.py:54,100-103`): the branch signal is param
/// *presence*, not value (QUIRK-mentioned-truthiness) — any present value,
/// including `"false"`, takes the `icontains` arm; only an absent param
/// takes the `exclude` arm.
pub fn mentioned_clause(param_present: bool) -> &'static str {
    if param_present {
        "\"notifications\".\"sender\" ILIKE '%mentioned%'"
    } else {
        "NOT (\"notifications\".\"sender\" ILIKE '%mentioned%')"
    }
}

/// `subscribed` type subquery (list, `base.py:107-115`): the subscriber's
/// issue ids minus issues they created or are assigned to. `$slug` /
/// `$user` bind the workspace slug / user id.
pub fn list_subscribed_issue_ids_sql(slug_param: &str, user_param: &str) -> String {
    format!(
        "SELECT U0.\"issue_id\" FROM \"{ISSUE_SUBSCRIBER_TABLE}\" U0 \
         WHERE (U0.\"workspace_id\" = {slug_param} AND U0.\"subscriber_id\" = {user_param} \
         AND NOT EXISTS (SELECT 1 FROM \"{ISSUE_TABLE}\" WHERE created_by_id = {user_param} AND id = U0.\"issue_id\") \
         AND NOT EXISTS (SELECT 1 FROM \"{ISSUE_ASSIGNEE_TABLE}\" WHERE id = U0.\"issue_id\" AND assignee_id = {user_param}))"
    )
}

/// `assigned` type subquery (list, `base.py:118-122`).
pub fn list_assigned_issue_ids_sql(slug_param: &str, user_param: &str) -> String {
    format!(
        "SELECT \"issue_id\" FROM \"{ISSUE_ASSIGNEE_TABLE}\" \
         WHERE workspace_id = {slug_param} AND assignee_id = {user_param}"
    )
}

/// `created` type subquery (list, `base.py:131-133`): issues the user
/// created. Reached only when [`created_member_guard_sql`] finds no
/// sub-15 active membership (else [`TypeFilter::Empty`]).
pub fn list_created_issue_ids_sql(slug_param: &str, user_param: &str) -> String {
    format!(
        "SELECT \"id\" FROM \"{ISSUE_TABLE}\" \
         WHERE workspace_id = {slug_param} AND created_by_id = {user_param}"
    )
}

/// The `created` member guard (list `:126-128`): a sub-15 active workspace
/// membership. When it hits, the whole list queryset becomes `.none()`
/// (QUIRK-created-none). `$slug` / `$user` bind the workspace slug / user.
pub fn created_member_guard_sql(slug_param: &str, user_param: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM \"{WORKSPACE_MEMBER_TABLE}\" \
         WHERE workspace_id = {slug_param} AND member_id = {user_param} \
         AND role < 15 AND is_active)"
    )
}

/// Outcome of compiling the list `type` param (`base.py:105-137`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeFilter {
    /// Extra `AND (...)` fragment over `entity_identifier`.
    Where(String),
    /// `.none()`: return `[]` without querying (QUIRK-created-none).
    Empty,
    /// No known branch (`type=all` default): `.filter(Q())` no-op.
    None,
}

/// Compile the list `type` param: comma-split (`:105`), known branches OR
/// together (`:115,122,134` via `q_filters`), applied at `:137`.
///
/// `created_member` is the evaluated [`created_member_guard_sql`] result.
/// Unknown tokens (including the default `"all"`) match no branch.
pub fn list_type_filter(
    param: &str,
    slug_param: &str,
    user_param: &str,
    created_member: bool,
) -> TypeFilter {
    let mut arms = Vec::new();
    let mut saw_created = false;
    for token in param.split(',') {
        match token {
            "subscribed" => arms.push(format!(
                "\"{n}\".\"entity_identifier\" IN ({sub})",
                n = NOTIFICATION_TABLE,
                sub = list_subscribed_issue_ids_sql(slug_param, user_param),
            )),
            "assigned" => arms.push(format!(
                "\"{n}\".\"entity_identifier\" IN ({sub})",
                n = NOTIFICATION_TABLE,
                sub = list_assigned_issue_ids_sql(slug_param, user_param),
            )),
            "created" => saw_created = true,
            _ => {}
        }
    }
    if saw_created {
        if created_member {
            return TypeFilter::Empty;
        }
        arms.push(format!(
            "\"{n}\".\"entity_identifier\" IN ({sub})",
            n = NOTIFICATION_TABLE,
            sub = list_created_issue_ids_sql(slug_param, user_param),
        ));
    }
    if arms.is_empty() {
        TypeFilter::None
    } else {
        TypeFilter::Where(format!("({})", arms.join(" OR ")))
    }
}

// ---------------------------------------------------------------------------
// Unit 3 — unread_counts (base.py:202-221)
// ---------------------------------------------------------------------------

/// Watching count Q1 (`:202-212`): unread + unarchived + unsnoozed rows
/// outside the `mentioned` sender class. No `entity_name` guard
/// (QUIRK-no-entity-guard). `$slug` / `$user` bind the scope.
pub fn unread_watching_count_sql(slug_param: &str, user_param: &str) -> String {
    let n = NOTIFICATION_TABLE;
    format!(
        "SELECT COUNT(*) AS \"total_unread_notifications_count\" FROM \"{n}\" \
         INNER JOIN \"{WORKSPACE_TABLE}\" ON (\"{n}\".\"workspace_id\" = \"{WORKSPACE_TABLE}\".\"id\") \
         WHERE (\"{WORKSPACE_TABLE}\".\"slug\" = {slug_param} \
         AND \"{n}\".\"receiver_id\" = {user_param} \
         AND \"{n}\".\"read_at\" IS NULL \
         AND \"{n}\".\"archived_at\" IS NULL \
         AND \"{n}\".\"snoozed_till\" IS NULL \
         AND NOT (\"{n}\".\"sender\" ILIKE '%mentioned%') \
         AND \"{n}\".\"deleted_at\" IS NULL)"
    )
}

/// Mention count Q2 (`:214-221`): the same base with the `mentioned`
/// sender class included instead of excluded.
pub fn unread_mention_count_sql(slug_param: &str, user_param: &str) -> String {
    let n = NOTIFICATION_TABLE;
    format!(
        "SELECT COUNT(*) AS \"mention_unread_notifications_count\" FROM \"{n}\" \
         INNER JOIN \"{WORKSPACE_TABLE}\" ON (\"{n}\".\"workspace_id\" = \"{WORKSPACE_TABLE}\".\"id\") \
         WHERE (\"{WORKSPACE_TABLE}\".\"slug\" = {slug_param} \
         AND \"{n}\".\"receiver_id\" = {user_param} \
         AND \"{n}\".\"read_at\" IS NULL \
         AND \"{n}\".\"archived_at\" IS NULL \
         AND \"{n}\".\"snoozed_till\" IS NULL \
         AND \"{n}\".\"sender\" ILIKE '%mentioned%' \
         AND \"{n}\".\"deleted_at\" IS NULL)"
    )
}

/// Response body keys (`:223-229`): the handler renders
/// `{"total_unread_notifications_count": int(q1),
/// "mention_unread_notifications_count": int(q2)}`, status 200.
pub fn unread_response_body(total: i64, mentions: i64) -> Value {
    serde_json::json!({
        "total_unread_notifications_count": total,
        "mention_unread_notifications_count": mentions,
    })
}

// ---------------------------------------------------------------------------
// Unit 4 — mark_all_read_filter + bulk update (base.py:239-287)
// ---------------------------------------------------------------------------

/// Mark-all-read candidate list (`:239-243`): receiver + `read_at IS NULL`
/// only — no entity guard, no mentioned handling — ordered
/// `snoozed_till, -created_at` like the list path.
pub fn mark_all_read_list_sql(slug_param: &str, user_param: &str) -> String {
    let n = NOTIFICATION_TABLE;
    format!(
        "SELECT {cols} FROM \"{n}\" \
         INNER JOIN \"{WORKSPACE_TABLE}\" ON (\"{n}\".\"workspace_id\" = \"{WORKSPACE_TABLE}\".\"id\") \
         WHERE (\"{WORKSPACE_TABLE}\".\"slug\" = {slug_param} \
         AND \"{n}\".\"receiver_id\" = {user_param} \
         AND \"{n}\".\"read_at\" IS NULL \
         AND \"{n}\".\"deleted_at\" IS NULL) \
         ORDER BY \"{n}\".\"snoozed_till\" ASC, \"{n}\".\"created_at\" DESC",
        cols = select_list(n),
    )
}

/// Mark-all-read `snoozed` filter (`:246-249`). Unlike the list path the
/// value comes from request *data* with default `False` (real booleans,
/// `:235`); the truthy branch repeats the same over-broad OR as the list
/// path. `$now` binds `timezone.now()`.
pub fn mark_snoozed_clause(snoozed: bool, now_param: &str) -> String {
    let n = NOTIFICATION_TABLE;
    if snoozed {
        format!("(\"{n}\".\"snoozed_till\" < {now_param} OR \"{n}\".\"snoozed_till\" IS NOT NULL)")
    } else {
        format!("(\"{n}\".\"snoozed_till\" >= {now_param} OR \"{n}\".\"snoozed_till\" IS NULL)")
    }
}

/// Mark-all-read `archived` filter (`:252-255`; data default `False`).
pub fn mark_archived_clause(archived: bool) -> &'static str {
    if archived {
        "\"notifications\".\"archived_at\" IS NOT NULL"
    } else {
        "\"notifications\".\"archived_at\" IS NULL"
    }
}

/// `watching` type subquery (mark-all-read, `:258-262`): the plain
/// subscriber list — no created/assigned exclusion, unlike the list
/// path's `subscribed` branch (QUIRK-type-spelling).
pub fn mark_watching_issue_ids_sql(slug_param: &str, user_param: &str) -> String {
    format!(
        "SELECT issue_id FROM {ISSUE_SUBSCRIBER_TABLE} \
         WHERE workspace_id = {slug_param} AND subscriber_id = {user_param}"
    )
}

/// `assigned` type subquery (mark-all-read, `:265-269`): same shape as the
/// list path's `assigned` branch.
pub fn mark_assigned_issue_ids_sql(slug_param: &str, user_param: &str) -> String {
    format!(
        "SELECT issue_id FROM {ISSUE_ASSIGNEE_TABLE} \
         WHERE workspace_id = {slug_param} AND assignee_id = {user_param}"
    )
}

/// Compile the mark-all-read `type` request-data value (`:258-281`):
/// exact match on `"watching"` / `"assigned"` / `"created"`; the default
/// `"all"` (and anything else) adds no clause. `created_member` is the
/// evaluated [`created_member_guard_sql`] result.
pub fn mark_type_filter(
    kind: &str,
    slug_param: &str,
    user_param: &str,
    created_member: bool,
) -> TypeFilter {
    let n = NOTIFICATION_TABLE;
    match kind {
        "watching" => TypeFilter::Where(format!(
            "\"{n}\".\"entity_identifier\" IN ({sub})",
            sub = mark_watching_issue_ids_sql(slug_param, user_param),
        )),
        "assigned" => TypeFilter::Where(format!(
            "\"{n}\".\"entity_identifier\" IN ({sub})",
            sub = mark_assigned_issue_ids_sql(slug_param, user_param),
        )),
        "created" => {
            if created_member {
                TypeFilter::Empty
            } else {
                TypeFilter::Where(format!(
                    "\"{n}\".\"entity_identifier\" IN ({sub})",
                    sub = list_created_issue_ids_sql(slug_param, user_param),
                ))
            }
        }
        _ => TypeFilter::None,
    }
}

/// `bulk_update(["read_at"], batch_size=100)` (`:287`): one `UPDATE` per
/// batch of at most 100 ids. `now_param` binds the row's own
/// `timezone.now()` value; `id_params` are the caller's id placeholders
/// for this batch.
pub fn mark_all_read_update_sql(now_param: &str, id_params: &[String]) -> String {
    format!(
        "UPDATE \"{NOTIFICATION_TABLE}\" SET \"read_at\" = {now_param} \
         WHERE \"id\" IN ({ids})",
        ids = id_params.join(", "),
    )
}

/// Batch size of the `bulk_update` (`:287`).
pub const BULK_UPDATE_BATCH_SIZE: usize = 100;

/// Split candidate ids into `bulk_update` batches of at most
/// [`BULK_UPDATE_BATCH_SIZE`]. An empty set yields no batches
/// (`bulk_update([])` is a no-op, still 200).
pub fn batch_ids(ids: &[String]) -> Vec<Vec<String>> {
    ids.chunks(BULK_UPDATE_BATCH_SIZE)
        .map(<[String]>::to_vec)
        .collect()
}

/// Per-row `read_at` stamping (`:283-286`): calls `now()` once *per row*
/// (QUIRK-per-row-now), so each touched row gets its own timestamp. The
/// handler zips these onto the candidate rows in order, then issues one
/// [`mark_all_read_update_sql`] per [`batch_ids`] chunk. Signals and
/// `save()` never run; `updated_at` is untouched.
pub fn read_stamps(count: usize, mut now: impl FnMut() -> String) -> Vec<String> {
    (0..count).map(|_| now()).collect()
}

/// Success body (`:288`): always `{"message": "Successful"}`, 200 —
/// regardless of how many rows were touched.
pub fn mark_all_read_body() -> Value {
    serde_json::json!({ "message": "Successful" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// FX-NOTIF-01, embedded from the fixture sub-issue's output.
    const NOTIFICATION_COLUMNS: &str =
        include_str!("../../../../fixtures/app_notifications/models/notification.columns.json");
    /// FX-NOTIF-06 list SQL.
    const LIST_SQL: &str = include_str!("../../../../fixtures/app_notifications/queries/list.sql");
    /// FX-NOTIF-06 list rows.
    const LIST_ROWS: &str =
        include_str!("../../../../fixtures/app_notifications/queries/list.rows.json");
    /// FX-NOTIF-07 unread SQL.
    const UNREAD_SQL: &str =
        include_str!("../../../../fixtures/app_notifications/queries/unread.sql");
    /// FX-NOTIF-07 unread rows.
    const UNREAD_ROWS: &str =
        include_str!("../../../../fixtures/app_notifications/queries/unread.rows.json");
    /// FX-NOTIF-08 mark-all-read SQL.
    const MARK_SQL: &str =
        include_str!("../../../../fixtures/app_notifications/queries/mark_all_read.sql");
    /// FX-NOTIF-08 mark-all-read rows.
    const MARK_ROWS: &str =
        include_str!("../../../../fixtures/app_notifications/queries/mark_all_read.rows.json");

    /// First whitespace-separated token of each fixture column entry is the
    /// physical column name (`"workspace_id uuid FK …"` → `workspace_id`).
    fn fixture_column_set(doc: &serde_json::Value) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for key in ["columns", "audit_columns"] {
            let entries = match key {
                "columns" => doc["columns"].as_array(),
                _ => doc["audit_columns"]["columns"].as_array(),
            };
            for entry in entries.expect("fixture column list") {
                let name = entry
                    .as_str()
                    .expect("column entry is a string")
                    .split_whitespace()
                    .next()
                    .expect("column entry names a column");
                out.insert(name.to_string());
            }
        }
        out
    }

    #[test]
    fn select_columns_match_fx_notif_01() {
        let doc: serde_json::Value =
            serde_json::from_str(NOTIFICATION_COLUMNS).expect("fixture parses");
        let pinned = fixture_column_set(&doc);
        let wired: BTreeSet<String> = NOTIFICATION_SELECT_COLUMNS
            .iter()
            .map(|c| c.to_string())
            .collect();
        assert_eq!(wired, pinned, "SELECT shape vs FX-NOTIF-01");
        assert_eq!(
            NOTIFICATION_SELECT_COLUMNS.len(),
            21,
            "6 audit + 15 model columns"
        );
    }

    #[test]
    fn base_scope_binds_both_context_params() {
        // Every read carries the full scope: handlers never get an
        // unscoped handle.
        for sql in [
            list_select_sql("$1", "$2"),
            unread_watching_count_sql("$1", "$2"),
            unread_mention_count_sql("$1", "$2"),
            mark_all_read_list_sql("$1", "$2"),
        ] {
            assert!(sql.contains("\"workspaces\".\"slug\" = $1"), "{sql}");
            assert!(sql.contains("\"receiver_id\" = $2"), "{sql}");
            assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        }
        let scope = NotificationScope {
            workspace_slug: "acme",
            user_id: "u-1",
        };
        // The scope pair is what the handler binds to $1 / $2, in order.
        assert_eq!(scope.bind_order(), ["acme", "u-1"]);
        assert!(base_where("n", "$1", "$2").contains("\"n\".\"deleted_at\" IS NULL"));
    }

    #[test]
    fn list_base_matches_fx_notif_06() {
        let sql = list_select_sql("$1", "$2");
        // Entity guard (:65), double Exists (:66-67), mentioned Case/When
        // (:68-74), ordering (:76) — each pinned by the fixture prose.
        for needle in [
            "\"notifications\".\"entity_name\" = 'issue'",
            "AS \"is_inbox_issue\"",
            "AS \"is_intake_issue\"",
            "CASE WHEN \"notifications\".\"sender\" ILIKE '%mentioned%' THEN true ELSE false END",
            "AS \"is_mentioned_notification\"",
            "ORDER BY \"notifications\".\"snoozed_till\" ASC, \"notifications\".\"created_at\" DESC",
            "U1.\"status\" IN (0, 2, -2)",
        ] {
            assert!(sql.contains(needle), "missing {needle}");
        }
        // The double annotation reuses one subquery text verbatim.
        let exists = intake_exists_sql("notifications", "$1");
        assert_eq!(sql.matches(&exists).count(), 2);
        // Same predicates the fixture SQL pins.
        for needle in [
            "INNER JOIN \"issue_intake\" U1 ON (U0.\"id\" = U1.\"issue_id\")",
            "U0.\"id\" = \"notifications\".\"entity_identifier\"",
        ] {
            assert!(LIST_SQL.contains(needle), "fixture pins {needle}");
            assert!(sql.contains(needle), "builder keeps {needle}");
        }
    }

    #[test]
    fn snoozed_branches_port_observed_bug() {
        // :81 true branch matches every set snoozed_till (past AND future).
        let t = snoozed_clause("true", "$3").expect("true is known");
        assert!(t.contains("\"snoozed_till\" < $3"));
        assert!(t.contains("\"snoozed_till\" IS NOT NULL"));
        let f = snoozed_clause("false", "$3").expect("false is known");
        assert!(f.contains("\"snoozed_till\" >= $3"));
        assert!(f.contains("\"snoozed_till\" IS NULL"));
        // Anything else is a KeyError → 500, not a silent default.
        let err = snoozed_clause("yes", "$3").expect_err("unknown snoozed");
        assert_eq!(err.param, "snoozed");
        let err = archived_clause("0").expect_err("unknown archived");
        assert_eq!(err.param, "archived");
        assert!(archived_clause("true").is_ok() && archived_clause("false").is_ok());
    }

    #[test]
    fn read_and_mentioned_clauses() {
        assert_eq!(read_clause(None), None);
        assert_eq!(
            read_clause(Some("false")),
            Some("\"notifications\".\"read_at\" IS NULL")
        );
        assert_eq!(
            read_clause(Some("true")),
            Some("\"notifications\".\"read_at\" IS NOT NULL")
        );
        assert_eq!(read_clause(Some("yes")), None, "silent ignore (:94-98)");
        // Presence — not value — picks the branch (:54,:100-103).
        assert!(mentioned_clause(true).contains("ILIKE"));
        assert!(!mentioned_clause(true).starts_with("NOT"));
        assert!(mentioned_clause(false).starts_with("NOT"));
    }

    #[test]
    fn list_type_branches_match_fixture() {
        // Default "all" matches no branch: .filter(Q()) no-op (:105-137).
        assert_eq!(list_type_filter("all", "$1", "$2", false), TypeFilter::None);
        assert_eq!(list_type_filter("", "$1", "$2", false), TypeFilter::None);
        // Subscribed carries the created/assigned exclusions (:110-112).
        let TypeFilter::Where(sub) = list_type_filter("subscribed", "$1", "$2", false) else {
            panic!("subscribed filters")
        };
        for needle in [
            "NOT EXISTS (SELECT 1 FROM \"issues\" WHERE created_by_id = $2",
            "NOT EXISTS (SELECT 1 FROM \"issue_assignees\" WHERE id = U0.\"issue_id\" AND assignee_id = $2)",
        ] {
            assert!(sub.contains(needle), "missing {needle}");
        }
        // Assigned + created OR together when combined.
        let TypeFilter::Where(both) = list_type_filter("assigned,created", "$1", "$2", false)
        else {
            panic!("assigned+created filters")
        };
        assert!(both.contains(" OR "));
        assert!(both.contains("FROM \"issue_assignees\""));
        assert!(both.contains("FROM \"issues\""));
        // Created + sub-15 membership short-circuits to .none() (:126-129).
        assert_eq!(
            list_type_filter("created", "$1", "$2", true),
            TypeFilter::Empty
        );
        let guard = created_member_guard_sql("$1", "$2");
        assert!(guard.contains("role < 15"));
        assert!(guard.contains("is_active"));
    }

    #[test]
    fn unread_counts_match_fx_notif_07() {
        let q1 = unread_watching_count_sql("$1", "$2");
        let q2 = unread_mention_count_sql("$1", "$2");
        // Shared base…
        for needle in [
            "\"read_at\" IS NULL",
            "\"archived_at\" IS NULL",
            "\"snoozed_till\" IS NULL",
        ] {
            assert!(q1.contains(needle) && q2.contains(needle), "{needle}");
        }
        // …split only on the mentioned class (:210 vs :220).
        assert!(q1.contains("NOT (\"notifications\".\"sender\" ILIKE '%mentioned%')"));
        assert!(q2.contains("\"sender\" ILIKE '%mentioned%'"));
        assert!(!q2.contains("NOT ("));
        // No entity_name guard here (fixture, as-is).
        assert!(!q1.contains("entity_name") && !q2.contains("entity_name"));
        // Fixture pins the same split.
        assert!(UNREAD_SQL.contains("NOT (\"notifications\".\"sender\" ILIKE '%mentioned%')"));
        // Rows golden: 1 watching + 1 mention.
        let rows: serde_json::Value = serde_json::from_str(UNREAD_ROWS).expect("fixture parses");
        assert_eq!(
            unread_response_body(
                rows["response"]["total_unread_notifications_count"]
                    .as_i64()
                    .expect("total is int"),
                rows["response"]["mention_unread_notifications_count"]
                    .as_i64()
                    .expect("mentions is int"),
            ),
            serde_json::json!({
                "total_unread_notifications_count": 1,
                "mention_unread_notifications_count": 1,
            })
        );
    }

    #[test]
    fn mark_all_read_matches_fx_notif_08() {
        let base = mark_all_read_list_sql("$1", "$2");
        assert!(base.contains("\"read_at\" IS NULL"));
        assert!(
            !base.contains("\"entity_name\" = 'issue'"),
            "no entity guard (:239-243)"
        );
        assert!(base.contains("ORDER BY \"notifications\".\"snoozed_till\" ASC"));
        // Data-driven booleans, same over-broad truthy OR as the list path.
        assert!(mark_snoozed_clause(true, "$3").contains("IS NOT NULL"));
        assert!(mark_snoozed_clause(false, "$3").contains("IS NULL"));
        assert_eq!(
            mark_archived_clause(false),
            "\"notifications\".\"archived_at\" IS NULL"
        );
        // Spelling difference: watching (plain list) vs subscribed.
        let TypeFilter::Where(watching) = mark_type_filter("watching", "$1", "$2", false) else {
            panic!("watching filters")
        };
        assert!(watching.contains("FROM issue_subscribers"));
        assert!(!watching.contains("NOT EXISTS"), "no exclusions (:259-261)");
        assert_eq!(mark_type_filter("all", "$1", "$2", false), TypeFilter::None);
        assert_eq!(
            mark_type_filter("created", "$1", "$2", true),
            TypeFilter::Empty
        );
        // Before/after golden: only the plain unread row flips.
        let rows: serde_json::Value = serde_json::from_str(MARK_ROWS).expect("fixture parses");
        let before = rows["before"].as_array().expect("before is an array");
        let after = rows["after"].as_array().expect("after is an array");
        assert_eq!(before.len(), after.len());
        // The op only ever *sets* read_at, never clears it: exactly one
        // null → non-null transition (a1, "plain unread"). Untouched rows
        // keep null in the after-state (a3 expired snooze excluded by the
        // default snoozed=false arm :249; a4 excluded by receiver scoping).
        let gained: Vec<_> = before
            .iter()
            .zip(after.iter())
            .filter(|(b, a)| b["read_at"].is_null() && !a["read_at"].is_null())
            .collect();
        assert_eq!(gained.len(), 1, "only a1 gains read_at under defaults");
        assert_eq!(gained[0].1["id"], "a1");
        assert_eq!(gained[0].1["read_at"], rows["scenario_now"]);
        assert_eq!(
            mark_all_read_body(),
            serde_json::json!({ "message": "Successful" })
        );
        // Fixture SQL pins the same batch write shape.
        assert!(MARK_SQL.contains("batch_size=100"));
    }

    #[test]
    fn bulk_update_batches_of_100_with_per_row_stamps() {
        assert_eq!(BULK_UPDATE_BATCH_SIZE, 100);
        // 250 ids → 100 / 100 / 50.
        let ids: Vec<String> = (0..250).map(|i| format!("id-{i}")).collect();
        let batches = batch_ids(&ids);
        assert_eq!(batches.len(), 3);
        assert_eq!(
            [batches[0].len(), batches[1].len(), batches[2].len()],
            [100, 100, 50]
        );
        assert!(batch_ids(&[]).is_empty(), "empty set is a no-op");
        let sql = mark_all_read_update_sql("$1", &["$2".to_string(), "$3".to_string()]);
        assert!(sql.contains("SET \"read_at\" = $1"));
        assert!(sql.contains("WHERE \"id\" IN ($2, $3)"));
        // Per-row now(): the clock runs once per row, not once per batch.
        let mut tick = 0;
        let stamps = read_stamps(3, || {
            tick += 1;
            format!("2026-09-29T12:00:00.00000{tick}Z")
        });
        assert_eq!(stamps.len(), 3);
        assert!(stamps[0] != stamps[1] && stamps[1] != stamps[2]);
    }

    #[test]
    fn list_rows_visibility_defaults_keep_plain_only() {
        // FX-NOTIF-06 rows golden: under full defaults only "plain one" is
        // visible (expired snooze fails the snoozed=false arm, mentioned
        // rows are excluded when the param is absent, page rows fail the
        // entity guard).
        let rows: serde_json::Value = serde_json::from_str(LIST_ROWS).expect("fixture parses");
        let defaults = &rows["visible_by_filter"]["full_defaults"];
        assert_eq!(defaults["visible"], serde_json::json!(["plain one"]));
        // mentioned=<present, any value> flips to the mentioned row only.
        let mentioned = &rows["visible_by_filter"]["mentioned_present_any_value"];
        assert_eq!(mentioned["visible"], serde_json::json!(["mentioned one"]));
    }
}
