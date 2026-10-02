#![forbid(unsafe_code)]

//! App project task call-sites + invite tokens (D-25, L8).
//!
//! Ports the `.delay()` call sites in `app/views/project/base.py`,
//! `app/views/project/member.py`, `app/views/estimate/base.py`, the invite
//! JWT in `app/views/project/invite.py:80-84`, and the `handle_exception`
//! table in `app/views/base.py` (`:110-165` viewset, `:211-248` apiview).
//! Task *implementations* live in `bgtasks/` (D-07 mail, D-08
//! webhooks/activity) and are never ported here.
//!
//! Fixture oracle: `rust-api/fixtures/app_project/FX-APROJ-08.tasks.json`
//! (PIDASHCONV-562). The tests below replay every golden: task names,
//! kwargs in call-site order, the JWT vectors, the 500 bodies, and the
//! 9-row exception table.
//!
//! Wire contract (Porting guide Jobs plane): `.delay(**kwargs)` publishes
//! a first-attempt Celery protocol v2 message with `args = []` and kwargs
//! in call-site order; `.delay(*args)` (the member mail loop) publishes
//! positional args with `kwargs = {}`. All five tasks are bare
//! `@shared_task`, so the wire names are the dotted module paths and
//! there is no queue override.
//!
//! Crate-graph note: `pidash-jobs` depends on `pidash-services`, so this
//! module cannot name `jobs::celery::CeleryTaskMessage` (that would be a
//! dependency cycle). It publishes the Celery-format body parts instead —
//! `task_name()` + `kwargs()` / `args()` per emit, exactly like
//! `app_intake::tasks` — and the handlers (L9–L12, in the `api` crate
//! which already depends on `pidash-jobs`) wrap them with
//! `CeleryTaskMessage::new(task, args, kwargs)` plus `queue::enqueue`.
//! No worker `Registry` handler is registered for these names: all five
//! tasks are Python-owned, so the worker forwards them to RabbitMQ in
//! Celery protocol v2.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-invite-500-first (`invite.py:62`): `.role` on a
//!   `SoftDeletionQuerySet` raises `AttributeError` on every non-empty
//!   create, before `bulk_create` — 0 rows persist and the `:104`
//!   `.delay`-on-`list` line is unreachable in practice (it would
//!   `AttributeError` the same way). Both render the generic 500 via
//!   [`invite_create_failure`].
//! * QUIRK-invite-jwt-email-dict (`invite.py:80-84`): the token embeds the
//!   *whole* `email` dict (`{"email", "role", ...}`) under `"email"`, not
//!   just the address. [`invite_token`] keeps that shape.
//! * QUIRK-estimate-in-loop-update (`estimate/base.py:210`): with
//!   `new_estimate_id` the `issues.update(...)` runs *inside* the per-issue
//!   loop, after each `.delay`. [`point_destroy_plan`] expands that exact
//!   interleave; the else branch enqueues only.
//! * QUIRK-model-activity-raw-data (`project/base.py:300-308`,
//!   `:369-377`): `requested_data` is the raw `request.data`, while the
//!   serializer validates `{**data, "intake_view": ...}` — the task never
//!   sees the alias key. The emit carries the raw map through.
//! * QUIRK-member-mail-resend (`member.py:143-150`): the mail loop iterates
//!   the *re-queried* `project_members`, which includes pre-existing rows
//!   (`bulk_create(..., ignore_conflicts=True)`), so existing members are
//!   re-mailed. The emit is per member id; the handler owns the re-query.
//! * QUIRK-dispatch-returns-exc (`base.py:161-163`, `:260-262`): the
//!   outer `except` returns `exc` instead of `response`. Unreachable from
//!   view errors — `handle_exception` never returns `None` and never
//!   re-raises — so there is no wire effect; [`handle_exception`] is total
//!   and the test pins that.
//! * QUIRK-uuid-string-args: `user_id` / `actor_id` / `event_id` /
//!   `issue_id` / member ids pass as UUID objects in Python (kombu encodes
//!   them `{"__type__": "uuid", ...}` on the wire, `UUID('...')` in the
//!   repr headers). The emits take the string form, like every merged
//!   Rust call site: all five Python consumers coerce (`QuerySet`
//!   lookups, `str(...)`, explicit `str | uuid.UUID` in
//!   `webhook_task.py:378-388`), so behavior is identical. The repr
//!   headers will show `'...'` instead of `UUID('...')`; they are
//!   informational only.

use base64::Engine as _;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Map, Value};
use sha2::Sha256;

/// Celery wire name for `recent_visited_task` (bare `@shared_task`).
pub const RECENT_VISITED_TASK: &str = "pi_dash.bgtasks.recent_visited_task.recent_visited_task";
/// Celery wire name for `model_activity` (bare `@shared_task`).
pub const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";
/// Celery wire name for `webhook_activity` (bare `@shared_task`).
pub const WEBHOOK_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.webhook_activity";
/// Celery wire name for `project_add_user_email` (bare `@shared_task`).
pub const PROJECT_ADD_USER_EMAIL_TASK: &str =
    "pi_dash.bgtasks.project_add_user_email_task.project_add_user_email";
/// Celery wire name for `issue_activity` (bare `@shared_task`).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// Kwarg order of `recent_visited_task.delay` (`project/base.py:246-252`).
pub const RECENT_VISITED_KWARG_ORDER: &[&str] = &[
    "slug",
    "project_id",
    "entity_name",
    "entity_identifier",
    "user_id",
];

/// Kwarg order of both `model_activity.delay` sites (`project/base.py:300-308`
/// create with `current_instance=None`, `:369-377` partial_update with the
/// pre-save `ProjectSerializer` dump).
pub const MODEL_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "model_name",
    "model_id",
    "requested_data",
    "current_instance",
    "actor_id",
    "slug",
    "origin",
];

/// Kwarg order of `webhook_activity.delay` (`project/base.py:405-417`).
pub const WEBHOOK_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "event",
    "verb",
    "field",
    "old_value",
    "new_value",
    "actor_id",
    "slug",
    "current_site",
    "event_id",
    "old_identifier",
    "new_identifier",
];

/// Kwarg order of both `issue_activity.delay` sites
/// (`estimate/base.py:199-209` with `new_estimate_id`, `:217-228` without).
/// Call-site order, which differs from the worker signature order
/// (`issue_activities_task.py:1504-1515`).
pub const ISSUE_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "type",
    "requested_data",
    "actor_id",
    "issue_id",
    "project_id",
    "current_instance",
    "epoch",
];

