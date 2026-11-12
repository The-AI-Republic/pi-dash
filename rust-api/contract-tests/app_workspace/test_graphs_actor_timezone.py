# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PIDASHCONV-1371: profile graphs bucket in the actor zone, not UTC.

Django activates ``request.user.user_timezone`` for every request
(``TimezoneMixin``, ``app/views/base.py:35-46``), so the ``__date`` filter
of the activity graph (``views/workspace/user.py:530``) and the
``EXTRACT(MONTH/WEEK ...)`` of the issues-completed graph (``:549-559``)
convert ``AT TIME ZONE '<user tz>'``. The bare ``Cast("created_at",
DateField())`` in the activity SELECT/GROUP BY is a plain cast in the
session zone (UTC).

Each test seeds a tenant whose actor zone is ``Asia/Kolkata`` (UTC+5:30,
no DST) with one row just across a UTC date boundary: in UTC it lands on
the earlier day/month, in the actor zone on the later one.
"""

import uuid
from datetime import date, timedelta

ACTOR_TZ = "Asia/Kolkata"

# 2026-03-01 01:30 in Kolkata.
COMPLETED_AT = "2026-02-28T20:00:00Z"
# ISO week of 2026-03-01 is 9; the endpoint buckets ``week % 4``.
EXPECTED_WEEK = 9 % 4


def _months_ago(today, months):
    """``date.today() + relativedelta(months=-months)`` with clamping."""
    total = today.year * 12 + (today.month - 1) - months
    year, month0 = divmod(total, 12)
    month = month0 + 1
    if month == 12:
        last_day = (date(year + 1, 1, 1) - timedelta(days=1)).day
    else:
        last_day = (date(year, month + 1, 1) - timedelta(days=1)).day
    return date(year, month, min(today.day, last_day))


def _seed_tenant(db, user_id, slug):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET user_timezone = %s WHERE id = %s",
                (ACTOR_TZ, user_id),
            )
        conn.commit()
    ws = db.make_workspace("Tz Ws", slug, user_id)
    db.make_workspace_member(ws["id"], user_id)
    project = db.make_project(ws["id"], f"CT{uuid.uuid4().hex[:6].upper()}")
    state_id = str(uuid.uuid4())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO states
                   (id, workspace_id, project_id, name, description, color, slug,
                    sequence, "group", is_triage, "default", created_at, updated_at)
                   VALUES (%s,%s,%s,'Done','','#808080','done',65535,
                           'completed',false,false,now(),now())""",
                (state_id, ws["id"], project["id"]),
            )
        conn.commit()
    return {"workspace": ws, "project": project, "state_id": state_id}


def _seed_completed_issue(db, tenant, user_id, completed_at):
    issue_id = str(uuid.uuid4())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO issues (id, name, description_json, priority,
                    sequence_id, project_id, workspace_id, state_id,
                    description_html, sort_order, is_draft, git_work_branch,
                    workpad, complexity_score, created_at, updated_at,
                    created_by_id, completed_at)
                   VALUES (%s,'Tz issue','{}','high',4201,%s,%s,%s,'<p></p>',
                           65535,false,'','',0,now(),now(),%s,%s)""",
                (
                    issue_id,
                    tenant["project"]["id"],
                    tenant["workspace"]["id"],
                    tenant["state_id"],
                    user_id,
                    completed_at,
                ),
            )
            cur.execute(
                """INSERT INTO issue_assignees (id, issue_id, assignee_id,
                    project_id, workspace_id, created_at, updated_at)
                   VALUES (%s,%s,%s,%s,%s,now(),now())""",
                (
                    str(uuid.uuid4()),
                    issue_id,
                    user_id,
                    tenant["project"]["id"],
                    tenant["workspace"]["id"],
                ),
            )
        conn.commit()
    return issue_id


def _seed_activity(db, tenant, user_id, created_at):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO issue_activities (id, verb, comment, attachments,
                    issue_id, project_id, workspace_id, actor_id, created_at,
                    updated_at)
                   VALUES (%s,'created','tz activity','{}',NULL,%s,%s,%s,%s,%s)""",
                (
                    str(uuid.uuid4()),
                    tenant["project"]["id"],
                    tenant["workspace"]["id"],
                    user_id,
                    created_at,
                    created_at,
                ),
            )
        conn.commit()


def test_completed_graph_buckets_in_actor_timezone(user_api, world):
    tenant = _seed_tenant(world["db"], world["user"]["id"], "ws-tz")
    _seed_completed_issue(world["db"], tenant, world["user"]["id"], COMPLETED_AT)
    base = "/api/users/me/workspaces/ws-tz/issues-completed-graph/"

    res = user_api.get(base + "?month=3")
    assert res.status_code == 200, res.text[:500]
    assert res.json() == [{"week": EXPECTED_WEEK, "completed_count": 1}]

    res = user_api.get(base + "?month=2")
    assert res.status_code == 200, res.text[:500]
    assert res.json() == []


def test_activity_graph_cutoff_uses_actor_timezone(user_api, world):
    tenant = _seed_tenant(world["db"], world["user"]["id"], "ws-tz")
    cutoff = _months_ago(date.today(), 6)
    # Just before local midnight on the cutoff eve: the UTC date is
    # ``cutoff - 1`` but the actor-zone date is the cutoff itself, so
    # Django's ``__date__gte`` filter keeps the row while a UTC filter
    # drops it. The bare Cast labels it with the UTC (session-zone) date.
    _seed_activity(
        world["db"],
        tenant,
        world["user"]["id"],
        f"{cutoff - timedelta(days=1)}T20:00:00Z",
    )
    # Control: inside the window in both zones.
    _seed_activity(world["db"], tenant, world["user"]["id"], f"{cutoff}T12:00:00Z")

    res = user_api.get("/api/users/me/workspaces/ws-tz/activity-graph/")
    assert res.status_code == 200, res.text[:500]
    assert res.json() == [
        {"created_date": str(cutoff - timedelta(days=1)), "activity_count": 1},
        {"created_date": str(cutoff), "activity_count": 1},
    ]
