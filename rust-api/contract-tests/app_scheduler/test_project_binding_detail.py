"""Project binding detail get/patch/delete contract."""

from . import seed_scheduler as seed_s
from .test_project_bindings import BINDING_KEYS, _install, _scheduler, bind_list
from .test_workspace_schedulers import sched_list


def bind_detail(world, bid):
    return f"{bind_list(world)}{bid}/"


def _installed(org):
    sched = _scheduler(org)
    r = _install(org, sched["id"])
    assert r.status_code == 201, r.text
    return sched, r.json()


def test_get_shape(org):
    _, binding = _installed(org)
    r = org.request("GET", bind_detail(org, binding["id"]), "admin")
    assert r.status_code == 200
    assert set(r.json().keys()) == BINDING_KEYS
    assert r.json()["id"] == binding["id"]


def test_get_unknown_id(org):
    r = org.request(
        "GET", bind_detail(org, "00000000-0000-0000-0000-000000000000"),
        "admin",
    )
    assert r.status_code == 404


def test_patch_toggle_enabled(org):
    _, binding = _installed(org)
    r = org.request(
        "PATCH", bind_detail(org, binding["id"]), "admin",
        json={"enabled": False},
    )
    assert r.status_code == 200, r.text
    assert r.json()["enabled"] is False


def test_patch_rrule_recomputes_next_run(org):
    _, binding = _installed(org)
    before = binding["next_run_at"]
    assert before is not None
    r = org.request(
        "PATCH", bind_detail(org, binding["id"]), "admin",
        json={"rrule": "FREQ=DAILY"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["rrule"] == "FREQ=DAILY"
    assert r.json()["next_run_at"] is not None


def test_patch_scheduler_locked(org):
    sched, binding = _installed(org)
    other = _scheduler(org, slug="other")
    r = org.request(
        "PATCH", bind_detail(org, binding["id"]), "admin",
        json={"scheduler": other["id"]},
    )
    assert r.status_code == 400
    assert r.json() == {
        "scheduler": ["scheduler cannot be changed; uninstall and re-install"]
    }


def test_patch_pod_must_belong_to_project(org, pg):
    from .conftest import make_world
    _, binding = _installed(org)
    other = make_world(pg)
    try:
        foreign_pod = seed_s.create_pod(
            other.conn, workspace_id=other.workspace["id"],
            project_id=other.project["id"],
        )
        r = org.request(
            "PATCH", bind_detail(org, binding["id"]), "admin",
            json={"pod": foreign_pod["id"]},
        )
        assert r.status_code == 400
        assert "pod" in r.json()
    finally:
        other.close()


def test_patch_pod_same_project(org):
    _, binding = _installed(org)
    pod = seed_s.create_pod(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"], name="runner-pod",
    )
    r = org.request(
        "PATCH", bind_detail(org, binding["id"]), "admin",
        json={"pod": pod["id"]},
    )
    assert r.status_code == 200, r.text
    assert r.json()["pod"] == pod["id"]
    assert r.json()["pod_name"] == "runner-pod"


def test_delete(org):
    _, binding = _installed(org)
    assert org.request("DELETE", bind_detail(org, binding["id"]), "admin").status_code == 204
    assert org.request("GET", bind_detail(org, binding["id"]), "admin").status_code == 404
    assert org.request("GET", bind_list(org), "admin").json() == []
