"""Dedicated-runner activity parity (NEWPIDASH-172, PDASHOSS01-297 drift).

Django writes two ``issue_activities`` rows per ``assigned_runner`` change
(synchronous ``epoch=1`` old="" plus async task row with previous runner
name and request epoch) and one async task row per ``assigned_pod`` change.
Rust must match exactly.

This contract case drives ``PATCH`` through the live API, waits for the
worker to drain the async task, and asserts the database rows and the
``GET .../history/?activity_type=issue-property`` read.

It runs in a worker suite (``runner_dedicated`` in ``WORKER_SUITES.txt``)
so the Django run gets a solo Celery worker and the Rust run gets
``pidash-api worker``; without a consumer the task rows never appear.
"""

import pytest

from _harness.db import wait_for_condition


def _issue_url(world, issue_id):
    return f"/api/workspaces/{world['ws']['slug']}/projects/{world['project']['id']}/issues/{issue_id}/"


def _history_url(world, issue_id):
    return (
        f"/api/workspaces/{world['ws']['slug']}/projects/{world['project']['id']}"
        f"/issues/{issue_id}/history/?activity_type=issue-property"
    )


def _runner_rows(seeder, issue_id, field="assigned_runner"):
    rows = seeder.db.fetchall(
        "SELECT id, verb, field, old_value, new_value, old_identifier, new_identifier, epoch, comment, created_at "
        "FROM issue_activities WHERE issue_id = %s AND field = %s ORDER BY created_at, epoch, id",
        (issue_id, field),
    )
    # psycopg dict rows; normalise UUIDs to str for comparison
    for row in rows:
        row["id"] = str(row["id"])
        for key in ("old_identifier", "new_identifier"):
            if row[key] is not None:
                row[key] = str(row[key])
    return rows


def _wait_for_runner_rows(seeder, issue_id, expected_count, field="assigned_runner"):
    def _check():
        rows = _runner_rows(seeder, issue_id, field=field)
        return rows if len(rows) >= expected_count else None

    return wait_for_condition(_check, what=f"{expected_count} {field} activity rows")


