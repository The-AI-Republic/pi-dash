"""Binding run-history contract (port of Django test_binding_runs.py, 474123e5).

Covers the project-scoped binding-runs endpoint (isolation, ordering,
pagination, permissions, private-runner gate) and the binding *detail*
shape's ``resolved_prompt`` / ``run_count`` / ``scheduler_source`` /
``scheduler_is_enabled`` additions.

Scope note (PIDASHCONV-1059 vs PIDASHCONV-1060): the run *rows* rendered
by this endpoint grow two keys (``scheduler_binding`` /
``scheduler_binding_detail``) in part 2, so these cases assert row
identity and the envelope but never the full row key set — the row
shape, the run-detail visibility grant, and the ``?scheduler_binding=``
filter belong to 1060's suite.
"""

from _harness import seed, sessions
from _harness.seed import Seed

from . import seed_scheduler as seed_s
from .test_project_bindings import BINDING_KEYS, _install, _scheduler, bind_list
from .test_project_binding_detail import bind_detail

DENIED = {"error": "You don't have the required permissions."}
NOT_FOUND_BINDING = {"detail": "No SchedulerBinding matches the given query."}

RUNS_ENVELOPE_KEYS = {
    "results", "count", "total_count", "total_pages", "page", "per_page",
}


def bind_runs(world, bid):
    return f"{bind_list(world)}{bid}/runs/"


def _installed(org, **extra):
    sched = _scheduler(org)
    r = _install(org, sched["id"], **extra)
    assert r.status_code == 201, r.text
    return sched, r.json()


def _pod(org):
    return seed_s.create_pod(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"],
    )


def _run(org, binding_id, pod_id, user_id, **extra):
    return seed_s.create_agent_run(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"], pod_id=pod_id,
        binding_id=binding_id, created_by_id=user_id,
        started_at=seed_s.hours_ago(1), **extra,
    )


def test_runs_returns_only_this_bindings_runs_newest_first(org):
    _, binding = _installed(org)
    other = _install(org, _scheduler(org, slug="other")["id"])
    assert other.status_code == 201, other.text
    pod = _pod(org)
    admin = org.admin["id"]
    first = _run(org, binding["id"], pod["id"], admin)
    second = _run(org, binding["id"], pod["id"], admin)
    _run(org, other.json()["id"], pod["id"], admin)  # must not appear

    r = org.request("GET", bind_runs(org, binding["id"]), "admin")
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == RUNS_ENVELOPE_KEYS
    assert [row["id"] for row in body["results"]] == [
        second["id"], first["id"],
    ]
    assert body["count"] == 2
    assert body["total_count"] == 2
    assert body["total_pages"] == 1
    assert body["page"] == 1