/// `entity_name` passed by the retrieve site (always `"project"`).
pub const RECENT_VISITED_ENTITY: &str = "project";
/// `model_name` passed by both `model_activity` sites (always `"project"`).
pub const MODEL_ACTIVITY_MODEL: &str = "project";
/// `event` / `verb` passed by the destroy site (always project/deleted).
pub const WEBHOOK_EVENT_PROJECT: &str = "project";
/// See [`WEBHOOK_EVENT_PROJECT`].
pub const WEBHOOK_VERB_DELETED: &str = "deleted";
/// `type` passed by both estimate-point-destroy sites.
pub const ISSUE_ACTIVITY_UPDATED: &str = "issue.activity.updated";

/// One `recent_visited_task.delay(...)` call (`project/base.py:246-252`).
/// `entity_identifier` is the same `pk` as `project_id`; both are emitted.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentVisitedEmit {
    pub slug: String,
    pub project_id: String,
    pub user_id: String,
}

impl RecentVisitedEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        RECENT_VISITED_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(RECENT_VISITED_KWARG_ORDER.len());
        kwargs.insert("slug".to_owned(), Value::String(self.slug.clone()));
        kwargs.insert(
            "project_id".to_owned(),
            Value::String(self.project_id.clone()),
        );
        kwargs.insert(
            "entity_name".to_owned(),
            Value::String(RECENT_VISITED_ENTITY.to_owned()),
        );
        kwargs.insert(
            "entity_identifier".to_owned(),
            Value::String(self.project_id.clone()),
        );
        kwargs.insert("user_id".to_owned(), Value::String(self.user_id.clone()));
        kwargs
    }
}

/// One `model_activity.delay(...)` call (`project/base.py:300-308`,
/// `:369-377`). `requested_data` is the raw `request.data` map (see
/// QUIRK-model-activity-raw-data); `current_instance` is the pre-rendered
/// `json.dumps(ProjectSerializer(project).data)` text on partial_update
/// and `None` on create; `origin` is the pre-rendered
/// `base_host(request, is_app=True)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelActivityEmit {
    pub model_id: String,
    pub requested_data: Map<String, Value>,
    pub current_instance: Option<String>,
    pub actor_id: String,
    pub slug: String,
    pub origin: String,
}

impl ModelActivityEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        MODEL_ACTIVITY_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(MODEL_ACTIVITY_KWARG_ORDER.len());
        kwargs.insert(
            "model_name".to_owned(),
            Value::String(MODEL_ACTIVITY_MODEL.to_owned()),
        );
        kwargs.insert("model_id".to_owned(), Value::String(self.model_id.clone()));
        kwargs.insert(
            "requested_data".to_owned(),
            Value::Object(self.requested_data.clone()),
        );
        kwargs.insert(
            "current_instance".to_owned(),
            self.current_instance
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        kwargs.insert("actor_id".to_owned(), Value::String(self.actor_id.clone()));
        kwargs.insert("slug".to_owned(), Value::String(self.slug.clone()));
        kwargs.insert("origin".to_owned(), Value::String(self.origin.clone()));
        kwargs
    }
}

/// Create path (`project/base.py:300-308`): `current_instance=None`.
pub fn model_activity_on_create(
    model_id: String,
    requested_data: Map<String, Value>,
    actor_id: String,
    slug: String,
    origin: String,
) -> ModelActivityEmit {
    ModelActivityEmit {
        model_id,
        requested_data,
        current_instance: None,
        actor_id,
        slug,
        origin,
    }
}

/// Partial-update path (`project/base.py:369-377`): `current_instance` is
/// the pre-save `ProjectSerializer` dump text.
pub fn model_activity_on_partial_update(
    model_id: String,
    requested_data: Map<String, Value>,
    current_instance: String,
    actor_id: String,
    slug: String,
    origin: String,
) -> ModelActivityEmit {
    ModelActivityEmit {
        model_id,
        requested_data,
        current_instance: Some(current_instance),
        actor_id,
        slug,
        origin,
    }
}

/// One `webhook_activity.delay(...)` call on destroy
/// (`project/base.py:405-417`). `event`/`verb` are fixed and every
/// value/identifier kwarg is `None`; `current_site` is the pre-rendered
/// `base_host(request, is_app=True)`.
#[derive(Debug, Clone, PartialEq)]
pub struct WebhookActivityEmit {
    pub actor_id: String,
    pub slug: String,
    pub current_site: String,
    pub event_id: String,
}

impl WebhookActivityEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        WEBHOOK_ACTIVITY_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(WEBHOOK_ACTIVITY_KWARG_ORDER.len());
        kwargs.insert(
            "event".to_owned(),
            Value::String(WEBHOOK_EVENT_PROJECT.to_owned()),
        );
        kwargs.insert(
            "verb".to_owned(),
            Value::String(WEBHOOK_VERB_DELETED.to_owned()),
        );
        kwargs.insert("field".to_owned(), Value::Null);
        kwargs.insert("old_value".to_owned(), Value::Null);
        kwargs.insert("new_value".to_owned(), Value::Null);
        kwargs.insert("actor_id".to_owned(), Value::String(self.actor_id.clone()));
        kwargs.insert("slug".to_owned(), Value::String(self.slug.clone()));
        kwargs.insert(
            "current_site".to_owned(),
            Value::String(self.current_site.clone()),
        );
        kwargs.insert("event_id".to_owned(), Value::String(self.event_id.clone()));
        kwargs.insert("old_identifier".to_owned(), Value::Null);
        kwargs.insert("new_identifier".to_owned(), Value::Null);
        kwargs
    }
}

/// One `project_add_user_email.delay(...)` call from the per-member loop
/// (`member.py:143-150`). Positional args — `(current_site,
/// project_member.id, request.user.id)` — with `kwargs = {}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectAddUserEmailEmit {
    pub current_site: String,
    pub project_member_id: String,
    pub invitor_id: String,
}

impl ProjectAddUserEmailEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        PROJECT_ADD_USER_EMAIL_TASK
    }

    /// `.delay()` positional args in call order.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::String(self.current_site.clone()),
            Value::String(self.project_member_id.clone()),
            Value::String(self.invitor_id.clone()),
        ]
    }

    /// Always empty: the call site passes positionals only.
    pub fn kwargs(&self) -> Map<String, Value> {
        Map::new()
    }
}

