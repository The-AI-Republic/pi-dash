#![forbid(unsafe_code)]

//! App-workspace enqueue closure: `workspace_seed` + `track_event` +
//! `workspace_invitation` + `issue_activity` + user-mail publishers (D-24,
//! stage 5).
//!
//! Port of the `.delay()` call sites in
//! `apps/api/pi_dash/app/views/workspace/{base,invite,join_request,draft}.py`
//! and `app/views/user/base.py` (PIDASHCONV-614) — the call sites only
//! (arg shapes + enqueue timing). Task BODIES are owned by D-07/D-08/D-09:
//!
//! * `WorkSpaceViewSet.create`, `workspace_seed.delay(workspace_id)`
//!   (`workspace/base.py:139`): positional, `serializer.data["id"]`
//!   (already a string), fired after the member write, before `track_event`
//!   and the 201.
//! * `WorkSpaceViewSet.create`, `track_event.delay(...)`
//!   (`workspace/base.py:141-153`): `WORKSPACE_CREATED`, after the seed
//!   enqueue.
//! * `WorkSpaceViewSet.destroy`, `track_event.delay(...)`
//!   (`workspace/base.py:188-200`): `WORKSPACE_DELETED`, fired BEFORE
//!   `super().destroy()` (`:201`) — the workspace row is still in the DB;
//!   `Profile.last_workspace_id` was already nulled (`:187`).
//! * `WorkspaceInvitationsViewset.create`, `workspace_invitation.delay(...)`
//!   (`workspace/invite.py:121-127`): positional
//!   `(email, workspace.id, token, current_site, request.user.email)`, one
//!   call per bulk-created invite (loop `:120`), after the `bulk_create`
//!   (`:113-115`). `token` is the HS256 JWT (`:96-100`), `current_site` is
//!   `base_host(app)` (`:117`); both are opaque strings here.
//! * `WorkspaceInvitationsViewset.create`, `track_event.delay(...)`
//!   (`workspace/invite.py:128-140`): `USER_INVITED_TO_WORKSPACE`, same
//!   per-invitation loop.
//! * `WorkspaceJoinEndpoint.post`, `track_event.delay(...)`
//!   (`workspace/invite.py:206-217`): `USER_JOINED_WORKSPACE`, fired for the
//!   JOINING user (`user.id`, not the requester) inside the
//!   accept-and-user-exists branch; the invite row is deleted AFTER the
//!   publish (`:220`). (The `user.last_workspace_id` write at `:204-205`
//!   targets `User`, which has no such field — a silent no-op write;
//!   handler-owned, noted for H-C PIDASHCONV-617.)
//! * `UserWorkspaceInvitationsViewSet.create`, `track_event.delay(...)`
//!   (`workspace/invite.py:275-286`): `USER_JOINED_WORKSPACE`, one call per
//!   accepted invitation (loop `:262`), after the per-invite member update.
//! * `WorkspaceJoinRequestViewSet.approve`, `track_event.delay(...)`
//!   (`workspace/join_request.py:226-237`): `USER_JOINED_WORKSPACE`, fired
//!   AFTER the atomic membership/profile/status block (`:204-224`).
//! * `WorkspaceDraftIssueViewSet.create_draft_to_issue`,
//!   `issue_activity.delay(type="issue.activity.created", ...)`
//!   (`workspace/draft.py:228-238`): unconditional on valid
//!   `IssueCreateSerializer.save()`; `requested_data` is the
//!   `json.dumps(request.data, DjangoJSONEncoder)` dump.
//! * `create_draft_to_issue`, `issue_activity.delay(
//!   type="cycle.activity.created", ...)` (`workspace/draft.py:250-265`):
//!   iff `request.data.cycle_id` is truthy; `requested_data=None`,
//!   `issue_id=None`, `current_instance` is the
//!   `{updated_cycle_issues, created_cycle_issues}` snapshot dump.
//! * `create_draft_to_issue`, `issue_activity.delay(
//!   type="module.activity.created", ...)` (`workspace/draft.py:285-297`):
//!   iff `request.data.module_ids` is non-empty, ONE call per module
//!   (list-comp `:284`); `requested_data` dumps
//!   `{"module_id": str(module)}`.
//! * `UserEndpoint.deactivate`, `user_deactivation_email.delay(...)`
//!   (`app/views/user/base.py:352`): positional
//!   `(base_host(app), user.id)`, after all membership/invite/session/
//!   profile/user writes, before `logout()` and the 204.
//! * `UserEndpoint.generate_email_verification_code`,
//!   `send_email_update_magic_code.delay(...)` (`app/views/user/base.py:163`):
//!   positional `(new_email, token)` after the 6-digit code is cached
//!   (600s, `:155-160`).
//! * `UserEndpoint.update_email`, `send_email_update_confirmation.delay(...)`
//!   (`app/views/user/base.py:244-246`): positional `(email,)`, called
//!   TWICE — new address first (`:244`), then the old one (`:246`) — after
//!   the save, cache delete and logout.
//!
//! Fixture oracle (PIDASHCONV-599):
//! `rust-api/fixtures/app_workspace/tasks/enqueue.golden.json` (F-W24-14;
//! `source` → `workspace/{base.py:139-200,invite.py:120-286,
//! join_request.py:226-237,draft.py:228-297}` +
//! `views/user/base.py:163-352`; 14 `calls` + `bugs` + `db_before_after`).
//! The tests below replay every call's task name, arg/kwarg order and
//! value shapes.
//!
//! Wire contract (Porting guide Jobs plane row): `.delay(**kwargs)`
//! publishes a first-attempt Celery protocol v2 message with `args = []`
//! and kwargs in call-site order; `.delay(*args)` publishes positional
//! args with `kwargs = {}`. All seven tasks are bare `@shared_task`
//! (`workspace_seed_task.py:503-504`, `event_tracking_task.py:61-62`,
//! `workspace_invitation_task.py:22-23`,
//! `issue_activities_task.py:1503-1504`,
//! `user_deactivation_email_task.py:22-23`,
//! `user_email_update_task.py:21-22,66-67`), so the wire names are the
//! dotted module paths in [`WORKSPACE_SEED_TASK`] / [`TRACK_EVENT_TASK`] /
//! [`WORKSPACE_INVITATION_TASK`] / [`ISSUE_ACTIVITY_TASK`] /
//! [`USER_DEACTIVATION_EMAIL_TASK`] /
//! [`SEND_EMAIL_UPDATE_MAGIC_CODE_TASK`] /
//! [`SEND_EMAIL_UPDATE_CONFIRMATION_TASK`] and there is no queue override.
//!
//! Crate-graph note: `pidash-jobs` depends on `pidash-services`, so this
//! module cannot name `jobs::celery::CeleryTaskMessage` (that would be a
//! dependency cycle). It publishes the Celery-format body parts instead —
//! task name + [`TrackEventEmit::kwargs`] / [`WorkspaceSeedEmit::args`]
//! (and the twins) — and the handlers (PIDASHCONV-615/617/619/622, in the
//! `api` crate which already depends on `pidash-jobs`) wrap them with
//! `CeleryTaskMessage::new(task, args, kwargs)` plus `queue::enqueue`
//! (the D-02 `space::intake` precedent), transactionally post-commit.
//! The dump *text* (`requested_data`, `current_instance`, `origin`) is the
//! caller's job: `json.dumps(..., cls=DjangoJSONEncoder)` in Python, i.e.
//! CPython `", "` / `": "` separators (the `api` crate's `python_dumps`
//! owns that rendering). No worker `Registry` handler is registered by
//! this module: the worker routes by name (registered locally, otherwise
//! forwarded to RabbitMQ in Celery protocol v2).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-none-project-id (`draft.py:255`): the cycle branch passes
//!   `project_id=str(self.kwargs.get("project_id", None))`, but the route
//!   (`urls/workspace.py:239`: `draft-to-issue/<draft_id>/`) carries only
//!   `slug` + `draft_id`, so the lookup always misses and the wire value
//!   is always the string `"None"`. [`draft_cycle_created_activity`] takes
//!   the kwarg as `Option<&str>` and renders `None` as `"None"`.
//! * QUIRK-raw-issue-ids (`draft.py:289-290` vs `:232-233`): the module
//!   branch passes `issue_id` / `project_id` RAW (serializer string /
//!   UUID object) while the issue branch `str()`-wraps both.
//!   Wire-identical (kombu JSON renders UUIDs as strings); the builders
//!   take the string form and the call table names the side.
//! * QUIRK-mixed-str (`join_request.py:227` vs `:231-232`): the approve
//!   emit passes top-level `user_id` RAW but `str()`-wraps both ids
//!   inside `event_properties`. Wire-identical; one string form.
//! * QUIRK-raw-invite-join (`invite.py:207,211-212` vs
//!   `join_request.py:231-232`): join-accept passes raw UUIDs (top-level
//!   AND in props) while approve `str()`-wraps the props side.
//!   Wire-identical; [`user_joined_event`] and
//!   [`join_request_approved_event`] are separate ctors over the same
//!   shape so the six-site table stays explicit.
//! * QUIRK-request-alias (`draft.py:288` vs `:253`):
//!   `def create_draft_to_issue(self, request, slug, draft_id)` (`:206`),
//!   so `request.user.id` (module branch) and `self.request.user.id`
//!   (cycle branch) are the same object — cosmetic only; every ctor takes
//!   `actor_id: &str` uniformly.
//! * QUIRK-confirmation-twice (`user/base.py:244-246`): the confirmation
//!   mail fires twice per update — new address first, old address second.
//!   The builder is per-call; the handler loops in that order.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde_json::{Map, Value};

