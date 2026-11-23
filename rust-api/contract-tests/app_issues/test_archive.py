"""Archive + bulk-operation shapes.

Seed: I5 is archived (completed state). The archive round-trip test
archives I3 and unarchives it again; bulk tests use API-created
throwaways so the seed is untouched.
"""

import json
import os
import time

from _harness.diffs import expect, is_rust

ARCHIVE_DETAIL_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "complexity_score", "start_date",
    "target_date", "sequence_id", "project_id", "parent_id",
    "type_id", "is_epic",
    "assigned_pod_id", "assigned_runner_id", "agent_executor", "created_at", "updated_at",
    "created_by", "updated_by", "is_draft", "archived_at", "is_synced",
    "description_html", "is_subscribed", "agent_ticker", "agent_status",
    "relations_summary", "has_open_blockers", "assigned_runner_detail",
}

FORBIDDEN = {"error": "You don't have the required permissions."}

ISSUE_TASK = "pi_dash.bgtasks.issue_activities_task.issue_activity"


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _drain_broker():
    """Pending Celery-protocol messages (Django's transport; [] without one)."""
    if not os.environ.get("CELERY_BROKER_URL"):
        return []
    from _harness import broker_probe

    # Drain to empty: the matrix runs workerless, so every earlier test's
    # .delay() piles up on the broker and the default 100-message cap
    # would stop short of this test's job (oldest first).
    return broker_probe.drain_queue(limit=10000)


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


def _reset_transports(db_conn):
    """Discard pending broker messages; return the rust queue baseline."""
    _drain_broker()
    return _queue_baseline(db_conn)


def _archived_activity_kwargs(db_conn, baseline, actor_id):
    """New ``issue.activity.updated`` enqueues carrying ``archived_at``."""
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
    return [
        kwargs for kwargs in out
        if "archived_at" in json.loads(kwargs["requested_data"])
    ]


def test_archived_list_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    resp = admin.get(f"{_base(ws, pid)}/archived-issues/")
    assert resp.status_code == 200
    body = resp.json()
    assert i5 in {row["id"] for row in body["results"]}
    row = next(r for r in body["results"] if r["id"] == i5)
    assert row["name"] == "I5 archived"
    assert row["archived_at"] is not None


