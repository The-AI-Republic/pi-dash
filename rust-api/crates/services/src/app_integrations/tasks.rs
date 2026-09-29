//! D-33 task publishers: webhook dispatch + github/git sync enqueue boundary.
//!
//! Port of the dispatch side of `apps/api/pi_dash/bgtasks/webhook_task.py`
//! (`webhook_activity` `:377-461`, `model_activity` `:463-506`,
//! `get_model_data` contract `:143-187`, the send-task enqueue `:435-451`
//! and the deactivation-email enqueue `:359-369`) plus the github/git sync
//! enqueue boundary (`github_sync_task.py:264-389`,
//! `git_sync_task.py:213-323`, `github_signals.py:31-74`, beat entry
//! `celery.py:113-118`).
//!
//! Everything here is pure: Celery task names and the kwargs each
//! `.delay()` publisher enqueues, in Celery wire format. Bodies stay where
//! they are ported — the fan-out/send/log/email bodies in D-08
//! (`pidash-jobs` `tasks_webhooks::webhook_fanout` / `webhook_send`) and
//! the sync scan bodies in D-05 (`pidash-jobs` `integrations::github_sync`
//! / `git_sync` / `github_signals`). This crate cannot depend on
//! `pidash-jobs`, so task-name literals and payload shapes are pinned
//! equal to those owners by the fixture replay tests below (the D-31
//! `app_assets::tasks` precedent), never re-derived.
//!
//! Transactional enqueue: publishers hand the `(task, args, kwargs)`
//! triples built here to the existing Postgres-backed queue through the
//! jobs kernel (`pidash-jobs` `queue::enqueue_in` on the request
//! transaction — the `SKIP LOCKED` claim pattern per the Porting guide).
//! No new queue tables: rows go to `rust_job_queue` on the default
//! `celery` queue with `NewJob::new` defaults (immediately visible,
//! `max_retries` 3 — which is exactly the `bind=True, max_retries=3`
//! budget of `sync_one_repo` / `sync_one_binding`).
//!
//! Fixture replayed by the unit tests beside this file:
//! `rust-api/fixtures/app_integrations/fx-tsk-01-webhook-dispatch.json`
//! (FX-TSK-01).
//!
//! Ported quirks (translate, don't redesign):
//!
//! * Q1 (`webhook_task.py:420-433`): the event flag filters are
//!   independent `if`s and an event with no matching branch (`user`,
//!   `intake_issue`, …) leaves the queryset UNFILTERED — every active
//!   webhook in the workspace fires. [`event_flag_for`] returns `None`
//!   for those.
//! * Q2 (`webhook_task.py:435-451`): `event_data` and the actor lookup
//!   are recomputed inside the per-webhook loop, so N webhooks repeat
//!   the same reads N times. The kwargs builder takes one rendered pair
//!   per webhook id, keeping the repetition visible in the call shape.
//! * Q3 (`webhook_task.py:486-488`): `model_activity` dispatches only
//!   for keys *present* in `current_instance`. A requested key absent
//!   there is silently ignored. [`plan_updated_dispatches`] keeps the
//!   `in`-guard. `model_activity` has no `deleted` branch at all —
//!   deletions enter through `webhook_activity` with verb `"deleted"`
//!   and `event_data = {"id": event_id}` ([`deleted_event_data`]).
//! * Q4 (`webhook_task.py:313-321`, re-pinned here): the send-task
//!   signature is computed over `json.dumps(payload)` with *default*
//!   separators (`', '`, `': '`), not the compact wire bytes
//!   `requests` sends. HMAC input rendering and signing live in D-08
//!   (`webhook_send::render_signature_input` / `sign_payload_hex`);
//!   the test below replays the fixture's executed vector against the
//!   same bytes to pin the quirk on this side of the boundary.
//! * Q5 (`celery.py:113-118`): the only sync beat entry keeps the
//!   legacy schedule name `github-issue-sync-every-4h` but fires
//!   `git_sync_task.sync_all_bindings`, so django-celery-beat updates
//!   the existing rows instead of running both pollers in parallel.
//!   `github_sync_task.sync_all_repos` has no beat entry.
//! * Q6 (`webhook_task.py:441`): `webhook_id` is passed to
//!   `webhook_send_task.delay` as a `UUID` object, not `str` — Celery
//!   serializes it on the wire. Builders here take the string form.
//!
//! # Publisher call sites (all in Python today; recorded, not moved)
//!
//! | Publisher | Python | Enqueued kwargs |
//! | --- | --- | --- |
//! | `model_activity` created | `webhook_task.py:466-482` | [`webhook_activity_kwargs`] with verb `"created"`, field/old/new `None`, identifiers `None` |
//! | `model_activity` updated | `webhook_task.py:486-504` | one [`webhook_activity_kwargs`] per changed key, verb `"updated"`, identifiers `None` |
//! | `webhook_activity` fan-out | `webhook_task.py:436-450` | one [`send_task_kwargs`] per matching webhook, action verbatim |
//! | send retry exhaustion | `webhook_task.py:359-369` | [`deactivation_email_kwargs`] |
//! | `sync_all_repos` fan-out | `github_sync_task.py:264-271` | [`sync_positional_wire`] under [`GITHUB_SYNC_ONE_REPO_TASK`] |
//! | `sync_all_bindings` fan-out | `git_sync_task.py:213-220` | [`sync_positional_wire`] under [`GIT_SYNC_ONE_BINDING_TASK`] |
//! | completion comment-back | `github_signals.py:64-74` | [`sync_positional_wire`] under the provider's `POST_COMPLETION_COMMENT` task, guarded by [`should_skip_completion`] |

