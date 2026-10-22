"""Contract tests: v1 scheduler writes (PDASHOSS01-278).

Ports the write half of Django's
``apps/api/pi_dash/tests/contract/api/test_schedulers.py``: the binding edit
(``PATCH .../schedulers/<id>/binding/``), run now (``POST .../run-now/``)
and the definition edit (``PATCH workspaces/<slug>/schedulers/<id>/``) —
each write, the permission denials, the 409s and the 412s.

Every test gets its own world (``world`` fixture): writes mutate the
binding and the definition, so nothing is shared with the read suites or
between tests.
"""

import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

DTSTART = "2026-10-01T00:00:00+00:00"
NEXT_RUN_AT = "2026-10-20T00:00:00+00:00"
NEXT_RUN_AT_RENDERED = "2026-10-20T00:00:00Z"
PROMPT = "Scan the project for stalled issues."

BINDING_KEYS = [
    "id", "scheduler", "scheduler_slug", "scheduler_name", "project",
    "workspace", "dtstart", "tzid", "rrule", "rdates", "exdates",
    "extra_context", "enabled", "outcome_mode", "pod", "pod_name",
    "next_run_at", "last_run", "last_run_status", "last_run_started_at",
    "last_run_ended_at", "last_error", "created_at", "updated_at",
]
SCHEDULER_KEYS = [
    "id", "workspace", "slug", "name", "description", "prompt", "color",
    "source", "is_enabled", "bindings", "created_at", "updated_at",
]
BINDING_WRITABLE = [
    "extra_context", "enabled", "dtstart", "rrule", "tzid", "rdates",
    "exdates", "outcome_mode", "pod",
]

CLASS_DENIED = {"detail": "You do not have permission to perform this action."}
ROLE_DENIED = {"error": "You don't have the required permissions."}
UNAUTHENTICATED = {"detail": "Authentication credentials were not provided."}
INVALID_TOKEN = {"detail": "Given API token is not valid"}
BINDING_NOT_FOUND = {"detail": "No SchedulerBinding matches the given query."}
SCHEDULER_NOT_FOUND = {"detail": "No Scheduler matches the given query."}
NOT_AN_OBJECT = {"error": "The request body must be a JSON object"}


# ---------------------------------------------------------------------------
# World
# ---------------------------------------------------------------------------


def _scheduler(conn, *, workspace_id, tag, source, created_by_id, is_enabled=True):
    return db.insert_row(
        conn,
        "schedulers",
        {
            "workspace_id": workspace_id,
            "slug": f"{source}-{tag}",
            "name": f"CT {source} {tag}",
            "description": "Reads the project and files issues.",
            "prompt": PROMPT,
            "source": source,
            "is_enabled": is_enabled,
            "color": "#3b82f6",
            "created_by_id": created_by_id,
        },
    )


def _binding(conn, *, workspace_id, project_id, scheduler_id, actor_id, **overrides):
    values = {
        "workspace_id": workspace_id,
        "project_id": project_id,
        "scheduler_id": scheduler_id,
        "dtstart": DTSTART,
        "tzid": "UTC",
        "rrule": "FREQ=HOURLY",
        "rdates": [],
        "exdates": [],
        "extra_context": "",
        "enabled": True,
        "outcome_mode": "create_issue",
        "next_run_at": NEXT_RUN_AT,
        "last_error": "",
        "actor_id": actor_id,
    }
    values.update(overrides)
    return db.insert_row(conn, "scheduler_bindings", values)


@pytest.fixture(scope="module")
def conn():
    connection = db.connect()
    yield connection
    connection.close()


