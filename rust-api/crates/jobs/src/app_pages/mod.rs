//! Page-view publish envelopes (D-30, jobs layer).
//!
//! Port of the four Celery `.delay()` call sites the page handlers trigger
//! in `apps/api/pi_dash/app/views/page/base.py`. Task *bodies* belong to
//! D-08/D-09 — this module ports only the publish envelopes: the same task
//! names, the same kwarg shapes, and the same enqueue conditions (including
//! the no-publish branches), verified against fixture F30-10
//! (`rust-api/fixtures/app_pages/tasks/publish.golden.json`).
//!
//! Call sites (`base.py`):
//!
//! * `page_transaction.delay` — create (`:144-148`, unconditional, old
//!   `None`), partial_update (`:187-192`, only when
//!   `request.data.get("description_html")` is truthy), description update
//!   (`:560-565`, same truthy gate), duplicate (`:613-617`, unconditional,
//!   old `None`, the NEW copy id).
//! * `track_page_version.delay` — description update only (`:568-572`,
//!   unconditional on success; `existing_instance` is the `:552`
//!   `json.dumps({"description_html": old}, cls=DjangoJSONEncoder)`).
//! * `recent_visited_task.delay` — retrieve only (`:237-243`), gated by the
//!   `track_visit` query param (`:205`: default `"true"`, `.lower() ==
//!   "true"`).
//! * `copy_s3_objects_of_description_and_assets.delay` — duplicate only
//!   (`:620-626`, unconditional).
//!
//! Wire construction delegates to the merged D-08/D-09 constructors (the
//! single source of wire truth for each task) and each site additionally
//! exposes its F-09 queue [`NewJob`], so handlers can `enqueue_in` the row
//! inside their own transaction and publish the identical Celery v2 body
//! over AMQP during coexistence. This module registers no [`Registry`]
//! handlers: the task names stay owned by D-08/D-09, and registering them
//! twice would collide.
//!
//! Ported bug (translate, don't redesign): the duplicate `copy_s3` call
//! passes the `project_id` loop variable (`:604` shadows the URL kwarg, so
//! at `:623` it is the LAST source project — or the URL kwarg unchanged
//! when the page has no projects). The wrapper takes the effective id as a
//! plain pass-through parameter, i.e. the quirk is preserved exactly.
//!
//!_kwargs key order_: Python's wire preserves the caller's insertion order
//! while Rust builds a sorted map (like every merged port); JSON objects
//! are unordered, so parsed-value equality is the contract the tests pin.

use serde_json::{Map, Value};

use crate::celery::CeleryTaskMessage;
use crate::queue::NewJob;
use crate::tasks_cleanup::assets;
use crate::tasks_cleanup::versions;
use crate::tasks_webhooks::{django_dumps, visit_page};

/// `request.data.get("description_html", "<p></p>")` (`base.py:137, 145,
/// 189, 562`): the default new-HTML when the key is absent.
pub const DEFAULT_NEW_DESCRIPTION_HTML: &str = "<p></p>";

/// Apply the `.get("description_html", "<p></p>")` default.
pub fn default_new_html(request_html: Option<&str>) -> &str {
    request_html.unwrap_or(DEFAULT_NEW_DESCRIPTION_HTML)
}

/// `if request.data.get("description_html"):` (`base.py:187, 560`):
/// Python truthiness — `None`, a missing key, and `""` are all falsy, any
/// non-empty string is truthy.
pub fn publish_on_html_present(request_html: Option<&str>) -> bool {
    request_html.is_some_and(|html| !html.is_empty())
}

/// `request.query_params.get("track_visit", "true").lower() == "true"`
/// (`base.py:205`): absent means `"true"` (publish); anything whose
/// lowercase form is not exactly `"true"` — `"false"`, `"0"`, `"yes"` —
/// suppresses the visit task.
pub fn track_visit_enabled(track_visit: Option<&str>) -> bool {
    track_visit
        .map(|raw| raw.to_lowercase() == "true")
        .unwrap_or(true)
}

/// `json.dumps({"description_html": old}, cls=DjangoJSONEncoder)`
/// (`base.py:552`): stdlib separators with ASCII-only escaping. For the
/// `str`-or-`None` values this field carries, [`django_dumps`] renders
/// byte-identical output (verified against CPython in the tests below).
pub fn existing_instance_json(old_description_html: Option<&str>) -> String {
    let mut fields = Map::new();
    fields.insert(
        "description_html".to_owned(),
        old_description_html.map_or(Value::Null, |html| Value::String(html.to_owned())),
    );
    django_dumps(&Value::Object(fields))
}

