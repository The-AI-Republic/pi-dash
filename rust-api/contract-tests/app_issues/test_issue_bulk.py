"""Bulk issue endpoints (PIDASHCONV-1677).

Ported from ``apps/api/pi_dash/tests/contract/app/test_issue_bulk_cloud.py``
(main, NEWFRONT-984): bulk update applies one properties object across a
selection (scalars replace, array properties union), validating the whole
batch before writing anything; bulk subscribe subscribes the caller,
idempotently. Runs against both backends — Django from a main worktree
(rust-dev ``apps/api`` predates the views) and Rust.

Activity fan-out lands on different transports per backend with no
worker running (Celery broker vs ``rust_job_queue``); the helpers read
both and only the active backend has rows.
"""

import json
import os
import uuid
from datetime import datetime, timedelta, timezone

import pytest

pytestmark = pytest.mark.contract

ISSUE_TASK = "pi_dash.bgtasks.issue_activities_task.issue_activity"

DENIED = {"detail": "You do not have permission to perform this action."}
UNAUTH = {"detail": "Authentication credentials were not provided."}
PROJECT_404 = {"error": "Project not found"}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _unique(tag):
    return f"{tag} {uuid.uuid4().hex[:8]}"


def _op_url(ws, pid):
    return f"{_base(ws, pid)}/bulk-operation-issues/"


def _sub_url(ws, pid):
    return f"{_base(ws, pid)}/bulk-subscribe-issues/"


def _drain_broker():
    """Pending Celery-protocol messages (Django's transport; [] without one)."""
    if not os.environ.get("CELERY_BROKER_URL"):
        return []
    from _harness import broker_probe

    return broker_probe.drain_queue()


def _rust_rows(db_conn, task, after_id):
    """Rows Rust serve left in ``rust_job_queue`` ([], table may be absent)."""
    with db_conn.cursor() as cur:
        cur.execute("SELECT to_regclass('public.rust_job_queue')")
        if cur.fetchone()[0] is None:
            return []
        cur.execute(
            "SELECT task, args, kwargs FROM rust_job_queue"
            " WHERE id > %s AND task = %s",
            (after_id, task),
        )
        out = []
        for found_task, args, kwargs in cur.fetchall():
            out.append({
                "task": found_task,
                "args": args if isinstance(args, list) else json.loads(args),
                "kwargs": kwargs if isinstance(kwargs, dict) else json.loads(kwargs),
            })
        return out


def _queue_baseline(db_conn):
    with db_conn.cursor() as cur:
        cur.execute("SELECT to_regclass('public.rust_job_queue')")
        if cur.fetchone()[0] is None:
            return 0
        cur.execute("SELECT COALESCE(MAX(id), 0) FROM rust_job_queue")
        return cur.fetchone()[0]


def _activity_kwargs(db_conn, baseline, actor_id):
    """New issue_activity kwargs on either transport since the baseline."""
    out = []
    for headers, payload in _drain_broker():
        if (headers or {}).get("task") != ISSUE_TASK:
            continue
        kwargs = payload[1] if isinstance(payload, list) and len(payload) > 1 else {}
        if isinstance(kwargs, dict) and str(kwargs.get("actor_id")) == str(actor_id):
            out.append(kwargs)
    for row in _rust_rows(db_conn, ISSUE_TASK, baseline):
        if str(row["kwargs"].get("actor_id")) == str(actor_id):
            out.append(row["kwargs"])
    return out


def _reset_transports(db_conn):
    """Discard pending broker messages; return the rust queue baseline."""
    _drain_broker()
    return _queue_baseline(db_conn)


def _make_issue(client, ws, pid, name):
    resp = client.post(f"{_base(ws, pid)}/issues/", json={"name": name})
    assert resp.status_code in (200, 201), resp.text
    body = resp.json()
    assert "id" in body, body
    return body["id"]


def _delete_issue(client, ws, pid, iid):
    try:
        client.delete(f"{_base(ws, pid)}/issues/{iid}/")
    except Exception:
        pass


def _now():
    return datetime.now(timezone.utc)


def _insert_label(db_conn, ws_id, pid, admin_id, name):
    lid = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO labels (created_at, updated_at, id, name, description,"
            " created_by_id, project_id, updated_by_id, workspace_id, color,"
            " sort_order) VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (_now(), _now(), lid, name, "", admin_id, pid, admin_id, ws_id,
             "#ff0000", 0.0),
        )
    return lid