/// Celery wire name for `workspace_seed` (bare `@shared_task` default;
/// also pinned by the jobs crate at
/// `jobs/src/tasks_cleanup/workspace_seed.rs` — same string, pinned by
/// value on both sides).
pub const WORKSPACE_SEED_TASK: &str = "pi_dash.bgtasks.workspace_seed_task.workspace_seed";

/// Celery wire name for `track_event` (bare `@shared_task` default; also
/// pinned at `api/src/auth_oauth/oauth_google.rs` — same string).
pub const TRACK_EVENT_TASK: &str = "pi_dash.bgtasks.event_tracking_task.track_event";

/// Celery wire name for `workspace_invitation` (bare `@shared_task`
/// default; also pinned by the jobs crate at
/// `jobs/src/tasks_mail/membership_mail.rs` — same string).
pub const WORKSPACE_INVITATION_TASK: &str =
    "pi_dash.bgtasks.workspace_invitation_task.workspace_invitation";

/// Celery wire name for `issue_activity` (bare `@shared_task` default;
/// pinned equal to the D-28 const — same string, cross-asserted in the
/// tests below since both live in this crate).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// Celery wire name for `user_deactivation_email` (bare `@shared_task`
/// default; also pinned by the jobs crate at
/// `jobs/src/tasks_mail/membership_mail.rs` — same string).
pub const USER_DEACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email";

/// Celery wire name for `send_email_update_magic_code` (bare default;
/// also pinned by the jobs crate at `jobs/src/tasks_mail/auth_mail.rs`).
pub const SEND_EMAIL_UPDATE_MAGIC_CODE_TASK: &str =
    "pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code";

/// Celery wire name for `send_email_update_confirmation` (bare default;
/// also pinned by the jobs crate at `jobs/src/tasks_mail/auth_mail.rs`).
pub const SEND_EMAIL_UPDATE_CONFIRMATION_TASK: &str =
    "pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation";

/// `event_name` values (`utils/analytics_events.py:5-8`).
pub const WORKSPACE_CREATED: &str = "workspace_created";
pub const WORKSPACE_DELETED: &str = "workspace_deleted";
pub const USER_INVITED_TO_WORKSPACE: &str = "user_invited_to_workspace";
pub const USER_JOINED_WORKSPACE: &str = "user_joined_workspace";

/// Activity `type` strings for the three draft `issue_activity` shapes.
pub const ISSUE_ACTIVITY_CREATED: &str = "issue.activity.created";
pub const CYCLE_ACTIVITY_CREATED: &str = "cycle.activity.created";
pub const MODULE_ACTIVITY_CREATED: &str = "module.activity.created";

/// `role` literal inside the workspace create/delete `event_properties`
/// (`workspace/base.py:149,196`).
pub const WORKSPACE_EVENT_ROLE_OWNER: &str = "owner";

/// Positional arg order of `workspace_seed.delay(workspace_id)`
/// (`workspace/base.py:139`).
pub const WORKSPACE_SEED_ARG_ORDER: &[&str] = &["workspace_id"];

/// Kwarg order of all six `track_event.delay` calls (identical order at
/// every site).
pub const TRACK_EVENT_KWARG_ORDER: &[&str] = &["user_id", "event_name", "slug", "event_properties"];