/// Lift a constructed message into its queue row: `args=[]` with the same
/// kwargs, so the worker forward path rebuilds the identical Celery v2
/// body whichever plane serves the task.
fn job_from_message(message: &CeleryTaskMessage) -> NewJob {
    NewJob::new(
        message.task.clone(),
        Value::Array(Vec::new()),
        Value::Object(message.kwargs.clone()),
    )
}

// ---------------------------------------------------------------------------
// `page_transaction` (`page_transaction_task.py:84`)
// ---------------------------------------------------------------------------

/// Create (`base.py:144-148`): unconditional after `serializer.save()`;
/// old HTML is `None`, the id is the NEW page id.
pub fn page_transaction_create_message(
    new_page_id: &str,
    request_html: Option<&str>,
) -> CeleryTaskMessage {
    visit_page::page_transaction_message(Some(default_new_html(request_html)), None, new_page_id)
}

/// [`page_transaction_create_message`] as its queue row.
pub fn page_transaction_create_job(new_page_id: &str, request_html: Option<&str>) -> NewJob {
    job_from_message(&page_transaction_create_message(new_page_id, request_html))
}

/// Partial update (`base.py:187-192`) and description update
/// (`base.py:560-565`): identical gate and shape — publish only when
/// `description_html` is truthy; old HTML is the pre-save snapshot
/// (`:183` / `:549`), the id is the URL kwarg. `None` renders the
/// no-publish branch.
pub fn page_transaction_save_message(
    page_id: &str,
    old_snapshot_html: Option<&str>,
    request_html: Option<&str>,
) -> Option<CeleryTaskMessage> {
    if !publish_on_html_present(request_html) {
        return None;
    }
    Some(visit_page::page_transaction_message(
        Some(default_new_html(request_html)),
        old_snapshot_html,
        page_id,
    ))
}

/// [`page_transaction_save_message`] as its queue row (`None` = no-publish
/// branch, no row is enqueued).
pub fn page_transaction_save_job(
    page_id: &str,
    old_snapshot_html: Option<&str>,
    request_html: Option<&str>,
) -> Option<NewJob> {
    page_transaction_save_message(page_id, old_snapshot_html, request_html)
        .map(|message| job_from_message(&message))
}

/// Duplicate (`base.py:613-617`): unconditional; new HTML is the copied
/// page's `description_html`, old is `None`, the id is the NEW copy id.
pub fn page_transaction_duplicate_message(
    new_copy_id: &str,
    copied_html: Option<&str>,
) -> CeleryTaskMessage {
    visit_page::page_transaction_message(copied_html, None, new_copy_id)
}

/// [`page_transaction_duplicate_message`] as its queue row.
pub fn page_transaction_duplicate_job(new_copy_id: &str, copied_html: Option<&str>) -> NewJob {
    job_from_message(&page_transaction_duplicate_message(
        new_copy_id,
        copied_html,
    ))
}

// ---------------------------------------------------------------------------
// `track_page_version` (`page_version_task.py:22`)
// ---------------------------------------------------------------------------

/// Description update (`base.py:568-572`): unconditional on serializer
/// success — runs alongside `page_transaction` whenever `description_html`
/// is present, and alone when it is not. No merged constructor exists for
/// this task yet, so the kwargs are built here in the same kwargs-only
/// `.delay(page_id=..., existing_instance=..., user_id=...)` shape the
/// call site uses.
pub fn track_page_version_message(
    page_id: &str,
    existing_instance: &str,
    user_id: &str,
) -> CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("page_id".to_owned(), Value::String(page_id.to_owned()));
    kwargs.insert(
        "existing_instance".to_owned(),
        Value::String(existing_instance.to_owned()),
    );
    kwargs.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
    CeleryTaskMessage::new(versions::TASK_TRACK_PAGE_VERSION, Vec::new(), kwargs)
}

/// [`track_page_version_message`] as its queue row.
pub fn track_page_version_job(page_id: &str, existing_instance: &str, user_id: &str) -> NewJob {
    job_from_message(&track_page_version_message(
        page_id,
        existing_instance,
        user_id,
    ))
}

// ---------------------------------------------------------------------------
// `recent_visited_task` (`recent_visited_task.py:17`)
// ---------------------------------------------------------------------------