/// One `issue_activity.delay(...)` call from estimate-point destroy
/// (`estimate/base.py:199-209`, `:217-228`). `requested_data` and
/// `current_instance` are the pre-rendered `json.dumps({"estimate_point":
/// ...})` texts (CPython default separators — see
/// [`estimate_point_dumps`]); `epoch` is `int(timezone.now().timestamp())`,
/// supplied by the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueActivityEmit {
    pub requested_data: String,
    pub actor_id: String,
    pub issue_id: String,
    pub project_id: String,
    pub current_instance: String,
    pub epoch: i64,
}

impl IssueActivityEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        ISSUE_ACTIVITY_TASK
    }

    /// `.delay()` kwargs in call-site order. `args` on the wire is `[]`.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(ISSUE_ACTIVITY_KWARG_ORDER.len());
        kwargs.insert(
            "type".to_owned(),
            Value::String(ISSUE_ACTIVITY_UPDATED.to_owned()),
        );
        kwargs.insert(
            "requested_data".to_owned(),
            Value::String(self.requested_data.clone()),
        );
        kwargs.insert("actor_id".to_owned(), Value::String(self.actor_id.clone()));
        kwargs.insert("issue_id".to_owned(), Value::String(self.issue_id.clone()));
        kwargs.insert(
            "project_id".to_owned(),
            Value::String(self.project_id.clone()),
        );
        kwargs.insert(
            "current_instance".to_owned(),
            Value::String(self.current_instance.clone()),
        );
        kwargs.insert("epoch".to_owned(), Value::Number(self.epoch.into()));
        kwargs
    }
}

/// `json.dumps({"estimate_point": ...})` exactly as the estimate destroy
/// branches render it: CPython *default* separators (`", "`, `": "`), so
/// `{"estimate_point": "…"}`, and `{"estimate_point": null}` for `None`.
/// The id is the `str(...)`-ed value, escaped per `json.dumps`
/// (`ensure_ascii`, a no-op for UUID-shaped ids).
pub fn estimate_point_dumps(estimate_point_id: Option<&str>) -> String {
    match estimate_point_id {
        Some(id) => format!(
            r#"{{"estimate_point": {}}}"#,
            ensure_ascii(&serde_json::to_string(id).expect("str serializes"))
        ),
        None => r#"{"estimate_point": null}"#.to_owned(),
    }
}

/// One issue touched by estimate-point destroy: its id plus its current
/// `estimate_point_id` (`None` renders `null`, mirroring the
/// `str(...) if ... else None` guards).
#[derive(Debug, Clone, PartialEq)]
pub struct IssuePointRef {
    pub issue_id: String,
    pub estimate_point_id: Option<String>,
}

/// One step of estimate-point destroy's activity loop, in execution order.
#[derive(Debug, Clone, PartialEq)]
pub enum PointDestroyStep {
    /// Enqueue one `issue_activity` emit for the issue.
    Emit(IssueActivityEmit),
    /// Run `issues.update(estimate_point_id=...)` — inside the loop, after
    /// the emit, on the `new_estimate_id` branch only.
    UpdateIssues {
        /// `str(new_estimate_id)`.
        estimate_point_id: String,
    },
}

/// Expand estimate-point destroy's activity loop (`estimate/base.py:192-228`)
/// into steps. With `new_estimate_id` each issue emits *then* updates
/// (QUIRK-estimate-in-loop-update); without it each issue only emits with
/// `requested_data={"estimate_point": null}` and no update runs.
/// `new_estimate_id` is `Some` for truthy values only: Python branches on
/// `if new_estimate_id:`, so falsy-but-present values (`""`, `0`) take the
/// else branch — callers map those to `None`, carrying the `str()`-ed value
/// in `Some`.
pub fn point_destroy_plan(
    issues: &[IssuePointRef],
    actor_id: &str,
    project_id: &str,
    new_estimate_id: Option<&str>,
    epoch: i64,
) -> Vec<PointDestroyStep> {
    let mut steps = Vec::new();
    for issue in issues {
        let emit = IssueActivityEmit {
            requested_data: estimate_point_dumps(new_estimate_id),
            actor_id: actor_id.to_owned(),
            issue_id: issue.issue_id.clone(),
            project_id: project_id.to_owned(),
            current_instance: estimate_point_dumps(issue.estimate_point_id.as_deref()),
            epoch,
        };
        steps.push(PointDestroyStep::Emit(emit));
        if let Some(new_id) = new_estimate_id {
            steps.push(PointDestroyStep::UpdateIssues {
                estimate_point_id: new_id.to_owned(),
            });
        }
    }
    steps
}

/// PyJWT header for the invite token: `{"typ": "JWT", "alg": "HS256"}`
/// rendered with `sort_keys=True` and compact separators
/// (`jwt/api_jws.py:157`, `:172-175`, PyJWT 2.12.0).
pub const INVITE_JWT_HEADER_JSON: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