def _insert_module(db_conn, ws_id, pid, admin_id, name):
    mid = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO modules (created_at, updated_at, id, name, description,"
            " created_by_id, project_id, updated_by_id, workspace_id, status,"
            " sort_order, view_props, logo_props)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (_now(), _now(), mid, name, "", admin_id, pid, admin_id, ws_id,
             "backlog", 65535, "{}", "{}"),
        )
    return mid


def _insert_cycle(db_conn, ws_id, pid, admin_id, name, done=False):
    cid = str(uuid.uuid4())
    if done:
        start, end = _now() - timedelta(days=7), _now() - timedelta(days=1)
    else:
        start, end = _now() + timedelta(days=1), _now() + timedelta(days=7)
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO cycles (created_at, updated_at, id, name, description,"
            " created_by_id, project_id, updated_by_id, workspace_id, owned_by_id,"
            " start_date, end_date, view_props, sort_order, progress_snapshot,"
            " logo_props, timezone, version)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (_now(), _now(), cid, name, "", admin_id, pid, admin_id, ws_id,
             admin_id, start, end, "{}", 65535, "{}", "{}", "UTC", 1),
        )
    return cid


def _insert_estimate_point(db_conn, ws_id, pid, admin_id, name):
    eid, pid_ = str(uuid.uuid4()), str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO estimates (created_at, updated_at, id, name, description,"
            " created_by_id, project_id, updated_by_id, workspace_id, type,"
            " last_used) VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (_now(), _now(), eid, name, "", admin_id, pid, admin_id, ws_id,
             "categories", False),
        )
        cur.execute(
            "INSERT INTO estimate_points (created_at, updated_at, id, key, value,"
            " estimate_id, created_by_id, project_id, updated_by_id, workspace_id,"
            " description) VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (_now(), _now(), pid_, 3, "3", eid, admin_id, pid, admin_id, ws_id, ""),
        )
    return eid, pid_


def _wipe_test_rows(db_conn, ws_id, issue_ids=(), label_ids=(), module_ids=(),
                    cycle_ids=(), estimate_ids=(), point_ids=()):
    """Hard-delete this test's own rows (bridges before parents)."""
    with db_conn.cursor() as cur:
        for table in ("issue_sequences", "issue_subscribers", "issue_assignees",
                      "issue_labels", "cycle_issues", "module_issues"):
            cur.execute(
                f"DELETE FROM {table} WHERE issue_id = ANY(%s)",
                (list(issue_ids),),
            )
        for table, ids in (("labels", label_ids), ("modules", module_ids),
                           ("cycles", cycle_ids), ("estimates", estimate_ids),
                           ("estimate_points", point_ids), ("issues", issue_ids)):
            if ids:
                cur.execute(
                    f"DELETE FROM {table} WHERE id = ANY(%s)", (list(ids),)
                )


def _issue_row(db_conn, iid):
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT state_id, priority, start_date, target_date,"
            " estimate_point_id, updated_at FROM issues WHERE id = %s",
            (iid,),
        )
        row = cur.fetchone()
        assert row is not None, f"issue {iid} missing"
        return {
            "state_id": str(row[0]) if row[0] else None,
            "priority": row[1],
            "start_date": str(row[2]) if row[2] else None,
            "target_date": str(row[3]) if row[3] else None,
            "estimate_point_id": str(row[4]) if row[4] else None,
            "updated_at": row[5],
        }


def _live_count(db_conn, table, iid):
    with db_conn.cursor() as cur:
        cur.execute(
            f"SELECT COUNT(*) FROM {table} WHERE issue_id = %s"
            " AND deleted_at IS NULL",
            (iid,),
        )
        return cur.fetchone()[0]