/// `event_properties` key order of the workspace-create emit
/// (`workspace/base.py:145-152`).
pub const WORKSPACE_CREATED_PROPS_ORDER: &[&str] = &[
    "user_id",
    "workspace_id",
    "workspace_slug",
    "role",
    "workspace_name",
    "created_at",
];

/// `event_properties` key order of the workspace-delete emit
/// (`workspace/base.py:192-199`).
pub const WORKSPACE_DELETED_PROPS_ORDER: &[&str] = &[
    "user_id",
    "workspace_id",
    "workspace_slug",
    "role",
    "workspace_name",
    "deleted_at",
];

/// `event_properties` key order of the invite-sent emit
/// (`workspace/invite.py:132-139`).
pub const USER_INVITED_PROPS_ORDER: &[&str] = &[
    "user_id",
    "workspace_id",
    "workspace_slug",
    "invitee_role",
    "invited_at",
    "invitee_email",
];

/// `event_properties` key order shared by the three
/// `USER_JOINED_WORKSPACE` emits (join-accept `invite.py:210-216`,
/// bulk-accept `invite.py:279-285`, join-request approve
/// `join_request.py:230-236` — identical keys in identical order; only
/// the raw-vs-`str()` side differs, see QUIRK-mixed-str /
/// QUIRK-raw-invite-join).
pub const USER_JOINED_PROPS_ORDER: &[&str] = &[
    "user_id",
    "workspace_id",
    "workspace_slug",
    "role",
    "joined_at",
];

/// Positional arg order of
/// `workspace_invitation.delay(email, workspace_id, token, current_site,
/// inviter)` (`workspace/invite.py:121-127`).
pub const WORKSPACE_INVITATION_ARG_ORDER: &[&str] =
    &["email", "workspace_id", "token", "current_site", "inviter"];

/// Kwarg order of all three draft `issue_activity.delay` calls
/// (`workspace/draft.py:228-238,250-265,285-297` — identical order at
/// every site; same order the D-28 owners pin).
pub const ISSUE_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "type",
    "requested_data",
    "actor_id",
    "issue_id",
    "project_id",
    "current_instance",
    "epoch",
    "notification",
    "origin",
];

/// Positional arg order of `user_deactivation_email.delay(current_site,
/// user_id)` (`app/views/user/base.py:352`).
pub const USER_DEACTIVATION_EMAIL_ARG_ORDER: &[&str] = &["current_site", "user_id"];

/// Positional arg order of `send_email_update_magic_code.delay(email,
/// token)` (`app/views/user/base.py:163`).
pub const SEND_EMAIL_UPDATE_MAGIC_CODE_ARG_ORDER: &[&str] = &["email", "token"];

/// Positional arg order of `send_email_update_confirmation.delay(email)`
/// (`app/views/user/base.py:244-246`).
pub const SEND_EMAIL_UPDATE_CONFIRMATION_ARG_ORDER: &[&str] = &["email"];

/// One `workspace_seed.delay(workspace_id)` call
/// (`workspace/base.py:139`).
///
/// `workspace_id` is `serializer.data["id"]` — already a string on the
/// Python side — and crosses positionally with `kwargs = {}`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceSeedEmit {
    pub workspace_id: String,
}

impl WorkspaceSeedEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        WORKSPACE_SEED_TASK
    }

    /// `.delay()` args in [`WORKSPACE_SEED_ARG_ORDER`]; kwargs are `{}`.
    pub fn args(&self) -> Vec<Value> {
        vec![Value::String(self.workspace_id.clone())]
    }

    /// `.delay()` args as ordered pairs in [`WORKSPACE_SEED_ARG_ORDER`].
    pub fn args_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![("workspace_id", Value::String(self.workspace_id.clone()))]
    }
}

/// Workspace-create publisher (`workspace/base.py:139`): positional
/// `serializer.data["id"]`, fired after the member write, before the
/// `track_event` enqueue and the 201.
pub fn workspace_seed_emit(workspace_id: &str) -> WorkspaceSeedEmit {
    WorkspaceSeedEmit {
        workspace_id: workspace_id.to_owned(),
    }
}

/// One `track_event.delay(...)` enqueue from the six workspace call sites.
///
/// Kwargs in call order: `user_id`, `event_name`, `slug`,
/// `event_properties`. `user_id` reaches `.delay()` as the raw id object
/// at every site (kombu JSON renders it as a string, so the field takes
/// the string form); `event_properties` is built in call-site key order
/// by the per-site constructors below. `args` on the wire is `[]`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEventEmit {
    pub user_id: String,
    pub event_name: String,
    pub slug: String,
    pub event_properties: Map<String, Value>,
}

impl TrackEventEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        TRACK_EVENT_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(TRACK_EVENT_KWARG_ORDER.len());
        kwargs.insert("user_id".to_owned(), Value::String(self.user_id.clone()));
        kwargs.insert(
            "event_name".to_owned(),
            Value::String(self.event_name.clone()),
        );
        kwargs.insert("slug".to_owned(), Value::String(self.slug.clone()));
        kwargs.insert(
            "event_properties".to_owned(),
            Value::Object(self.event_properties.clone()),
        );
        kwargs
    }
}

/// Shared constructor behind the six `track_event` publishers.
fn track_event(
    user_id: &str,
    event_name: &str,
    slug: &str,
    event_properties: Map<String, Value>,
) -> TrackEventEmit {
    TrackEventEmit {
        user_id: user_id.to_owned(),
        event_name: event_name.to_owned(),
        slug: slug.to_owned(),
        event_properties,
    }
}

/// Workspace-create publisher (`workspace/base.py:141-153`):
/// `event_name=WORKSPACE_CREATED`, fired after the seed enqueue.
/// `workspace_id` is `data["id"]` (serializer string);
/// `event_properties.user_id` is the raw `request.user.id`.
pub fn workspace_created_event(
    user_id: &str,
    workspace_id: &str,
    slug: &str,
    workspace_name: &str,
    created_at: &str,
) -> TrackEventEmit {
    let mut props = Map::with_capacity(WORKSPACE_CREATED_PROPS_ORDER.len());
    props.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
    props.insert(
        "workspace_id".to_owned(),
        Value::String(workspace_id.to_owned()),
    );
    props.insert("workspace_slug".to_owned(), Value::String(slug.to_owned()));
    props.insert(
        "role".to_owned(),
        Value::String(WORKSPACE_EVENT_ROLE_OWNER.to_owned()),
    );
    props.insert(
        "workspace_name".to_owned(),
        Value::String(workspace_name.to_owned()),
    );
    props.insert(
        "created_at".to_owned(),
        Value::String(created_at.to_owned()),
    );
    track_event(user_id, WORKSPACE_CREATED, slug, props)
}