use serde_json::{Map, Value};

/// `webhook_activity` (`webhook_task.py:376`): full Celery name. A plain
/// `@shared_task` — no bind, no autoretry. Body in D-08
/// (`tasks_webhooks::webhook_fanout::WEBHOOK_ACTIVITY_TASK_NAME`, same string).
pub const WEBHOOK_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.webhook_activity";
/// `model_activity` (`webhook_task.py:462`): full Celery name. A plain
/// `@shared_task`. Body in D-08 (`WEBHOOK_FANOUT_TASK_NAMES`, same string).
pub const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";
/// `webhook_send_task` (`webhook_task.py:264-275`): full Celery name
/// (`bind=True`, autoretry on `RequestException`, `retry_backoff=600`,
/// `max_retries=5`, `retry_jitter=True`). Body in D-08
/// (`tasks_webhooks::webhook_send::WEBHOOK_SEND_TASK_NAME`, same string).
pub const WEBHOOK_SEND_TASK: &str = "pi_dash.bgtasks.webhook_task.webhook_send_task";
/// `send_webhook_deactivation_email` (`webhook_task.py:191`): full Celery
/// name, plain `@shared_task`. Body in D-08
/// (`tasks_webhooks::webhook_send::DEACTIVATION_EMAIL_TASK_NAME`, same string).
pub const DEACTIVATION_EMAIL_TASK: &str =
    "pi_dash.bgtasks.webhook_task.send_webhook_deactivation_email";

/// `sync_all_repos` (`github_sync_task.py:264`, plain `@shared_task`).
/// Body in D-05 (`integrations::github_sync::SYNC_ALL_REPOS_TASK`, same string).
pub const GITHUB_SYNC_ALL_REPOS_TASK: &str = "pi_dash.bgtasks.github_sync_task.sync_all_repos";
/// `sync_one_repo` (`github_sync_task.py:274`, `bind=True, max_retries=3`).
/// Body in D-05 (`SYNC_ONE_REPO_TASK`, same string).
pub const GITHUB_SYNC_ONE_REPO_TASK: &str = "pi_dash.bgtasks.github_sync_task.sync_one_repo";
/// `post_completion_comment` (`github_sync_task.py:338`, plain
/// `@shared_task`). Body in D-05 (`POST_COMPLETION_COMMENT_TASK`, same string).
pub const GITHUB_POST_COMPLETION_COMMENT_TASK: &str =
    "pi_dash.bgtasks.github_sync_task.post_completion_comment";