def test_member_updates_scalars_across_selection(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    made = [_make_issue(member, ws, pid, f"Bulk scalar {n}") for n in ("a", "b")]
    est, point = _insert_estimate_point(
        db_conn, seed["ws"], pid, seed["admin"], _unique("Bulk est"))
    try:
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={
                "issue_ids": made,
                "properties": {
                    "state_id": seed["state_done"],
                    "priority": "high",
                    "start_date": "2026-11-10",
                    "target_date": "2026-11-17",
                    "estimate_point": point,
                },
            },
        )
        assert resp.status_code == 200, resp.text
        assert resp.json() == {"message": "2 issues were updated", "updated": 2}
        for iid in made:
            row = _issue_row(db_conn, iid)
            assert row["state_id"] == seed["state_done"], row
            assert row["priority"] == "high", row
            assert row["start_date"] == "2026-11-10", row
            assert row["target_date"] == "2026-11-17", row
            assert row["estimate_point_id"] == point, row
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        assert len(acts) == 2, acts
        assert {a["type"] for a in acts} == {"issue.activity.updated"}
        assert {a["issue_id"] for a in acts} == set(made)
        first = next(a for a in acts if a["issue_id"] == made[0])
        requested = json.loads(first["requested_data"])
        assert requested == {
            "state_id": seed["state_done"],
            "priority": "high",
            "start_date": "2026-11-10",
            "target_date": "2026-11-17",
            "estimate_point": point,
        }, requested
        # Byte parity: json.dumps spacing + applied key order.
        assert first["requested_data"] == json.dumps(requested), first
        previous = json.loads(first["current_instance"])
        assert previous["priority"] == "none", previous
        assert previous["start_date"] == "None", previous
        assert first["notification"] is True
        assert first["origin"] == os.environ["CONTRACT_APP_BASE_URL"].rstrip("/")
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE issues SET estimate_point_id = NULL WHERE id = ANY(%s)",
                (made,),
            )
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=made)
        with db_conn.cursor() as cur:
            cur.execute(
                "DELETE FROM estimate_points WHERE id = %s", (point,))
            cur.execute("DELETE FROM estimates WHERE id = %s", (est,))


def test_arrays_union_onto_existing_rows(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk union")
    labels = [_insert_label(db_conn, seed["ws"], pid, seed["admin"], _unique(f"BU L{n}"))
              for n in (1, 2)]
    mid = _insert_module(db_conn, seed["ws"], pid, seed["admin"], _unique("BU M"))
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO issue_labels (created_at, updated_at, id,"
                " created_by_id, issue_id, label_id, project_id, updated_by_id,"
                " workspace_id) VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s)",
                (str(uuid.uuid4()), seed["admin"], iid, labels[0], pid,
                 seed["admin"], seed["ws"]),
            )
            cur.execute(
                "INSERT INTO issue_assignees (created_at, updated_at, id,"
                " assignee_id, created_by_id, issue_id, project_id, updated_by_id,"
                " workspace_id) VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s)",
                (str(uuid.uuid4()), seed["member"], seed["admin"], iid, pid,
                 seed["admin"], seed["ws"]),
            )
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={
                "issue_ids": [iid],
                "properties": {
                    "label_ids": labels,
                    "assignee_ids": [seed["member"], seed["admin"]],
                    "module_ids": [mid],
                },
            },
        )
        assert resp.status_code == 200, resp.text
        assert _live_count(db_conn, "issue_labels", iid) == 2
        assert _live_count(db_conn, "issue_assignees", iid) == 2
        assert _live_count(db_conn, "module_issues", iid) == 1
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        by_type = {}
        for act in acts:
            by_type.setdefault(act["type"], []).append(act)
        assert set(by_type) == {"module.activity.created", "issue.activity.updated"}
        assert json.loads(by_type["module.activity.created"][0]["requested_data"]) == {
            "module_id": mid}
        assert json.loads(by_type["issue.activity.updated"][0]["requested_data"]) == {
            "label_ids": labels,
            "assignee_ids": [seed["member"], seed["admin"]],
            "module_ids": [mid],
        }
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid],
                        label_ids=labels, module_ids=[mid])