/// Workspace-delete publisher (`workspace/base.py:188-200`):
/// `event_name=WORKSPACE_DELETED`, fired BEFORE `super().destroy()`.
/// `workspace_id` is the raw `workspace.id` UUID object (wire string);
/// `deleted_at` is `str(timezone.now().isoformat())`, rendered by the
/// caller.
pub fn workspace_deleted_event(
    user_id: &str,
    workspace_id: &str,
    slug: &str,
    workspace_name: &str,
    deleted_at: &str,
) -> TrackEventEmit {
    let mut props = Map::with_capacity(WORKSPACE_DELETED_PROPS_ORDER.len());
    props.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
    props.insert(
        "workspace_id".to_owned(),
        Value::String(workspace_id.to_owned()),
    );
    props.insert("workspace_slug".to_owned(), Value::String(slug.to_owned()));
    props.insert(
        "role".to_owned(),
        Value::String(WORKSPACE_EVENT_ROLE_OWNER.to_owned()),
    );
    props.insert(
        "workspace_name".to_owned(),
        Value::String(workspace_name.to_owned()),
    );
    props.insert(
        "deleted_at".to_owned(),
        Value::String(deleted_at.to_owned()),
    );
    track_event(user_id, WORKSPACE_DELETED, slug, props)
}

/// Invite-sent publisher (`workspace/invite.py:128-140`):
/// `event_name=USER_INVITED_TO_WORKSPACE`, one call per bulk-created
/// invite. `workspace_id` is the raw `workspace.id` UUID object (wire
/// string); `invitee_role` is the raw `invitation.role` int;
/// `invited_at` is `str(timezone.now())`, rendered by the caller.
pub fn user_invited_event(
    user_id: &str,
    workspace_id: &str,
    slug: &str,
    invitee_role: i32,
    invited_at: &str,
    invitee_email: &str,
) -> TrackEventEmit {
    let mut props = Map::with_capacity(USER_INVITED_PROPS_ORDER.len());
    props.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
    props.insert(
        "workspace_id".to_owned(),
        Value::String(workspace_id.to_owned()),
    );
    props.insert("workspace_slug".to_owned(), Value::String(slug.to_owned()));
    props.insert(
        "invitee_role".to_owned(),
        Value::Number(invitee_role.into()),
    );
    props.insert(
        "invited_at".to_owned(),
        Value::String(invited_at.to_owned()),
    );
    props.insert(
        "invitee_email".to_owned(),
        Value::String(invitee_email.to_owned()),
    );
    track_event(user_id, USER_INVITED_TO_WORKSPACE, slug, props)
}

/// Shared `USER_JOINED_WORKSPACE` props builder behind
/// [`user_joined_event`] (join-accept + bulk-accept) and
/// [`join_request_approved_event`] (approve): identical keys in identical
/// order — only the raw-vs-`str()` side differs
/// (QUIRK-mixed-str / QUIRK-raw-invite-join), which is wire-identical.
fn user_joined_props(
    user_id: &str,
    workspace_id: &str,
    slug: &str,
    role: i32,
    joined_at: &str,
) -> Map<String, Value> {
    let mut props = Map::with_capacity(USER_JOINED_PROPS_ORDER.len());
    props.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
    props.insert(
        "workspace_id".to_owned(),
        Value::String(workspace_id.to_owned()),
    );
    props.insert("workspace_slug".to_owned(), Value::String(slug.to_owned()));
    props.insert("role".to_owned(), Value::Number(role.into()));
    props.insert("joined_at".to_owned(), Value::String(joined_at.to_owned()));
    props
}

/// Join-accept publisher (`workspace/invite.py:206-217`) and bulk-accept
/// publisher (`workspace/invite.py:275-286`):
/// `event_name=USER_JOINED_WORKSPACE`. `user_id` is the JOINING user's id
/// (raw UUID object, wire string) — `user.id` at the join site, the
/// per-invitation loop at the bulk site; `role` is the raw invite `role`
/// int; `joined_at` is `str(timezone.now())`, rendered by the caller.
pub fn user_joined_event(
    user_id: &str,
    workspace_id: &str,
    slug: &str,
    role: i32,
    joined_at: &str,
) -> TrackEventEmit {
    let props = user_joined_props(user_id, workspace_id, slug, role, joined_at);
    track_event(user_id, USER_JOINED_WORKSPACE, slug, props)
}

/// Join-request approve publisher (`workspace/join_request.py:226-237`):
/// `event_name=USER_JOINED_WORKSPACE`, fired after the atomic block.
/// QUIRK-mixed-str: top-level `user_id` is the raw `requester.id` while
/// both ids inside `event_properties` are `str()`-wrapped — wire-identical,
/// so this ctor takes the same string forms as [`user_joined_event`].
pub fn join_request_approved_event(
    requester_id: &str,
    workspace_id: &str,
    slug: &str,
    role: i32,
    joined_at: &str,
) -> TrackEventEmit {
    let props = user_joined_props(requester_id, workspace_id, slug, role, joined_at);
    track_event(requester_id, USER_JOINED_WORKSPACE, slug, props)
}

/// One `workspace_invitation.delay(email, workspace_id, token,
/// current_site, inviter)` call (`workspace/invite.py:121-127`).
///
/// Positional, one call per bulk-created invite. `workspace_id` is the raw
/// `workspace.id` UUID object (wire string); `token` is the HS256 JWT
/// string; `inviter` is `request.user.email`. Kwargs are `{}`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceInvitationEmit {
    pub email: String,
    pub workspace_id: String,
    pub token: String,
    pub current_site: String,
    pub inviter: String,
}

impl WorkspaceInvitationEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        WORKSPACE_INVITATION_TASK
    }

    /// `.delay()` args in [`WORKSPACE_INVITATION_ARG_ORDER`]; kwargs are `{}`.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::String(self.email.clone()),
            Value::String(self.workspace_id.clone()),
            Value::String(self.token.clone()),
            Value::String(self.current_site.clone()),
            Value::String(self.inviter.clone()),
        ]
    }

    /// `.delay()` args as ordered pairs in [`WORKSPACE_INVITATION_ARG_ORDER`].
    pub fn args_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("email", Value::String(self.email.clone())),
            ("workspace_id", Value::String(self.workspace_id.clone())),
            ("token", Value::String(self.token.clone())),
            ("current_site", Value::String(self.current_site.clone())),
            ("inviter", Value::String(self.inviter.clone())),
        ]
    }
}

