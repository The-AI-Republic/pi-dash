"""Tenant-isolation cases for the loop domain.

Preferences are per-user rows: one user's opt-outs never leak into another
user's settings. Targets are per-workspace cursors: the ``workspace`` filter
scopes to exactly one workspace. Jobs themselves are instance-global on
purpose — every user sees the same catalog.
"""

from . import seed_loop as seed_l
from .test_admin_targets import targets as targets_url
from .test_user_job_toggle import job_url
from .test_user_settings import SETTINGS


def test_prefs_are_per_user(org):
    seed_l.create_loop_job(org.conn, slug=f"pu-{org.tag}", public_name="PU")
    assert org.app(
        "PATCH", job_url(f"pu-{org.tag}"), "member", json={"enabled": False}
    ).status_code == 200
    for role, expected in (("member", False), ("admin", True), ("guest", True)):
        body = org.app("GET", SETTINGS, role).json()
        job = next(j for j in body["jobs"] if j["slug"] == f"pu-{org.tag}")
        assert job["enabled"] is expected, role


def test_master_pause_is_per_user(org):
    seed_l.create_loop_job(org.conn, slug=f"mp-{org.tag}", public_name="MP")
    assert org.app("PATCH", SETTINGS, "member", json={"enabled": False}).status_code == 200
    assert org.app("GET", SETTINGS, "member").json()["enabled"] is False
    assert org.app("GET", SETTINGS, "admin").json()["enabled"] is True


def test_outsider_toggle_only_affects_self(org):
    seed_l.create_loop_job(org.conn, slug=f"ot-{org.tag}", public_name="OT")
    assert org.app(
        "PATCH", job_url(f"ot-{org.tag}"), "outsider", json={"enabled": False}
    ).status_code == 200
    rows = org.conn.execute(
        """SELECT user_id FROM loop_user_preferences
        WHERE job_id=(SELECT id FROM loop_jobs WHERE slug=%s) AND deleted_at IS NULL""",
        (f"ot-{org.tag}",),
    ).fetchall()
    assert [str(r[0]) for r in rows] == [org.outsider["id"]]
    job = next(
        j
        for j in org.app("GET", SETTINGS, "member").json()["jobs"]
        if j["slug"] == f"ot-{org.tag}"
    )
    assert job["enabled"] is True


def test_targets_scoped_to_workspace_filter(org):
    job = seed_l.create_loop_job(org.conn, slug=f"ws-{org.tag}", public_name="WS")
    seed_l.configure_llm(org.conn, user_id=org.member["id"])
    seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=org.member["id"], next_run_at=seed_l.hours_ago(1),
    )
    seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.other_ws["id"],
        user_id=org.other_user["id"], next_run_at=seed_l.hours_ago(1),
    )
    r = org.adm(
        "GET", targets_url(job["id"]), "admin",
        params={"workspace": org.workspace["slug"]},
    )
    assert [t["workspace_slug"] for t in r.json()["results"]] == [org.workspace["slug"]]
    r = org.adm(
        "GET", targets_url(job["id"]), "admin",
        params={"workspace": org.other_ws["slug"]},
    )
    assert [t["workspace_slug"] for t in r.json()["results"]] == [org.other_ws["slug"]]


def test_job_catalog_is_instance_global(org):
    # No workspace scoping on the catalog: a user from another workspace sees
    # the same enabled jobs.
    seed_l.create_loop_job(org.conn, slug=f"ig-{org.tag}", public_name="IG")
    mine = {j["slug"] for j in org.app("GET", SETTINGS, "member").json()["jobs"]}
    theirs = {j["slug"] for j in org.app("GET", SETTINGS, "other").json()["jobs"]}
    assert f"ig-{org.tag}" in mine
    assert mine == theirs