def test_runs_pagination_envelope(org):
    _, binding = _installed(org)
    pod = _pod(org)
    for _ in range(3):
        _run(org, binding["id"], pod["id"], org.admin["id"])

    r = org.request(
        "GET", bind_runs(org, binding["id"]) + "?page=2&per_page=2", "admin",
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["page"] == 2
    assert body["per_page"] == 2
    assert body["total_count"] == 3
    assert body["total_pages"] == 2
    assert body["count"] == 1
    assert len(body["results"]) == 1


def test_runs_row_carries_stable_run_fields(org):
    # Identity + the long-stable row keys. The full key set is 1060's
    # (it adds scheduler_binding / scheduler_binding_detail there).
    _, binding = _installed(org)
    pod = _pod(org)
    run = _run(org, binding["id"], pod["id"], org.admin["id"])

    r = org.request("GET", bind_runs(org, binding["id"]), "admin")
    assert r.status_code == 200, r.text
    (row,) = r.json()["results"]
    assert row["id"] == run["id"]
    assert row["status"] == "completed"
    assert row["prompt"] == "Seeded run"
    assert row["pod"] == pod["id"]
    assert row["pod_detail"]["id"] == pod["id"]
    assert row["created_by"] == org.admin["id"]
    assert row["error_diagnostic"] is None


def test_runs_readable_by_all_project_roles(org):
    _, binding = _installed(org)
    _run(org, binding["id"], _pod(org)["id"], org.admin["id"])

    for role in ("admin", "member", "guest"):
        r = org.request("GET", bind_runs(org, binding["id"]), role)
        assert r.status_code == 200, role
        assert r.json()["total_count"] == 1, role


def test_runs_forbidden_for_workspace_member_outside_project(org):
    _, binding = _installed(org)
    # Workspace member with no project membership (mirrors the Django
    # case's outsider): the PROJECT gate 403s before the lookup.
    user = seed.create_user(
        org.conn, email=f"wsonly-{org.tag}@ct.example.com",
        username=f"wsonly-{org.tag}",
        password_field=sessions.make_password_hash(f"pw-{org.tag}"),
    )
    seed.add_workspace_member(
        org.conn, workspace_id=org.workspace["id"], user_id=user["id"],
        role=seed.MEMBER,
    )
    org.cookies["wsonly"] = sessions.login(
        org.conn, user["id"], user["password_field"]
    )

    r = org.request("GET", bind_runs(org, binding["id"]), "wsonly")
    assert r.status_code == 403
    assert r.json() == DENIED


def test_runs_forbidden_for_outsider(org):
    _, binding = _installed(org)
    r = org.request("GET", bind_runs(org, binding["id"]), "outsider")
    assert r.status_code == 403
    assert r.json() == DENIED


def test_runs_404_when_binding_belongs_to_another_project(org):
    _, binding = _installed(org)
    second = seed.create_project(
        org.conn, workspace_id=org.workspace["id"],
        identifier=f"D{org.tag}".upper(), name="Second",
        created_by_id=org.admin["id"],
    )
    seed.add_project_member(
        org.conn, project_id=second["id"],
        workspace_id=org.workspace["id"], user_id=org.admin["id"],
        role=seed.ADMIN,
    )
    path = (
        f"/api/workspaces/{org.workspace['slug']}"
        f"/projects/{second['id']}/scheduler-bindings/{binding['id']}/runs/"
    )
    r = org.request("GET", path, "admin")
    assert r.status_code == 404
    assert r.json() == NOT_FOUND_BINDING


def test_runs_unknown_binding_id_404s(org):
    r = org.request(
        "GET", bind_runs(org, "00000000-0000-0000-0000-000000000000"),
        "admin",
    )
    assert r.status_code == 404
    assert r.json() == NOT_FOUND_BINDING


def test_runs_hides_private_runner_runs_of_other_users(org):
    """Project standing must not reveal a run executing on someone else's
    private machine — mirrors the global runs list's gate."""
    _, binding = _installed(org)
    pod = _pod(org)
    helper = Seed(org.conn)
    machine = helper.dev_machine(org.member["id"], host_label="theirs.local")
    runner = helper.runner(
        org.member["id"], org.workspace["id"], pod["id"],
        name="their-private-runner", dev_machine_id=machine,
    )
    _run(
        org, binding["id"], pod["id"], org.member["id"],
        runner_id=runner,
    )
    visible = _run(org, binding["id"], pod["id"], org.member["id"])

    r = org.request("GET", bind_runs(org, binding["id"]), "admin")
    assert r.status_code == 200, r.text
    assert [row["id"] for row in r.json()["results"]] == [visible["id"]]

    # The runner's owner sees both.
    r = org.request("GET", bind_runs(org, binding["id"]), "member")
    assert r.status_code == 200, r.text
    assert r.json()["total_count"] == 2


def test_runs_forbidden_for_admin_of_other_workspace(org, pg):
    from .conftest import make_world
    other = make_world(pg)
    try:
        sched = seed_s.create_scheduler(
            other.conn, workspace_id=other.workspace["id"], slug="o",
            name="O",
        )
        binding = seed_s.create_binding(
            other.conn, workspace_id=other.workspace["id"],
            project_id=other.project["id"], scheduler_id=sched["id"],
            dtstart=seed_s.hours_ago(1),
        )
        path = (
            f"/api/workspaces/{other.workspace['slug']}"
            f"/projects/{other.project['id']}"
            f"/scheduler-bindings/{binding['id']}/runs/"
        )
        r = org.request("GET", path, "admin")
        assert r.status_code == 403
        assert r.json() == DENIED
    finally:
        other.close()


def test_detail_exposes_resolved_prompt_run_count_and_scheduler_join(org):
    sched, binding = _installed(org)
    r = org.request(
        "PATCH", bind_detail(org, binding["id"]), "admin",
        json={"extra_context": "Focus on the API."},
    )
    assert r.status_code == 200, r.text
    pod = _pod(org)
    _run(org, binding["id"], pod["id"], org.admin["id"])

    r = org.request("GET", bind_detail(org, binding["id"]), "admin")
    assert r.status_code == 200, r.text
    body = r.json()
    # Composed exactly like a dispatched run: scheduler prompt, then extra
    # context; nothing appended (PDASHOSS01-281).
    assert body["resolved_prompt"] == "x\n\nFocus on the API."
    assert body["run_count"] == 1
    assert body["scheduler_source"] == "builtin"
    assert body["scheduler_is_enabled"] is True
    assert body["scheduler"] == sched["id"]


def test_patch_response_carries_detail_keys(org):
    _, binding = _installed(org)
    r = org.request(
        "PATCH", bind_detail(org, binding["id"]), "admin",
        json={"enabled": False},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["enabled"] is False
    assert set(body.keys()) == BINDING_KEYS | {
        "resolved_prompt", "run_count", "scheduler_source",
        "scheduler_is_enabled",
    }
    # PATCH renders the post-save row through the detail shape: the
    # prompt composes from the install defaults (prompt "x", empty
    # extra context) with no work-mode section.
    assert body["resolved_prompt"] == "x"