/// Invite-create publisher (`workspace/invite.py:121-127`): positional
/// `(invitation.email, workspace.id, invitation.token, current_site,
/// request.user.email)`, one call per bulk-created invite.
pub fn workspace_invitation_emit(
    email: &str,
    workspace_id: &str,
    token: &str,
    current_site: &str,
    inviter: &str,
) -> WorkspaceInvitationEmit {
    WorkspaceInvitationEmit {
        email: email.to_owned(),
        workspace_id: workspace_id.to_owned(),
        token: token.to_owned(),
        current_site: current_site.to_owned(),
        inviter: inviter.to_owned(),
    }
}

/// One `issue_activity.delay(...)` enqueue from the three draft-to-issue
/// call sites (`workspace/draft.py:228-238,250-265,285-297`).
///
/// Kwargs in call order: `type`, `requested_data`, `actor_id`, `issue_id`,
/// `project_id`, `current_instance`, `epoch`, `notification`, `origin`
/// (same order the D-28 owners pin). `requested_data` / `current_instance`
/// are the pre-rendered `json.dumps` texts (or `None` where the site
/// passes `None`); `issue_id` is `None` on the cycle shape; `project_id`
/// is the string form at every site except the cycle shape's `"None"`
/// (QUIRK-none-project-id); `epoch` is
/// `int(timezone.now().timestamp())` supplied by the caller;
/// `notification` is always `true` in this domain.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceIssueActivityEmit {
    pub activity_type: String,
    pub requested_data: Option<String>,
    pub actor_id: String,
    pub issue_id: Option<String>,
    pub project_id: String,
    pub current_instance: Option<String>,
    pub epoch: i64,
    pub notification: bool,
    pub origin: String,
}

impl WorkspaceIssueActivityEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        ISSUE_ACTIVITY_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(ISSUE_ACTIVITY_KWARG_ORDER.len());
        kwargs.insert("type".to_owned(), Value::String(self.activity_type.clone()));
        kwargs.insert(
            "requested_data".to_owned(),
            self.requested_data
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        kwargs.insert("actor_id".to_owned(), Value::String(self.actor_id.clone()));
        kwargs.insert(
            "issue_id".to_owned(),
            self.issue_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        kwargs.insert(
            "project_id".to_owned(),
            Value::String(self.project_id.clone()),
        );
        kwargs.insert(
            "current_instance".to_owned(),
            self.current_instance
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        kwargs.insert("epoch".to_owned(), Value::Number(self.epoch.into()));
        kwargs.insert("notification".to_owned(), Value::Bool(self.notification));
        kwargs.insert("origin".to_owned(), Value::String(self.origin.clone()));
        kwargs
    }
}

/// Shared constructor for the three draft `issue_activity` emits; each
/// call-site wrapper below fixes `activity_type`, the `None` sides and
/// `notification = true`.
#[allow(clippy::too_many_arguments)]
fn workspace_issue_activity(
    activity_type: &str,
    requested_data: Option<String>,
    actor_id: &str,
    issue_id: Option<String>,
    project_id: String,
    current_instance: Option<String>,
    epoch: i64,
    origin: &str,
) -> WorkspaceIssueActivityEmit {
    WorkspaceIssueActivityEmit {
        activity_type: activity_type.to_owned(),
        requested_data,
        actor_id: actor_id.to_owned(),
        issue_id,
        project_id,
        current_instance,
        epoch,
        notification: true,
        origin: origin.to_owned(),
    }
}

/// Draft-to-issue publisher (`workspace/draft.py:228-238`):
/// `type="issue.activity.created"`, unconditional on valid save.
/// `requested_data` is the `json.dumps(request.data)` dump text (caller's
/// `python_dumps`); `issue_id` / `project_id` are the `str()`-wrapped
/// side (QUIRK-raw-issue-ids); `current_instance=None`.
pub fn draft_issue_created_activity(
    requested_data: String,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    epoch: i64,
    origin: &str,
) -> WorkspaceIssueActivityEmit {
    workspace_issue_activity(
        ISSUE_ACTIVITY_CREATED,
        Some(requested_data),
        actor_id,
        Some(issue_id.to_owned()),
        project_id.to_owned(),
        None,
        epoch,
        origin,
    )
}

/// Draft-to-issue cycle publisher (`workspace/draft.py:250-265`):
/// `type="cycle.activity.created"`, iff `request.data.cycle_id` is truthy.
/// `requested_data=None`, `issue_id=None`; `current_instance` is the
/// `{updated_cycle_issues, created_cycle_issues}` snapshot dump text
/// (caller's `python_dumps`). QUIRK-none-project-id: `project_id_kwarg` is
/// `self.kwargs.get("project_id", None)` — the route never carries it, so
/// `None` renders as the string `"None"`, exactly like `str(None)`.
pub fn draft_cycle_created_activity(
    actor_id: &str,
    project_id_kwarg: Option<&str>,
    current_instance: String,
    epoch: i64,
    origin: &str,
) -> WorkspaceIssueActivityEmit {
    workspace_issue_activity(
        CYCLE_ACTIVITY_CREATED,
        None,
        actor_id,
        None,
        project_id_kwarg.unwrap_or("None").to_owned(),
        Some(current_instance),
        epoch,
        origin,
    )
}

/// Draft-to-issue module publisher (`workspace/draft.py:285-297`):
/// `type="module.activity.created"`, ONE call per `module_ids` entry.
/// `requested_data` dumps `{"module_id": str(module)}` (caller's
/// `python_dumps`; [`module_id_ref_json`] pins the shape);
/// QUIRK-raw-issue-ids: `issue_id` (serializer string) / `project_id`
/// (UUID object) cross RAW — string form on the wire either way;
/// `current_instance=None`.
pub fn draft_module_created_activity(
    requested_data: String,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    epoch: i64,
    origin: &str,
) -> WorkspaceIssueActivityEmit {
    workspace_issue_activity(
        MODULE_ACTIVITY_CREATED,
        Some(requested_data),
        actor_id,
        Some(issue_id.to_owned()),
        project_id.to_owned(),
        None,
        epoch,
        origin,
    )
}

/// `{"module_id": str(...)}` ref dump for the module branch
/// (`workspace/draft.py:287`). Separator rendering is the caller's job
/// (`python_dumps` in the `api` crate); this pins the shape for tests.
pub fn module_id_ref_json(module_id: &str) -> String {
    format!("{{\"module_id\": {}}}", Value::String(module_id.to_owned()))
}

/// One `user_deactivation_email.delay(current_site, user_id)` call
/// (`app/views/user/base.py:352`).
///
/// Positional. `current_site` is `base_host(app)`; `user_id` is the raw
/// `user.id` UUID object (wire string). Fired after all
/// membership/invite/session/profile/user writes, before `logout()`.
/// Kwargs are `{}`.
#[derive(Debug, Clone, PartialEq)]
pub struct UserDeactivationEmailEmit {
    pub current_site: String,
    pub user_id: String,
}

impl UserDeactivationEmailEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        USER_DEACTIVATION_EMAIL_TASK
    }

    /// `.delay()` args in [`USER_DEACTIVATION_EMAIL_ARG_ORDER`]; kwargs are `{}`.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::String(self.current_site.clone()),
            Value::String(self.user_id.clone()),
        ]
    }

    /// `.delay()` args as ordered pairs in [`USER_DEACTIVATION_EMAIL_ARG_ORDER`].
    pub fn args_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("current_site", Value::String(self.current_site.clone())),
            ("user_id", Value::String(self.user_id.clone())),
        ]
    }
}