/// `sync_all_bindings` (`git_sync_task.py:213`, plain `@shared_task`).
/// Body in D-05 (`integrations::git_sync::SYNC_ALL_BINDINGS_TASK`, same string).
pub const GIT_SYNC_ALL_BINDINGS_TASK: &str = "pi_dash.bgtasks.git_sync_task.sync_all_bindings";
/// `sync_one_binding` (`git_sync_task.py:223`, `bind=True, max_retries=3`).
/// Body in D-05 (`SYNC_ONE_BINDING_TASK`, same string).
pub const GIT_SYNC_ONE_BINDING_TASK: &str = "pi_dash.bgtasks.git_sync_task.sync_one_binding";
/// `post_completion_comment` (`git_sync_task.py:281`, plain
/// `@shared_task`). Body in D-05 (`POST_COMPLETION_COMMENT_TASK`, same string).
pub const GIT_POST_COMPLETION_COMMENT_TASK: &str =
    "pi_dash.bgtasks.git_sync_task.post_completion_comment";

/// Every Celery task name this dispatch surface enqueues under, in Python
/// definition order (webhook tasks, then github sync, then git sync).
pub const TASK_NAMES: [&str; 10] = [
    WEBHOOK_ACTIVITY_TASK,
    MODEL_ACTIVITY_TASK,
    WEBHOOK_SEND_TASK,
    DEACTIVATION_EMAIL_TASK,
    GITHUB_SYNC_ALL_REPOS_TASK,
    GITHUB_SYNC_ONE_REPO_TASK,
    GITHUB_POST_COMPLETION_COMMENT_TASK,
    GIT_SYNC_ALL_BINDINGS_TASK,
    GIT_SYNC_ONE_BINDING_TASK,
    GIT_POST_COMPLETION_COMMENT_TASK,
];

/// `webhook_send_task` retry budget (`webhook_task.py:264-270`,
/// `max_retries=5`). The queue row default (3) does NOT apply here: the
/// send task carries its own budget, and the worker's retry verdict must
/// use this value. D-08 owns the retry spec; this const names what the
/// deactivation trigger below compares against.
pub const WEBHOOK_SEND_MAX_RETRIES: u32 = 5;

/// True once the send task has exhausted its retries
/// (`webhook_task.py:357`: `self.request.retries >= self.max_retries`).
/// The caller then runs the filtered deactivation `UPDATE`
/// (`Webhook.objects.filter(pk=...).update(is_active=False)` —
/// `updated_at` untouched) and enqueues [`deactivation_email_kwargs`].
pub fn should_deactivate(retries: u32) -> bool {
    retries >= WEBHOOK_SEND_MAX_RETRIES
}

/// The event flag column for the dispatch filter
/// (`webhook_task.py:420-433`). `None` means no flag branch matched and
/// the queryset stays UNFILTERED (Q1, ported as-is). Same mapping as
/// D-08 `tasks_webhooks::webhook_fanout::event_flag_for`; pinned equal
/// by the fixture's `filter_map` replay test below.
pub fn event_flag_for(event: &str) -> Option<&'static str> {
    match event {
        "project" => Some("project"),
        "issue" => Some("issue"),
        "module" | "module_issue" => Some("module"),
        "cycle" | "cycle_issue" => Some("cycle"),
        "issue_comment" => Some("issue_comment"),
        _ => None,
    }
}

/// `event_data` for the `"deleted"` verb (`webhook_task.py:441`):
/// `{"id": event_id}` — no model read. Any other verb serializes the
/// live row via `get_model_data` (D-08 owns the read; the caller passes
/// the rendered value into [`send_task_kwargs`]).
pub fn deleted_event_data(event_id: &str) -> Value {
    Value::Object(Map::from_iter([(
        "id".to_owned(),
        Value::String(event_id.to_owned()),
    )]))
}