@pytest.fixture
def world(conn):
    tag = db.new_tag()
    users = {
        role: db.create_user(conn, tag + role, first_name=f"Ct{role}")
        for role in (
            "owner", "padmin", "member", "guest", "wsmember", "wsguest",
            "outsider", "wsadmin",
        )
    }
    ws = db.create_workspace(conn, tag + "a", users["owner"]["id"])
    ws_b = db.create_workspace(conn, tag + "b", users["outsider"]["id"])
    for role, ws_role in (
        ("owner", db.ADMIN),
        ("padmin", db.MEMBER),
        ("member", db.MEMBER),
        ("guest", db.MEMBER),
        ("wsmember", db.MEMBER),
        ("wsguest", db.GUEST),
        ("wsadmin", db.ADMIN),
    ):
        db.add_workspace_member(conn, ws["id"], users[role]["id"], ws_role)
    db.add_workspace_member(conn, ws_b["id"], users["outsider"]["id"], db.ADMIN)

    # Names chosen so `affected_projects` (ordered by project name) is
    # deterministic: "A..." before "B...".
    project = db.create_project(
        conn, ws["id"], tag + "p", name=f"A project {tag}",
        identifier=f"WA{tag[-8:]}".upper(),
        created_by_id=users["owner"]["id"],
    )
    other_project = db.create_project(
        conn, ws["id"], tag + "q", name=f"B project {tag}",
        identifier=f"WB{tag[-8:]}".upper(),
        created_by_id=users["owner"]["id"],
    )
    for role, project_role in (
        ("owner", db.ADMIN),
        ("padmin", db.ADMIN),
        ("member", db.MEMBER),
        ("guest", db.GUEST),
        ("wsadmin", db.MEMBER),
    ):
        db.add_project_member(conn, ws["id"], project["id"], users[role]["id"], project_role)
    # The owner is not a member of the second project: the blast radius
    # still lists it.
    db.add_project_member(
        conn, ws["id"], other_project["id"], users["padmin"]["id"], db.ADMIN
    )

    owner_id = users["owner"]["id"]
    manifest = _scheduler(
        conn, workspace_id=ws["id"], tag=tag, source="manifest", created_by_id=owner_id
    )
    builtin = _scheduler(
        conn, workspace_id=ws["id"], tag=tag, source="builtin", created_by_id=owner_id
    )
    bare = _scheduler(
        conn, workspace_id=ws["id"], tag=tag + "x", source="manifest", created_by_id=owner_id
    )
    other_ws_scheduler = _scheduler(
        conn, workspace_id=ws_b["id"], tag=tag, source="manifest",
        created_by_id=users["outsider"]["id"],
    )
    binding = _binding(
        conn, workspace_id=ws["id"], project_id=project["id"],
        scheduler_id=manifest["id"], actor_id=owner_id,
    )
    other_binding = _binding(
        conn, workspace_id=ws["id"], project_id=other_project["id"],
        scheduler_id=manifest["id"], actor_id=owner_id,
    )
    builtin_binding = _binding(
        conn, workspace_id=ws["id"], project_id=project["id"],
        scheduler_id=builtin["id"], actor_id=owner_id,
    )
    pod = db.insert_row(
        conn,
        "pod",
        {
            "name": f"pod-{tag}",
            "description": "Seeded for contract tests",
            "is_default": True,
            "workspace_id": ws["id"],
            "project_id": project["id"],
        },
    )
    foreign_pod = db.insert_row(
        conn,
        "pod",
        {
            "name": f"pod-b-{tag}",
            "description": "Seeded for contract tests",
            "is_default": True,
            "workspace_id": ws["id"],
            "project_id": other_project["id"],
        },
    )
    keys = {
        role: db.create_api_token(conn, user["id"], tag + "k" + role)["token"]
        for role, user in users.items()
    }
    return {
        "tag": tag,
        "users": users,
        "ws": ws,
        "project": project,
        "other_project": other_project,
        "manifest": manifest,
        "builtin": builtin,
        "bare": bare,
        "other_ws_scheduler": other_ws_scheduler,
        "binding": binding,
        "other_binding": other_binding,
        "builtin_binding": builtin_binding,
        "pod": pod,
        "foreign_pod": foreign_pod,
        "keys": keys,
    }


def binding_url(w, scheduler=None, project=None):
    sid = (scheduler or w["manifest"])["id"]
    pid = (project or w["project"])["id"]
    return f"/api/v1/workspaces/{w['ws']['slug']}/projects/{pid}/schedulers/{sid}/binding/"


