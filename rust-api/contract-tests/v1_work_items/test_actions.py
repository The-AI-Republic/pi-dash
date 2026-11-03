"""Contract: work-item actions — move, re-tick, wait, run-ai, agent-run yield
and agent-run release-pin.

Shapes are the live Django wire format. ``run-ai`` returns 409 with a
machine-readable ``reason`` when no runner can take the run (the bare
contract environment has none); that 409 shape is itself the contract.
"""

import uuid

import pytest

from .conftest import issue_url

pytestmark = pytest.mark.contract


def test_move_requires_project(api, world):
    response = api.post(
        issue_url(world, world["issue"]["id"]) + "move/", json={}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "project is required"}


def test_move_to_other_project(api, world, seeder):
    target = seeder.create_project(world["workspace"]["id"], name="Target project")
    seeder.create_project_member(
        world["workspace"]["id"], target["id"], world["owner"]["id"], role=20
    )
    seeder.create_state(world["workspace"]["id"], target["id"])
    response = api.post(
        issue_url(world, world["issue"]["id"]) + "move/",
        json={"project": target["id"]},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["project"] == target["id"]
    # The issue now lives under the target project.
    moved = api.get(
        f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{target['id']}"
        f"/work-items/{world['issue']['id']}/"
    )
    assert moved.status_code == 200


def test_retick_shape(api, world):
    response = api.post(issue_url(world, world["issue"]["id"]) + "re-tick/", json={})
    assert response.status_code == 200
    body = response.json()
    assert {"granted", "reason", "run_id"} <= set(body)
    assert isinstance(body["granted"], bool)


def test_wait_shape(api, world):
    response = api.post(issue_url(world, world["issue"]["id"]) + "wait/", json={})
    assert response.status_code == 200
    body = response.json()
    assert set(body) == {"applied", "reason"}


def test_run_ai_no_runner_is_409(api, world):
    response = api.post(
        issue_url(world, world["issue"]["id"]) + "run-ai/", json={"prompt": "do it"}
    )
    assert response.status_code == 409
    body = response.json()
    assert set(body) == {"error", "reason"}
    assert body["reason"] in ("active_run_exists", "no_pod", "no_eligible_runner")


def test_run_ai_unknown_issue_is_404(api, world):
    response = api.post(
        issue_url(world, "00000000-0000-0000-0000-000000000000") + "run-ai/", json={}
    )
    assert response.status_code == 404


def test_yield_happy_path(api, world, seeder):
    pod = seeder.create_pod(world["workspace"]["id"], world["project"]["id"])
    run = seeder.create_agent_run(
        world["workspace"]["id"], world["owner"]["id"], pod["id"], world["issue"]["id"]
    )
    response = api.post(
        f"/api/v1/workspaces/{world['workspace']['slug']}/agent-runs/{run['id']}/yield/",
        json={"outcome": "done", "note": "finished"},
    )
    assert response.status_code == 200, response.text
    assert response.json() == {
        "ok": True,
        "run_id": run["id"],
        "work_item_id": world["issue"]["id"],
        "outcome": "done",
        "stop_ticking": False,
    }


def _yield(api, world, run_id, payload):
    return api.post(
        f"/api/v1/workspaces/{world['workspace']['slug']}/agent-runs/{run_id}/yield/",
        json=payload,
    )


def _run_with(world, seeder):
    pod = seeder.create_pod(world["workspace"]["id"], world["project"]["id"])
    return seeder.create_agent_run(
        world["workspace"]["id"], world["owner"]["id"], pod["id"], world["issue"]["id"]
    )


def test_yield_stores_stop_ticking(api, world, seeder, db):
    """Port of Django test_yield_stores_stop_ticking (PDASHOSS01-248)."""
    run = _run_with(world, seeder)
    response = _yield(api, world, run["id"], {"outcome": "done", "stop_ticking": True})
    assert response.status_code == 200, response.text
    assert response.json()["stop_ticking"] is True
    row = db.fetchone("SELECT done_payload FROM agent_run WHERE id = %s", (run["id"],))
    assert row["done_payload"]["status"] == "done"
    assert row["done_payload"]["stop_ticking"] is True


def test_yield_without_stop_ticking_stores_no_flag(api, world, seeder, db):
    """Port of Django test_yield_without_stop_ticking_stores_no_flag."""
    run = _run_with(world, seeder)
    response = _yield(api, world, run["id"], {"outcome": "done"})
    assert response.status_code == 200, response.text
    assert response.json()["stop_ticking"] is False
    row = db.fetchone("SELECT done_payload FROM agent_run WHERE id = %s", (run["id"],))
    assert "stop_ticking" not in row["done_payload"]


@pytest.mark.parametrize("flag", ["yes", 1])
def test_yield_rejects_a_non_boolean_stop_ticking(api, world, seeder, db, flag):
    """Port of Django test_yield_rejects_a_non_boolean_stop_ticking.

    ``isinstance(flag, bool)`` is strict: ``1`` 400s exactly like ``"yes"``.
    """
    run = _run_with(world, seeder)
    response = _yield(api, world, run["id"], {"outcome": "done", "stop_ticking": flag})
    assert response.status_code == 400
    assert response.json() == {"error": "stop_ticking must be a boolean"}
    row = db.fetchone("SELECT done_payload FROM agent_run WHERE id = %s", (run["id"],))
    assert not (row["done_payload"] or {})


def test_yield_null_stop_ticking_reads_as_absent(api, world, seeder, db):
    """An explicit JSON null is ``None`` in Django: absent, not a 400."""
    run = _run_with(world, seeder)
    response = _yield(api, world, run["id"], {"outcome": "done", "stop_ticking": None})
    assert response.status_code == 200, response.text
    assert response.json()["stop_ticking"] is False
    row = db.fetchone("SELECT done_payload FROM agent_run WHERE id = %s", (run["id"],))
    assert "stop_ticking" not in row["done_payload"]


def test_yield_still_requires_an_outcome_with_stop_ticking(api, world, seeder):
    """Port of Django test_yield_still_requires_an_outcome_with_stop_ticking."""
    run = _run_with(world, seeder)
    response = _yield(api, world, run["id"], {"stop_ticking": True})
    assert response.status_code == 400
    assert "outcome" in response.json()["error"]


def test_yield_requires_outcome(api, world, seeder):
    pod = seeder.create_pod(world["workspace"]["id"], world["project"]["id"])
    run = seeder.create_agent_run(
        world["workspace"]["id"], world["owner"]["id"], pod["id"], world["issue"]["id"]
    )
    response = api.post(
        f"/api/v1/workspaces/{world['workspace']['slug']}/agent-runs/{run['id']}/yield/",
        json={},
    )
    assert response.status_code == 400
    body = response.json()
    assert set(body) == {"error", "allowed"}
    assert "done" in body["allowed"]


def test_yield_foreign_run_is_404(other_api, world, other_world):
    """Tenant isolation: another tenant's run id resolves to not-found."""
    response = other_api.post(
        f"/api/v1/workspaces/{other_world['workspace']['slug']}"
        f"/agent-runs/{world['issue']['id']}/yield/",
        json={"outcome": "done"},
    )
    assert response.status_code == 404
    assert response.json() == {"error": "run not found"}


def test_yield_anonymous_is_401(anon, world, seeder):
    pod = seeder.create_pod(world["workspace"]["id"], world["project"]["id"])
    run = seeder.create_agent_run(
        world["workspace"]["id"], world["owner"]["id"], pod["id"], world["issue"]["id"]
    )
    response = anon.post(
        f"/api/v1/workspaces/{world['workspace']['slug']}/agent-runs/{run['id']}/yield/",
        json={"outcome": "done"},
    )
    assert response.status_code == 401


# -- release-pin (PDASHOSS01-272) ------------------------------------------


def _release_pin(client, world, run_id):
    return client.post(
        f"/api/v1/workspaces/{world['workspace']['slug']}/agent-runs/{run_id}/release-pin/"
    )


def _pinned_queued_run(world, seeder, db, *, status="queued", pinned=True):
    pod = seeder.create_pod(world["workspace"]["id"], world["project"]["id"])
    run = seeder.create_agent_run(
        world["workspace"]["id"],
        world["owner"]["id"],
        pod["id"],
        world["issue"]["id"],
        status=status,
    )
    runner = None
    if pinned:
        # An offline runner: the post-commit pod drain assigns nothing.
        runner = seeder.enroll_runner(
            world["owner"]["id"],
            world["workspace"]["id"],
            pod["id"],
            f"contract-enroll-{uuid.uuid4().hex}",
        )
        db.execute(
            "UPDATE agent_run SET pinned_runner_id = %s WHERE id = %s",
            (runner["id"], run["id"]),
        )
    return run, runner


def test_release_pin_happy_path(api, world, seeder, db):
    run, runner = _pinned_queued_run(world, seeder, db)
    response = _release_pin(api, world, run["id"])
    assert response.status_code == 200, response.text
    assert response.json() == {
        "ok": True,
        "run_id": run["id"],
        "work_item_id": world["issue"]["id"],
        "previous_pinned_runner_id": runner["id"],
    }
    assert list(response.json()) == [
        "ok",
        "run_id",
        "work_item_id",
        "previous_pinned_runner_id",
    ]
    row = db.fetchone(
        "SELECT pinned_runner_id, status FROM agent_run WHERE id = %s", (run["id"],)
    )
    assert row["pinned_runner_id"] is None
    assert row["status"] == "queued"


def test_release_pin_unpinned_is_409(api, world, seeder, db):
    run, _ = _pinned_queued_run(world, seeder, db, pinned=False)
    response = _release_pin(api, world, run["id"])
    assert response.status_code == 409
    assert response.json() == {"error": "run not pinned", "code": "not_pinned"}


def test_release_pin_not_queued_is_409(api, world, seeder, db):
    run, runner = _pinned_queued_run(world, seeder, db, status="running")
    response = _release_pin(api, world, run["id"])
    assert response.status_code == 409
    assert response.json() == {"error": "run not queued", "code": "not_queued"}
    row = db.fetchone("SELECT pinned_runner_id FROM agent_run WHERE id = %s", (run["id"],))
    assert str(row["pinned_runner_id"]) == runner["id"]


def test_release_pin_foreign_run_is_404(other_api, world, other_world):
    """Tenant isolation: another tenant's run id resolves to not-found."""
    response = other_api.post(
        f"/api/v1/workspaces/{other_world['workspace']['slug']}"
        f"/agent-runs/{world['issue']['id']}/release-pin/"
    )
    assert response.status_code == 404
    assert response.json() == {"error": "run not found"}


def test_release_pin_anonymous_is_401(anon, world, seeder, db):
    run, _ = _pinned_queued_run(world, seeder, db, pinned=False)
    response = _release_pin(anon, world, run["id"])
    assert response.status_code == 401