def test_cycle_moves_between_cycles(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk move")
    src = _insert_cycle(db_conn, seed["ws"], pid, seed["admin"], _unique("BM src"))
    dst = _insert_cycle(db_conn, seed["ws"], pid, seed["admin"], _unique("BM dst"))
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO cycle_issues (created_at, updated_at, id,"
                " created_by_id, issue_id, cycle_id, project_id, updated_by_id,"
                " workspace_id) VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s)",
                (str(uuid.uuid4()), seed["admin"], iid, src, pid,
                 seed["admin"], seed["ws"]),
            )
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"cycle_id": dst}},
        )
        assert resp.status_code == 200, resp.text
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT cycle_id FROM cycle_issues WHERE issue_id = %s"
                " AND deleted_at IS NULL",
                (iid,),
            )
            rows = cur.fetchall()
        assert [str(r[0]) for r in rows] == [dst], rows
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        created = [a for a in acts if a["type"] == "cycle.activity.created"]
        assert len(created) == 1, acts
        assert json.loads(created[0]["requested_data"]) == {"cycles_list": [iid]}
        current = json.loads(created[0]["current_instance"])
        assert current["updated_cycle_issues"] == [{
            "old_cycle_id": src, "new_cycle_id": dst, "issue_id": iid}], current
        assert json.loads(current["created_cycle_issues"]) == [], current
        assert created[0]["issue_id"] is None
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid], cycle_ids=[src, dst])


def test_cycle_set_serializes_created_bridge(clients, seed, db_conn):
    # The single cycle.activity.created carries the Django-serialized
    # created bridge rows (spaced separators, millis datetimes).
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk cycle new")
    cid = _insert_cycle(db_conn, seed["ws"], pid, seed["admin"], _unique("BC new"))
    try:
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"cycle_id": cid}},
        )
        assert resp.status_code == 200, resp.text
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT id, created_at, updated_at, created_by_id, updated_by_id"
                " FROM cycle_issues WHERE issue_id = %s AND deleted_at IS NULL",
                (iid,),
            )
            (bid, created_at, updated_at, created_by, updated_by) = cur.fetchone()
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        created = [a for a in acts if a["type"] == "cycle.activity.created"]
        assert len(created) == 1, acts
        current = json.loads(created[0]["current_instance"])
        assert current["updated_cycle_issues"] == [], current
        bridges = json.loads(current["created_cycle_issues"])

        def _millis(moment):
            moment = moment.astimezone(timezone.utc)
            text = moment.strftime("%Y-%m-%dT%H:%M:%S")
            if moment.microsecond:
                text += ".%03d" % (moment.microsecond // 1000)
            return text + "Z"

        expected = [{
            "model": "db.cycleissue",
            "pk": str(bid),
            "fields": {
                "created_at": _millis(created_at),
                "updated_at": _millis(updated_at),
                "created_by": str(created_by),
                "updated_by": str(updated_by),
                "deleted_at": None,
                "project": pid,
                "workspace": seed["ws"],
                "issue": iid,
                "cycle": cid,
            },
        }]
        assert bridges == expected, (
            f"actual={bridges!r}\nexpected={expected!r}\n"
            f"db_row={(bid, created_at, updated_at, created_by, updated_by)!r}")
        # Byte parity with Django's serialize("json", ...): spaced
        # separators in model/pk/fields order.
        assert current["created_cycle_issues"] == json.dumps(bridges), current
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid], cycle_ids=[cid])


def test_null_cycle_clears_membership(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk clear")
    cid = _insert_cycle(db_conn, seed["ws"], pid, seed["admin"], _unique("BC clear"))
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO cycle_issues (created_at, updated_at, id,"
                " created_by_id, issue_id, cycle_id, project_id, updated_by_id,"
                " workspace_id) VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s)",
                (str(uuid.uuid4()), seed["admin"], iid, cid, pid,
                 seed["admin"], seed["ws"]),
            )
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"cycle_id": None}},
        )
        assert resp.status_code == 200, resp.text
        assert _live_count(db_conn, "cycle_issues", iid) == 0
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        deleted = [a for a in acts if a["type"] == "cycle.activity.deleted"]
        assert len(deleted) == 1, acts
        assert json.loads(deleted[0]["requested_data"]) == {
            "cycle_id": None, "issues": [iid]}, deleted
        assert deleted[0]["issue_id"] == iid
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid], cycle_ids=[cid])


def test_merged_date_violation_fails_batch_untouched(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    made = [_make_issue(member, ws, pid, f"Bulk dates {n}") for n in ("a", "b")]
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE issues SET start_date = %s WHERE id = %s",
                ("2026-11-10", made[0]),
            )
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={
                "issue_ids": made,
                "properties": {"target_date": "2026-11-01"},
            },
        )
        assert resp.status_code == 400, resp.text
        assert resp.json() == {"error": "Start date cannot exceed target date"}
        for iid in made:
            assert _issue_row(db_conn, iid)["target_date"] is None
        assert _activity_kwargs(db_conn, baseline, seed["member"]) == []
    finally:
        for iid in made:
            _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=made)