def run_now_url(w, scheduler=None):
    sid = (scheduler or w["manifest"])["id"]
    return (
        f"/api/v1/workspaces/{w['ws']['slug']}/projects/{w['project']['id']}"
        f"/schedulers/{sid}/run-now/"
    )


def detail_url(w, scheduler=None):
    sid = (scheduler or w["manifest"])["id"]
    return (
        f"/api/v1/workspaces/{w['ws']['slug']}/projects/{w['project']['id']}"
        f"/schedulers/{sid}/"
    )


def definition_url(w, scheduler_id=None):
    sid = scheduler_id or w["manifest"]["id"]
    return f"/api/v1/workspaces/{w['ws']['slug']}/schedulers/{sid}/"


def send(method, key, path, *, json=None, headers=None, content=None):
    with http.client(key) as c:
        return c.request(method, path, json=json, headers=headers, content=content)


def binding_row(conn, binding_id):
    return db.fetch_one(
        conn, "SELECT * FROM scheduler_bindings WHERE id = %s", (binding_id,)
    )


def scheduler_row(conn, scheduler_id):
    return db.fetch_one(conn, "SELECT * FROM schedulers WHERE id = %s", (scheduler_id,))


# ---------------------------------------------------------------------------
# PATCH .../binding/
# ---------------------------------------------------------------------------


def test_admin_edits_extra_context(world, conn):
    text = "## Rules\n\nRule D: queued runs are not stuck.\n"
    r = send("PATCH", world["keys"]["owner"], binding_url(world), json={"extra_context": text})
    assert r.status_code == 200, r.text
    body = r.json()
    assert list(body.keys()) == BINDING_KEYS + ["previous"]
    # DRF `CharField` trims surrounding whitespace, as on the web app.
    assert body["extra_context"] == text.strip()
    assert body["previous"] == {"extra_context": ""}
    # The read surface shows it, and the other project's install is untouched.
    read = http.get(world["keys"]["owner"], detail_url(world)).json()
    assert read["bindings"][0]["extra_context"] == text.strip()
    assert binding_row(conn, world["other_binding"]["id"])["extra_context"] == ""


def test_enable_toggle_records_previous(world):
    key = world["keys"]["owner"]
    off = send("PATCH", key, binding_url(world), json={"enabled": False})
    assert off.status_code == 200
    assert off.json()["enabled"] is False
    assert off.json()["previous"] == {"enabled": True}
    on = send("PATCH", key, binding_url(world), json={"enabled": True})
    assert on.json()["enabled"] is True
    assert on.json()["previous"] == {"enabled": False}