/// One `webhook_activity.delay(...)` enqueue from `model_activity`
/// (`webhook_task.py:466-482` created, `:486-504` updated).
///
/// Kwargs in call order: `event`, `verb`, `field`, `old_value`,
/// `new_value`, `actor_id`, `slug`, `current_site`, `event_id`,
/// `old_identifier`, `new_identifier`. Values pass through verbatim
/// (the send task normalizes via `DjangoJSONEncoder`, not the publisher).
/// Same shape as D-08
/// `tasks_webhooks::webhook_fanout::webhook_activity_kwargs`; pinned
/// equal by the fixture replay test.
#[allow(clippy::too_many_arguments)]
pub fn webhook_activity_kwargs(
    event: &str,
    verb: &str,
    field: Option<&str>,
    old_value: Value,
    new_value: Value,
    actor_id: &str,
    slug: &str,
    current_site: &str,
    event_id: &str,
    old_identifier: Option<&str>,
    new_identifier: Option<&str>,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(11);
    kwargs.insert("event".to_owned(), Value::String(event.to_owned()));
    kwargs.insert("verb".to_owned(), Value::String(verb.to_owned()));
    kwargs.insert(
        "field".to_owned(),
        field.map_or(Value::Null, |f| Value::String(f.to_owned())),
    );
    kwargs.insert("old_value".to_owned(), old_value);
    kwargs.insert("new_value".to_owned(), new_value);
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_owned()));
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert(
        "current_site".to_owned(),
        Value::String(current_site.to_owned()),
    );
    kwargs.insert("event_id".to_owned(), Value::String(event_id.to_owned()));
    kwargs.insert(
        "old_identifier".to_owned(),
        old_identifier.map_or(Value::Null, |i| Value::String(i.to_owned())),
    );
    kwargs.insert(
        "new_identifier".to_owned(),
        new_identifier.map_or(Value::Null, |i| Value::String(i.to_owned())),
    );
    kwargs
}

/// One `webhook_send_task.delay(...)` enqueue from the `webhook_activity`
/// fan-out (`webhook_task.py:436-450`).
///
/// Kwargs in call order: `webhook_id`, `slug`, `event`, `event_data`,
/// `action`, `current_site`, `activity`. `action` is the activity verb
/// verbatim (`created`/`updated`/`deleted` — the POST/PATCH/PUT/DELETE
/// map lives in the send-task body in D-08, not here). `webhook_id`
/// arrives as the string form of `webhook.id` (Q6). Same shape as D-08
/// `tasks_webhooks::webhook_send::webhook_send_task_kwargs`; pinned
/// equal by the fixture's `enqueue_payload` replay test.
#[allow(clippy::too_many_arguments)]
pub fn send_task_kwargs(
    webhook_id: &str,
    slug: &str,
    event: &str,
    event_data: Value,
    action: &str,
    current_site: &str,
    activity: Value,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(7);
    kwargs.insert(
        "webhook_id".to_owned(),
        Value::String(webhook_id.to_owned()),
    );
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("event".to_owned(), Value::String(event.to_owned()));
    kwargs.insert("event_data".to_owned(), event_data);
    kwargs.insert("action".to_owned(), Value::String(action.to_owned()));
    kwargs.insert(
        "current_site".to_owned(),
        Value::String(current_site.to_owned()),
    );
    kwargs.insert("activity".to_owned(), activity);
    kwargs
}

/// `send_webhook_deactivation_email.delay(...)` on retry exhaustion
/// (`webhook_task.py:359-369`): `webhook_id` (the `UUID` object, Q6),
/// `receiver_id` (`webhook.created_by_id`), `reason` (`str(e)`),
/// `current_site`. Same shape as D-08
/// `tasks_webhooks::webhook_send::deactivation_email_kwargs`; pinned
/// equal by the fixture replay test.
pub fn deactivation_email_kwargs(
    webhook_id: &str,
    receiver_id: &str,
    reason: &str,
    current_site: &str,
) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(4);
    kwargs.insert(
        "webhook_id".to_owned(),
        Value::String(webhook_id.to_owned()),
    );
    kwargs.insert(
        "receiver_id".to_owned(),
        Value::String(receiver_id.to_owned()),
    );
    kwargs.insert("reason".to_owned(), Value::String(reason.to_owned()));
    kwargs.insert(
        "current_site".to_owned(),
        Value::String(current_site.to_owned()),
    );
    kwargs
}