def test_unknown_property_is_rejected(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk unknown prop")
    try:
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"name": "renamed"}},
        )
        assert resp.status_code == 400, resp.text
        # A bare string, not a list (the view's custom to_internal_value).
        assert resp.json() == {
            "properties": {"name": "This property cannot be bulk-edited."}}
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid],
                  "properties": {"zzz": 1, "name": "renamed"}},
        )
        assert resp.status_code == 400, resp.text
        assert resp.json() == {"properties": {
            "name": "This property cannot be bulk-edited.",
            "zzz": "This property cannot be bulk-edited.",
        }}
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid])


def test_empty_properties_are_rejected(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk empty props")
    try:
        resp = member.post(
            _op_url(ws, pid), json={"issue_ids": [iid], "properties": {}})
        assert resp.status_code == 400, resp.text
        assert resp.json() == {"properties": {"non_field_errors": [
            "Properties must name at least one bulk-editable field."]}}
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid])


def test_empty_selection_is_rejected(clients, seed):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.post(
        _op_url(ws, pid),
        json={"issue_ids": [], "properties": {"priority": "high"}})
    assert resp.status_code == 400, resp.text
    assert resp.json() == {
        "issue_ids": ["Ensure this field has at least 1 elements."]}


def test_unknown_reference_fails_batch_untouched(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk bad ref")
    missing = str(uuid.uuid4())
    try:
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid],
                  "properties": {"state_id": missing, "priority": "urgent"}},
        )
        assert resp.status_code == 400, resp.text
        assert resp.json() == {"error": f"State {missing} does not exist"}
        assert _issue_row(db_conn, iid)["priority"] == "none"
        assert _activity_kwargs(db_conn, baseline, seed["member"]) == []
        other = str(uuid.uuid4())
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"label_ids": [missing, other]}})
        assert resp.status_code == 400, resp.text
        assert resp.json() == {
            "error": f"Labels do not exist: {', '.join(sorted([missing, other]))}"}
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid])


def test_unknown_ids_are_silently_skipped(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk skipped")
    cid = _insert_cycle(db_conn, seed["ws"], pid, seed["admin"], _unique("BS empty"))
    try:
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid, str(uuid.uuid4())],
                  "properties": {"priority": "low"}},
        )
        assert resp.status_code == 200, resp.text
        assert resp.json() == {"message": "1 issues were updated", "updated": 1}
        assert _issue_row(db_conn, iid)["priority"] == "low"
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [str(uuid.uuid4())],
                  "properties": {"priority": "low"}},
        )
        assert resp.status_code == 200, resp.text
        assert resp.json() == {"message": "0 issues were updated", "updated": 0}
        # A cycle set still fires its single created activity for an
        # empty selection (with empty lists).
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [str(uuid.uuid4())],
                  "properties": {"cycle_id": cid}},
        )
        assert resp.status_code == 200, resp.text
        assert resp.json() == {"message": "0 issues were updated", "updated": 0}
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        assert len(acts) == 1, acts
        assert acts[0]["type"] == "cycle.activity.created"
        assert json.loads(acts[0]["requested_data"]) == {"cycles_list": []}
        current = json.loads(acts[0]["current_instance"])
        assert current["updated_cycle_issues"] == []
        assert json.loads(current["created_cycle_issues"]) == []
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid], cycle_ids=[cid])


def test_completed_cycle_is_refused(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk done cycle")
    cid = _insert_cycle(db_conn, seed["ws"], pid, seed["admin"], _unique("BD done"), done=True)
    try:
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"cycle_id": cid}},
        )
        assert resp.status_code == 400, resp.text
        assert resp.json() == {"error":
            "The Cycle has already been completed so no new issues can be added"}
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid], cycle_ids=[cid])


