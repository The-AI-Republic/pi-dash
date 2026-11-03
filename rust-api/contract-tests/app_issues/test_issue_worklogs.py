# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-issue worklog entries (PIDASHCONV-1676).

Ported from ``apps/api/pi_dash/tests/contract/app/test_issue_worklogs.py``
(rust-dev, NEWFRONT-672, ISS-206 cloud leg): reads are open to every
active project member; logging is for project admins and assignees on
live, non-intake items (guests never); edits and removals are
admin-or-author. Every write enqueues an ``issue_activity`` job whose
``worklog.activity.*`` row the feed reads through its own key.

The Django suite mocks Celery (``no_celery.delay``) and calls the
activity builders directly (``TestWorklogActivities`` /
``TestWorklogUnits``). No worker runs in HTTP suites, so the enqueue is
observed directly instead: Django ``.delay()``s onto the Celery broker,
Rust serve INSERTs into ``rust_job_queue`` (each helper reads both; only
the active backend has rows — the ``test_issue_bulk.py`` pattern). The
builder row outputs (field/verb/minutes) are pinned by the services
unit tests next to ``plan_create/update/delete_worklog``; this file pins
the payloads the worker derives them from (``requested_data`` /
``current_instance``), plus the ``__str__``-free HTTP surface
(Django's ``TestWorklogUnits.test_entry_string_*`` has no HTTP
equivalent and ``_can_log`` is covered by the role matrix below).

