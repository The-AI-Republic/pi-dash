"""Permission contract for the scheduler endpoints.

Unauthenticated callers get 401; authenticated callers without the role get
403. The member/item cases below double as the deliberate-removal detector:
if an ``allow_permission`` gate is widened or dropped, the 403 assertions
start failing.
"""

import pytest

from _harness import seed
from . import seed_scheduler as seed_s
from .test_project_bindings import bind_list
from .test_workspace_schedulers import sched_list

DENIED = {"error": "You don't have the required permissions."}
ANON = {"detail": "Authentication credentials were not provided."}


@pytest.fixture()
def seeded(org):
    sched = org.request(
        "POST", f"/api/workspaces/{org.workspace['slug']}/schedulers/",
        "admin", json={"slug": "perm", "name": "Perm", "prompt": "x"},
    ).json()
    binding = seed_s.create_binding(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"], scheduler_id=sched["id"],
        dtstart=seed_s.hours_ago(1),
    )
    slug = org.workspace["slug"]
    pid = org.project["id"]
    return {
        "GET /schedulers/": ("GET", f"/api/workspaces/{slug}/schedulers/"),
        "POST /schedulers/": ("POST", f"/api/workspaces/{slug}/schedulers/"),
        "GET /schedulers/<id>/": (
            "GET", f"/api/workspaces/{slug}/schedulers/{sched['id']}/"),
        "PATCH /schedulers/<id>/": (
            "PATCH", f"/api/workspaces/{slug}/schedulers/{sched['id']}/"),
        "DELETE /schedulers/<id>/": (
            "DELETE", f"/api/workspaces/{slug}/schedulers/{sched['id']}/"),
        "GET /bindings/": (
            "GET",
            f"/api/workspaces/{slug}/projects/{pid}/scheduler-bindings/"),
        "POST /bindings/": (
            "POST",
            f"/api/workspaces/{slug}/projects/{pid}/scheduler-bindings/"),
        "GET /bindings/<id>/": (
            "GET",
            f"/api/workspaces/{slug}/projects/{pid}/scheduler-bindings/{binding['id']}/"),
        "PATCH /bindings/<id>/": (
            "PATCH",
            f"/api/workspaces/{slug}/projects/{pid}/scheduler-bindings/{binding['id']}/"),
        "DELETE /bindings/<id>/": (
            "DELETE",
            f"/api/workspaces/{slug}/projects/{pid}/scheduler-bindings/{binding['id']}/"),
        "GET /occurrences/": (
            "GET",
            f"/api/workspaces/{slug}/projects/{pid}/scheduler-bindings/occurrences/"),
    }


def test_unauthenticated_is_401_everywhere(org, seeded):
    for name, (method, path) in seeded.items():
        r = org.request(method, path, None, json={})
        assert r.status_code == 401, name
        assert r.json() == ANON, name


def test_create_scheduler_forbidden_for_member(org):
    # REMOVAL DETECTOR: the list endpoint allows ADMIN on POST. Widening
    # allowed_roles (or dropping the gate) turns this 403 into a 201.
    r = org.request(
        "POST", sched_list(org), "member",
        json={"slug": "m", "name": "M", "prompt": "x"},
    )
    assert r.status_code == 403
    assert r.json() == DENIED


def test_scheduler_detail_admin_only(org, seeded):
    for name in ("GET /schedulers/<id>/", "PATCH /schedulers/<id>/",
                 "DELETE /schedulers/<id>/"):
        method, path = seeded[name]
        for role in ("member", "guest"):
            r = org.request(method, path, role, json={"name": "x"})
            assert r.status_code == 403, (name, role)
            assert r.json() == DENIED, (name, role)


def test_binding_writes_admin_only(org, seeded):
    for name in ("POST /bindings/", "PATCH /bindings/<id>/",
                 "DELETE /bindings/<id>/"):
        method, path = seeded[name]
        for role in ("member", "guest"):
            payload = {"enabled": False} if method != "POST" else {
                "scheduler": "00000000-0000-0000-0000-000000000000",
                "project": org.project["id"],
                "dtstart": seed_s.hours_ago(1),
            }
            r = org.request(method, path, role, json=payload)
            assert r.status_code == 403, (name, role)
            assert r.json() == DENIED, (name, role)


def test_reads_open_to_all_project_roles(org, seeded):
    for name in ("GET /schedulers/", "GET /bindings/",
                 "GET /bindings/<id>/", "GET /occurrences/"):
        method, path = seeded[name]
        for role in ("admin", "member", "guest"):
            assert org.request(method, path, role).status_code == 200, (name, role)


def test_inactive_membership_denied(org, seeded):
    org.conn.execute(
        "UPDATE workspace_members SET is_active=false WHERE member_id=%s",
        (org.member["id"],),
    )
    org.conn.execute(
        "UPDATE project_members SET is_active=false WHERE member_id=%s",
        (org.member["id"],),
    )
    method, path = seeded["GET /schedulers/"]
    r = org.request(method, path, "member")
    assert r.status_code == 403
    assert r.json() == DENIED


def test_workspace_admin_without_project_membership_cannot_install(org, pg):
    # The PROJECT-level fallback still needs *some* active project
    # membership: workspace admin alone is not enough.
    ws_admin = seed.create_user(
        org.conn, email=f"wsadmin-{org.tag}@ct.example.com",
        username=f"wsadmin-{org.tag}",
        password_field=org.admin["password_field"],
    )
    seed.add_workspace_member(
        org.conn, workspace_id=org.workspace["id"], user_id=ws_admin["id"],
        role=seed.ADMIN,
    )
    from _harness import sessions
    org.cookies["wsadmin"] = sessions.login(
        org.conn, ws_admin["id"], ws_admin["password_field"]
    )
    sched = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": "w", "name": "W", "prompt": "x"},
    ).json()
    r = org.request(
        "POST", bind_list(org), "wsadmin",
        json={"scheduler": sched["id"], "project": org.project["id"],
              "dtstart": seed_s.hours_ago(1)},
    )
    assert r.status_code == 403
    assert r.json() == DENIED