/// Deactivate publisher (`app/views/user/base.py:352`): positional
/// `(base_host(app), user.id)`.
pub fn user_deactivation_email_emit(
    current_site: &str,
    user_id: &str,
) -> UserDeactivationEmailEmit {
    UserDeactivationEmailEmit {
        current_site: current_site.to_owned(),
        user_id: user_id.to_owned(),
    }
}

/// One `send_email_update_magic_code.delay(email, token)` call
/// (`app/views/user/base.py:163`).
///
/// Positional `(new_email, token)` after the 6-digit code is cached
/// (600s). Kwargs are `{}`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailUpdateMagicCodeEmit {
    pub email: String,
    pub token: String,
}

impl EmailUpdateMagicCodeEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        SEND_EMAIL_UPDATE_MAGIC_CODE_TASK
    }

    /// `.delay()` args in [`SEND_EMAIL_UPDATE_MAGIC_CODE_ARG_ORDER`]; kwargs are `{}`.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::String(self.email.clone()),
            Value::String(self.token.clone()),
        ]
    }

    /// `.delay()` args as ordered pairs in [`SEND_EMAIL_UPDATE_MAGIC_CODE_ARG_ORDER`].
    pub fn args_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("email", Value::String(self.email.clone())),
            ("token", Value::String(self.token.clone())),
        ]
    }
}

/// Generate-code publisher (`app/views/user/base.py:163`): positional
/// `(new_email, token)`.
pub fn email_update_magic_code_emit(email: &str, token: &str) -> EmailUpdateMagicCodeEmit {
    EmailUpdateMagicCodeEmit {
        email: email.to_owned(),
        token: token.to_owned(),
    }
}

/// One `send_email_update_confirmation.delay(email)` call
/// (`app/views/user/base.py:244-246`).
///
/// Positional `(email,)`. QUIRK-confirmation-twice: the handler fires
/// this twice per update — new address first (`:244`), old address
/// second (`:246`). Kwargs are `{}`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailUpdateConfirmationEmit {
    pub email: String,
}

impl EmailUpdateConfirmationEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        SEND_EMAIL_UPDATE_CONFIRMATION_TASK
    }

    /// `.delay()` args in [`SEND_EMAIL_UPDATE_CONFIRMATION_ARG_ORDER`]; kwargs are `{}`.
    pub fn args(&self) -> Vec<Value> {
        vec![Value::String(self.email.clone())]
    }

    /// `.delay()` args as ordered pairs in [`SEND_EMAIL_UPDATE_CONFIRMATION_ARG_ORDER`].
    pub fn args_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![("email", Value::String(self.email.clone()))]
    }
}