/// Retrieve (`base.py:236-243`): only when `track_visit_enabled`; the call
/// site hardcodes `entity_name="page"` (`:239`) and passes the URL kwargs
/// straight through.
pub fn recent_visited_message(
    track_visit: Option<&str>,
    slug: &str,
    page_id: &str,
    user_id: &str,
    project_id: Option<&str>,
) -> Option<CeleryTaskMessage> {
    if !track_visit_enabled(track_visit) {
        return None;
    }
    Some(visit_page::recent_visited_task_message(
        "page",
        Some(page_id),
        user_id,
        project_id,
        slug,
    ))
}

/// [`recent_visited_message`] as its queue row (`None` = `track_visit=false`
/// branch, no row is enqueued).
pub fn recent_visited_job(
    track_visit: Option<&str>,
    slug: &str,
    page_id: &str,
    user_id: &str,
    project_id: Option<&str>,
) -> Option<NewJob> {
    recent_visited_message(track_visit, slug, page_id, user_id, project_id)
        .map(|message| job_from_message(&message))
}

// ---------------------------------------------------------------------------
// `copy_s3_objects_of_description_and_assets` (`copy_s3_object.py:123`)
// ---------------------------------------------------------------------------

/// Duplicate (`base.py:620-626`): unconditional, `entity_name="PAGE"`,
/// `entity_identifier` is the NEW copy id, and `project_id` is the
/// effective (shadowed loop-var) id passed straight through — see the
/// module docs for the ported quirk.
pub fn copy_s3_duplicate_message(
    new_copy_id: &str,
    effective_project_id: &str,
    slug: &str,
    user_id: &str,
) -> CeleryTaskMessage {
    assets::delay_copy(
        "PAGE",
        new_copy_id,
        Some(effective_project_id),
        slug,
        user_id,
    )
}

