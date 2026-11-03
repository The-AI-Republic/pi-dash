"""PIDASHCONV-1181: app_cycles activity `origin` = APP_BASE_URL else WEB_URL.

Django `base_host(request, is_app=True)` (`utils/host.py:17-67`) returns
`APP_BASE_URL` when truthy, else `WEB_URL` when truthy, else a 500. The Rust
port must do the same at every app_cycles activity-enqueue site.

These tests pin that under two server configurations, so they need a backend
booted with the matching URLs (the shared HTTP matrix boots `WEB_URL ==
APP_BASE_URL`, which cannot tell the two apart):

* ``app_base_url`` — backend booted with ``WEB_URL`` and a *different*
  ``APP_BASE_URL`` (env ``CONTRACT_APP_BASE_URL``). Cycle create/PATCH/destroy
  and transfer activities must all carry ``origin == APP_BASE_URL``.
* ``web_url_only`` — backend booted with ``WEB_URL`` only (``CONTRACT_APP_BASE_URL``
  empty). Cycle-issue create/destroy must answer 201/204 and carry
  ``origin == WEB_URL``; the Rust port used to 500.

They run in ``.github/workflows/rust-api-contract-tests-app-cycles-origin.yml``
(excluded from the shared matrix via ``TASK_ORACLE_IGNORES.txt``); each job
selects its test with ``-k app_base_url`` / ``-k web_url_only``.

Activity rows land in different transports per backend and no worker runs:
Rust serve INSERTs into ``rust_job_queue``; Django ``.delay()``s onto the
Celery broker. Each helper reads both, and only the active backend has rows.
"""

import json
import os

import pytest

from _harness import env

from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract

MODEL_TASK = "pi_dash.bgtasks.webhook_task.model_activity"
ISSUE_TASK = "pi_dash.bgtasks.issue_activities_task.issue_activity"


def _configured_app_base_url() -> str:
    """The backend's APP_BASE_URL (``""`` when unset)."""
    return os.environ.get("CONTRACT_APP_BASE_URL", "").rstrip("/")


def _drain_broker() -> list[dict]:
    """Celery-protocol messages pending on the broker (Django's transport)."""
    from _harness import broker_probe

    rows: list[dict] = []
    for _headers, payload in broker_probe.drain_queue():
        if isinstance(payload, list) and len(payload) > 1 and isinstance(payload[1], dict):
            rows.append(payload[1])
    return rows


def _rust_queue(task: str) -> list[dict]:
    """Activity rows Rust serve left in ``rust_job_queue`` (table may be absent)."""
    with env.connect() as conn, conn.cursor() as cur:
        cur.execute("SELECT to_regclass('public.rust_job_queue')")
        if cur.fetchone()[0] is None:
            return []
        cur.execute("SELECT kwargs FROM rust_job_queue WHERE task = %s", (task,))
        rows = []
        for (kwargs,) in cur.fetchall():
            rows.append(kwargs if isinstance(kwargs, dict) else json.loads(kwargs))
        return rows


def _activities_for(actor_id: str) -> list[dict]:
    rows = _drain_broker() + _rust_queue(MODEL_TASK) + _rust_queue(ISSUE_TASK)
    # Django serialises model_activity's actor_id as a UUID object and
    # issue_activity's as a string; stringify both sides.
    return [row for row in rows if str(row.get("actor_id")) == str(actor_id)]


@pytest.fixture()
def tenant(make_tenant):
    """A fresh tenant whose own ``cycle_issues`` rows are cleared before teardown.

    ``_harness.seed.cleanup`` sweeps issues/workspaces but not the ``cycles``
    / ``cycle_issues`` rows these tests POST; teardown's project/issue deletes
    would hit their FKs. Hard-delete this test's own rows (unique project id)
    first, bridges before cycles.
    """
    built = make_tenant()
    yield built
    with built["conn"].cursor() as cur:
        cur.execute(
            "DELETE FROM cycle_issues WHERE project_id = %s",
            (built["project"]["id"],),
        )
        cur.execute(
            "DELETE FROM cycles WHERE project_id = %s",
            (built["project"]["id"],),
        )


def test_activity_origin_prefers_app_base_url(tenant):
    app_base = _configured_app_base_url()
    web = env.base_url().rstrip("/")
    assert app_base, "this job needs the backend booted with CONTRACT_APP_BASE_URL"
    assert app_base != web, "this job needs APP_BASE_URL different from WEB_URL"

    urls = cycle_urls(tenant)
    cycle = tenant["client"].post(urls["cycles"], json={"name": "Origin A"}).json()
    pk = cycle["id"]
    assert tenant["client"].patch(
        urls["detail"](pk), json={"name": "Origin A patched"}
    ).status_code == 200

    issue = create_issue_for(tenant, name="origin transfer")
    source = tenant["client"].post(
        urls["cycles"], json={"name": "Origin A source"}
    ).json()
    assert tenant["client"].post(
        urls["cycle_issues"](source["id"]), json={"issues": [issue["id"]]}
    ).status_code == 201
    assert tenant["client"].post(
        urls["transfer"](source["id"]), json={"new_cycle_id": pk}
    ).status_code == 200
    assert tenant["client"].delete(urls["detail"](pk)).status_code in (200, 204)

    rows = _activities_for(tenant["user"]["id"])
    assert rows, "expected enqueued activity rows"
    assert {row.get("origin") for row in rows} == {app_base}

    # create + PATCH enqueue cycle model_activity rows for the same pk;
    # destroy enqueues an issue_activity fan-out.
    model = [
        row for row in rows
        if row.get("model_name") == "cycle" and row.get("model_id") == pk
    ]
    assert len(model) >= 2, f"expected create/PATCH activities, got {len(model)}"
    assert any(row.get("type") == "cycle.activity.deleted" for row in rows)
    assert any(row.get("type") == "cycle.activity.created" for row in rows)


def test_cycle_issue_activity_origin_web_url_only(tenant):
    app_base = _configured_app_base_url()
    web = env.base_url().rstrip("/")
    assert app_base == "", "this job needs the backend booted with WEB_URL only"

    urls = cycle_urls(tenant)
    pk = tenant["client"].post(urls["cycles"], json={"name": "Origin B"}).json()["id"]
    issue = create_issue_for(tenant, name="origin web-only")

    created = tenant["client"].post(
        urls["cycle_issues"](pk), json={"issues": [issue["id"]]}
    )
    assert created.status_code == 201, created.text[:300]
    assert created.json() == {"message": "success"}

    deleted = tenant["client"].delete(urls["cycle_issue_detail"](pk, issue["id"]))
    assert deleted.status_code == 204, deleted.text[:300]

    rows = _activities_for(tenant["user"]["id"])
    assert rows, "expected enqueued activity rows"
    assert {row.get("origin") for row in rows} == {web}