def test_archived_reads_guest_allowed(clients, seed):
    # PDASHOSS01-323: guests read the archives. IS has
    # guest_view_all_features on, so they see admin-created I5 too.
    guest = clients["guest"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    resp = guest.get(f"{_base(ws, pid)}/archived-issues/")
    assert resp.status_code == 200, resp.text
    assert i5 in {row["id"] for row in resp.json()["results"]}
    resp = guest.get(f"{_base(ws, pid)}/issues/{i5}/archive/")
    assert resp.status_code == 200, resp.text
    assert set(resp.json()) == ARCHIVE_DETAIL_KEYS


def test_archived_reads_guest_scoping_without_view_all(clients, seed,
                                                       db_conn):
    # IS2 has guest_view_all_features off: like the live list/detail,
    # guests see only the archived issues they created.
    admin, guest = clients["admin"], clients["guest"]
    ws, p2, j1, j2 = (
        seed["ws_slug"], seed["project2"], seed["j1"], seed["j2"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET archived_at = CURRENT_DATE"
            " WHERE id IN (%s, %s)", (j1, j2))
    try:
        resp = guest.get(f"{_base(ws, p2)}/archived-issues/")
        assert resp.status_code == 200, resp.text
        assert {row["id"] for row in resp.json()["results"]} == {j2}
        assert resp.json()["total_count"] == 1
        grouped = guest.get(
            f"{_base(ws, p2)}/archived-issues/", params={"group_by": "priority"})
        assert grouped.status_code == 200, grouped.text
        assert grouped.json()["total_count"] == 1
        resp = guest.get(f"{_base(ws, p2)}/issues/{j1}/archive/")
        assert resp.status_code == 403
        assert resp.json() == {
            "error": "You are not allowed to view this issue"}
        resp = guest.get(f"{_base(ws, p2)}/issues/{j2}/archive/")
        assert resp.status_code == 200, resp.text
        assert resp.json()["id"] == j2
        # Admins stay unscoped.
        body = admin.get(f"{_base(ws, p2)}/archived-issues/").json()
        assert {row["id"] for row in body["results"]} == {j1, j2}
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE issues SET archived_at = NULL WHERE id IN (%s, %s)",
                (j1, j2))


def test_archive_writes_guest_denied(clients, seed):
    guest = clients["guest"]
    ws, pid, i3, i5 = (
        seed["ws_slug"], seed["project"], seed["issue3"], seed["issue5"])
    resp = guest.post(f"{_base(ws, pid)}/issues/{i3}/archive/")
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN
    resp = guest.request("DELETE", f"{_base(ws, pid)}/issues/{i5}/archive/")
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN


def test_archive_retrieve_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    resp = admin.get(f"{_base(ws, pid)}/issues/{i5}/archive/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == ARCHIVE_DETAIL_KEYS
    assert body["name"] == "I5 archived"
    assert body["archived_at"] is not None
    assert body["relations_summary"] == {"blocked_by": [], "blocking": []}


def test_archive_roundtrip_on_completed(clients, seed):
    admin = clients["admin"]
    ws, pid, i3, i5 = (
        seed["ws_slug"], seed["project"], seed["issue3"], seed["issue5"])
    resp = admin.post(f"{_base(ws, pid)}/issues/{i3}/archive/")
    assert resp.status_code == 200
    assert set(resp.json()) == {"archived_at"}
    try:
        archived = {
            row["id"] for row in
            admin.get(f"{_base(ws, pid)}/archived-issues/").json()["results"]
        }
        assert {i3, i5} <= archived
    finally:
        assert admin.request(
            "DELETE", f"{_base(ws, pid)}/issues/{i3}/archive/").status_code == 204
    archived = {
        row["id"] for row in
        admin.get(f"{_base(ws, pid)}/archived-issues/").json()["results"]
    }
    assert i3 not in archived
    assert i5 in archived


def test_archive_open_state_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(f"{_base(ws, pid)}/issues/{i1}/archive/")
    assert resp.status_code == 400
    assert resp.json() == {
        "error": "Can only archive completed or cancelled state group issue"}


def test_bulk_archive_roundtrip(clients, seed):
    admin = clients["admin"]
    ws, pid, done = (
        seed["ws_slug"], seed["project"], seed["state_done"])
    made = [
        admin.post(f"{_base(ws, pid)}/issues/",
                   json={"name": f"Bulk {n}"}).json()
        for n in ("a", "b")
    ]
    try:
        for row in made:
            assert admin.patch(
                f"{_base(ws, pid)}/issues/{row['id']}/",
                json={"state_id": done},
            ).status_code == 204
        resp = admin.post(
            f"{_base(ws, pid)}/bulk-archive-issues/",
            json={"issue_ids": [row["id"] for row in made]},
        )
        assert resp.status_code == 200
        assert set(resp.json()) == {"archived_at"}
        archived = {
            row["id"] for row in admin.get(
                f"{_base(ws, pid)}/archived-issues/").json()["results"]
        }
        assert {row["id"] for row in made} <= archived
    finally:
        for row in made:
            admin.request(
                "DELETE", f"{_base(ws, pid)}/issues/{row['id']}/archive/")
            admin.delete(f"{_base(ws, pid)}/issues/{row['id']}/")


def test_bulk_archive_open_state_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(
        f"{_base(ws, pid)}/bulk-archive-issues/", json={"issue_ids": [i1]})
    assert resp.status_code == 400
    assert resp.json() == {
        "error_code": 4091, "error_message": "INVALID_ARCHIVE_STATE_GROUP"}


def test_bulk_archive_empty_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        f"{_base(ws, pid)}/bulk-archive-issues/", json={"issue_ids": []})
    assert resp.status_code == 400
    assert resp.json() == {"error": "Issue IDs are required"}


def test_bulk_delete_roundtrip(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    made = [
        admin.post(f"{_base(ws, pid)}/issues/",
                   json={"name": f"Gone {n}"}).json()
        for n in ("a", "b")
    ]
    resp = admin.request(
        "DELETE", f"{_base(ws, pid)}/bulk-delete-issues/",
        json={"issue_ids": [row["id"] for row in made]},
    )
    assert resp.status_code == 200
    assert resp.json() == {"message": "2 issues were deleted"}
    for row in made:
        assert admin.get(
            f"{_base(ws, pid)}/issues/{row['id']}/").status_code == 404


def test_bulk_delete_empty_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.request(
        "DELETE", f"{_base(ws, pid)}/bulk-delete-issues/",
        json={"issue_ids": []},
    )
    assert resp.status_code == 400
    assert resp.json() == {"error": "Issue IDs are required"}


def test_bulk_delete_member_denied(clients, seed):
    member = clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = member.request(
        "DELETE", f"{_base(ws, pid)}/bulk-delete-issues/",
        json={"issue_ids": [i1]},
    )
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN


def test_archived_list_state_id_in_filter(clients, seed):
    # PIDASHCONV-1317: the archived list runs the same rich-filter
    # compiler; UUID lists must bind typed (pre-fix Rust 500d).
    admin = clients["admin"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    resp = admin.get(
        f"{_base(ws, pid)}/archived-issues/",
        params={"filters": json.dumps(
            {"and": [{"state_id__in": seed["state_done"]}]})},
    )
    assert resp.status_code == 200, resp.text
    assert {row["id"] for row in resp.json()["results"]} == {i5}
    resp = admin.get(
        f"{_base(ws, pid)}/archived-issues/",
        params={"filters": json.dumps(
            {"and": [{"state_id__in": seed["state_backlog"]}]})},
    )
    assert resp.status_code == 200, resp.text
    assert i5 not in {row["id"] for row in resp.json()["results"]}


def test_completed_at_preserved_without_state_change(clients, seed, db_conn):
    """NEWPIDASH-351 (DIFF-62, PB-0306, PB-0286): archiving, unarchiving,
    PATCHing an unrelated field of, and soft-deleting a completed issue
    must not rewrite its ``completed_at`` stamp.

    Django's ``Issue.save()`` restamps ``completed_at`` from the state
    group on every save, so each leg below visibly moves the stamp; the
    Rust save stamps only on transitions into ``completed`` and preserves
    the original stamp here.
    """
    admin = clients["admin"]
    ws, pid, done = seed["ws_slug"], seed["project"], seed["state_done"]
    made = []
    try:
        for name in ("DIFF-62 archive probe", "DIFF-62 patch probe",
                     "DIFF-62 delete probe"):
            row = admin.post(
                f"{_base(ws, pid)}/issues/", json={"name": name}).json()
            assert admin.patch(
                f"{_base(ws, pid)}/issues/{row['id']}/",
                json={"state_id": done}).status_code == 204
            made.append(row["id"])
        aid, bid, did = made
        before_a = admin.get(
            f"{_base(ws, pid)}/issues/{aid}/").json()["completed_at"]
        before_b = admin.get(
            f"{_base(ws, pid)}/issues/{bid}/").json()["completed_at"]
        assert before_a is not None and before_b is not None
        time.sleep(1.2)
        # Archive leg (PB-0306).
        assert admin.post(
            f"{_base(ws, pid)}/issues/{aid}/archive/").status_code == 200
        after_archive = admin.get(
            f"{_base(ws, pid)}/issues/{aid}/archive/").json()["completed_at"]
        assert (after_archive != before_a) == expect(
            "DIFF-62", django=True, rust=False)
        # Unarchive leg (PB-0306).
        assert admin.request(
            "DELETE",
            f"{_base(ws, pid)}/issues/{aid}/archive/").status_code == 204
        after_unarchive = admin.get(
            f"{_base(ws, pid)}/issues/{aid}/").json()["completed_at"]
        assert (after_unarchive != before_a) == expect(
            "DIFF-62", django=True, rust=False)
        # Unrelated PATCH leg (PB-0286).
        assert admin.patch(
            f"{_base(ws, pid)}/issues/{bid}/",
            json={"priority": "high"}).status_code == 204
        after_patch = admin.get(
            f"{_base(ws, pid)}/issues/{bid}/").json()["completed_at"]
        assert (after_patch != before_b) == expect(
            "DIFF-62", django=True, rust=False)
        # Soft-delete leg: the destroy save preserves the stamp too.
        from datetime import datetime
        before_d = admin.get(
            f"{_base(ws, pid)}/issues/{did}/").json()["completed_at"]
        assert admin.delete(
            f"{_base(ws, pid)}/issues/{did}/").status_code == 204
        with db_conn.cursor() as cur:
            cur.execute("SELECT completed_at FROM issues WHERE id = %s",
                        (did,))
            after_delete = cur.fetchone()[0]
        assert (after_delete != datetime.fromisoformat(before_d)) == expect(
            "DIFF-62", django=True, rust=False)
    finally:
        for iid in made:
            admin.request("DELETE", f"{_base(ws, pid)}/issues/{iid}/archive/")
            admin.delete(f"{_base(ws, pid)}/issues/{iid}/")


def test_completed_at_transitions_with_state_group(clients, seed):
    """NEWPIDASH-351 (DIFF-62): moving into a completed-group state stamps
    ``completed_at`` and moving back out clears it — on both backends.
    """
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    backlog, done = seed["state_backlog"], seed["state_done"]
    made = admin.post(
        f"{_base(ws, pid)}/issues/",
        json={"name": "DIFF-62 transition probe"}).json()
    iid = made["id"]
    try:
        assert admin.get(
            f"{_base(ws, pid)}/issues/{iid}/").json()["completed_at"] is None
        assert admin.patch(
            f"{_base(ws, pid)}/issues/{iid}/",
            json={"state_id": done}).status_code == 204
        stamped = admin.get(
            f"{_base(ws, pid)}/issues/{iid}/").json()["completed_at"]
        assert stamped is not None
        assert admin.patch(
            f"{_base(ws, pid)}/issues/{iid}/",
            json={"state_id": backlog}).status_code == 204
        assert admin.get(
            f"{_base(ws, pid)}/issues/{iid}/").json()["completed_at"] is None
    finally:
        admin.delete(f"{_base(ws, pid)}/issues/{iid}/")


def test_archived_grouped_rows_match_ungrouped_shape(clients, seed):
    # PIDASHCONV-1510: Django renders grouped rows through the same
    # issue_on_results dicts as the flat list: sort_order is a JSON
    # number and datetimes end with Z.
    admin = clients["admin"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    flat = next(
        r for r in admin.get(
            f"{_base(ws, pid)}/archived-issues/").json()["results"]
        if r["id"] == i5)
    resp = admin.get(
        f"{_base(ws, pid)}/archived-issues/",
        params={"group_by": "priority"},
    )
    assert resp.status_code == 200, resp.text
    rows = resp.json()["results"]["high"]["results"]
    row = next(r for r in rows if r["id"] == i5)
    assert isinstance(row["sort_order"], float), row["sort_order"]
    assert row["sort_order"] == flat["sort_order"]
    assert row["created_at"] == flat["created_at"]
    assert row["updated_at"] == flat["updated_at"]
    assert row["created_at"].endswith("Z"), row["created_at"]
    assert row["updated_at"].endswith("Z"), row["updated_at"]
    sub = admin.get(
        f"{_base(ws, pid)}/archived-issues/",
        params={"group_by": "priority", "sub_group_by": "state_id"},
    )
    assert sub.status_code == 200, sub.text
    cell = sub.json()["results"]["high"]["results"][seed["state_done"]]
    srow = next(r for r in cell["results"] if r["id"] == i5)
    assert isinstance(srow["sort_order"], float), srow["sort_order"]
    assert srow["sort_order"] == flat["sort_order"]
    assert srow["created_at"] == flat["created_at"]
    assert srow["updated_at"] == flat["updated_at"]


def test_archived_grouped_created_by_buckets_and_totals(clients, seed):
    # NEWPIDASH-350 (DIFF-58, old bug fix, PB-0300/PB-0301): the old
    # archived list 500s on group_by/sub_group_by=created_by (archive
    # passes no queryset to issue_group_values, so None.values_list
    # raises) and counts every group with archived_at__isnull=True. The
    # new list buckets over the archived set like the live list and
    # counts archived rows.
    admin, member, guest = (
        clients["admin"], clients["member"], clients["guest"])
    ws, pid, done = seed["ws_slug"], seed["project"], seed["state_done"]
    admin_id, member_id = seed["admin"], seed["member"]
    made = member.post(
        f"{_base(ws, pid)}/issues/", json={"name": "DIFF-58 member issue"})
    assert made.status_code == 201, made.text
    mid = made.json()["id"]
    try:
        assert member.patch(
            f"{_base(ws, pid)}/issues/{mid}/",
            json={"state_id": done}).status_code == 204
        assert member.post(
            f"{_base(ws, pid)}/issues/{mid}/archive/").status_code == 200
        assert admin.post(
            f"{_base(ws, pid)}/issues/{seed['issue3']}/archive/"
        ).status_code == 200
        # I5 + I3 (admin-created) and M1 (member-created) are archived.
        resp = admin.get(
            f"{_base(ws, pid)}/archived-issues/",
            params={"group_by": "created_by", "per_page": 1},
        )
        assert resp.status_code == expect("DIFF-58", django=500, rust=200)
        if is_rust():
            buckets = resp.json()["results"]
            assert set(buckets) == {admin_id, member_id}, buckets.keys()
            assert buckets[admin_id]["total_results"] == 2
            assert buckets[member_id]["total_results"] == 1
            assert len(buckets[admin_id]["results"]) == 1
            assert sum(
                cell["total_results"] for cell in buckets.values()) == 3
            assert resp.json()["total_count"] == 3
        gresp = guest.get(
            f"{_base(ws, pid)}/archived-issues/",
            params={"group_by": "created_by"},
        )
        assert gresp.status_code == expect("DIFF-58", django=500, rust=200)
        sub = admin.get(
            f"{_base(ws, pid)}/archived-issues/",
            params={"group_by": "priority", "sub_group_by": "created_by"},
        )
        assert sub.status_code == expect("DIFF-58", django=500, rust=200)
        if is_rust():
            total = sum(
                cell["total_results"]
                for group in sub.json()["results"].values()
                for cell in group["results"].values()
            )
            assert total == 3, sub.json()["results"].keys()
    finally:
        admin.request(
            "DELETE", f"{_base(ws, pid)}/issues/{seed['issue3']}/archive/")
        admin.request("DELETE", f"{_base(ws, pid)}/issues/{mid}/archive/")
        admin.delete(f"{_base(ws, pid)}/issues/{mid}/")


def test_archived_grouped_target_date_and_start_date(clients, seed):
    # NEWPIDASH-350 (DIFF-58, old bug fix, PB-0301): group_by of
    # target_date/start_date 500s on the old archived list for the same
    # missing-queryset reason as created_by. The new list buckets over
    # the archived set.
    admin = clients["admin"]
    ws, pid, done = seed["ws_slug"], seed["project"], seed["state_done"]
    made = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "DIFF-58 dated issue"})
    assert made.status_code == 201, made.text
    aid = made.json()["id"]
    try:
        assert admin.patch(
            f"{_base(ws, pid)}/issues/{aid}/",
            json={"state_id": done,
                  "target_date": "2026-11-17",
                  "start_date": "2026-11-10"}).status_code == 204
        assert admin.post(
            f"{_base(ws, pid)}/issues/{aid}/archive/").status_code == 200
        # I5 (undated) and A1 (dated) are archived.
        resp = admin.get(
            f"{_base(ws, pid)}/archived-issues/",
            params={"group_by": "target_date", "per_page": 1},
        )
        assert resp.status_code == expect("DIFF-58", django=500, rust=200)
        if is_rust():
            buckets = resp.json()["results"]
            assert buckets["2026-11-17"]["total_results"] == 1
            assert buckets["None"]["total_results"] == 1
            assert len(buckets["None"]["results"]) == 1
        sresp = admin.get(
            f"{_base(ws, pid)}/archived-issues/",
            params={"group_by": "start_date"},
        )
        assert sresp.status_code == expect("DIFF-58", django=500, rust=200)
        if is_rust():
            sbuckets = sresp.json()["results"]
            assert sbuckets["2026-11-10"]["total_results"] == 1
            assert sbuckets["None"]["total_results"] == 1
    finally:
        admin.request("DELETE", f"{_base(ws, pid)}/issues/{aid}/archive/")
        admin.delete(f"{_base(ws, pid)}/issues/{aid}/")


def test_archived_grouped_state_totals_count_archived_rows(clients, seed):
    # NEWPIDASH-350 (DIFF-58, old bug fix, PB-0300): the old archived
    # list counts each group with archived_at__isnull=True, so the Done
    # bucket of two archived Done issues reads total_results 1 (and the
    # second page is never offered). The new list counts archived rows.
    admin = clients["admin"]
    ws, pid, done = seed["ws_slug"], seed["project"], seed["state_done"]
    assert admin.post(
        f"{_base(ws, pid)}/issues/{seed['issue3']}/archive/"
    ).status_code == 200
    try:
        # I3 + I5 are archived, both in the Done state.
        resp = admin.get(
            f"{_base(ws, pid)}/archived-issues/",
            params={"group_by": "state_id", "per_page": 1},
        )
        assert resp.status_code == 200, resp.text
        bucket = resp.json()["results"][done]
        assert len(bucket["results"]) == 1
        assert bucket["total_results"] == expect("DIFF-58", django=1, rust=2)
        if is_rust():
            assert resp.json()["total_count"] == 2
    finally:
        admin.request(
            "DELETE", f"{_base(ws, pid)}/issues/{seed['issue3']}/archive/")


def test_bulk_archive_rejected_enqueues_no_archive_activity(clients, seed, db_conn):
    # NEWPIDASH-352 (DIFF-64, old bug fix, PB-0302): bulk-archive used to
    # enqueue the "archived" activity per valid issue and only then hit
    # the 400 on the first open-state row, leaving false archive history
    # with nothing archived. The fix validates every row first, then
    # writes the UPDATE, then enqueues — so a 400 enqueues nothing and
    # the 400 body stays byte-identical.
    # The suite runs workerless on both halves (app_issues is not in
    # WORKER_SUITES), so the test observes the enqueue itself: Django's
    # .delay() on the Celery broker, Rust's INSERT into rust_job_queue
    # (the NEWPIDASH-25 pattern from test_issue_bulk.py). That enqueue is
    # what a worker turns into the issue_activities row.
    admin = clients["admin"]
    ws, pid, done = seed["ws_slug"], seed["project"], seed["state_done"]
    first = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "DIFF-64 open issue"})
    assert first.status_code == 201, first.text
    open_id = first.json()["id"]
    second = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "DIFF-64 done issue"})
    assert second.status_code == 201, second.text
    done_id = second.json()["id"]
    try:
        assert admin.patch(
            f"{_base(ws, pid)}/issues/{done_id}/",
            json={"state_id": done}).status_code == 204
        # Both backends gate newest-first (Django Issue.Meta.ordering is
        # -created_at; Rust orders by created_at desc), so the done issue
        # created second is checked first: the old code enqueues for it,
        # then 400s on the open one.
        baseline = _reset_transports(db_conn)
        resp = admin.post(
            f"{_base(ws, pid)}/bulk-archive-issues/",
            json={"issue_ids": [done_id, open_id]})
        assert resp.status_code == 400, resp.text
        assert resp.json() == {
            "error_code": 4091, "error_message": "INVALID_ARCHIVE_STATE_GROUP"}
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT id FROM issues WHERE id = ANY(%s)"
                " AND archived_at IS NULL",
                ([done_id, open_id],),
            )
            assert {str(row[0]) for row in cur.fetchall()} == {done_id, open_id}
        acts = _archived_activity_kwargs(db_conn, baseline, seed["admin"])
        assert [act["issue_id"] for act in acts] == expect(
            "DIFF-64", django=[done_id], rust=[])
    finally:
        _drain_broker()
        admin.delete(f"{_base(ws, pid)}/issues/{done_id}/")
        admin.delete(f"{_base(ws, pid)}/issues/{open_id}/")