def test_guest_is_refused(clients, seed):
    guest = clients["guest"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = guest.post(
        _op_url(ws, pid),
        json={"issue_ids": [seed["issue1"]],
              "properties": {"priority": "high"}})
    assert resp.status_code == 403, resp.text
    assert resp.json() == DENIED


def test_outsider_is_refused(clients, seed):
    outsider = clients["outsider"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = outsider.post(
        _op_url(ws, pid),
        json={"issue_ids": [seed["issue1"]],
              "properties": {"priority": "high"}})
    assert resp.status_code == 403, resp.text
    assert resp.json() == DENIED


def test_unauthenticated_is_401(seed, base_url):
    import httpx

    ws, pid = seed["ws_slug"], seed["project"]
    with httpx.Client(base_url=base_url, timeout=10) as anon:
        resp = anon.post(
            _op_url(ws, pid),
            json={"issue_ids": [seed["issue1"]],
                  "properties": {"priority": "high"}})
    assert resp.status_code == 401, resp.text
    assert resp.json() == UNAUTH


def test_serializer_error_shapes(clients, seed, db_conn):
    # Exact DRF bodies for the bulk validators (probed live on Django).
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk shapes")
    try:
        url = _op_url(ws, pid)
        cases = [
            ({}, {"issue_ids": ["This field is required."],
                  "properties": ["This field is required."]}),
            ({"issue_ids": [iid]},
             {"properties": ["This field is required."]}),
            ({"properties": {"priority": "high"}},
             {"issue_ids": ["This field is required."]}),
            ({"issue_ids": None, "properties": {"priority": "high"}},
             {"issue_ids": ["This field may not be null."]}),
            ({"issue_ids": "nope", "properties": {"priority": "high"}},
             {"issue_ids": ['Expected a list of items but got type "str".']}),
            ({"issue_ids": [iid, "nope"], "properties": {"priority": "high"}},
             {"issue_ids": {"1": ["Must be a valid UUID."]}}),
            ({"issue_ids": [iid], "properties": None},
             {"properties": ["This field may not be null."]}),
            ({"issue_ids": [iid], "properties": "str"},
             {"properties": {"non_field_errors": [
                 "Invalid data. Expected a dictionary, but got str."]}}),
            ({"issue_ids": [iid], "properties": {"priority": "bogus"}},
             {"properties": {"priority": ['"bogus" is not a valid choice.']}}),
            ({"issue_ids": [iid], "properties": {"priority": None}},
             {"properties": {"priority": ["This field may not be null."]}}),
            ({"issue_ids": [iid], "properties": {"state_id": "nope"}},
             {"properties": {"state_id": ["Must be a valid UUID."]}}),
            ({"issue_ids": [iid], "properties": {"start_date": "11/10/2026"}},
             {"properties": {"start_date": [
                 "Date has wrong format. Use one of these formats instead:"
                 " YYYY-MM-DD."]}}),
            ({"issue_ids": [iid],
              "properties": {"start_date": "2026-11-10",
                             "target_date": "2026-11-01"}},
             {"properties": {"non_field_errors": [
                 "Start date cannot exceed target date"]}}),
        ]
        for payload, expected in cases:
            resp = member.post(url, json=payload)
            assert resp.status_code == 400, (payload, resp.text)
            assert resp.json() == expected, (payload, resp.text)
        resp = member.post(url, content=b"[1]",
                           headers={"Content-Type": "application/json"})
        assert resp.status_code == 400, resp.text
        assert resp.json() == {"non_field_errors": [
            "Invalid data. Expected a dictionary, but got list."]}
        resp = member.post(url, content=b"null",
                           headers={"Content-Type": "application/json"})
        assert resp.status_code == 400, resp.text
        assert resp.json() == {"non_field_errors": ["No data provided"]}
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid])


def test_tenant_isolation(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk tenant")
    try:
        # Foreign issues are silently skipped, never touched.
        foreign_before = _issue_row(db_conn, seed["other_issue"])
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid, seed["other_issue"]],
                  "properties": {"priority": "low"}},
        )
        assert resp.status_code == 200, resp.text
        assert resp.json() == {"message": "1 issues were updated", "updated": 1}
        assert _issue_row(db_conn, seed["other_issue"]) == foreign_before
        # A cycle from another project does not resolve here.
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT id FROM projects WHERE workspace_id = %s AND id != %s"
                " LIMIT 1",
                (seed["ws"], pid),
            )
            other_pid = str(cur.fetchone()[0])
        other_cycle = _insert_cycle(
            db_conn, seed["ws"], other_pid, seed["admin"], _unique("BT foreign"))
        try:
            resp = member.post(
                _op_url(ws, pid),
                json={"issue_ids": [iid],
                      "properties": {"cycle_id": other_cycle}},
            )
            assert resp.status_code == 400, resp.text
            assert resp.json() == {"error":
                f"Cycle {other_cycle} does not exist in this project"}
        finally:
            with db_conn.cursor() as cur:
                cur.execute("DELETE FROM cycles WHERE id = %s", (other_cycle,))
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid])