Seed notes: the member is assigned to I1 (the loggable issue); I2 has
no assignees (the refusal issue). Tests that create entries remove them
again (admin DELETE in a finally), so the list assertions stay exact.
"""

import json
import os
import uuid
from datetime import datetime, timezone

import pytest

pytestmark = pytest.mark.contract

ISSUE_TASK = "pi_dash.bgtasks.issue_activities_task.issue_activity"

WORKLOG_KEYS = {
    "id", "actor_detail", "created_at", "updated_at", "deleted_at",
    "minutes", "logged_on", "note", "created_by", "updated_by",
    "project", "workspace", "issue", "actor",
}

ACTOR_KEYS = {
    "id", "first_name", "last_name", "avatar", "avatar_url",
    "is_bot", "display_name",
}

FORBIDDEN = {"error": "You don't have the required permissions."}
NOT_LOGGER = {"error": "Only admins and assignees can log time on this work item."}
INTAKE_REFUSAL = {"error": "Time cannot be logged on an intake item."}
ENTRY_NOT_FOUND = {"detail": "No IssueWorklog matches the given query."}
OBJECT_NOT_FOUND = {"error": "The required object does not exist."}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _coll(ws, pid, iid):
    return f"{_base(ws, pid)}/issues/{iid}/worklogs/"


def _now():
    return datetime.now(timezone.utc)


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


def _worklog_kwargs(db_conn, baseline, actor_id):
    """New worklog issue_activity kwargs on either transport since baseline."""
    out = []
    for headers, payload in _drain_broker():
        if (headers or {}).get("task") != ISSUE_TASK:
            continue
        kwargs = payload[1] if isinstance(payload, list) and len(payload) > 1 else {}
        if (isinstance(kwargs, dict)
                and str(kwargs.get("type", "")).startswith("worklog.activity.")
                and str(kwargs.get("actor_id")) == str(actor_id)):
            out.append(kwargs)
    for row in _rust_rows(db_conn, ISSUE_TASK, baseline):
        kwargs = row["kwargs"]
        if (str(kwargs.get("type", "")).startswith("worklog.activity.")
                and str(kwargs.get("actor_id")) == str(actor_id)):
            out.append(kwargs)
    return out


def _reset_transports(db_conn):
    """Discard pending broker messages; return the rust queue baseline."""
    _drain_broker()
    return _queue_baseline(db_conn)


def _cleanup_entry(admin, url, entry_id):
    try:
        admin.request("DELETE", f"{url}{entry_id}/")
    except Exception:
        pass


# ---------------------------------------------------------------------------
# Reads
# ---------------------------------------------------------------------------


def test_member_lists_entries_newest_first(clients, seed):
    admin, member = clients["admin"], clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    created = []
    try:
        for minutes, day in ((30, "2026-10-19"), (90, "2026-10-20")):
            resp = admin.post(url, json={"minutes": minutes, "logged_on": day})
            assert resp.status_code == 201, resp.text
            created.append(resp.json()["id"])
        resp = member.get(url)
        assert resp.status_code == 200
        rows = resp.json()
        assert [row["id"] for row in rows] == [created[1], created[0]]
        assert set(rows[0]) == WORKLOG_KEYS
        assert rows[0]["minutes"] == 90
        assert rows[0]["logged_on"] == "2026-10-20"
        assert set(rows[0]["actor_detail"]) == ACTOR_KEYS
        assert rows[0]["actor_detail"]["id"] == seed["admin"]
        assert rows[0]["actor"] == seed["admin"]
    finally:
        for entry_id in created:
            _cleanup_entry(admin, url, entry_id)


def test_guest_lists_entries(clients, seed):
    admin, guest = clients["admin"], clients["guest"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    resp = admin.post(url, json={"minutes": 30, "logged_on": "2026-10-20"})
    assert resp.status_code == 201, resp.text
    entry_id = resp.json()["id"]
    try:
        resp = guest.get(url)
        assert resp.status_code == 200
        assert [row["id"] for row in resp.json()] == [entry_id]
    finally:
        _cleanup_entry(admin, url, entry_id)


def test_outsider_lists_nothing(clients, seed):
    admin, outsider = clients["admin"], clients["outsider"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    resp = admin.post(url, json={"minutes": 30, "logged_on": "2026-10-20"})
    assert resp.status_code == 201, resp.text
    entry_id = resp.json()["id"]
    try:
        resp = outsider.get(url)
        assert resp.status_code == 200
        assert resp.json() == []
    finally:
        _cleanup_entry(admin, url, entry_id)


def test_member_retrieves_single_entry(clients, seed):
    admin, member = clients["admin"], clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    resp = admin.post(
        url, json={"minutes": 30, "logged_on": "2026-10-20", "note": "Deep work"})
    assert resp.status_code == 201, resp.text
    entry_id = resp.json()["id"]
    try:
        resp = member.get(f"{url}{entry_id}/")
        assert resp.status_code == 200
        body = resp.json()
        assert set(body) == WORKLOG_KEYS
        assert body["id"] == entry_id
        assert body["note"] == "Deep work"
    finally:
        _cleanup_entry(admin, url, entry_id)


def test_outsider_retrieve_is_not_found(clients, seed):
    admin, outsider = clients["admin"], clients["outsider"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    resp = admin.post(url, json={"minutes": 30, "logged_on": "2026-10-20"})
    assert resp.status_code == 201, resp.text
    entry_id = resp.json()["id"]
    try:
        resp = outsider.get(f"{url}{entry_id}/")
        assert resp.status_code == 404
        assert resp.json() == ENTRY_NOT_FOUND
    finally:
        _cleanup_entry(admin, url, entry_id)


# ---------------------------------------------------------------------------
# Create
# ---------------------------------------------------------------------------


def test_assigned_member_logs_time(clients, seed, db_conn):
    member = clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    baseline = _reset_transports(db_conn)
    resp = member.post(
        url, json={"minutes": 90, "logged_on": "2026-10-20", "note": "Deep work"})
    assert resp.status_code == 201, resp.text
    body = resp.json()
    try:
        assert set(body) == WORKLOG_KEYS
        assert body["minutes"] == 90
        assert body["logged_on"] == "2026-10-20"
        assert body["note"] == "Deep work"
        assert body["actor"] == seed["member"]
        assert body["actor_detail"]["id"] == seed["member"]
        assert body["created_by"] == seed["member"]
        assert body["updated_by"] is None
        assert body["issue"] == i1
        (kwargs,) = _worklog_kwargs(db_conn, baseline, seed["member"])
        assert kwargs["type"] == "worklog.activity.created"
        requested = json.loads(kwargs["requested_data"])
        assert requested["minutes"] == 90
        assert requested["id"] == body["id"]
        assert kwargs["current_instance"] is None
        assert kwargs["issue_id"] == i1
        assert kwargs["project_id"] == pid
        assert kwargs["notification"] is True
    finally:
        _cleanup_entry(clients["admin"], url, body["id"])


def test_admin_logs_time_without_assignment(clients, seed, db_conn):
    admin = clients["admin"]
    ws, pid, i2 = seed["ws_slug"], seed["project"], seed["issue2"]
    url = _coll(ws, pid, i2)
    baseline = _reset_transports(db_conn)
    resp = admin.post(url, json={"minutes": 30, "logged_on": "2026-10-20"})
    assert resp.status_code == 201, resp.text
    body = resp.json()
    try:
        assert body["note"] == ""
        assert body["actor"] == seed["admin"]
        (kwargs,) = _worklog_kwargs(db_conn, baseline, seed["admin"])
        assert kwargs["type"] == "worklog.activity.created"
    finally:
        _cleanup_entry(admin, url, body["id"])


def test_unassigned_member_is_refused(clients, seed, db_conn):
    member = clients["member"]
    ws, pid, i2 = seed["ws_slug"], seed["project"], seed["issue2"]
    baseline = _reset_transports(db_conn)
    resp = member.post(
        _coll(ws, pid, i2), json={"minutes": 30, "logged_on": "2026-10-20"})
    assert resp.status_code == 403
    assert resp.json() == NOT_LOGGER
    assert _worklog_kwargs(db_conn, baseline, seed["member"]) == []


def test_guest_is_refused_even_when_assigned(clients, seed, db_conn):
    admin, guest = clients["admin"], clients["guest"]
    ws, pid, i2 = seed["ws_slug"], seed["project"], seed["issue2"]
    assignee_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO issue_assignees (created_at, updated_at, id,"
            " assignee_id, created_by_id, issue_id, project_id,"
            " updated_by_id, workspace_id)"
            " SELECT %s,%s,%s,%s,%s,%s,%s,%s,workspace_id FROM issues WHERE id=%s",
            (_now(), _now(), assignee_id, seed["guest"], seed["admin"],
             i2, pid, seed["admin"], i2),
        )
    try:
        baseline = _reset_transports(db_conn)
        resp = guest.post(
            _coll(ws, pid, i2), json={"minutes": 30, "logged_on": "2026-10-20"})
        assert resp.status_code == 403
        assert resp.json() == FORBIDDEN
        assert _worklog_kwargs(db_conn, baseline, seed["guest"]) == []
    finally:
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM issue_assignees WHERE id = %s", (assignee_id,))


def test_outsider_is_refused(clients, seed, db_conn):
    outsider = clients["outsider"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    baseline = _reset_transports(db_conn)
    resp = outsider.post(
        _coll(ws, pid, i1), json={"minutes": 30, "logged_on": "2026-10-20"})
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN
    assert _worklog_kwargs(db_conn, baseline, seed["outsider"]) == []


def test_intake_issue_is_refused(clients, seed, db_conn):
    member = clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    intake_id, link_id = str(uuid.uuid4()), str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO intakes (created_at, updated_at, id, name,"
            " description, is_default, view_props, created_by_id, project_id,"
            " updated_by_id, workspace_id, logo_props)"
            " SELECT %s,%s,%s,'Requests','',false,'{}',%s,%s,%s,workspace_id,'{}'"
            " FROM issues WHERE id=%s",
            (_now(), _now(), intake_id, seed["admin"], pid, seed["admin"], i1),
        )
        cur.execute(
            "INSERT INTO intake_issues (created_at, updated_at, id, status,"
            " created_by_id, intake_id, issue_id, project_id, updated_by_id,"
            " workspace_id, extra)"
            " SELECT %s,%s,%s,-2,%s,%s,%s,%s,%s,workspace_id,'{}'"
            " FROM issues WHERE id=%s",
            (_now(), _now(), link_id, seed["admin"], intake_id, i1,
             pid, seed["admin"], i1),
        )
    try:
        baseline = _reset_transports(db_conn)
        resp = member.post(
            _coll(ws, pid, i1), json={"minutes": 30, "logged_on": "2026-10-20"})
        assert resp.status_code == 400
        assert resp.json() == INTAKE_REFUSAL
        assert _worklog_kwargs(db_conn, baseline, seed["member"]) == []
    finally:
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM intake_issues WHERE id = %s", (link_id,))
            cur.execute("DELETE FROM intakes WHERE id = %s", (intake_id,))


def test_invalid_payloads_are_refused(clients, seed, db_conn):
    member = clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    baseline = _reset_transports(db_conn)
    cases = [
        ({"minutes": 0, "logged_on": "2026-10-20"},
         {"minutes": ["Ensure this value is greater than or equal to 1."]}),
        ({"minutes": -30, "logged_on": "2026-10-20"},
         {"minutes": ["Ensure this value is greater than or equal to 1."]}),
        ({"minutes": 2 ** 40, "logged_on": "2026-10-20"},
         {"minutes": ["Ensure this value is less than or equal to 2147483647."]}),
        ({"minutes": "abc", "logged_on": "2026-10-20"},
         {"minutes": ["A valid integer is required."]}),
        ({"logged_on": "2026-10-20"},
         {"minutes": ["This field is required."]}),
        ({"minutes": 30, "logged_on": "soon"},
         {"logged_on": ["Date has wrong format. Use one of these formats"
                        " instead: YYYY-MM-DD."]}),
        ({"minutes": 30},
         {"logged_on": ["This field is required."]}),
        ({"minutes": 30, "logged_on": "2026-10-20", "note": None},
         {"note": ["This field may not be null."]}),
    ]
    for payload, expected in cases:
        resp = member.post(url, json=payload)
        assert resp.status_code == 400, payload
        assert resp.json() == expected, payload
    assert _worklog_kwargs(db_conn, baseline, seed["member"]) == []


def test_actor_is_forced_to_the_caller(clients, seed):
    admin, member = clients["admin"], clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    resp = member.post(url, json={"minutes": 30, "logged_on": "2026-10-20",
                                  "actor": seed["admin"]})
    assert resp.status_code == 201, resp.text
    body = resp.json()
    try:
        assert body["actor"] == seed["member"]
        assert body["actor_detail"]["id"] == seed["member"]
    finally:
        _cleanup_entry(admin, url, body["id"])


def test_create_on_missing_issue_is_not_found(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        _coll(ws, pid, str(uuid.uuid4())),
        json={"minutes": 30, "logged_on": "2026-10-20"})
    assert resp.status_code == 404
    assert resp.json() == OBJECT_NOT_FOUND


# ---------------------------------------------------------------------------
# Update / delete
# ---------------------------------------------------------------------------


def _extra_member_client(db_conn, base_url, seed):
    """A second MEMBER of the project (Django's ``other_member_user``).

    Returns ``(client, user_id, cleanup)``; the caller runs ``cleanup()``
    to remove the seeded user, memberships and session rows.
    """
    from _harness import seed as H
    from _harness.auth import authed_client, forge_session_key, get_secret_key

    user = H.user(db_conn, "is-extra-member")
    uid = str(user["id"])
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT id FROM workspaces WHERE slug = %s", (seed["ws_slug"],))
        ws_id = str(cur.fetchone()[0])
    H.workspace_member(db_conn, ws_id, uid, role=15)
    H.project_member(db_conn, seed["project"], ws_id, uid, role=15)
    with db_conn.cursor() as cur:
        cur.execute("SELECT password FROM users WHERE id = %s", (uid,))
        pw = cur.fetchone()[0]
    skey = forge_session_key(
        db_conn, user_id=uid, password_field=pw, secret=get_secret_key())
    client = authed_client(base_url, skey)

    def cleanup():
        client.close()
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM sessions WHERE session_key = %s", (skey,))
            cur.execute(
                "DELETE FROM project_members WHERE member_id = %s", (uid,))
            cur.execute(
                "DELETE FROM workspace_members WHERE member_id = %s", (uid,))
            cur.execute("DELETE FROM users WHERE id = %s", (uid,))

    return client, uid, cleanup


def _member_entry(clients, seed):
    """A member-authored entry on I1; returns ``(url, entry_id)``."""
    member = clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = _coll(ws, pid, i1)
    resp = member.post(url, json={"minutes": 90, "logged_on": "2026-10-20"})
    assert resp.status_code == 201, resp.text
    return url, resp.json()["id"]


def test_author_edits_own_entry(clients, seed, db_conn):
    admin, member = clients["admin"], clients["member"]
    url, entry_id = _member_entry(clients, seed)
    try:
        baseline = _reset_transports(db_conn)
        resp = member.patch(f"{url}{entry_id}/",
                            json={"minutes": 120, "note": "Still going"})
        assert resp.status_code == 200, resp.text
        body = resp.json()
        assert body["minutes"] == 120
        assert body["note"] == "Still going"
        assert body["updated_by"] == seed["member"]
        (kwargs,) = _worklog_kwargs(db_conn, baseline, seed["member"])
        assert kwargs["type"] == "worklog.activity.updated"
        assert json.loads(kwargs["requested_data"])["minutes"] == 120
        current = json.loads(kwargs["current_instance"])
        assert current["minutes"] == 90
        assert current["id"] == entry_id
    finally:
        _cleanup_entry(admin, url, entry_id)


def test_admin_edits_foreign_entry(clients, seed, db_conn):
    admin = clients["admin"]
    url, entry_id = _member_entry(clients, seed)
    try:
        baseline = _reset_transports(db_conn)
        resp = admin.patch(f"{url}{entry_id}/", json={"note": "Admin correction"})
        assert resp.status_code == 200, resp.text
        assert resp.json()["note"] == "Admin correction"
        (kwargs,) = _worklog_kwargs(db_conn, baseline, seed["admin"])
        assert kwargs["type"] == "worklog.activity.updated"
        # Minutes untouched: the handler falls back to the before-image.
        assert "minutes" not in json.loads(kwargs["requested_data"])
        assert json.loads(kwargs["current_instance"])["minutes"] == 90
    finally:
        _cleanup_entry(admin, url, entry_id)


def test_other_member_cannot_edit(clients, seed, db_conn, base_url):
    admin = clients["admin"]
    url, entry_id = _member_entry(clients, seed)
    other, _, cleanup = _extra_member_client(db_conn, base_url, seed)
    try:
        baseline = _reset_transports(db_conn)
        resp = other.patch(f"{url}{entry_id}/", json={"note": "Hijack"})
        assert resp.status_code == 403
        assert resp.json() == FORBIDDEN
    finally:
        cleanup()
        _cleanup_entry(admin, url, entry_id)


def test_guest_cannot_edit(clients, seed, db_conn):
    admin, guest = clients["admin"], clients["guest"]
    url, entry_id = _member_entry(clients, seed)
    try:
        baseline = _reset_transports(db_conn)
        resp = guest.patch(f"{url}{entry_id}/", json={"note": "Hijack"})
        assert resp.status_code == 403
        assert resp.json() == FORBIDDEN
        assert _worklog_kwargs(db_conn, baseline, seed["guest"]) == []
    finally:
        _cleanup_entry(admin, url, entry_id)


def test_author_removes_own_entry(clients, seed, db_conn):
    admin, member = clients["admin"], clients["member"]
    url, entry_id = _member_entry(clients, seed)
    baseline = _reset_transports(db_conn)
    resp = member.delete(f"{url}{entry_id}/")
    assert resp.status_code == 204, resp.text
    try:
        assert member.get(f"{url}{entry_id}/").status_code == 404
        (kwargs,) = _worklog_kwargs(db_conn, baseline, seed["member"])
        assert kwargs["type"] == "worklog.activity.deleted"
        assert json.loads(kwargs["requested_data"]) == {"worklog_id": entry_id}
        assert json.loads(kwargs["current_instance"])["minutes"] == 90
    finally:
        _cleanup_entry(admin, url, entry_id)


def test_admin_removes_foreign_entry(clients, seed, db_conn):
    admin = clients["admin"]
    url, entry_id = _member_entry(clients, seed)
    baseline = _reset_transports(db_conn)
    resp = admin.delete(f"{url}{entry_id}/")
    assert resp.status_code == 204, resp.text
    try:
        assert admin.get(f"{url}{entry_id}/").status_code == 404
        (kwargs,) = _worklog_kwargs(db_conn, baseline, seed["admin"])
        assert kwargs["type"] == "worklog.activity.deleted"
    finally:
        _cleanup_entry(admin, url, entry_id)


def test_other_member_cannot_remove(clients, seed, db_conn, base_url):
    admin, member = clients["admin"], clients["member"]
    url, entry_id = _member_entry(clients, seed)
    other, _, cleanup = _extra_member_client(db_conn, base_url, seed)
    try:
        resp = other.delete(f"{url}{entry_id}/")
        assert resp.status_code == 403
        assert resp.json() == FORBIDDEN
        assert member.get(f"{url}{entry_id}/").status_code == 200
    finally:
        cleanup()
        _cleanup_entry(admin, url, entry_id)


def test_patch_missing_entry_is_not_found(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.patch(f"{_coll(ws, pid, i1)}{uuid.uuid4()}/",
                       json={"note": "x"})
    assert resp.status_code == 404
    assert resp.json() == OBJECT_NOT_FOUND


def test_delete_missing_entry_is_not_found(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.request("DELETE", f"{_coll(ws, pid, i1)}{uuid.uuid4()}/")
    assert resp.status_code == 404
    assert resp.json() == OBJECT_NOT_FOUND
