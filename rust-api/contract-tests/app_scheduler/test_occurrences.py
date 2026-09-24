"""Occurrences calendar endpoint contract.

Merges past ``AgentRun`` rows (``kind=past``) with future RRULE expansion
(``kind=scheduled``) over a ``from``/``to`` window defaulting to ±30 days.
"""

from . import seed_scheduler as seed_s
from .test_project_bindings import bind_list
from .test_workspace_schedulers import sched_list


def occurrences(world):
    return f"{bind_list(world)}occurrences/"


OCCURRENCE_KEYS = {
    "binding_id", "scheduler_id", "scheduler_name", "scheduler_color",
    "dtstart", "tzid", "kind", "agent_run_id", "status",
}


def _hourly(org, slug="hourly", **kw):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": slug, "name": slug, "prompt": "x", "color": "#10b981"},
    )
    assert r.status_code == 201, r.text
    binding = seed_s.create_binding(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"], scheduler_id=r.json()["id"],
        dtstart=seed_s.hours_ago(25), rrule="FREQ=HOURLY", **kw,
    )
    return r.json(), binding


def test_default_window_shape(org):
    _hourly(org)
    r = org.request("GET", occurrences(org), "guest")
    assert r.status_code == 200
    body = r.json()
    assert set(body.keys()) == {"occurrences", "has_more", "next_window_start"}
    assert body["has_more"] is False
    assert body["next_window_start"] is None
    assert len(body["occurrences"]) > 0
    for occ in body["occurrences"]:
        assert set(occ.keys()) == OCCURRENCE_KEYS
        assert occ["kind"] == "scheduled"
        assert occ["agent_run_id"] is None
        assert occ["status"] is None
        assert occ["scheduler_name"] == "hourly"
    dts = [occ["dtstart"] for occ in body["occurrences"]]
    assert dts == sorted(dts)


def test_explicit_window_bounds(org):
    _hourly(org)
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": seed_s.days_ago(2), "to": seed_s.days_from_now(2)},
    )
    assert r.status_code == 200, r.text
    rows = r.json()["occurrences"]
    assert len(rows) > 0
    lo, hi = seed_s.days_ago(2), seed_s.days_from_now(2)
    assert all(lo <= occ["dtstart"] <= hi for occ in rows)


def test_disabled_scheduler_excluded(org):
    sched, _ = _hourly(org, slug="off")
    assert org.request(
        "PATCH", f"{sched_list(org)}{sched['id']}/", "admin",
        json={"is_enabled": False},
    ).status_code == 200
    rows = org.request("GET", occurrences(org), "guest").json()["occurrences"]
    assert all(occ["scheduler_name"] != "off" for occ in rows)


def test_disabled_binding_excluded(org):
    _hourly(org, slug="quiet", enabled=False)
    rows = org.request("GET", occurrences(org), "guest").json()["occurrences"]
    assert all(occ["scheduler_name"] != "quiet" for occ in rows)


def test_past_runs_merged(org):
    _, binding = _hourly(org)
    pod = seed_s.create_pod(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"],
    )
    run = seed_s.create_agent_run(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"], pod_id=pod["id"],
        binding_id=binding["id"], created_by_id=org.admin["id"],
        started_at=seed_s.days_ago(2),
    )
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": seed_s.days_ago(3), "to": seed_s.days_from_now(1)},
    )
    assert r.status_code == 200, r.text
    past = [o for o in r.json()["occurrences"] if o["kind"] == "past"]
    assert len(past) == 1
    assert set(past[0].keys()) == OCCURRENCE_KEYS
    assert past[0]["agent_run_id"] == run["id"]
    assert past[0]["status"] == "completed"
    assert past[0]["binding_id"] == binding["id"]
    assert past[0]["tzid"] == "UTC"


def test_empty_project(org):
    body = org.request("GET", occurrences(org), "guest").json()
    assert body == {"occurrences": [], "has_more": False,
                    "next_window_start": None}


def test_invalid_window(org):
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": seed_s.days_from_now(1), "to": seed_s.days_ago(1)},
    )
    assert r.status_code == 400
    assert r.json() == {
        "error": "invalid_window", "detail": "`to` must be >= `from`",
    }


def test_window_too_large(org):
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": seed_s.days_ago(80), "to": seed_s.days_from_now(11)},
    )
    assert r.status_code == 400
    assert r.json() == {
        "error": "window_too_large",
        "detail": "date window must be <= 90 days",
    }


def test_garbage_window_falls_back_to_defaults(org):
    _hourly(org)
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": "not-a-date", "to": "also-not-a-date"},
    )
    assert r.status_code == 200
    assert len(r.json()["occurrences"]) > 0


def test_cap_truncates(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": "fast", "name": "fast", "prompt": "x"},
    )
    assert r.status_code == 201, r.text
    seed_s.create_binding(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"], scheduler_id=r.json()["id"],
        dtstart=seed_s.days_ago(80), rrule="FREQ=MINUTELY",
    )
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": seed_s.days_ago(80), "to": seed_s.days_from_now(9)},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert len(body["occurrences"]) == 5000
    assert body["has_more"] is True
    assert isinstance(body["next_window_start"], str)