/// One planned `webhook_activity` dispatch from `model_activity`: the
/// verb plus the per-key diff. Identifiers are always `None` here
/// (`webhook_task.py:466-504` never sets them).
pub struct ActivityDispatch {
    /// `"created"` or `"updated"` (`model_activity` never emits `"deleted"`).
    pub verb: &'static str,
    /// The changed key (`None` on the created path).
    pub field: Option<String>,
    /// Previous value (`Null` on the created path).
    pub old_value: Value,
    /// Requested value (`Null` on the created path).
    pub new_value: Value,
}

/// The created path (`webhook_task.py:464-482`): `current_instance is
/// None` enqueues exactly one `webhook_activity.delay(verb='created',
/// field/old/new None, identifiers None)`.
pub fn plan_created_dispatch() -> ActivityDispatch {
    ActivityDispatch {
        verb: "created",
        field: None,
        old_value: Value::Null,
        new_value: Value::Null,
    }
}

/// The updated path (`webhook_task.py:488-504`): one dispatch per key of
/// `requested_data` that is present in `current_instance` (Q3) and whose
/// values differ. `requested_data` is never iterated for deletions.
///
/// Value comparison here is plain JSON equality for the common case;
/// cross-type Python-`==` edges (`True == 1`, `1 == 1.0`) are owned by
/// D-08 (`tasks_webhooks::webhook_fanout::py_json_eq`, pinned against a
/// CPython oracle) and the handler layer must route those through it.
pub fn plan_updated_dispatches(
    requested_data: &Map<String, Value>,
    current_instance: &Map<String, Value>,
) -> Vec<ActivityDispatch> {
    let mut out = Vec::new();
    for (key, requested_value) in requested_data {
        let Some(current_value) = current_instance.get(key) else {
            continue;
        };
        if current_value != requested_value {
            out.push(ActivityDispatch {
                verb: "updated",
                field: Some(key.clone()),
                old_value: current_value.clone(),
                new_value: requested_value.clone(),
            });
        }
    }
    out
}

/// Celery wire `(args, kwargs)` for the positional single-id sync
/// enqueues: `sync_one_repo.delay(str(sync_id))`
/// (`github_sync_task.py:271`), `sync_one_binding.delay(str(binding_id))`
/// (`git_sync_task.py:219`) and both `post_completion_comment.delay(str(id))`
/// (`github_sync_task.py:74`, `git_sync_task.py:74` via
/// `github_signals.py:64-74`). Positional `.delay(id)` is
/// `args=[str]`, `kwargs={}` on the wire — the same shape D-05's
/// `fanout_job` / `completion_job` build; pinned equal by the wire test.
pub fn sync_positional_wire(id: &str) -> (Value, Map<String, Value>) {
    (Value::Array(vec![Value::String(id.to_owned())]), Map::new())
}