/// Update-email publisher (`app/views/user/base.py:244-246`): positional
/// `(email,)` — the handler calls this twice (new, then old).
pub fn email_update_confirmation_emit(email: &str) -> EmailUpdateConfirmationEmit {
    EmailUpdateConfirmationEmit {
        email: email.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Committed evidence replayed here without a database:
    /// `rust-api/fixtures/app_workspace/tasks/enqueue.golden.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/app_workspace/tasks/enqueue.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn key_order(kwargs: &Map<String, Value>) -> Vec<&str> {
        kwargs.keys().map(String::as_str).collect()
    }

    fn arg_names(pairs: &[(&'static str, Value)]) -> Vec<&'static str> {
        pairs.iter().map(|(name, _)| *name).collect()
    }

    #[test]
    fn fixture_names_the_python_sources() {
        let f = fixture();
        let source = f["source"].as_str().expect("source line");
        for fragment in [
            "base.py:139-200",
            "invite.py:120-286",
            "join_request.py:226-237",
            "draft.py:228-297",
            "views/user/base.py:163-352",
        ] {
            assert!(source.contains(fragment), "source names {fragment}");
        }
        // Fourteen enqueue calls transcribed, no more.
        assert_eq!(
            f["calls"].as_array().expect("calls array").len(),
            14,
            "fixture covers exactly the fourteen enqueue calls"
        );
        // All seven delay names appear across the calls.
        let tasks: Vec<&str> = f["calls"]
            .as_array()
            .expect("calls array")
            .iter()
            .map(|c| c["task"].as_str().expect("task name"))
            .collect();
        for name in [
            "workspace_seed.delay",
            "track_event.delay",
            "workspace_invitation.delay",
            "issue_activity.delay",
            "user_deactivation_email.delay",
            "send_email_update_magic_code.delay",
            "send_email_update_confirmation.delay",
        ] {
            assert!(tasks.contains(&name), "fixture covers {name}");
        }
        // Six track_event sites, three issue_activity shapes.
        assert_eq!(
            tasks.iter().filter(|t| **t == "track_event.delay").count(),
            6,
            "six track_event call sites"
        );
        assert_eq!(
            tasks
                .iter()
                .filter(|t| **t == "issue_activity.delay")
                .count(),
            3,
            "three issue_activity shapes"
        );
    }

    #[test]
    fn task_names_match_the_python_celery_names() {
        assert_eq!(
            WORKSPACE_SEED_TASK,
            "pi_dash.bgtasks.workspace_seed_task.workspace_seed"
        );
        assert_eq!(
            TRACK_EVENT_TASK,
            "pi_dash.bgtasks.event_tracking_task.track_event"
        );
        assert_eq!(
            WORKSPACE_INVITATION_TASK,
            "pi_dash.bgtasks.workspace_invitation_task.workspace_invitation"
        );
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            USER_DEACTIVATION_EMAIL_TASK,
            "pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email"
        );
        assert_eq!(
            SEND_EMAIL_UPDATE_MAGIC_CODE_TASK,
            "pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code"
        );
        assert_eq!(
            SEND_EMAIL_UPDATE_CONFIRMATION_TASK,
            "pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation"
        );
        // Same string the D-28 owners pin (same crate — asserted, not
        // duplicated by value).
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            crate::app_modules::tasks::ISSUE_ACTIVITY_TASK
        );
        assert_eq!(
            ISSUE_ACTIVITY_KWARG_ORDER,
            crate::app_modules::tasks::ISSUE_ACTIVITY_KWARG_ORDER
        );
    }

    #[test]
    fn workspace_seed_is_positional() {
        // Fixture `calls[0]` (`workspace/base.py:139`): positional
        // serializer id, fired before track_event and the 201.
        let emit = workspace_seed_emit("ws-1");
        assert_eq!(emit.task_name(), WORKSPACE_SEED_TASK);
        assert_eq!(emit.args(), vec![json!("ws-1")]);
        assert_eq!(arg_names(&emit.args_pairs()), WORKSPACE_SEED_ARG_ORDER);
    }

    #[test]
    fn workspace_created_matches_fixture() {
        // Fixture `calls[1]` (`workspace/base.py:141-153`): role owner,
        // created_at from the response data.
        let emit = workspace_created_event(
            "user-1",
            "ws-1",
            "ws-slug",
            "WS",
            "2026-01-01T00:00:00+00:00",
        );
        assert_eq!(emit.task_name(), TRACK_EVENT_TASK);
        assert_eq!(emit.event_name, WORKSPACE_CREATED);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), TRACK_EVENT_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            json!({
                "user_id": "user-1",
                "event_name": "workspace_created",
                "slug": "ws-slug",
                "event_properties": {
                    "user_id": "user-1",
                    "workspace_id": "ws-1",
                    "workspace_slug": "ws-slug",
                    "role": "owner",
                    "workspace_name": "WS",
                    "created_at": "2026-01-01T00:00:00+00:00",
                },
            })
        );
        assert_eq!(
            key_order(&emit.event_properties),
            WORKSPACE_CREATED_PROPS_ORDER
        );
    }

    #[test]
    fn workspace_deleted_matches_fixture() {
        // Fixture `calls[2]` (`workspace/base.py:188-200`): deleted_at
        // rendered by the caller, published before the row delete.
        let emit = workspace_deleted_event(
            "user-1",
            "ws-1",
            "ws-slug",
            "WS",
            "2026-01-02T00:00:00+00:00",
        );
        assert_eq!(emit.task_name(), TRACK_EVENT_TASK);
        assert_eq!(emit.event_name, WORKSPACE_DELETED);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), TRACK_EVENT_KWARG_ORDER);
        assert_eq!(
            kwargs["event_properties"],
            json!({
                "user_id": "user-1",
                "workspace_id": "ws-1",
                "workspace_slug": "ws-slug",
                "role": "owner",
                "workspace_name": "WS",
                "deleted_at": "2026-01-02T00:00:00+00:00",
            })
        );
        assert_eq!(
            key_order(&emit.event_properties),
            WORKSPACE_DELETED_PROPS_ORDER
        );
    }

    #[test]
    fn invitation_is_positional_and_invite_track_matches_fixture() {
        // Fixture `calls[3]` (`invite.py:121-127`): positional
        // (email, workspace.id, token, current_site, inviter) per invite.
        let invite = workspace_invitation_emit(
            "new@example.com",
            "ws-1",
            "jwt-token",
            "https://app.example",
            "owner@example.com",
        );
        assert_eq!(invite.task_name(), WORKSPACE_INVITATION_TASK);
        assert_eq!(
            invite.args(),
            vec![
                json!("new@example.com"),
                json!("ws-1"),
                json!("jwt-token"),
                json!("https://app.example"),
                json!("owner@example.com"),
            ]
        );
        assert_eq!(
            arg_names(&invite.args_pairs()),
            WORKSPACE_INVITATION_ARG_ORDER
        );
        // Fixture `calls[4]` (`invite.py:128-140`): same loop, invitee
        // role int + invitee email in props.
        let track = user_invited_event(
            "user-1",
            "ws-1",
            "ws-slug",
            5,
            "2026-01-01 00:00:00+00:00",
            "new@example.com",
        );
        assert_eq!(track.task_name(), TRACK_EVENT_TASK);
        assert_eq!(track.event_name, USER_INVITED_TO_WORKSPACE);
        let kwargs = track.kwargs();
        assert_eq!(key_order(&kwargs), TRACK_EVENT_KWARG_ORDER);
        assert_eq!(
            kwargs["event_properties"],
            json!({
                "user_id": "user-1",
                "workspace_id": "ws-1",
                "workspace_slug": "ws-slug",
                "invitee_role": 5,
                "invited_at": "2026-01-01 00:00:00+00:00",
                "invitee_email": "new@example.com",
            })
        );
        assert_eq!(key_order(&track.event_properties), USER_INVITED_PROPS_ORDER);
    }

    #[test]
    fn joined_sites_share_one_shape() {
        // Fixture `calls[5]` (join-accept `invite.py:206-217`), `calls[6]`
        // (bulk-accept `invite.py:275-286`) and `calls[7]` (approve
        // `join_request.py:226-237`): same keys, same order — the approve
        // site str-wraps the props ids (QUIRK-mixed-str) and join passes
        // raw UUIDs (QUIRK-raw-invite-join), wire-identical.
        let joined =
            user_joined_event("user-9", "ws-1", "ws-slug", 15, "2026-01-01 00:00:00+00:00");
        let approved = join_request_approved_event(
            "user-9",
            "ws-1",
            "ws-slug",
            15,
            "2026-01-01 00:00:00+00:00",
        );
        for emit in [&joined, &approved] {
            assert_eq!(emit.task_name(), TRACK_EVENT_TASK);
            assert_eq!(emit.event_name, USER_JOINED_WORKSPACE);
            assert_eq!(key_order(&emit.kwargs()), TRACK_EVENT_KWARG_ORDER);
            assert_eq!(key_order(&emit.event_properties), USER_JOINED_PROPS_ORDER);
        }
        assert_eq!(joined.kwargs(), approved.kwargs());
        assert_eq!(
            joined.kwargs()["event_properties"],
            json!({
                "user_id": "user-9",
                "workspace_id": "ws-1",
                "workspace_slug": "ws-slug",
                "role": 15,
                "joined_at": "2026-01-01 00:00:00+00:00",
            })
        );
    }

    #[test]
    fn draft_issue_created_matches_fixture() {
        // Fixture `calls[8]` (`draft.py:228-238`): requested_data is the
        // request dump text, current_instance null, str-wrapped ids.
        let emit = draft_issue_created_activity(
            "{\"name\": \"I\"}".to_owned(),
            "user-1",
            "issue-9",
            "project-1",
            1_700_000_000,
            "https://app.example",
        );
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            json!({
                "type": "issue.activity.created",
                "requested_data": "{\"name\": \"I\"}",
                "actor_id": "user-1",
                "issue_id": "issue-9",
                "project_id": "project-1",
                "current_instance": null,
                "epoch": 1_700_000_000,
                "notification": true,
                "origin": "https://app.example",
            })
        );
    }

    #[test]
    fn draft_cycle_created_renders_the_none_project_id_bug() {
        // Fixture `calls[9]` (`draft.py:250-265` + QUIRK-none-project-id):
        // requested_data null, issue_id null, project_id the string
        // "None" — the route never carries a project_id kwarg.
        let emit = draft_cycle_created_activity(
            "user-1",
            None,
            "{\"updated_cycle_issues\": null}".to_owned(),
            1_700_000_000,
            "https://app.example",
        );
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(kwargs["type"], json!("cycle.activity.created"));
        assert_eq!(kwargs["requested_data"], Value::Null);
        assert_eq!(kwargs["issue_id"], Value::Null);
        assert_eq!(
            kwargs["project_id"],
            json!("None"),
            "str(None) bug ported verbatim (draft.py:255)"
        );
        assert_eq!(
            kwargs["current_instance"],
            json!("{\"updated_cycle_issues\": null}")
        );
        assert_eq!(kwargs["notification"], json!(true));
        // A present kwarg would cross verbatim (str() of a string).
        let present = draft_cycle_created_activity(
            "user-1",
            Some("project-1"),
            "{}".to_owned(),
            1_700_000_000,
            "https://app.example",
        );
        assert_eq!(present.kwargs()["project_id"], json!("project-1"));
    }

    #[test]
    fn draft_module_created_is_per_module_with_raw_ids() {
        // Fixture `calls[10]` (`draft.py:285-297` +
        // QUIRK-raw-issue-ids): requested_data dumps {"module_id":
        // str(module)}, ids cross RAW (string form on the wire),
        // current_instance null — one emit per module_ids entry.
        assert_eq!(
            module_id_ref_json("module-1"),
            "{\"module_id\": \"module-1\"}"
        );
        for module_id in ["module-1", "module-2"] {
            let emit = draft_module_created_activity(
                module_id_ref_json(module_id),
                "user-1",
                "issue-9",
                "project-1",
                1_700_000_000,
                "https://app.example",
            );
            assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
            let kwargs = emit.kwargs();
            assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
            assert_eq!(kwargs["type"], json!("module.activity.created"));
            assert_eq!(
                kwargs["requested_data"],
                Value::String(module_id_ref_json(module_id))
            );
            assert_eq!(kwargs["issue_id"], json!("issue-9"));
            assert_eq!(kwargs["project_id"], json!("project-1"));
            assert_eq!(kwargs["current_instance"], Value::Null);
            assert_eq!(kwargs["notification"], json!(true));
        }
    }

    #[test]
    fn user_mail_emits_are_positional() {
        // Fixture `calls[11]` (`views/user/base.py:352`): positional
        // (current_site, user.id), before logout.
        let deactivation = user_deactivation_email_emit("https://app.example", "user-1");
        assert_eq!(deactivation.task_name(), USER_DEACTIVATION_EMAIL_TASK);
        assert_eq!(
            deactivation.args(),
            vec![json!("https://app.example"), json!("user-1")]
        );
        assert_eq!(
            arg_names(&deactivation.args_pairs()),
            USER_DEACTIVATION_EMAIL_ARG_ORDER
        );
        // Fixture `calls[12]` (`views/user/base.py:163`): positional
        // (new_email, token) after the cache set.
        let magic = email_update_magic_code_emit("new@example.com", "123456");
        assert_eq!(magic.task_name(), SEND_EMAIL_UPDATE_MAGIC_CODE_TASK);
        assert_eq!(
            magic.args(),
            vec![json!("new@example.com"), json!("123456")]
        );
        assert_eq!(
            arg_names(&magic.args_pairs()),
            SEND_EMAIL_UPDATE_MAGIC_CODE_ARG_ORDER
        );
        // Fixture `calls[13]` (`views/user/base.py:244-246`):
        // QUIRK-confirmation-twice — new address, then old.
        let new_mail = email_update_confirmation_emit("new@example.com");
        let old_mail = email_update_confirmation_emit("old@example.com");
        for emit in [&new_mail, &old_mail] {
            assert_eq!(emit.task_name(), SEND_EMAIL_UPDATE_CONFIRMATION_TASK);
            assert_eq!(
                arg_names(&emit.args_pairs()),
                SEND_EMAIL_UPDATE_CONFIRMATION_ARG_ORDER
            );
        }
        assert_eq!(new_mail.args(), vec![json!("new@example.com")]);
        assert_eq!(old_mail.args(), vec![json!("old@example.com")]);
    }

    #[test]
    fn event_and_activity_names_are_pinned() {
        assert_eq!(WORKSPACE_CREATED, "workspace_created");
        assert_eq!(WORKSPACE_DELETED, "workspace_deleted");
        assert_eq!(USER_INVITED_TO_WORKSPACE, "user_invited_to_workspace");
        assert_eq!(USER_JOINED_WORKSPACE, "user_joined_workspace");
        assert_eq!(ISSUE_ACTIVITY_CREATED, "issue.activity.created");
        assert_eq!(CYCLE_ACTIVITY_CREATED, "cycle.activity.created");
        assert_eq!(MODULE_ACTIVITY_CREATED, "module.activity.created");
        assert_eq!(WORKSPACE_EVENT_ROLE_OWNER, "owner");
    }
}