/// Invite token (`invite.py:80-84`): `jwt.encode({"email": email,
/// "timestamp": datetime.now().timestamp()}, SECRET_KEY, HS256)`.
///
/// `email` is the *whole* email dict (QUIRK-invite-jwt-email-dict), kept in
/// the caller's key order; `timestamp` is the caller's
/// `datetime.now().timestamp()` float; `secret_key` is
/// `settings.SECRET_KEY`. Byte-identical to PyJWT 2.12.0: sorted compact
/// header, compact `ensure_ascii` payload in insertion order, HMAC-SHA256
/// over `header_b64.payload_b64` with the UTF-8 secret bytes, unpadded
/// base64url segments.
pub fn invite_token(email: &Map<String, Value>, timestamp: f64, secret_key: &str) -> String {
    let email_json = ensure_ascii(
        &serde_json::to_string(&Value::Object(email.clone())).expect("JSON object serializes"),
    );
    // Insertion order: `email` first, `timestamp` last.
    let body = format!(
        r#"{{"email":{email_json},"timestamp":{}}}"#,
        python_float_repr(timestamp)
    );
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let signing_input = format!(
        "{}.{}",
        engine.encode(INVITE_JWT_HEADER_JSON.as_bytes()),
        engine.encode(body.as_bytes())
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(signing_input.as_bytes());
    format!(
        "{}.{}",
        signing_input,
        engine.encode(mac.finalize().into_bytes())
    )
}

/// `json.dumps(..., ensure_ascii=True)` over compact `serde_json` output:
/// `serde_json` already escapes `"`, `\`, control chars and emits raw
/// UTF-8 otherwise, so only 0x7f (`\u007f`) and non-ASCII (lowercase
/// `\uXXXX`, surrogate pairs above U+FFFF) need rewriting.
fn ensure_ascii(compact_json: &str) -> String {
    let mut out = String::with_capacity(compact_json.len());
    for c in compact_json.chars() {
        if c.is_ascii() && c != '\u{7f}' {
            out.push(c);
        } else if c == '\u{7f}' {
            out.push_str("\\u007f");
        } else {
            let n = c as u32;
            if n < 0x1_0000 {
                out.push_str(&format!("\\u{n:04x}"));
            } else {
                let v = n - 0x1_0000;
                let (hi, lo) = (0xd800 + (v >> 10), 0xdc00 + (v & 0x3ff));
                out.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
            }
        }
    }
    out
}

/// CPython `repr` of a float, as `json.dumps` renders it: shortest
/// round-trip digits (identical between CPython's dtoa and Ryū — both are
/// correctly rounded shortest, which is unique), `NaN` / `Infinity` /
/// `-Infinity` for non-finite, and CPython's `r`-format rule
/// (`pystrtod.c`): exponent iff the scientific exponent is `< -4` or
/// `>= 16`, else plain with `.0` when integral; exponents render
/// `e+16` / `e-05` (sign, minimum two digits).
fn python_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_owned();
    }
    if f.is_infinite() {
        if f > 0.0 {
            return "Infinity".to_owned();
        }
        return "-Infinity".to_owned();
    }
    if f == 0.0 {
        // Ryū/serde already render `0.0` / `-0.0` like CPython.
        return serde_json::Number::from_f64(f)
            .expect("finite float converts")
            .to_string();
    }
    // Shortest digits from Ryū; the point placement is ours.
    let rendered = serde_json::Number::from_f64(f)
        .expect("finite float converts")
        .to_string();
    let (negative, rest) = match rendered.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, rendered.as_str()),
    };
    let (mantissa, ryu_exp): (&str, i32) = match rest.find('e') {
        Some(i) => {
            let (m, e) = rest.split_at(i);
            (m, e[1..].parse().expect("Ryū exponent parses"))
        }
        None => (rest, 0),
    };
    let frac_len = match mantissa.find('.') {
        Some(i) => (mantissa.len() - i - 1) as i32,
        None => 0,
    };
    // Value = digits × 10^(ryu_exp - frac_len). Strip leading zeros
    // for the scientific exponent (leading zeros never change the
    // integer value, so the shift is untouched).
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_start_matches('0');
    let shift = ryu_exp - frac_len;
    // Scientific exponent: position of the leading digit.
    let exp = shift + digits.len() as i32 - 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-4..16).contains(&exp) {
        // Plain: decimal point `digits.len() + shift` digits from the left.
        let point = digits.len() as i32 + shift;
        if point <= 0 {
            out.push('0');
            out.push('.');
            for _ in 0..-point {
                out.push('0');
            }
            out.push_str(digits);
        } else if point as usize >= digits.len() {
            out.push_str(digits);
            for _ in 0..(point as usize - digits.len()) {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            let point = point as usize;
            out.push_str(&digits[..point]);
            out.push('.');
            out.push_str(&digits[point..]);
        }
    } else {
        // Exponent: `d` or `d.rest`, then `e±XX`.
        let (head, tail) = digits.split_at(1);
        out.push_str(head);
        if !tail.is_empty() {
            out.push('.');
            out.push_str(tail);
        }
        out.push_str(&format!("e{exp:+03}"));
    }
    out
}

/// `handle_exception`'s `IntegrityError` branch: 400.
pub const INTEGRITY_ERROR_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s `ValidationError` (Django) branch: 400.
pub const VALIDATION_ERROR_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch: 404.
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s `KeyError` branch: 400.
pub const KEY_ERROR_BODY: &str = r#"{"error":"The required key does not exist."}"#;
/// `handle_exception`'s generic branch: 500. Also the invite-create and
/// favorites-list bodies.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// The invite-create outcome (`invite.py:54-111`): `:62` raises
/// `AttributeError` first on every non-empty create (before `bulk_create`,
/// so 0 rows persist) and `:104` is unreachable; either way the wire
/// result is the generic 500. Handlers return this without writing.
pub fn invite_create_failure() -> (u16, &'static str) {
    (500, SERVER_ERROR_BODY)
}

/// Every error `BaseViewSet.handle_exception` / `BaseAPIView.handle_exception`
/// (`app/views/base.py:110-165`, `:211-248`) can render, in the order the
/// code checks them. The `ApiException` variant covers the
/// `super().handle_exception(exc)` path — DRF 3.15.2 `exception_handler`
/// (`rest_framework/views.py:71-101`) via the project's
/// `auth_exception_handler` (which additionally pins `NotAuthenticated`
/// to 401): scalar details render `{"Detail": ...}` (capital D), list/dict
/// details render as-is; Django `Http404(*args)` /
/// `PermissionDenied(*args)` become `NotFound(*args)` /
/// `PermissionDenied(*args)` first.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewError {
    /// `IntegrityError` → 400.
    IntegrityError,
    /// Django `ValidationError` → 400.
    ValidationError,
    /// `ObjectDoesNotExist` → 404.
    ObjectDoesNotExist,
    /// Django `Http404(message)` → 404 `{"Detail": message}`. Arg-less
    /// `Http404()` renders DRF's `NotFound.default_detail` (`"Not found."`)
    /// — callers pass that text.
    Http404(String),
    /// Django `PermissionDenied(message)` → 403 `{"Detail": message}`.
    /// Arg-less renders `"You do not have permission to perform this action."`
    /// — callers pass that text.
    DjangoPermissionDenied(String),
    /// `KeyError` → 400.
    KeyError,
    /// DRF `APIException` and subclasses (`NotAuthenticated` → 401,
    /// `NotFound` → 404, `PermissionDenied` → 403, custom → its code).
    ApiException {
        /// `exc.status_code`.
        status: u16,
        /// `exc.detail`.
        detail: Value,
    },
    /// Anything else (`AttributeError`, `AssertionError`, …) → generic 500.
    Other,
}