/// The completion comment-back guard (`github_signals.py:56-74`): skip
/// when the sync row's `metadata` already carries a
/// `completion_comment_id`. Python tests truthiness
/// (`metadata.get(...)`); absent, null or empty-string ids proceed.
/// (State-transition detection — pre/post-save snapshot, `created`,
/// same-state and non-`completed`-group skips — executes in D-05's
/// `integrations::github_signals` port; this is only the enqueue
/// precondition.)
pub fn should_skip_completion(metadata: &Map<String, Value>) -> bool {
    match metadata.get("completion_comment_id") {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Beat schedule name kept for the git poller (`celery.py:114-117`).
/// Q5: the legacy `github-issue-sync-every-4h` name is kept deliberately
/// so django-celery-beat updates the existing rows.
pub const GITHUB_SYNC_BEAT_NAME: &str = "github-issue-sync-every-4h";

/// Task the beat entry fires (`celery.py:115`): the provider-neutral
/// poller, not the legacy per-repo one.
pub const GITHUB_SYNC_BEAT_TASK: &str = GIT_SYNC_ALL_BINDINGS_TASK;

/// Beat cadence (`celery.py:116`): `crontab(minute=0, hour="*/4")`.
pub const GITHUB_SYNC_BEAT_SCHEDULE: &str = "crontab(minute=0, hour=*/4)";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Committed evidence replayed here without a database:
    /// `rust-api/fixtures/app_integrations/fx-tsk-01-webhook-dispatch.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/app_integrations/fx-tsk-01-webhook-dispatch.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn fixture_names_the_python_sources() {
        let f = fixture();
        let comment = f["_comment"].as_str().expect("comment");
        for source in [
            "bgtasks/webhook_task.py:58-90",
            "143-187",
            "377-506",
            "189-375",
        ] {
            assert!(comment.contains(source), "trace names {source}");
        }
        // Dispatch scope cites the send path without duplicating it.
        assert!(f["db_after"]
            .as_str()
            .expect("db_after")
            .contains("is_active=False"));
        assert!(f["db_before"]
            .as_str()
            .expect("db_before")
            .contains("webhook_send_task"));
    }

    #[test]
    fn task_names_match_the_python_celery_names() {
        // Plain `@shared_task` names are the module path; these literals
        // must stay byte-identical to the D-08/D-05 owners (this crate
        // cannot import pidash-jobs, so the pin is by value).
        assert_eq!(
            WEBHOOK_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.webhook_activity"
        );
        assert_eq!(
            MODEL_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        assert_eq!(
            WEBHOOK_SEND_TASK,
            "pi_dash.bgtasks.webhook_task.webhook_send_task"
        );
        assert_eq!(
            DEACTIVATION_EMAIL_TASK,
            "pi_dash.bgtasks.webhook_task.send_webhook_deactivation_email"
        );
        assert_eq!(
            GITHUB_SYNC_ONE_REPO_TASK,
            "pi_dash.bgtasks.github_sync_task.sync_one_repo"
        );
        assert_eq!(
            GITHUB_POST_COMPLETION_COMMENT_TASK,
            "pi_dash.bgtasks.github_sync_task.post_completion_comment"
        );
        assert_eq!(
            GIT_SYNC_ONE_BINDING_TASK,
            "pi_dash.bgtasks.git_sync_task.sync_one_binding"
        );
        assert_eq!(
            GIT_POST_COMPLETION_COMMENT_TASK,
            "pi_dash.bgtasks.git_sync_task.post_completion_comment"
        );
        assert_eq!(TASK_NAMES.len(), 10);
    }

    #[test]
    fn send_kwargs_replay_the_fixture_enqueue_payload() {
        let f = fixture();
        let payload = &f["enqueue_payload"];
        // The fixture's `task` slot names the .delay call site.
        assert_eq!(payload["task"], json!("webhook_send_task.delay"));
        // Webhook ids cross .delay as UUID objects (Q6); the wire holds str.
        assert!(payload["webhook_id"]
            .as_str()
            .expect("webhook_id note")
            .contains("UUID, NOT str"));

        let event_data = json!({"title": "Pinned"});
        let activity = json!({
            "field": null,
            "new_value": null,
            "old_value": null,
            "actor": {"id": "actor-1"},
            "old_identifier": null,
            "new_identifier": null,
        });
        for verb in ["created", "updated", "deleted"] {
            let kwargs = send_task_kwargs(
                "wid-1",
                "ws-1",
                "issue",
                event_data.clone(),
                verb,
                "https://app.example",
                activity.clone(),
            );
            // Seven kwargs in .delay call order (webhook_task.py:436-450).
            let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
            assert_eq!(
                keys,
                [
                    "webhook_id",
                    "slug",
                    "event",
                    "event_data",
                    "action",
                    "current_site",
                    "activity"
                ],
                "verb {verb}"
            );
            // The activity verb passes through verbatim here; the
            // POST/PATCH/PUT/DELETE map lives in the send body (D-08).
            assert_eq!(kwargs["action"], json!(verb), "verb {verb}");
            assert_eq!(kwargs["webhook_id"], json!("wid-1"));
        }
        // Fixture's activity slot keys match the builder's activity value.
        for key in [
            "field",
            "new_value",
            "old_value",
            "actor",
            "old_identifier",
            "new_identifier",
        ] {
            assert!(activity.get(key).is_some(), "activity key {key}");
        }
    }

    #[test]
    fn activity_kwargs_follow_the_delay_call_order() {
        let kwargs = webhook_activity_kwargs(
            "issue",
            "updated",
            Some("title"),
            json!("old"),
            json!("new"),
            "actor-1",
            "ws-1",
            "https://app.example",
            "iid-1",
            None,
            None,
        );
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
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
                "new_identifier"
            ]
        );
        assert_eq!(kwargs["field"], json!("title"));
        assert_eq!(kwargs["old_identifier"], Value::Null);
    }

    #[test]
    fn deactivation_kwargs_carry_receiver_and_reason() {
        // webhook_task.py:359-369: receiver is webhook.created_by_id,
        // reason is str(e), webhook_id the UUID object (Q6).
        let kwargs = deactivation_email_kwargs("wid-1", "user-9", "boom", "https://app.example");
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["webhook_id", "receiver_id", "reason", "current_site"]
        );
        assert_eq!(kwargs["receiver_id"], json!("user-9"));
        assert_eq!(kwargs["reason"], json!("boom"));
    }

    #[test]
    fn created_plan_is_a_single_created_dispatch() {
        // webhook_task.py:466-482: current_instance None -> one created
        // dispatch, field/old/new and both identifiers None.
        let plan = plan_created_dispatch();
        assert_eq!(plan.verb, "created");
        assert_eq!(plan.field, None);
        assert_eq!(plan.old_value, Value::Null);
        assert_eq!(plan.new_value, Value::Null);
        let kwargs = webhook_activity_kwargs(
            "issue",
            plan.verb,
            plan.field.as_deref(),
            plan.old_value,
            plan.new_value,
            "actor-1",
            "ws-1",
            "https://app.example",
            "iid-1",
            None,
            None,
        );
        assert_eq!(kwargs["verb"], json!("created"));
        assert_eq!(kwargs["field"], Value::Null);
    }

    #[test]
    fn updated_plan_skips_absent_and_equal_keys() {
        // webhook_task.py:488-504: only keys present in current_instance
        // with differing values dispatch; identifiers stay None.
        let requested: Map<String, Value> = Map::from_iter([
            ("title".to_owned(), json!("new")),
            ("same".to_owned(), json!(1)),
            ("gone".to_owned(), json!("x")),
        ]);
        let current: Map<String, Value> = Map::from_iter([
            ("title".to_owned(), json!("old")),
            ("same".to_owned(), json!(1)),
        ]);
        let plans = plan_updated_dispatches(&requested, &current);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].verb, "updated");
        assert_eq!(plans[0].field.as_deref(), Some("title"));
        assert_eq!(plans[0].old_value, json!("old"));
        assert_eq!(plans[0].new_value, json!("new"));
    }

    #[test]
    fn event_flag_map_replays_the_fixture_filter_map() {
        let f = fixture();
        let filter_map = &f["webhook_activity"]["filter_map"];
        assert_eq!(filter_map["project"], json!("project=True"));
        assert_eq!(filter_map["issue"], json!("issue=True"));
        assert_eq!(filter_map["module/module_issue"], json!("module=True"));
        assert_eq!(filter_map["cycle/cycle_issue"], json!("cycle=True"));
        assert_eq!(filter_map["issue_comment"], json!("issue_comment=True"));

        assert_eq!(event_flag_for("project"), Some("project"));
        assert_eq!(event_flag_for("issue"), Some("issue"));
        assert_eq!(event_flag_for("module"), Some("module"));
        assert_eq!(event_flag_for("module_issue"), Some("module"));
        assert_eq!(event_flag_for("cycle"), Some("cycle"));
        assert_eq!(event_flag_for("cycle_issue"), Some("cycle"));
        assert_eq!(event_flag_for("issue_comment"), Some("issue_comment"));
        // Q1: no branch matches -> unfiltered, every active webhook fires.
        assert_eq!(event_flag_for("user"), None);
        assert_eq!(event_flag_for("intake_issue"), None);
    }

    #[test]
    fn deleted_verb_uses_id_only_event_data() {
        // webhook_task.py:441: {'id': event_id} when verb == 'deleted'.
        assert_eq!(deleted_event_data("iid-1"), json!({"id": "iid-1"}));
    }

    #[test]
    fn sync_wire_is_positional_id_with_empty_kwargs() {
        // .delay(str(id)) is args=[str], kwargs={} on the wire — the same
        // shape D-05's fanout_job/completion_job build.
        for (task, id) in [
            (GITHUB_SYNC_ONE_REPO_TASK, "sync-1"),
            (GIT_SYNC_ONE_BINDING_TASK, "binding-1"),
            (GITHUB_POST_COMPLETION_COMMENT_TASK, "gsync-1"),
            (GIT_POST_COMPLETION_COMMENT_TASK, "isync-1"),
        ] {
            let (args, kwargs) = sync_positional_wire(id);
            assert_eq!(args, json!([id]), "task {task}");
            assert!(kwargs.is_empty(), "task {task}");
            assert!(task.contains("pi_dash.bgtasks."));
        }
    }

    #[test]
    fn completion_guard_skips_commented_rows_only() {
        // github_signals.py:56-74: metadata carrying completion_comment_id
        // skips; absent ids proceed.
        let empty = Map::new();
        assert!(!should_skip_completion(&empty));
        let commented = Map::from_iter([("completion_comment_id".to_owned(), json!("12345"))]);
        assert!(should_skip_completion(&commented));
        let nulled = Map::from_iter([("completion_comment_id".to_owned(), Value::Null)]);
        assert!(!should_skip_completion(&nulled));
    }

    #[test]
    fn beat_entry_keeps_the_legacy_name_on_the_new_poller() {
        // celery.py:113-118 (Q5): legacy schedule name, new task, 4h crontab.
        assert_eq!(GITHUB_SYNC_BEAT_NAME, "github-issue-sync-every-4h");
        assert_eq!(
            GITHUB_SYNC_BEAT_TASK,
            "pi_dash.bgtasks.git_sync_task.sync_all_bindings"
        );
        assert_eq!(GITHUB_SYNC_BEAT_SCHEDULE, "crontab(minute=0, hour=*/4)");
    }

    #[test]
    fn retry_exhaustion_deactivates_at_max_retries() {
        assert!(!should_deactivate(0));
        assert!(!should_deactivate(WEBHOOK_SEND_MAX_RETRIES - 1));
        assert!(should_deactivate(WEBHOOK_SEND_MAX_RETRIES));
        assert_eq!(WEBHOOK_SEND_MAX_RETRIES, 5);
    }

    #[test]
    fn hmac_wire_vector_replays_the_fixture_quirk() {
        // Q4 re-pin (fixture hmac_wire_quirk_b4, EXECUTED): the signature
        // input is json.dumps with DEFAULT separators (', ', ': '), and
        // HMAC-SHA256 over it with the secret hexes to the pinned value.
        // D-08 owns the renderer/signer; this replays the executed bytes
        // on this side of the boundary.
        use hmac::{Hmac, KeyInit, Mac};
        let f = fixture();
        let vector = &f["hmac_wire_quirk_b4"];
        assert_eq!(vector["executed"], json!(true));
        let wire = vector["json_wire"].as_str().expect("json_wire");
        // Default-separator rendering carries ': ' after every colon.
        assert!(wire.contains("\": \""));
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(b"s3cret")
            .expect("HMAC-SHA256 accepts any key length");
        mac.update(wire.as_bytes());
        let hex = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(
            hex,
            vector["sha256_hex_secret_s3cret"].as_str().expect("hex")
        );
    }
}