/// [`copy_s3_duplicate_message`] as its queue row.
pub fn copy_s3_duplicate_job(
    new_copy_id: &str,
    effective_project_id: &str,
    slug: &str,
    user_id: &str,
) -> NewJob {
    job_from_message(&copy_s3_duplicate_message(
        new_copy_id,
        effective_project_id,
        slug,
        user_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PAGE_ID: &str = "11111111-1111-4111-8111-111111111111";
    const PROJECT_ID: &str = "22222222-2222-4222-8222-222222222222";
    const USER_ID: &str = "33333333-3333-4333-8333-333333333333";
    const SLUG: &str = "acme";

    fn kwargs_of(message: &CeleryTaskMessage) -> Value {
        Value::Object(message.kwargs.clone())
    }

    #[test]
    fn create_publishes_unconditionally_with_default_html() {
        // F30-10 row 1: absent description_html renders "<p></p>", old None.
        let message = page_transaction_create_message(PAGE_ID, None);
        assert_eq!(message.task, visit_page::PAGE_TRANSACTION_TASK_NAME);
        assert!(message.args.is_empty());
        assert_eq!(
            kwargs_of(&message),
            json!({"new_description_html": "<p></p>", "old_description_html": null, "page_id": PAGE_ID})
        );
        let job = page_transaction_create_job(PAGE_ID, Some("<p>hello</p>"));
        assert_eq!(job.task, visit_page::PAGE_TRANSACTION_TASK_NAME);
        assert_eq!(job.args, json!([]));
        assert_eq!(
            job.kwargs,
            kwargs_of(&page_transaction_create_message(
                PAGE_ID,
                Some("<p>hello</p>")
            ))
        );
    }

    #[test]
    fn partial_update_gate_matches_python_truthiness() {
        // F30-10 row 2: None / missing / "" publish nothing; non-empty publishes.
        assert!(!publish_on_html_present(None));
        assert!(!publish_on_html_present(Some("")));
        assert!(publish_on_html_present(Some("<p></p>")));
        assert!(page_transaction_save_message(PAGE_ID, Some("<p>old</p>"), None).is_none());
        assert!(page_transaction_save_message(PAGE_ID, Some("<p>old</p>"), Some("")).is_none());
        let message =
            page_transaction_save_message(PAGE_ID, Some("<p>old</p>"), Some("<p>new</p>"))
                .expect("truthy html publishes");
        assert_eq!(
            kwargs_of(&message),
            json!({"new_description_html": "<p>new</p>", "old_description_html": "<p>old</p>", "page_id": PAGE_ID})
        );
        // Description update (:560-565) shares the gate and shape.
        let described =
            page_transaction_save_message(PAGE_ID, None, Some("<p>n</p>")).expect("publishes");
        assert_eq!(
            kwargs_of(&described),
            json!({"new_description_html": "<p>n</p>", "old_description_html": null, "page_id": PAGE_ID})
        );
    }

    #[test]
    fn existing_instance_matches_cpython_dumps_bytes() {
        // Oracle values generated with CPython `json.dumps(..., cls=DjangoJSONEncoder)`
        // for str/None inputs (the encoder only extends datetime/Decimal/UUID).
        assert_eq!(
            existing_instance_json(Some("<p>hi</p>")),
            "{\"description_html\": \"<p>hi</p>\"}"
        );
        assert_eq!(existing_instance_json(None), "{\"description_html\": null}");
        assert_eq!(
            existing_instance_json(Some("caf\u{e9} \"q\" \\ back \n tab \u{1F44D}")),
            r#"{"description_html": "caf\u00e9 \"q\" \\ back \n tab \ud83d\udc4d"}"#,
        );
    }

    #[test]
    fn track_page_version_is_unconditional_on_success() {
        // F30-10 row 5: no gate — the description handler always publishes
        // on serializer success, alongside page_transaction when html is
        // present and alone when it is not.
        let instance = existing_instance_json(Some("<p>old</p>"));
        let message = track_page_version_message(PAGE_ID, &instance, USER_ID);
        assert_eq!(message.task, versions::TASK_TRACK_PAGE_VERSION);
        assert!(message.args.is_empty());
        assert_eq!(
            kwargs_of(&message),
            json!({"page_id": PAGE_ID, "existing_instance": instance, "user_id": USER_ID})
        );
        let job = track_page_version_job(PAGE_ID, &instance, USER_ID);
        assert_eq!(job.args, json!([]));
        assert_eq!(job.kwargs, kwargs_of(&message));
    }

    #[test]
    fn retrieve_visit_gate_defaults_true_and_matches_lower_compare() {
        // F30-10 row 6: absent param publishes; only lowercase-"true" keeps it.
        assert!(track_visit_enabled(None));
        for raw in ["true", "True", "TRUE", "tRuE"] {
            assert!(track_visit_enabled(Some(raw)), "{raw} enables");
        }
        for raw in ["false", "False", "FALSE", "0", "yes", ""] {
            assert!(!track_visit_enabled(Some(raw)), "{raw} suppresses");
        }
        assert!(recent_visited_message(None, SLUG, PAGE_ID, USER_ID, Some(PROJECT_ID)).is_some());
        assert!(
            recent_visited_message(Some("false"), SLUG, PAGE_ID, USER_ID, Some(PROJECT_ID))
                .is_none()
        );
        assert!(
            recent_visited_job(Some("false"), SLUG, PAGE_ID, USER_ID, Some(PROJECT_ID)).is_none()
        );
        let message = recent_visited_message(None, SLUG, PAGE_ID, USER_ID, Some(PROJECT_ID))
            .expect("publishes");
        assert_eq!(message.task, visit_page::RECENT_VISITED_TASK_NAME);
        assert_eq!(
            kwargs_of(&message),
            json!({"entity_name": "page", "entity_identifier": PAGE_ID, "user_id": USER_ID, "project_id": PROJECT_ID, "slug": SLUG})
        );
    }

    #[test]
    fn duplicate_ports_shadowed_project_id_as_observed() {
        // F30-10 rows 4+7: page_transaction (copied html, old None, NEW id)
        // plus copy_s3 with the shadowed loop-var project id passed through.
        let new_copy_id = "44444444-4444-4444-8444-444444444444";
        let shadowed = "55555555-5555-4555-8555-555555555555";
        let tx = page_transaction_duplicate_message(new_copy_id, Some("<p>copied</p>"));
        assert_eq!(
            kwargs_of(&tx),
            json!({"new_description_html": "<p>copied</p>", "old_description_html": null, "page_id": new_copy_id})
        );
        let copy = copy_s3_duplicate_message(new_copy_id, shadowed, SLUG, USER_ID);
        assert_eq!(copy.task, assets::TASK_COPY_S3_OBJECTS);
        assert_eq!(
            kwargs_of(&copy),
            json!({"entity_name": "PAGE", "entity_identifier": new_copy_id, "project_id": shadowed, "slug": SLUG, "user_id": USER_ID})
        );
        let job = copy_s3_duplicate_job(new_copy_id, shadowed, SLUG, USER_ID);
        assert_eq!(job.args, json!([]));
        assert_eq!(job.kwargs, kwargs_of(&copy));
    }
}
