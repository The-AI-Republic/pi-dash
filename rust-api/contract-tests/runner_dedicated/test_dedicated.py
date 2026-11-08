"""Dedicated-runner backend contract (PDASHOSS01-297, design §18 rows).

The same suite must pass byte-for-byte against Django (the #3000
branch) and the Rust server (§17). Read: issue + project fields.
Write: E-code rejections. Dispatch: run-ai H-11 refusal, release-pin
H-13 refusal, cross-project pod-move refusal.
"""


def _issue_url(world, issue_id):
    return f"/api/workspaces/{world['ws']['slug']}/projects/{world['project']['id']}/issues/{issue_id}/"


def test_issue_detail_carries_dedicated_read_fields(admin_client, world, dedicated_issue, online_runner):
    resp = admin_client.get(_issue_url(world, world["issue1"]["id"]))
    assert resp.status_code == 200, resp.text[:500]
    body = resp.json()
    assert body["assigned_runner_id"] == online_runner["id"]
    detail = body["assigned_runner_detail"]
    assert detail["id"] == online_runner["id"]
    assert detail["name"] == online_runner["name"]


def test_issue_detail_plain_issue_has_null_runner(admin_client, world):
    resp = admin_client.get(_issue_url(world, world["issue2"]["id"]))
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["assigned_runner_id"] is None


def test_project_carries_lost_action_default(admin_client, world):
    resp = admin_client.get(
        f"/api/workspaces/{world['ws']['slug']}/projects/{world['project']['id']}/"
    )
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["dedicated_runner_lost_action"] == "fail_to_backlog"


def test_patch_assigns_runner(admin_client, world, online_runner):
    resp = admin_client.patch(
        _issue_url(world, world["issue2"]["id"]),
        json={"assigned_runner_id": online_runner["id"], "assigned_pod_id": world["pod"]["id"]},
    )
    assert resp.status_code in (200, 204), resp.text[:500]


def test_patch_runner_wrong_project_rejected(admin_client, world, other_project_runner):
    resp = admin_client.patch(
        _issue_url(world, world["issue2"]["id"]),
        json={"assigned_runner_id": other_project_runner["id"]},
    )
    assert resp.status_code == 400, resp.text[:500]
    # DRF field-error shape: ErrorDetail renders as its string; the
    # E-code is carried in the unit tests, the wire is the detail.
    assert resp.json()["assigned_runner"][0] == "runner is in a different project"


def test_patch_foreign_runner_rejected(admin_client, world, foreign_runner):
    resp = admin_client.patch(
        _issue_url(world, world["issue2"]["id"]),
        json={"assigned_runner_id": foreign_runner["id"]},
    )
    assert resp.status_code == 400, resp.text[:500]
    assert resp.json()["assigned_runner"][0] == "runner not found"


def test_run_ai_refuses_offline_dedicated(admin_client, world, dedicated_issue, offline_runner):
    # Point the issue at the OFFLINE runner: H-11 refuses the human run.
    assign = admin_client.patch(
        _issue_url(world, world["issue1"]["id"]),
        json={"assigned_runner_id": offline_runner["id"], "assigned_pod_id": world["pod"]["id"]},
    )
    assert assign.status_code in (200, 204), assign.text[:500]
    resp = admin_client.post(
        "/api/runners/runs/",
        json={"triggered_by": "run_ai", "work_item": world["issue1"]["id"]},
    )
    assert resp.status_code == 409, resp.text[:500]
    assert resp.json()["reason"] == "assigned_runner_offline"
    assert resp.json()["options"] == ["pod_once", "change_runner"]


def test_release_pin_refused_for_dedicated(admin_client, world, dedicated_run, online_runner):
    resp = admin_client.post(f"/api/runners/runs/{dedicated_run['id']}/release-pin/")
    assert resp.status_code == 409, resp.text[:500]
    body = resp.json()
    assert body["code"] == "dedicated_pin_not_releasable"
    assert online_runner["name"] in body["error"]
    assert "dr-pod" in body["error"]


def test_merge_moves_dedicated_assignment(admin_client, world, merge_world):
    resp = admin_client.post(
        f"/api/v1/runner/dev-machines/{merge_world['source_id']}/merge-into/",
        json={"target_dev_machine_id": merge_world["target_id"]},
    )
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["collisions"] == [
        {"revoked_runner_id": merge_world["loser_id"],
         "survivor_runner_id": merge_world["survivor_id"]}
    ]
    body = admin_client.get(_issue_url(world, world["issue1"]["id"])).json()
    assert body["assigned_runner_id"] == merge_world["survivor_id"]
    assert body["assigned_pod_id"] == merge_world["pod_b_id"]


def test_merge_repoints_queued_same_pod(admin_client, world, merge_world, merge_run):
    resp = admin_client.post(
        f"/api/v1/runner/dev-machines/{merge_world['source_id']}/merge-into/",
        json={"target_dev_machine_id": merge_world["target_id"]},
    )
    assert resp.status_code == 200, resp.text[:500]
    body = admin_client.get(f"/api/runners/runs/{merge_run['id']}/").json()
    assert body["pinned_runner"] == str(merge_world["survivor_id"])


def test_cross_project_pod_move_refused(admin_client, world, dedicated_issue, online_runner, project2, seeder):
    seeder.create_pod(world["ws"]["id"], project2["id"], name="dr-pod2")
    pods = admin_client.get(f"/api/runners/pods/?project={project2['id']}")
    assert pods.status_code == 200, pods.text[:500]
    pod2 = pods.json()[0]
    resp = admin_client.patch(
        f"/api/runners/{online_runner['id']}/",
        json={"pod": pod2["id"]},
    )
    assert resp.status_code == 409, resp.text[:500]
    assert resp.json()["code"] == "runner_has_assigned_issues"