def test_assigned_runner_writes_two_rows_per_change(admin_client, world, seeder):
    # Two runners with probe-visible names (issue probe: A/B/null sequence).
    runner_a = seeder.enroll_runner(
        world["admin"]["id"], world["ws"]["id"], world["pod"]["id"], "probe-a", name="probe runner A"
    )
    runner_b = seeder.enroll_runner(
        world["admin"]["id"], world["ws"]["id"], world["pod"]["id"], "probe-b", name="probe runner B"
    )
    seeder.db.execute(
        "UPDATE runner SET status='online', last_heartbeat_at=now() WHERE id IN (%s, %s)",
        (runner_a["id"], runner_b["id"]),
    )
    issue_id = world["issue2"]["id"]
    url = _issue_url(world, issue_id)

    # None -> A
    resp = admin_client.patch(url, json={"assigned_runner_id": runner_a["id"]})
    assert resp.status_code in (200, 204), resp.text[:500]
    # A -> B
    resp = admin_client.patch(url, json={"assigned_runner_id": runner_b["id"]})
    assert resp.status_code in (200, 204), resp.text[:500]
    # B -> null (clear)
    resp = admin_client.patch(url, json={"assigned_runner_id": None})
    assert resp.status_code in (200, 204), resp.text[:500]

    # Two rows per change: one sync (epoch 1, old_value "") + one task
    # (epoch=request, old_value=previous name). First change's old is ""
    # on both rows; later changes differ.
    rows = _wait_for_runner_rows(seeder, issue_id, 6, field="assigned_runner")
    assert len(rows) == 6, f"expected 6 rows, got {len(rows)}: {rows}"

    # Partition by epoch: sync rows have epoch 1.0, task rows have a
    # request-epoch (>1). There must be exactly three of each.
    sync = [r for r in rows if r["epoch"] == 1]
    task = [r for r in rows if r["epoch"] != 1]
    assert len(sync) == 3, f"sync rows {sync}"
    assert len(task) == 3, f"task rows {task}"

    # Every row is a runner update with the right comment/verb.
    for row in rows:
        assert row["verb"] == "updated"
        assert row["field"] == "assigned_runner"
        assert row["comment"] == "updated the dedicated runner to"

    # Sync rows always carry old_value "" and old_identifier None.
    for row in sync:
        assert row["old_value"] == "", row
        assert row["old_identifier"] is None, row

    # Task rows carry the previous runner name/identifier.
    # First change's previous is None -> "" / None on the task row too.
    # But we can assert the task pair for the B and null changes.
    task_by_new = {(r["new_identifier"]): r for r in task}
    # A was the first assignment: old "" regardless.
    row_a = task_by_new.get(runner_a["id"])
    assert row_a is not None, f"missing task row for A: {task}"
    assert row_a["old_value"] == ""
    assert row_a["old_identifier"] is None
    assert row_a["new_value"] == "probe runner A"
    # B carries previous A.
    row_b = task_by_new.get(runner_b["id"])
    assert row_b is not None, f"missing task row for B: {task}"
    assert row_b["old_value"] == "probe runner A"
    assert row_b["old_identifier"] == runner_a["id"]
    assert row_b["new_value"] == "probe runner B"
    # Clearing carries previous B.
    # The null row has new_identifier None; find it by epoch.
    null_task = next((r for r in task if r["new_identifier"] is None), None)
    assert null_task is not None, f"missing task row for clear: {task}"
    assert null_task["old_value"] == "probe runner B"
    assert null_task["old_identifier"] == runner_b["id"]
    assert null_task["new_value"] == ""

    # Sync rows: one per change with the new name/identifier and empty old.
    sync_by_new = {}
    for r in sync:
        sync_by_new.setdefault(r["new_identifier"], []).append(r)
    assert runner_a["id"] in sync_by_new, f"sync A {sync}"
    assert runner_b["id"] in sync_by_new, f"sync B {sync}"
    # Three sync rows: A, B and the clear (None). The clear's new is "".
    sync_null = next((r for r in sync if r["new_identifier"] is None), None)
    assert sync_null is not None
    assert sync_null["new_value"] == ""
    assert sync_null["old_value"] == ""

    # History read returns the same six rows.
    resp = admin_client.get(_history_url(world, issue_id))
    assert resp.status_code == 200, resp.text[:500]
    history = resp.json()
    # History merges activities and comments, sorted by created_at; no
    # comments exist on issue2, so the payload is the six activities.
    assert len(history) == 6, f"history length {len(history)}: {history}"
    history_by_id = {row["id"] for row in history}
    db_ids = {str(row["id"]) for row in rows}
    assert history_by_id == db_ids, (history_by_id, db_ids)
    # Each history row carries the same old/new/identifiers/epoch as DB.
    history_by_id_map = {row["id"]: row for row in history}
    for db_row in rows:
        h = history_by_id_map[str(db_row["id"])]
        # History serializes old_value/new_value as string or None;
        # our DB rows use "" for empty. Normalize.
        assert (h["old_value"] or "") == (db_row["old_value"] or ""), (h, db_row)
        assert (h["new_value"] or "") == (db_row["new_value"] or ""), (h, db_row)
        # Identifiers may be rendered as string or None.
        assert (h.get("old_identifier") or None) == (db_row["old_identifier"] or None), (h, db_row)
        assert (h.get("new_identifier") or None) == (db_row["new_identifier"] or None), (h, db_row)
        # Epoch is rendered as number; compare numerically.
        assert h["epoch"] == db_row["epoch"] or str(h["epoch"]) == str(db_row["epoch"]), (h, db_row)


def test_assigned_pod_writes_task_row(admin_client, world, seeder):
    pod_a = world["pod"]
    # Second pod in same project.
    pod_b = seeder.create_pod(world["ws"]["id"], world["project"]["id"], name="dr-pod-b")
    issue_id = world["issue1"]["id"]
    url = _issue_url(world, issue_id)
    # Issue1 is plain initially (no pod assignment check needed): set pod.
    # First set to pod_b (change from default? world pod is default).
    # To make a deterministic previous, ensure issue's pod is pod_a first.
    seeder.db.execute(
        "UPDATE issues SET assigned_pod_id=%s WHERE id=%s",
        (pod_a["id"], issue_id),
    )
    resp = admin_client.patch(url, json={"assigned_pod_id": pod_b["id"]})
    assert resp.status_code in (200, 204), resp.text[:500]

    rows = _wait_for_runner_rows(seeder, issue_id, 1, field="assigned_pod")
    assert len(rows) == 1, f"pod rows {rows}"
    row = rows[0]
    assert row["verb"] == "updated"
    assert row["field"] == "assigned_pod"
    assert row["comment"] == "updated the execution pod to"
    assert row["old_value"] == pod_a["name"]
    assert row["new_value"] == pod_b["name"]
    assert row["old_identifier"] == pod_a["id"]
    assert row["new_identifier"] == pod_b["id"]
    assert row["epoch"] != 1  # request epoch, not the sync epoch
