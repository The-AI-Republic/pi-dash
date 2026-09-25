"""Project binding list/install contract.

The install payload is also the wire format installed runners consume
(``pod``/``pod_name``/``next_run_at``/``last_run_*``), so the shape
assertion pins those fields explicitly.
"""

from . import seed_scheduler as seed_s
from .test_workspace_schedulers import sched_list


def bind_list(world):
    return (
        f"/api/workspaces/{world.workspace['slug']}"
        f"/projects/{world.project['id']}/scheduler-bindings/"
    )


BINDING_KEYS = {
    "id", "scheduler", "scheduler_slug", "scheduler_name",
    "scheduler_color", "project", "workspace", "dtstart", "tzid", "rrule",
    "rdates", "exdates", "extra_context", "enabled", "outcome_mode", "pod",
    "pod_name", "next_run_at", "last_run", "last_run_status",
    "last_run_ended_at", "last_error", "actor", "created_at", "updated_at",
}


def _scheduler(org, slug="sched"):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": slug, "name": slug, "prompt": "x", "color": "#10b981"},
    )
    assert r.status_code == 201, r.text
    return r.json()


def _install(org, scheduler_id, **extra):
    payload = {
        "scheduler": scheduler_id,
        "project": org.project["id"],
        "dtstart": seed_s.hours_ago(1),
        "tzid": "UTC",
        "rrule": "FREQ=HOURLY",
    }
    payload.update(extra)
    return org.request("POST", bind_list(org), "admin", json=payload)


def test_list_empty(org):
    r = org.request("GET", bind_list(org), "admin")
    assert r.status_code == 200
    assert r.json() == []


def test_install_shape(org):
    sched = _scheduler(org)
    r = _install(org, sched["id"])
    assert r.status_code == 201, r.text
    body = r.json()
    assert set(body.keys()) == BINDING_KEYS
    assert body["scheduler"] == sched["id"]
    assert body["scheduler_slug"] == "sched"
    assert body["scheduler_name"] == "sched"
    assert body["scheduler_color"] == "#10b981"
    assert body["project"] == org.project["id"]
    assert body["workspace"] == org.workspace["id"]
    assert body["tzid"] == "UTC"
    assert body["rrule"] == "FREQ=HOURLY"
    assert body["rdates"] == []
    assert body["exdates"] == []
    assert body["extra_context"] == ""
    assert body["enabled"] is True
    assert body["outcome_mode"] == "create_issue"
    # Runner wire fields: install computes the first fire time and records
    # the installing actor; nothing has run yet.
    assert body["pod"] is None
    assert body["pod_name"] is None
    assert body["next_run_at"] is not None
    assert body["last_run"] is None
    assert body["last_run_status"] is None
    assert body["last_run_ended_at"] is None
    assert body["last_error"] == ""
    assert body["actor"] == org.admin["id"]


def test_install_requires_project_in_body(org):
    # Contract quirk: the view pins the project at save() but the serializer
    # still requires it in the payload.
    sched = _scheduler(org)
    r = org.request(
        "POST", bind_list(org), "admin",
        json={"scheduler": sched["id"], "dtstart": seed_s.hours_ago(1),
              "rrule": "FREQ=HOURLY"},
    )
    assert r.status_code == 400
    assert r.json() == {"project": ["This field is required."]}


def test_install_bad_rrule(org):
    sched = _scheduler(org)
    r = _install(org, sched["id"], rrule="FREQ=NEVER")
    assert r.status_code == 400
    assert "rrule" in r.json()


def test_install_bad_tzid(org):
    sched = _scheduler(org)
    r = _install(org, sched["id"], tzid="Mars/Olympus")
    assert r.status_code == 400
    assert "tzid" in r.json()


def test_install_rdates_must_be_array(org):
    sched = _scheduler(org)
    r = _install(org, sched["id"], rdates="tomorrow")
    assert r.status_code == 400
    assert "rdates" in r.json()


def test_install_extra_context_too_long(org):
    sched = _scheduler(org)
    r = _install(org, sched["id"], extra_context="x" * (16 * 1024 + 1))
    assert r.status_code == 400
    assert "extra_context" in r.json()


def test_install_canonicalises_rrule_prefix(org):
    sched = _scheduler(org)
    r = _install(org, sched["id"], rrule="RRULE:FREQ=DAILY")
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["rrule"] == "FREQ=DAILY"
    assert body["tzid"] == "UTC"


def test_install_disabled_scheduler_404(org):
    sched = _scheduler(org)
    assert org.request(
        "PATCH", f"{sched_list(org)}{sched['id']}/", "admin",
        json={"is_enabled": False},
    ).status_code == 200
    r = _install(org, sched["id"])
    assert r.status_code == 404
    assert r.json() == {"detail": "No Scheduler matches the given query."}


def test_install_duplicate(org):
    sched = _scheduler(org)
    assert _install(org, sched["id"]).status_code == 201
    r = _install(org, sched["id"])
    assert r.status_code == 400
    assert r.json() == {
        "non_field_errors": [
            "The fields scheduler, project must make a unique set."
        ]
    }


def test_list_ordered_newest_first(org):
    first = _scheduler(org, slug="first")
    second = _scheduler(org, slug="second")
    assert _install(org, first["id"]).status_code == 201
    assert _install(org, second["id"]).status_code == 201
    r = org.request("GET", bind_list(org), "admin")
    assert r.status_code == 200
    rows = r.json()
    assert [row["scheduler_slug"] for row in rows] == ["second", "first"]
    for row in rows:
        assert set(row.keys()) == BINDING_KEYS