def test_deleted_project_is_404(clients, seed, db_conn):
    from .conftest import PROJECT_COLUMNS, _project_row

    member = clients["member"]
    ws = seed["ws_slug"]
    tpid = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO projects (%s) VALUES (%s)" % (
                PROJECT_COLUMNS, ",".join(["%s"] * 30)),
            _project_row(tpid, _unique("Bulk gone"),
                         f"BG{uuid.uuid4().hex[:6]}", seed["ws"], True),
        )
        cur.execute(
            "INSERT INTO project_members (created_at, updated_at, id, role,"
            " member_id, project_id, workspace_id, view_props, default_props,"
            " sort_order, preferences, is_active)"
            " VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (str(uuid.uuid4()), 15, seed["member"], tpid, seed["ws"],
             "{}", "{}", 0.0, "{}", True),
        )
        cur.execute("UPDATE projects SET deleted_at = now() WHERE id = %s", (tpid,))
    try:
        resp = member.post(
            _op_url(ws, tpid),
            json={"issue_ids": [seed["issue1"]],
                  "properties": {"priority": "high"}})
        assert resp.status_code == 404, resp.text
        assert resp.json() == PROJECT_404
        resp = member.post(
            _sub_url(ws, tpid), json={"issue_ids": [seed["issue1"]]})
        assert resp.status_code == 404, resp.text
        assert resp.json() == PROJECT_404
    finally:
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM project_members WHERE project_id = %s", (tpid,))
            cur.execute("DELETE FROM projects WHERE id = %s", (tpid,))


def test_deleted_state_is_accepted(clients, seed, db_conn):
    # The state check uses all_state_objects (soft-deleted rows count).
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk del state")
    doomed = str(uuid.uuid4())
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO states (created_at, updated_at, id, name,"
                " description, color, slug, project_id, workspace_id, sequence,"
                " \"group\", \"default\", is_triage)"
                " VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (doomed, _unique("Doomed"), "", "#000000", _unique("doomed").lower().replace(" ", "-"), pid, seed["ws"],
                 0.0, "backlog", False, False),
            )
            cur.execute("UPDATE states SET deleted_at = now() WHERE id = %s",
                        (doomed,))
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"state_id": doomed}},
        )
        assert resp.status_code == 200, resp.text
        assert _issue_row(db_conn, iid)["state_id"] == doomed
    finally:
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid])
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM states WHERE id = %s", (doomed,))


def test_staying_cycle_still_fires_created(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk staying")
    cid = _insert_cycle(db_conn, seed["ws"], pid, seed["admin"], _unique("BS stay"))
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO cycle_issues (created_at, updated_at, id,"
                " created_by_id, issue_id, cycle_id, project_id, updated_by_id,"
                " workspace_id) VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s)",
                (str(uuid.uuid4()), seed["admin"], iid, cid, pid,
                 seed["admin"], seed["ws"]),
            )
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"cycle_id": cid}},
        )
        assert resp.status_code == 200, resp.text
        assert _live_count(db_conn, "cycle_issues", iid) == 1
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        created = [a for a in acts if a["type"] == "cycle.activity.created"]
        assert len(created) == 1, acts
        current = json.loads(created[0]["current_instance"])
        assert current["updated_cycle_issues"] == [], current
        assert json.loads(current["created_cycle_issues"]) == [], current
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid], cycle_ids=[cid])


