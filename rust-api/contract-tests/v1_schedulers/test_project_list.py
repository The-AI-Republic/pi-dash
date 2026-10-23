"""Contract tests: v1 project scheduler list (PIDASHCONV-1061).

Ports ``TestProjectSchedulerList`` from Django's
``apps/api/pi_dash/tests/contract/api/test_schedulers.py`` (PDASHOSS01-225)
plus the binding shape pin and the slug-or-UUID project routing matrix.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402

KEY = "pl"

BINDING_KEYS = [
    "id", "scheduler", "scheduler_slug", "scheduler_name", "project",
    "workspace", "dtstart", "tzid", "rrule", "rdates", "exdates",
    "extra_context", "enabled", "pod", "pod_name",
    "next_run_at", "last_run", "last_run_status", "last_run_started_at",
    "last_run_ended_at", "last_error", "created_at", "updated_at",
]

FORBIDDEN_BODY = {"detail": "You do not have permission to perform this action."}
PROJECT_NOT_FOUND_BODY = {"detail": "Project not found"}


def base(seed, project_id=None):
    pid = project_id if project_id is not None else seed["project"]["id"]
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{pid}/schedulers/"


def test_member_lists_project_schedulers(seed):
    r = http.get(seed["keys"][KEY], base(seed))
    body = r.json()
    rows = body["results"]
    assert len(rows) == 1
    row = rows[0]
    assert row["slug"] == seed["scheduler"]["slug"]
    assert row["name"] == f"CT Scheduler {seed['tag']}"
    assert len(row["bindings"]) == 1
    b = row["bindings"][0]
    assert list(b.keys()) == BINDING_KEYS
    assert b["rrule"] == "FREQ=HOURLY"
    assert b["enabled"] is True
    assert b["next_run_at"] is not None
    assert b["next_run_at"] == "2026-10-20T00:00:00Z"
    assert b["id"] == str(seed["binding"]["id"])
    assert b["scheduler"] == str(seed["scheduler"]["id"])
    assert b["scheduler_slug"] == seed["scheduler"]["slug"]
    assert b["dtstart"] == "2026-10-01T00:00:00Z"


def test_uninstalled_scheduler_not_listed(seed):
    # The bare definition exists but is not installed on this project.
    r = http.get(seed["keys"][KEY], base(seed))
    slugs = [row["slug"] for row in r.json()["results"]]
    assert seed["bare_scheduler"]["slug"] not in slugs


def test_nonmember_denied(seed):
    r = http.get(seed["keys"]["nonmember"], base(seed), expect=403)
    assert r.json() == FORBIDDEN_BODY


def test_outsider_denied(seed):
    r = http.get(seed["keys"]["outsider"], base(seed), expect=403)
    assert r.json() == FORBIDDEN_BODY


def test_project_identifier_routing(seed):
    # The slug-or-UUID rewrite accepts the project identifier too.
    by_uuid = http.get(seed["keys"][KEY], base(seed)).json()
    by_ident = http.get(
        seed["keys"][KEY], base(seed, seed["project"]["identifier"])
    ).json()
    assert by_ident == by_uuid


def test_unknown_project_slug_404(seed):
    r = http.get(
        seed["keys"][KEY], base(seed, f"NOPE-{seed['tag']}"), expect=404
    )
    assert r.json() == PROJECT_NOT_FOUND_BODY


def test_nonexistent_project_uuid_403(seed):
    # UUID-looking input passes the rewrite unchecked; the gate then
    # misses and denies.
    r = http.get(
        seed["keys"][KEY],
        base(seed, "00000000-0000-0000-0000-000000000000"),
        expect=403,
    )
    assert r.json() == FORBIDDEN_BODY


def test_other_workspace_project_denied(seed):
    # A project id from another workspace never passes this gate.
    r = http.get(
        seed["keys"][KEY], base(seed, seed["project_b"]["id"]), expect=403
    )
    assert r.json() == FORBIDDEN_BODY