/// Port of `handle_exception`: total — every variant maps to a concrete
/// (status, body), mirroring that the Python inner handler never returns
/// `None` and never re-raises (QUIRK-dispatch-returns-exc). Bodies are
/// compact raw-UTF-8 JSON per DRF's `JSONRenderer` defaults
/// (`UNICODE_JSON` / `COMPACT_JSON`, both `True`, unoverridden).
pub fn handle_exception(err: &ViewError) -> (u16, String) {
    match err {
        ViewError::IntegrityError => (400, INTEGRITY_ERROR_BODY.to_owned()),
        ViewError::ValidationError => (400, VALIDATION_ERROR_BODY.to_owned()),
        ViewError::ObjectDoesNotExist => (404, OBJECT_NOT_FOUND_BODY.to_owned()),
        ViewError::Http404(message) => (404, detail_body(&Value::String(message.clone()))),
        ViewError::DjangoPermissionDenied(message) => {
            (403, detail_body(&Value::String(message.clone())))
        }
        ViewError::KeyError => (400, KEY_ERROR_BODY.to_owned()),
        ViewError::ApiException { status, detail } => (*status, detail_body(detail)),
        ViewError::Other => (500, SERVER_ERROR_BODY.to_owned()),
    }
}

/// DRF `exception_handler`'s body rule (`rest_framework/views.py:93-96`):
/// list/dict details render as-is, anything else renders `{"Detail": …}`.
pub fn detail_body(detail: &Value) -> String {
    match detail {
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(detail).expect("JSON value serializes")
        }
        _ => format!(
            r#"{{"Detail":{}}}"#,
            serde_json::to_string(detail).expect("JSON value serializes")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_order(kwargs: &Map<String, Value>) -> Vec<&str> {
        kwargs.keys().map(String::as_str).collect()
    }

    fn email_dict(pairs: &[(&str, Value)]) -> Map<String, Value> {
        let mut map = Map::with_capacity(pairs.len());
        for (k, v) in pairs {
            map.insert((*k).to_owned(), v.clone());
        }
        map
    }

    #[test]
    fn wire_names_are_bare_shared_task_paths() {
        assert_eq!(
            RECENT_VISITED_TASK,
            "pi_dash.bgtasks.recent_visited_task.recent_visited_task"
        );
        assert_eq!(
            MODEL_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        assert_eq!(
            WEBHOOK_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.webhook_activity"
        );
        assert_eq!(
            PROJECT_ADD_USER_EMAIL_TASK,
            "pi_dash.bgtasks.project_add_user_email_task.project_add_user_email"
        );
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
    }

    /// Fixture `recent_visited_on_retrieve[0]` (`project/base.py:246-252`):
    /// kwargs in call-site order. `user_id` is the string form of the
    /// fixture's `{"__type__": "uuid"}` value (QUIRK-uuid-string-args).
    #[test]
    fn recent_visited_matches_fixture() {
        let emit = RecentVisitedEmit {
            slug: "fx08w-6923273a".to_owned(),
            project_id: "063e5ab4-4fe7-4d95-894f-5b7a4443bab9".to_owned(),
            user_id: "db5932ed-aebf-48e2-8757-499eb04dc52b".to_owned(),
        };
        assert_eq!(emit.task_name(), RECENT_VISITED_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), RECENT_VISITED_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "slug": "fx08w-6923273a",
                "project_id": "063e5ab4-4fe7-4d95-894f-5b7a4443bab9",
                "entity_name": "project",
                "entity_identifier": "063e5ab4-4fe7-4d95-894f-5b7a4443bab9",
                "user_id": "db5932ed-aebf-48e2-8757-499eb04dc52b",
            })
        );
    }

    /// Fixture `model_activity_on_create[0]` (`project/base.py:300-308`):
    /// raw `request.data` map, `current_instance` null.
    #[test]
    fn model_activity_create_matches_fixture() {
        let mut requested = Map::new();
        requested.insert("name".to_owned(), Value::String("Task 350A7091".to_owned()));
        requested.insert(
            "identifier".to_owned(),
            Value::String("T350A709".to_owned()),
        );
        let emit = model_activity_on_create(
            "063e5ab4-4fe7-4d95-894f-5b7a4443bab9".to_owned(),
            requested,
            "db5932ed-aebf-48e2-8757-499eb04dc52b".to_owned(),
            "fx08w-6923273a".to_owned(),
            "http://127.0.0.1:8460".to_owned(),
        );
        assert_eq!(emit.task_name(), MODEL_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), MODEL_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "model_name": "project",
                "model_id": "063e5ab4-4fe7-4d95-894f-5b7a4443bab9",
                "requested_data": {"name": "Task 350A7091", "identifier": "T350A709"},
                "current_instance": null,
                "actor_id": "db5932ed-aebf-48e2-8757-499eb04dc52b",
                "slug": "fx08w-6923273a",
                "origin": "http://127.0.0.1:8460",
            })
        );
    }

    /// Fixture `model_activity_on_partial_update[0]`
    /// (`project/base.py:369-377`): the pre-save serializer dump passes
    /// through verbatim as a string; kwarg order is unchanged.
    #[test]
    fn model_activity_partial_update_carries_current_instance() {
        let mut requested = Map::new();
        requested.insert("description".to_owned(), Value::String("upd".to_owned()));
        let dump = r#"{"id": "063e5ab4-4fe7-4d95-894f-5b7a4443bab9", "name": "Task 350A7091"}"#;
        let emit = model_activity_on_partial_update(
            "063e5ab4-4fe7-4d95-894f-5b7a4443bab9".to_owned(),
            requested,
            dump.to_owned(),
            "db5932ed-aebf-48e2-8757-499eb04dc52b".to_owned(),
            "fx08w-6923273a".to_owned(),
            "http://127.0.0.1:8460".to_owned(),
        );
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), MODEL_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            kwargs.get("current_instance"),
            Some(&Value::String(dump.to_owned()))
        );
        assert_eq!(
            kwargs.get("requested_data"),
            Some(&serde_json::json!({"description": "upd"}))
        );
    }

    /// Fixture `webhook_activity_on_destroy.tasks[0]`
    /// (`project/base.py:405-417`): fixed event/verb, nulls, ids as strings.
    /// (`tasks[1]` is the `soft_delete_related_objects` signal emit, not a
    /// D-25 call site.)
    #[test]
    fn webhook_activity_destroy_matches_fixture() {
        let emit = WebhookActivityEmit {
            actor_id: "db5932ed-aebf-48e2-8757-499eb04dc52b".to_owned(),
            slug: "fx08w-6923273a".to_owned(),
            current_site: "http://127.0.0.1:8460".to_owned(),
            event_id: "16afe627-6029-41fc-bd59-4a6dcd8870ba".to_owned(),
        };
        assert_eq!(emit.task_name(), WEBHOOK_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), WEBHOOK_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "event": "project",
                "verb": "deleted",
                "field": null,
                "old_value": null,
                "new_value": null,
                "actor_id": "db5932ed-aebf-48e2-8757-499eb04dc52b",
                "slug": "fx08w-6923273a",
                "current_site": "http://127.0.0.1:8460",
                "event_id": "16afe627-6029-41fc-bd59-4a6dcd8870ba",
                "old_identifier": null,
                "new_identifier": null,
            })
        );
    }

    /// Fixture `project_add_user_email_on_member_create[0]`
    /// (`member.py:143-150`): positional args, empty kwargs.
    #[test]
    fn project_add_user_email_matches_fixture() {
        let emit = ProjectAddUserEmailEmit {
            current_site: "http://127.0.0.1:8460".to_owned(),
            project_member_id: "057513c5-bd23-4706-a0ea-b063d95b08b7".to_owned(),
            invitor_id: "db5932ed-aebf-48e2-8757-499eb04dc52b".to_owned(),
        };
        assert_eq!(emit.task_name(), PROJECT_ADD_USER_EMAIL_TASK);
        assert_eq!(
            emit.args(),
            vec![
                Value::String("http://127.0.0.1:8460".to_owned()),
                Value::String("057513c5-bd23-4706-a0ea-b063d95b08b7".to_owned()),
                Value::String("db5932ed-aebf-48e2-8757-499eb04dc52b".to_owned()),
            ]
        );
        assert!(emit.kwargs().is_empty());
    }

    /// `estimate_point_dumps` renders CPython default separators
    /// (`", "`, `": "`), not compact JSON.
    #[test]
    fn estimate_point_dumps_uses_cpython_default_separators() {
        assert_eq!(
            estimate_point_dumps(Some("c4e32397-132a-487c-80fa-725c7a766ea9")),
            r#"{"estimate_point": "c4e32397-132a-487c-80fa-725c7a766ea9"}"#
        );
        assert_eq!(estimate_point_dumps(None), r#"{"estimate_point": null}"#);
    }

    /// `estimate_point_dumps` escapes like `json.dumps` (`ensure_ascii`):
    /// oracle `json.dumps({"estimate_point": 'a"bé\x01'})`. `new_estimate_id`
    /// is unvalidated request data and the first-issue emit precedes the
    /// `issues.update` that would reject garbage, so this shape is
    /// wire-observable.
    #[test]
    fn estimate_point_dumps_escapes_like_cpython() {
        assert_eq!(
            estimate_point_dumps(Some("a\"bé\x01")),
            "{\"estimate_point\": \"a\\\"b\\u00e9\\u0001\"}"
        );
    }

    /// Fixture `issue_activity_on_point_destroy.with_new_estimate_id`
    /// (`estimate/base.py:199-209`): one emit per issue, dumps with
    /// CPython separators, kwarg order pinned.
    #[test]
    fn issue_activity_with_new_estimate_matches_fixture() {
        let issues = [IssuePointRef {
            issue_id: "31af8870-2aaf-4fa7-8b3b-e20f0d61b6af".to_owned(),
            estimate_point_id: Some("88bd172f-e7ce-4500-aec6-341ce3f51970".to_owned()),
        }];
        let steps = point_destroy_plan(
            &issues,
            "db5932ed-aebf-48e2-8757-499eb04dc52b",
            "063e5ab4-4fe7-4d95-894f-5b7a4443bab9",
            Some("c4e32397-132a-487c-80fa-725c7a766ea9"),
            1790979535,
        );
        assert_eq!(steps.len(), 2);
        let PointDestroyStep::Emit(emit) = &steps[0] else {
            panic!("first step emits");
        };
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            Value::Object(kwargs),
            serde_json::json!({
                "type": "issue.activity.updated",
                "requested_data": r#"{"estimate_point": "c4e32397-132a-487c-80fa-725c7a766ea9"}"#,
                "actor_id": "db5932ed-aebf-48e2-8757-499eb04dc52b",
                "issue_id": "31af8870-2aaf-4fa7-8b3b-e20f0d61b6af",
                "project_id": "063e5ab4-4fe7-4d95-894f-5b7a4443bab9",
                "current_instance": r#"{"estimate_point": "88bd172f-e7ce-4500-aec6-341ce3f51970"}"#,
                "epoch": 1790979535,
            })
        );
        assert_eq!(
            steps[1],
            PointDestroyStep::UpdateIssues {
                estimate_point_id: "c4e32397-132a-487c-80fa-725c7a766ea9".to_owned(),
            }
        );
    }

    /// Fixture `issue_activity_on_point_destroy.without_new_estimate_id`
    /// (`estimate/base.py:217-228`): `requested_data` dumps `None`, and no
    /// update step runs. (The fixture's loop ran zero times, so this
    /// replays the branch shape with one synthetic issue.)
    #[test]
    fn issue_activity_without_new_estimate_has_no_update() {
        let issues = [IssuePointRef {
            issue_id: "31af8870-2aaf-4fa7-8b3b-e20f0d61b6af".to_owned(),
            estimate_point_id: Some("88bd172f-e7ce-4500-aec6-341ce3f51970".to_owned()),
        }];
        let steps = point_destroy_plan(
            &issues,
            "db5932ed-aebf-48e2-8757-499eb04dc52b",
            "063e5ab4-4fe7-4d95-894f-5b7a4443bab9",
            None,
            1790979535,
        );
        assert_eq!(steps.len(), 1);
        let PointDestroyStep::Emit(emit) = &steps[0] else {
            panic!("only step emits");
        };
        let kwargs = emit.kwargs();
        assert_eq!(key_order(&kwargs), ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            kwargs.get("requested_data"),
            Some(&Value::String(r#"{"estimate_point": null}"#.to_owned()))
        );
        assert_eq!(
            kwargs.get("current_instance"),
            Some(&Value::String(
                r#"{"estimate_point": "88bd172f-e7ce-4500-aec6-341ce3f51970"}"#.to_owned()
            ))
        );
    }

    /// QUIRK-estimate-in-loop-update: with `new_estimate_id` the update
    /// follows *each* emit (`estimate/base.py:210` is inside the loop).
    #[test]
    fn point_destroy_plan_interleaves_emit_then_update_per_issue() {
        let issues = [
            IssuePointRef {
                issue_id: "aaaaaaaa-2aaf-4fa7-8b3b-e20f0d61b6af".to_owned(),
                estimate_point_id: Some("88bd172f-e7ce-4500-aec6-341ce3f51970".to_owned()),
            },
            IssuePointRef {
                issue_id: "bbbbbbbb-2aaf-4fa7-8b3b-e20f0d61b6af".to_owned(),
                estimate_point_id: None,
            },
        ];
        let steps = point_destroy_plan(
            &issues,
            "db5932ed-aebf-48e2-8757-499eb04dc52b",
            "063e5ab4-4fe7-4d95-894f-5b7a4443bab9",
            Some("c4e32397-132a-487c-80fa-725c7a766ea9"),
            1790979535,
        );
        assert_eq!(steps.len(), 4);
        assert!(matches!(steps[0], PointDestroyStep::Emit(_)));
        assert!(matches!(steps[1], PointDestroyStep::UpdateIssues { .. }));
        assert!(matches!(steps[2], PointDestroyStep::Emit(_)));
        assert!(matches!(steps[3], PointDestroyStep::UpdateIssues { .. }));
        // The `None` old point renders `null`, mirroring the
        // `str(...) if ... else None` guard.
        let PointDestroyStep::Emit(second) = &steps[2] else {
            panic!("third step emits");
        };
        assert_eq!(
            second.kwargs().get("current_instance"),
            Some(&Value::String(r#"{"estimate_point": null}"#.to_owned()))
        );
    }

    /// PyJWT 2.12.0 oracle vectors (`jwt.encode({"email": ..., "timestamp":
    /// ...}, "562-fixtures-secret-not-for-prod", HS256)`; the secret is the
    /// test-only one the fixture method publishes). Header is
    /// `eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9` on all of them.
    #[test]
    fn invite_token_matches_pyjwt_vectors() {
        const SECRET: &str = "562-fixtures-secret-not-for-prod";
        const HEADER: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
        let cases: &[(Map<String, Value>, f64, &str, &str)] = &[
            (
                email_dict(&[
                    ("email", Value::String("e@x.y".to_owned())),
                    ("role", Value::Number(15.into())),
                ]),
                1720000000.123,
                "eyJlbWFpbCI6eyJlbWFpbCI6ImVAeC55Iiwicm9sZSI6MTV9LCJ0aW1lc3RhbXAiOjE3MjAwMDAwMDAuMTIzfQ",
                "Tf5E8bPdww-CuvgKNuG4creCH6QmDnsMnQCrVgKNmpM",
            ),
            (
                email_dict(&[
                    ("email", Value::String("a@b.c".to_owned())),
                    ("role", Value::Number(5.into())),
                ]),
                1720000000.0,
                "eyJlbWFpbCI6eyJlbWFpbCI6ImFAYi5jIiwicm9sZSI6NX0sInRpbWVzdGFtcCI6MTcyMDAwMDAwMC4wfQ",
                "hpLh5oTpk_zRDkE3i_w9APOMofY_8-6U3ZicDttuG_0",
            ),
            (
                email_dict(&[
                    ("email", Value::String("u@x.y".to_owned())),
                    ("role", Value::String("admin".to_owned())),
                    (
                        "note",
                        Value::String("héllo \u{1f600} q back".to_owned()),
                    ),
                ]),
                1790979535.999999,
                "eyJlbWFpbCI6eyJlbWFpbCI6InVAeC55Iiwicm9sZSI6ImFkbWluIiwibm90ZSI6ImhcdTAwZTlsbG8gXHVkODNkXHVkZTAwIHEgYmFjayJ9LCJ0aW1lc3RhbXAiOjE3OTA5Nzk1MzUuOTk5OTk5fQ",
                "-zLgKtOI08eX_n-KFPaag1cys1vru-4BCJuSX7r4c9Q",
            ),
            (
                email_dict(&[
                    ("email", Value::String("x@y.z".to_owned())),
                    ("role", Value::Number(20.into())),
                ]),
                1600000000.000001,
                "eyJlbWFpbCI6eyJlbWFpbCI6InhAeS56Iiwicm9sZSI6MjB9LCJ0aW1lc3RhbXAiOjE2MDAwMDAwMDAuMDAwMDAxfQ",
                "Xc2Ob6YpnjFz8cLtwsAEC89KirFyrfjwe4VND4YBWwE",
            ),
            (
                email_dict(&[
                    ("email", Value::String("q@w.e".to_owned())),
                    ("role", Value::Number(5.into())),
                    (
                        "x",
                        Value::String("A\"B\\C\u{1}\u{7f}\nZ".to_owned()),
                    ),
                ]),
                1720000000.1234567,
                "eyJlbWFpbCI6eyJlbWFpbCI6InFAdy5lIiwicm9sZSI6NSwieCI6IkFcIkJcXENcdTAwMDFcdTAwN2ZcbloifSwidGltZXN0YW1wIjoxNzIwMDAwMDAwLjEyMzQ1Njd9",
                "JeoGBvndKI-cXX-NUUKcO3udKYRgnvpdDA00MeNYDMw",
            ),
            (
                email_dict(&[
                    ("email", Value::String("z@z.zz".to_owned())),
                    ("role", Value::Number(0.into())),
                ]),
                2000000000.5,
                "eyJlbWFpbCI6eyJlbWFpbCI6InpAei56eiIsInJvbGUiOjB9LCJ0aW1lc3RhbXAiOjIwMDAwMDAwMDAuNX0",
                "8Sq2rae2GKi55cOOyqWpSWkWadO7eamGJZVy-MAi7Uc",
            ),
        ];
        for (email, timestamp, payload_b64, sig_b64) in cases {
            let token = invite_token(email, *timestamp, SECRET);
            assert_eq!(
                token,
                format!("{HEADER}.{payload_b64}.{sig_b64}"),
                "payload {payload_b64}"
            );
            assert_eq!(token.split('.').count(), 3);
        }
    }

    /// The header const base64url-encodes to the standard PyJWT HS256
    /// header segment.
    #[test]
    fn invite_jwt_header_segment_is_standard() {
        use base64::Engine;
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(INVITE_JWT_HEADER_JSON),
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"
        );
    }

    /// `python_float_repr` matches CPython `repr` (verified against the
    /// interpreter): shortest digits, `.0` kept, Python exponent style,
    /// `NaN`/`Infinity` spellings.
    #[test]
    fn float_repr_matches_cpython() {
        assert_eq!(python_float_repr(1720000000.123), "1720000000.123");
        assert_eq!(python_float_repr(1720000000.0), "1720000000.0");
        assert_eq!(python_float_repr(1600000000.000001), "1600000000.000001");
        assert_eq!(python_float_repr(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(python_float_repr(-0.0), "-0.0");
        assert_eq!(python_float_repr(100.0), "100.0");
        assert_eq!(python_float_repr(0.0001), "0.0001");
        assert_eq!(python_float_repr(0.00001), "1e-05");
        assert_eq!(python_float_repr(0.000015), "1.5e-05");
        assert_eq!(python_float_repr(123456.789), "123456.789");
        assert_eq!(python_float_repr(1e15), "1000000000000000.0");
        assert_eq!(python_float_repr(1e16), "1e+16");
        assert_eq!(python_float_repr(9999999999999999.0), "1e+16");
        assert_eq!(
            python_float_repr(1.2345678901234568e20),
            "1.2345678901234568e+20"
        );
        assert_eq!(python_float_repr(1.5e-5), "1.5e-05");
        assert_eq!(python_float_repr(5e-324), "5e-324");
        assert_eq!(
            python_float_repr(1.7976931348623157e308),
            "1.7976931348623157e+308"
        );
        assert_eq!(python_float_repr(f64::NAN), "NaN");
        assert_eq!(python_float_repr(f64::INFINITY), "Infinity");
        assert_eq!(python_float_repr(f64::NEG_INFINITY), "-Infinity");
    }

    /// `ensure_ascii` matches `json.dumps(ensure_ascii=True)`: 0x7f and
    /// non-ASCII become lowercase `\uXXXX` (surrogate pairs above U+FFFF);
    /// `serde_json`'s own escapes pass through untouched.
    #[test]
    fn ensure_ascii_matches_python() {
        assert_eq!(
            ensure_ascii("{\"a\":\"h\u{e9}llo\"}"),
            r#"{"a":"h\u00e9llo"}"#
        );
        assert_eq!(ensure_ascii("\"\u{7f}\""), r#""\u007f""#);
        assert_eq!(ensure_ascii("\"é\""), r#""\u00e9""#);
        assert_eq!(ensure_ascii("\"\u{1f600}\""), r#""\ud83d\ude00""#);
        assert_eq!(ensure_ascii(r#""\u0001\n""#), r#""\u0001\n""#);
    }

    /// Fixture `invite_create_500` + `favorites_list_500`: both 500 paths
    /// render the generic body. `:62` raises first (0 rows persist);
    /// `:104` is unreachable — see [`invite_create_failure`].
    #[test]
    fn invite_create_failure_is_generic_500() {
        assert_eq!(
            invite_create_failure(),
            (
                500,
                r#"{"error":"Something went wrong please try again later"}"#
            )
        );
        assert_eq!(SERVER_ERROR_BODY, invite_create_failure().1);
    }

    /// Fixture `handle_exception_viewset`: all nine rows, byte for byte.
    #[test]
    fn handle_exception_viewset_matches_fixture() {
        assert_eq!(
            handle_exception(&ViewError::IntegrityError),
            (400, r#"{"error":"The payload is not valid"}"#.to_owned())
        );
        assert_eq!(
            handle_exception(&ViewError::ValidationError),
            (400, r#"{"error":"Please provide valid detail"}"#.to_owned())
        );
        assert_eq!(
            handle_exception(&ViewError::ObjectDoesNotExist),
            (
                404,
                r#"{"error":"The required object does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            handle_exception(&ViewError::Http404("Project not found".to_owned())),
            (404, r#"{"Detail":"Project not found"}"#.to_owned())
        );
        assert_eq!(
            handle_exception(&ViewError::KeyError),
            (
                400,
                r#"{"error":"The required key does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            handle_exception(&ViewError::Other),
            (
                500,
                r#"{"error":"Something went wrong please try again later"}"#.to_owned()
            )
        );
        assert_eq!(
            handle_exception(&ViewError::ApiException {
                status: 500,
                detail: Value::String("boom".to_owned()),
            }),
            (500, r#"{"Detail":"boom"}"#.to_owned())
        );
        assert_eq!(
            handle_exception(&ViewError::ApiException {
                status: 404,
                detail: Value::String("nope".to_owned()),
            }),
            (404, r#"{"Detail":"nope"}"#.to_owned())
        );
        assert_eq!(
            handle_exception(&ViewError::ApiException {
                status: 403,
                detail: Value::String("denied".to_owned()),
            }),
            (403, r#"{"Detail":"denied"}"#.to_owned())
        );
    }

    /// Fixture `handle_exception_apiview` (same generic arm) plus the
    /// project `auth_exception_handler` rows: `NotAuthenticated` is pinned
    /// to 401, Django `PermissionDenied` maps like `Http404`.
    #[test]
    fn handle_exception_apiview_and_auth_rows() {
        assert_eq!(
            handle_exception(&ViewError::Other).0,
            500,
            "apiview AttributeError arm"
        );
        assert_eq!(
            handle_exception(&ViewError::ApiException {
                status: 401,
                detail: Value::String("Authentication credentials were not provided.".to_owned()),
            }),
            (
                401,
                r#"{"Detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            handle_exception(&ViewError::DjangoPermissionDenied("denied".to_owned())),
            (403, r#"{"Detail":"denied"}"#.to_owned())
        );
    }

    /// DRF `exception_handler` (`views.py:93-96`): list/dict details render
    /// as-is, without the `Detail` envelope.
    #[test]
    fn detail_body_passes_through_lists_and_dicts() {
        assert_eq!(detail_body(&serde_json::json!(["a", 1])), r#"["a",1]"#);
        assert_eq!(detail_body(&serde_json::json!({"a": 1})), r#"{"a":1}"#);
        assert_eq!(detail_body(&Value::Null), r#"{"Detail":null}"#);
        assert_eq!(detail_body(&Value::Bool(true)), r#"{"Detail":true}"#);
    }

    /// QUIRK-dispatch-returns-exc: the outer `except` has no wire effect
    /// because the inner handler is total — every variant maps to a
    /// concrete (status, non-empty body) and nothing raises.
    #[test]
    fn handle_exception_is_total() {
        let variants = [
            ViewError::IntegrityError,
            ViewError::ValidationError,
            ViewError::ObjectDoesNotExist,
            ViewError::Http404(String::new()),
            ViewError::DjangoPermissionDenied(String::new()),
            ViewError::KeyError,
            ViewError::ApiException {
                status: 418,
                detail: Value::Null,
            },
            ViewError::Other,
        ];
        for variant in &variants {
            let (status, body) = handle_exception(variant);
            assert!((400..600).contains(&status), "{variant:?}");
            assert!(!body.is_empty(), "{variant:?}");
            assert!(serde_json::from_str::<Value>(&body).is_ok(), "{variant:?}");
        }
    }
}