def test_cadence_edit_recomputes_next_run_at(world):
    r = send(
        "PATCH", world["keys"]["owner"], binding_url(world),
        json={"rrule": "FREQ=HOURLY;INTERVAL=3"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["rrule"] == "FREQ=HOURLY;INTERVAL=3"
    assert body["previous"] == {
        "rrule": "FREQ=HOURLY",
        "next_run_at": NEXT_RUN_AT_RENDERED,
    }
    assert body["next_run_at"] != NEXT_RUN_AT_RENDERED
    nxt = datetime.fromisoformat(body["next_run_at"].replace("Z", "+00:00"))
    assert nxt > datetime.now(timezone.utc)
    # Every third hour from the 00:00 anchor.
    assert nxt.hour % 3 == 0 and nxt.minute == 0


def test_pod_and_outcome_mode(world):
    r = send(
        "PATCH", world["keys"]["owner"], binding_url(world),
        json={"pod": str(world["pod"]["id"]), "outcome_mode": "apply_fix"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["pod"] == str(world["pod"]["id"])
    assert body["pod_name"] == world["pod"]["name"]
    assert body["outcome_mode"] == "apply_fix"
    assert body["previous"] == {"pod": None, "outcome_mode": "create_issue"}


def test_invalid_values_are_field_errors(world, conn):
    key = world["keys"]["owner"]
    bad_rule = send("PATCH", key, binding_url(world), json={"rrule": "FREQ=NEVER"})
    assert bad_rule.status_code == 400
    assert list(bad_rule.json().keys()) == ["rrule"]
    foreign = send("PATCH", key, binding_url(world), json={"pod": str(world["foreign_pod"]["id"])})
    assert foreign.status_code == 400
    assert list(foreign.json().keys()) == ["pod"]
    bad_mode = send("PATCH", key, binding_url(world), json={"outcome_mode": "nope"})
    assert bad_mode.status_code == 400
    assert bad_mode.json() == {"outcome_mode": ['"nope" is not a valid choice.']}
    row = binding_row(conn, world["binding"]["id"])
    assert row["rrule"] == "FREQ=HOURLY" and row["pod_id"] is None


def test_unwritable_field_is_rejected(world, conn):
    r = send(
        "PATCH", world["keys"]["owner"], binding_url(world),
        json={"prompt": "x", "extra_context": "y", "next_run_at": None},
    )
    assert r.status_code == 400
    assert r.json() == {
        "error": "Not writable on a scheduler binding: next_run_at, prompt",
        "writable_fields": BINDING_WRITABLE,
    }
    assert binding_row(conn, world["binding"]["id"])["extra_context"] == ""


def test_empty_and_non_object_bodies(world):
    key = world["keys"]["owner"]
    empty = send("PATCH", key, binding_url(world), json={})
    assert empty.status_code == 400
    assert empty.json() == {"error": "No fields to update", "writable_fields": BINDING_WRITABLE}
    only_token = send(
        "PATCH", key, binding_url(world), json={"expected_updated_at": "*"}
    )
    assert only_token.json()["error"] == "No fields to update"
    listed = send("PATCH", key, binding_url(world), json=["extra_context"])
    assert listed.status_code == 400
    assert listed.json() == NOT_AN_OBJECT


def test_binding_404_when_not_installed(world):
    r = send(
        "PATCH", world["keys"]["owner"], binding_url(world, scheduler=world["bare"]),
        json={"enabled": False},
    )
    assert r.status_code == 404
    assert r.json() == BINDING_NOT_FOUND


@pytest.mark.parametrize(
    ("role", "status", "body"),
    [
        ("member", 403, ROLE_DENIED),
        ("guest", 403, CLASS_DENIED),
        ("wsmember", 403, CLASS_DENIED),
        ("outsider", 403, CLASS_DENIED),
    ],
)
def test_binding_write_denials(world, conn, role, status, body):
    key = world["keys"][role]
    for method, path, payload in (
        ("PATCH", binding_url(world), {"extra_context": "nope"}),
        ("POST", run_now_url(world), None),
    ):
        r = send(method, key, path, json=payload)
        assert (r.status_code, r.json()) == (status, body), (method, r.text)
    row = binding_row(conn, world["binding"]["id"])
    assert row["extra_context"] == ""
    assert row["next_run_at"].isoformat() == NEXT_RUN_AT


def test_binding_write_auth_failures(world):
    for method, path in (("PATCH", binding_url(world)), ("POST", run_now_url(world))):
        anon = send(method, None, path, json={"enabled": False})
        assert (anon.status_code, anon.json()) == (401, UNAUTHENTICATED)
        bad = send(method, "plane_api_not-a-real-key", path, json={"enabled": False})
        assert (bad.status_code, bad.json()) == (403, INVALID_TOKEN)


def test_workspace_admin_in_project_passes(world):
    # Project role Member, workspace Admin: the decorator's bypass.
    r = send("PATCH", world["keys"]["wsadmin"], binding_url(world), json={"extra_context": "ok"})
    assert r.status_code == 200, r.text


def test_binding_precondition(world, conn):
    key = world["keys"]["owner"]
    token = http.get(key, detail_url(world)).json()["bindings"][0]["updated_at"]
    first = send(
        "PATCH", key, binding_url(world),
        json={"extra_context": "first", "expected_updated_at": token},
    )
    assert first.status_code == 200, first.text
    stale = send(
        "PATCH", key, binding_url(world),
        json={"extra_context": "second", "expected_updated_at": token},
    )
    assert stale.status_code == 412
    assert list(stale.json().keys()) == ["error", "updated_at"]
    assert stale.json()["error"] == "The scheduler binding changed since it was read"
    header = send(
        "PATCH", key, binding_url(world), json={"extra_context": "third"},
        headers={"If-Match": token},
    )
    assert header.status_code == 412
    assert binding_row(conn, world["binding"]["id"])["extra_context"] == "first"
    current = send(
        "PATCH", key, binding_url(world), json={"extra_context": "fourth"},
        headers={"If-Match": f'"{stale.json()["updated_at"]}"'},
    )
    assert current.status_code == 200, current.text
    for garbage in ("yesterday", "2026-10-01T00:00:00", "2026-13-01T00:00:00+00:00"):
        r = send(
            "PATCH", key, binding_url(world), json={"extra_context": "x"},
            headers={"If-Match": garbage},
        )
        assert r.status_code == 400, garbage
        assert r.json() == {
            "error": "If-Match / expected_updated_at must be the scheduler binding's "
            "updated_at, as an ISO 8601 datetime with an offset"
        }
    assert binding_row(conn, world["binding"]["id"])["extra_context"] == "fourth"


def test_binding_route_other_methods_405(world):
    for method in ("GET", "PUT", "POST", "DELETE"):
        r = send(method, world["keys"]["owner"], binding_url(world), json={})
        assert r.status_code == 405, method


# ---------------------------------------------------------------------------
# POST .../run-now/
# ---------------------------------------------------------------------------


def test_run_now_marks_due(world, conn):
    before = datetime.now(timezone.utc) - timedelta(seconds=5)
    r = send("POST", world["keys"]["owner"], run_now_url(world))
    assert r.status_code == 200, r.text
    body = r.json()
    assert list(body.keys()) == BINDING_KEYS + ["previous"]
    assert body["previous"] == {"next_run_at": NEXT_RUN_AT_RENDERED}
    due = datetime.fromisoformat(body["next_run_at"].replace("Z", "+00:00"))
    assert before <= due <= datetime.now(timezone.utc) + timedelta(seconds=5)
    row = binding_row(conn, world["binding"]["id"])
    assert row["next_run_at"] == due
    assert row["enabled"] is True


def test_run_now_with_json_body_and_star(world):
    r = send(
        "POST", world["keys"]["owner"], run_now_url(world),
        json={"expected_updated_at": "*"},
    )
    assert r.status_code == 200, r.text


def _assert_conflict(r, code, message):
    assert r.status_code == 409, r.text
    assert r.json() == {"error": message, "code": code}
    assert list(r.json().keys()) == ["error", "code"]


def test_run_now_conflicts(world, conn):
    key = world["keys"]["owner"]
    db.execute(
        db.get_database_url(),
        "UPDATE scheduler_bindings SET enabled = false WHERE id = %s",
        (world["binding"]["id"],),
    )
    _assert_conflict(
        send("POST", key, run_now_url(world)),
        "binding_disabled",
        "The scheduler is disabled on this project.",
    )
    db.execute(
        db.get_database_url(),
        "UPDATE scheduler_bindings SET enabled = true WHERE id = %s",
        (world["binding"]["id"],),
    )
    db.execute(
        db.get_database_url(),
        "UPDATE schedulers SET is_enabled = false WHERE id = %s",
        (world["manifest"]["id"],),
    )
    _assert_conflict(
        send("POST", key, run_now_url(world)),
        "scheduler_disabled",
        "The scheduler definition is disabled.",
    )
    db.execute(
        db.get_database_url(),
        "UPDATE schedulers SET is_enabled = true WHERE id = %s",
        (world["manifest"]["id"],),
    )
    run = db.insert_row(
        conn,
        "agent_run",
        {
            "status": "queued",
            "prompt": "Seeded run",
            "run_config": {},
            "required_capabilities": {},
            "thread_id": "seed-thread",
            "error": "",
            "workspace_id": world["ws"]["id"],
            "pod_id": world["pod"]["id"],
            "created_by_id": world["users"]["owner"]["id"],
            "llm_model": "seed-model",
            "refusal_category": "",
            "trigger": "scheduler",
            "executor_kind": "local_runner",
            "dispatch_attempts": 0,
            "cancel_reason": "",
            "error_code": "",
            "tool_plan": {},
            "phase_kind": "",
            "agent_metadata": {},
            "usage": {"input": 1, "output": 1, "total": 2},
            "scheduler_binding_id": world["binding"]["id"],
        },
    )
    _assert_conflict(
        send("POST", key, run_now_url(world)),
        "run_active",
        "A run from this scheduler is already active.",
    )
    db.execute(
        db.get_database_url(),
        "UPDATE agent_run SET status = 'completed' WHERE id = %s",
        (run["id"],),
    )
    assert send("POST", key, run_now_url(world)).status_code == 200
    row = binding_row(conn, world["binding"]["id"])
    assert row["enabled"] is True


def test_run_now_spent_schedule(world, conn):
    db.execute(
        db.get_database_url(),
        "UPDATE scheduler_bindings SET rrule = 'FREQ=DAILY;COUNT=1' WHERE id = %s",
        (world["binding"]["id"],),
    )
    _assert_conflict(
        send("POST", world["keys"]["owner"], run_now_url(world)),
        "no_future_occurrence",
        "The schedule has no future occurrence; edit the cadence first.",
    )
    row = binding_row(conn, world["binding"]["id"])
    assert row["enabled"] is True
    assert row["next_run_at"].isoformat() == NEXT_RUN_AT


def test_run_now_404_and_precondition(world):
    key = world["keys"]["owner"]
    missing = send("POST", key, run_now_url(world, scheduler=world["bare"]))
    assert (missing.status_code, missing.json()) == (404, BINDING_NOT_FOUND)
    stale = send(
        "POST", key, run_now_url(world), headers={"If-Match": "2020-01-01T00:00:00Z"}
    )
    assert stale.status_code == 412
    assert stale.json()["error"] == "The scheduler binding changed since it was read"


def test_run_now_route_other_methods_405(world):
    for method in ("GET", "PUT", "PATCH", "DELETE"):
        r = send(method, world["keys"]["owner"], run_now_url(world), json={})
        assert r.status_code == 405, method


# ---------------------------------------------------------------------------
# PATCH workspaces/<slug>/schedulers/<id>/
# ---------------------------------------------------------------------------


def test_workspace_admin_replaces_prompt(world, conn):
    new_prompt = "Line one.\n\nLine two, with \"quotes\" and `code`."
    r = send(
        "PATCH", world["keys"]["owner"], definition_url(world),
        json={"prompt": new_prompt},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert list(body.keys()) == SCHEDULER_KEYS + ["affected_projects", "previous"]
    assert body["prompt"] == new_prompt
    assert body["source"] == "manifest"
    assert body["previous"] == {"prompt": PROMPT}
    assert body["affected_projects"] == [
        {
            "id": str(world["project"]["id"]),
            "identifier": world["project"]["identifier"],
            "name": world["project"]["name"],
            "binding_enabled": True,
        },
        {
            "id": str(world["other_project"]["id"]),
            "identifier": world["other_project"]["identifier"],
            "name": world["other_project"]["name"],
            "binding_enabled": True,
        },
    ]
    assert [b["project"] for b in body["bindings"]] == [
        str(world["project"]["id"]),
        str(world["other_project"]["id"]),
    ]
    assert list(body["bindings"][0].keys()) == BINDING_KEYS
    read = http.get(world["keys"]["owner"], detail_url(world)).json()
    assert read["prompt"] == new_prompt
    assert scheduler_row(conn, world["manifest"]["id"])["prompt"] == new_prompt


def test_name_and_description(world):
    r = send(
        "PATCH", world["keys"]["owner"], definition_url(world),
        json={"description": "", "name": "  Renamed  "},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["name"] == "Renamed"
    assert body["description"] == ""
    assert body["previous"] == {
        "description": "Reads the project and files issues.",
        "name": world["manifest"]["name"],
    }
    assert body["bindings"][0]["scheduler_name"] == "Renamed"


def test_builtin_definition_is_rejected(world, conn):
    r = send(
        "PATCH", world["keys"]["owner"], definition_url(world, world["builtin"]["id"]),
        json={"prompt": "Do something else."},
    )
    _assert_conflict(
        r,
        "builtin_scheduler",
        "Built-in schedulers cannot be edited; put per-project instructions "
        "in the binding's extra_context instead.",
    )
    assert scheduler_row(conn, world["builtin"]["id"])["prompt"] == PROMPT


def test_definition_validation(world, conn):
    key = world["keys"]["owner"]
    unwritable = send(
        "PATCH", key, definition_url(world), json={"prompt": "x", "source": "manifest"}
    )
    assert unwritable.status_code == 400
    assert unwritable.json() == {
        "error": "Not writable on a scheduler definition: source",
        "writable_fields": ["prompt", "name", "description"],
    }
    blank = send("PATCH", key, definition_url(world), json={"prompt": "   "})
    assert blank.status_code == 400
    assert blank.json() == {"prompt": ["This field may not be blank."]}
    listed = send("PATCH", key, definition_url(world), json=[1])
    assert (listed.status_code, listed.json()) == (400, NOT_AN_OBJECT)
    assert scheduler_row(conn, world["manifest"]["id"])["prompt"] == PROMPT


@pytest.mark.parametrize(
    ("role", "body"),
    [
        ("padmin", ROLE_DENIED),
        ("member", ROLE_DENIED),
        ("wsmember", ROLE_DENIED),
        ("wsguest", CLASS_DENIED),
        ("outsider", CLASS_DENIED),
    ],
)
def test_definition_denials(world, conn, role, body):
    r = send(
        "PATCH", world["keys"][role], definition_url(world), json={"prompt": "nope"}
    )
    assert (r.status_code, r.json()) == (403, body)
    assert scheduler_row(conn, world["manifest"]["id"])["prompt"] == PROMPT


def test_definition_auth_failures(world):
    anon = send("PATCH", None, definition_url(world), json={"prompt": "x"})
    assert (anon.status_code, anon.json()) == (401, UNAUTHENTICATED)


def test_project_admin_keeps_binding_write(world):
    # Workspace member + project admin: the definition is refused (above),
    # the binding edit goes through.
    r = send("PATCH", world["keys"]["padmin"], binding_url(world), json={"extra_context": "x"})
    assert r.status_code == 200, r.text


def test_definition_404s(world):
    key = world["keys"]["owner"]
    for sid in (
        "00000000-0000-0000-0000-000000000000",
        world["other_ws_scheduler"]["id"],
    ):
        r = send("PATCH", key, definition_url(world, sid), json={"prompt": "x"})
        assert (r.status_code, r.json()) == (404, SCHEDULER_NOT_FOUND)


def test_definition_precondition(world, conn):
    key = world["keys"]["owner"]
    stale = send(
        "PATCH", key, definition_url(world), json={"prompt": "x"},
        headers={"If-Match": "2020-01-01T00:00:00+00:00"},
    )
    assert stale.status_code == 412
    body = stale.json()
    assert body["error"] == "The scheduler changed since it was read"
    ok = send(
        "PATCH", key, definition_url(world),
        json={"prompt": "fresh", "expected_updated_at": body["updated_at"]},
    )
    assert ok.status_code == 200, ok.text
    assert scheduler_row(conn, world["manifest"]["id"])["prompt"] == "fresh"


def test_definition_route_other_methods_405(world):
    for method in ("GET", "PUT", "POST", "DELETE"):
        r = send(method, world["keys"]["owner"], definition_url(world), json={})
        assert r.status_code == 405, method
