"""Prompt issue context reads M2M through-rows unguarded (PIDASHCONV-1231).

Django builds the prompt lists with ``issue.labels.all()`` /
``issue.assignees.all()`` (apps/api/pi_dash/prompting/context.py:536-537).
A forward M2M manager filters only through the target model's default
manager (``labels.deleted_at IS NULL``; no filter on users) and never
through the through table's ``deleted_at`` — while every issue PATCH
carrying ``label_ids``/``assignee_ids`` soft-deletes all through rows and
bulk-recreates them (app/serializers/issue.py:475,496). So the prompt
lists removed labels/assignees and repeats re-added ones once per
historical row. Assignees order by ``User.Meta.ordering =
("-created_at",)`` (db/models/user.py:137).

The Rust prompt loaders (``BUNDLE_LABELS_SQL`` /
``BUNDLE_ASSIGNEES_SQL`` in services/src/orchestration/creation.rs and
``load_labels`` / ``load_assignees`` in api/src/prompting/mod.rs) wrongly
add ``il.deleted_at IS NULL`` / ``ia.deleted_at IS NULL`` and order
assignees by ``users.id``. These tests re-PATCH labels and assignees
through the API, then compare the preview prompt's ``- Labels:`` /
``- Assignees:`` lines against the Django rendering.
"""

from datetime import datetime, timezone


def _preview_url(tenant, kind):
    return (f"/api/workspaces/{tenant['workspace']['slug']}"
            f"/prompts/{kind}/preview")


def _issue_url(tenant, project_id, issue_id):
    return (f"/api/workspaces/{tenant['workspace']['slug']}"
            f"/projects/{project_id}/issues/{issue_id}/")


def _prompt_line(prompt, prefix):
    for line in prompt.splitlines():
        if line.startswith(prefix):
            return line
    raise AssertionError(f"no {prefix!r} line in preview")


def test_preview_labels_show_stale_and_repeated_rows(api, seed, tenant_a,
                                                    project_a, write_a):
    """Re-added labels repeat per through row; removed labels still render."""
    ws = tenant_a["workspace"]["id"]
    state_id = seed.state(project_a, ws)
    issue_id = seed.issue(ws, project_a, name="CT1231 labels",
                          state_id=state_id)
    l1 = seed.label(ws, project_a, name="CT1231-L1")
    l2 = seed.label(ws, project_a, name="CT1231-L2")
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE labels SET created_at = %s WHERE id = %s",
                    (datetime(2020, 1, 1, tzinfo=timezone.utc), l1))
        cur.execute("UPDATE labels SET created_at = %s WHERE id = %s",
                    (datetime(2021, 6, 1, tzinfo=timezone.utc), l2))

    url = _issue_url(tenant_a, project_a, issue_id)
    r = api.patch(url, headers=write_a, json={"label_ids": [l1, l2]})
    assert r.status_code == 204, r.text
    r = api.patch(url, headers=write_a, json={"label_ids": [l1]})
    assert r.status_code == 204, r.text
    r = api.patch(url, headers=write_a, json={"label_ids": [l1]})
    assert r.status_code == 204, r.text

    with seed.conn.cursor() as cur:
        cur.execute("SELECT l.name, il.deleted_at IS NULL"
                    " FROM issue_labels il JOIN labels l ON l.id = il.label_id"
                    " WHERE il.issue_id = %s", (issue_id,))
        history = sorted(cur.fetchall(), key=str)
    assert history == [("CT1231-L1", False), ("CT1231-L1", False),
                       ("CT1231-L1", True), ("CT1231-L2", False)], history

    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_id)})
    assert r.status_code == 200, r.text
    # Labels order by labels.created_at DESC (Label.Meta.ordering), so the
    # newer L2 sorts first; every through row renders, live or not.
    assert _prompt_line(r.json()["prompt"], "- Labels:") == (
        "- Labels: CT1231-L2, CT1231-L1, CT1231-L1, CT1231-L1")


def test_preview_assignees_newest_first_with_stale_rows(api, seed, tenant_a,
                                                       project_a, write_a):
    """Assignees render newest-first incl. stale rows (User -created_at)."""
    ws = tenant_a["workspace"]["id"]
    state_id = seed.state(project_a, ws)
    issue_id = seed.issue(ws, project_a, name="CT1231 assignees",
                          state_id=state_id)
    ua = seed.user()
    ub = seed.user()
    # Older user takes the smaller UUID so ORDER BY users.id would list the
    # older user first while -created_at lists the newer one first: the two
    # orders oppose each other whatever UUIDs were drawn.
    older, newer = (ua, ub) if ua["id"] < ub["id"] else (ub, ua)
    assert newer["id"] > older["id"]
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE users SET created_at = %s, display_name = %s"
                    " WHERE id = %s",
                    (datetime(2020, 2, 1, tzinfo=timezone.utc),
                     "CT1231 Assignee One", older["id"]))
        cur.execute("UPDATE users SET created_at = %s, display_name = %s"
                    " WHERE id = %s",
                    (datetime(2021, 7, 1, tzinfo=timezone.utc),
                     "CT1231 Assignee Two", newer["id"]))
    for u in (older, newer):
        seed.member(ws, u["id"], role=15)
        seed.project_member(project_a, ws, u["id"], role=15)

    url = _issue_url(tenant_a, project_a, issue_id)
    r = api.patch(url, headers=write_a,
                  json={"assignee_ids": [older["id"], newer["id"]]})
    assert r.status_code == 204, r.text
    r = api.patch(url, headers=write_a,
                  json={"assignee_ids": [newer["id"]]})
    assert r.status_code == 204, r.text

    with seed.conn.cursor() as cur:
        cur.execute("SELECT assignee_id, deleted_at IS NULL"
                    " FROM issue_assignees WHERE issue_id = %s", (issue_id,))
        history = sorted((str(a), live) for a, live in cur.fetchall())
    assert history == sorted([(str(newer["id"]), False),
                              (str(newer["id"]), True),
                              (str(older["id"]), False)]), history

    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_id)})
    assert r.status_code == 200, r.text
    # Newest user first, one entry per through row (the re-added newer user
    # renders twice, the removed older user once).
    assert _prompt_line(r.json()["prompt"], "- Assignees:") == (
        "- Assignees: CT1231 Assignee Two, CT1231 Assignee Two,"
        " CT1231 Assignee One")