def test_module_readd_fires_no_module_activity(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk readd")
    mid = _insert_module(db_conn, seed["ws"], pid, seed["admin"], _unique("BR M"))
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO module_issues (created_at, updated_at, id,"
                " created_by_id, issue_id, module_id, project_id, updated_by_id,"
                " workspace_id) VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s)",
                (str(uuid.uuid4()), seed["admin"], iid, mid, pid,
                 seed["admin"], seed["ws"]),
            )
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"module_ids": [mid]}},
        )
        assert resp.status_code == 200, resp.text
        assert _live_count(db_conn, "module_issues", iid) == 1
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        assert [a["type"] for a in acts] == ["issue.activity.updated"], acts
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid], module_ids=[mid])


def test_empty_arrays_are_noop_but_touch_updated(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(member, ws, pid, "Bulk noop")
    try:
        before = _issue_row(db_conn, iid)["updated_at"]
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _op_url(ws, pid),
            json={"issue_ids": [iid], "properties": {"label_ids": []}},
        )
        assert resp.status_code == 200, resp.text
        assert _issue_row(db_conn, iid)["updated_at"] > before
        acts = _activity_kwargs(db_conn, baseline, seed["member"])
        assert len(acts) == 1, acts
        assert json.loads(acts[0]["requested_data"]) == {}
        assert json.loads(acts[0]["current_instance"]) == {}
    finally:
        _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid])


def test_member_subscribes_to_selection(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    made = [_make_issue(member, ws, pid, f"Bulk sub {n}") for n in ("a", "b")]
    try:
        resp = member.post(_sub_url(ws, pid), json={"issue_ids": made})
        assert resp.status_code == 200, resp.text
        assert resp.json() == {"message": "Subscribed to 2 issues",
                               "subscribed": 2}
        with db_conn.cursor() as cur:
            for iid in made:
                cur.execute(
                    "SELECT COUNT(*) FROM issue_subscribers WHERE issue_id = %s"
                    " AND subscriber_id = %s AND deleted_at IS NULL",
                    (iid, seed["member"]),
                )
                assert cur.fetchone()[0] == 1, iid
    finally:
        for iid in made:
            _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=made)


def test_resubscribe_is_idempotent(clients, seed, db_conn):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    made = [_make_issue(member, ws, pid, f"Bulk resub {n}") for n in ("a", "b")]
    try:
        url = _sub_url(ws, pid)
        assert member.post(url, json={"issue_ids": made}).status_code == 200
        resp = member.post(url, json={"issue_ids": made})
        assert resp.status_code == 200, resp.text
        assert resp.json() == {"message": "Subscribed to 0 issues",
                               "subscribed": 0}
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT COUNT(*) FROM issue_subscribers WHERE issue_id = ANY(%s)"
                " AND subscriber_id = %s AND deleted_at IS NULL",
                (made, seed["member"]),
            )
            assert cur.fetchone()[0] == 2
        # Unknown ids are skipped, not errors.
        resp = member.post(
            url, json={"issue_ids": [made[0], str(uuid.uuid4())]})
        assert resp.status_code == 200, resp.text
        assert resp.json()["subscribed"] == 0
    finally:
        for iid in made:
            _delete_issue(member, ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=made)


def test_guest_can_subscribe(clients, seed, db_conn):
    guest = clients["guest"]
    ws, pid = seed["ws_slug"], seed["project"]
    iid = _make_issue(clients["admin"], ws, pid, "Bulk guest sub")
    try:
        resp = guest.post(_sub_url(ws, pid), json={"issue_ids": [iid]})
        assert resp.status_code == 200, resp.text
        assert resp.json() == {"message": "Subscribed to 1 issues",
                               "subscribed": 1}
    finally:
        _delete_issue(clients["admin"], ws, pid, iid)
        _wipe_test_rows(db_conn, seed["ws"], issue_ids=[iid])


def test_subscribe_outsider_is_refused(clients, seed):
    outsider = clients["outsider"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = outsider.post(_sub_url(ws, pid), json={"issue_ids": [seed["issue1"]]})
    assert resp.status_code == 403, resp.text
    assert resp.json() == DENIED


def test_subscribe_empty_selection_is_rejected(clients, seed):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.post(_sub_url(ws, pid), json={"issue_ids": []})
    assert resp.status_code == 400, resp.text
    assert resp.json() == {
        "issue_ids": ["Ensure this field has at least 1 elements."]}
    resp = member.post(_sub_url(ws, pid), json={})
    assert resp.status_code == 400, resp.text
    assert resp.json() == {"issue_ids": ["This field is required."]}
