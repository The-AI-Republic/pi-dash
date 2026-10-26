"""PIDASHCONV-1453: analytics date buckets and filters use the actor zone.

Django's ``BaseAPIView`` activates ``request.user.user_timezone``
(``TimezoneMixin``, ``app/views/base.py:35-46``), so every date bucket and
date filter on a ``DateTimeField`` — ``ExtractYear``/``ExtractMonth``
(``utils/analytics_plot.py:43-50``), ``TruncMonth``, ``__date`` lookups and
``__year`` bounds — compiles to ``... AT TIME ZONE '<user tz>'``. The Rust
analytics SQL never binds the actor zone, so it buckets and filters in the
session zone (UTC).

Each test seeds one tenant whose actor zone is ``America/New_York`` with
issues just across UTC day/month boundaries: in UTC they land on the later
day/month, in the actor zone on the earlier one. Every test fails on the
pre-fix Rust, passes after the fix, and passes on Django.
"""

import base64
import hashlib
import json
import uuid
from datetime import datetime, timezone

import pytest

from _harness.auth import authed_client, forge_session_key, get_secret_key

ACTOR_TZ = "America/New_York"
WS_SLUG = "az-ws"

# A: 2026-09-30 22:00 in New York (day + month boundary).
CREATED_A = "2026-10-01T02:00:00Z"
# C: created 2026-03-31 21:00, completed 2026-03-31 23:00 in New York.
CREATED_C = "2026-04-01T01:00:00Z"
COMPLETED_C = "2026-04-01T03:00:00Z"

PW_SALT = "contract93seed"
_PW_DK = hashlib.pbkdf2_hmac("sha256", b"contract93-password", PW_SALT.encode(), 10000)
PASSWORD_FIELD = "pbkdf2_sha256$10000$%s$%s" % (
    PW_SALT,
    base64.b64encode(_PW_DK).decode(),
)

USER_COLUMNS = (
    "password, id, username, email, first_name, last_name, avatar,"
    " date_joined, created_at, updated_at, last_location, created_location,"
    " is_superuser, is_managed, is_password_expired, is_active, is_staff,"
    " is_email_verified, is_password_autoset, token, user_timezone,"
    " last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,"
    " is_bot, display_name, is_email_valid, is_password_reset_required"
)

PROJECT_COLUMNS = (
    "created_at, updated_at, id, name, description, network, identifier,"
    " workspace_id, cycle_view, module_view, issue_views_view, page_view,"
    " intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,"
    " is_issue_type_enabled, guest_view_all_features, timezone,"
    " members_can_edit_states, repo_url, base_branch,"
    " agent_default_interval_seconds, agent_default_max_ticks,"
    " agent_ticking_enabled, is_default,"
    " agent_review_default_interval_seconds, default_agent_executor,"
    " agent_test_default_interval_seconds"
)


def _wipe(cur):
    cur.execute(
        "DELETE FROM sessions WHERE user_id IN "
        "(SELECT id::text FROM users WHERE username LIKE 'az\\_%')"
    )
    for table in (
        "cycle_issues", "cycles", "analytic_views", "issues", "states",
        "project_members", "projects", "workspace_members",
    ):
        cur.execute(
            "DELETE FROM " + table + " WHERE workspace_id IN "
            "(SELECT id FROM workspaces WHERE slug LIKE 'az-%')"
        )
    cur.execute("DELETE FROM workspaces WHERE slug LIKE 'az-%'")
    cur.execute("DELETE FROM users WHERE username LIKE 'az\\_%'")


@pytest.fixture(scope="module")
def tzworld(db_conn, base_url):
    ids = {k: str(uuid.uuid4()) for k in (
        "user", "ws", "project", "st_backlog", "st_done",
        "issue_a", "issue_c", "cycle", "view",
    )}
    now = datetime.now(timezone.utc)
    secret = get_secret_key()
    with db_conn.cursor() as cur:
        _wipe(cur)
        cur.execute(
            "INSERT INTO users (%s) VALUES (%s)" % (USER_COLUMNS, ",".join(["%s"] * 29)),
            (PASSWORD_FIELD, ids["user"], "az_ny", "az-ny@example.com",
             "AZ NY", "User", "", now, now, now, "", "", False, False, False, True,
             False, True, False, uuid.uuid4().hex, ACTOR_TZ, "", "",
             "email", "", False, "AZ NY", True, False),
        )
        # Backdated so the monthly zero-fill spans the seeded months.
        cur.execute(
            "INSERT INTO workspaces (created_at, updated_at, id, name, slug,"
            " owner_id, timezone, background_color) VALUES (%s,%s,%s,%s,%s,%s,%s,%s)",
            ("2026-01-15T00:00:00Z", now, ids["ws"], "AZ WS", WS_SLUG,
             ids["user"], "UTC", "#ffffff"),
        )
        cur.execute(
            "INSERT INTO workspace_members (created_at, updated_at, id, role,"
            " member_id, workspace_id, view_props, default_props, issue_props,"
            " is_active, explored_features, getting_started_checklist, tips)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (now, now, str(uuid.uuid4()), 20, ids["user"], ids["ws"],
             "{}", "{}", "{}", True, "{}", "{}", "{}"),
        )
        cur.execute(
            "INSERT INTO projects (%s) VALUES (%s)"
            % (PROJECT_COLUMNS, ",".join(["%s"] * 30)),
            (now, now, ids["project"], "AZ Project", "", 0, "AZ1453", ids["ws"], True,
             True, True, True, True, 0, 0, "{}", True, True, True, "UTC",
             True, "", "", 3600, 100, True, False, 10800, "muse", 10800),
        )
        cur.execute(
            "INSERT INTO project_members (created_at, updated_at, id, role,"
            " member_id, project_id, workspace_id, view_props, default_props,"
            " sort_order, preferences, is_active)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (now, now, str(uuid.uuid4()), 20, ids["user"], ids["project"],
             ids["ws"], "{}", "{}", 0.0, "{}", True),
        )
        for key, name, group in (
            ("st_backlog", "AZ Backlog", "backlog"),
            ("st_done", "AZ Done", "completed"),
        ):
            cur.execute(
                "INSERT INTO states (created_at, updated_at, id, name, description,"
                " color, slug, project_id, workspace_id, sequence, \"group\","
                " \"default\", is_triage) VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (now, now, ids[key], name, "", "#000000", name.lower().replace(" ", "-"),
                 ids["project"], ids["ws"], 0.0, group, False, False),
            )
        for key, name, priority, state, seq, created, completed in (
            ("issue_a", "A boundary", "high", "st_backlog", 1, CREATED_A, None),
            ("issue_c", "C completed", "medium", "st_done", 2, CREATED_C, COMPLETED_C),
        ):
            cur.execute(
                "INSERT INTO issues (created_at, updated_at, id, name,"
                " description_json, priority, sequence_id, created_by_id, project_id,"
                " state_id, workspace_id, description_html, sort_order, point,"
                " completed_at, is_draft, git_work_branch, workpad, complexity_score)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (created, created, ids[key], name, "{}", priority, seq, ids["user"],
                 ids["project"], ids[state], ids["ws"], "", 0.0, seq, completed,
                 False, "", "", 0),
            )
        cur.execute(
            "INSERT INTO cycles (id, name, description, start_date, end_date, owned_by_id,"
            " project_id, workspace_id, view_props, sort_order, external_source, external_id,"
            " progress_snapshot, archived_at, logo_props, timezone, version, created_by_id,"
            " created_at, updated_at) VALUES"
            " (%s, 'AZ Cycle', '', %s, %s, %s, %s, %s, '{}', 65535, null, null, '{}', null,"
            " '{}', 'UTC', 1, %s, now(), now())",
            (ids["cycle"], "2026-09-28T00:00:00Z", "2026-10-03T00:00:00Z",
             ids["user"], ids["project"], ids["ws"], ids["user"]),
        )
        cur.execute(
            "INSERT INTO cycle_issues (id, created_at, updated_at, cycle_id, issue_id,"
            " project_id, workspace_id, created_by_id)"
            " VALUES (%s, %s, %s, %s, %s, %s, %s, %s)",
            (str(uuid.uuid4()), CREATED_A, CREATED_A,
             ids["cycle"], ids["issue_a"], ids["project"], ids["ws"], ids["user"]),
        )
        cur.execute(
            "INSERT INTO analytic_views (created_at, updated_at, id, name,"
            " description, query, query_dict, created_by_id, workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (now, now, ids["view"], "AV-TZ", "1453 tz view",
             json.dumps({"workspace__slug": WS_SLUG}),
             json.dumps({"x_axis": "created_at", "y_axis": "issue_count"}),
             ids["user"], ids["ws"]),
        )
        session = forge_session_key(
            db_conn, user_id=ids["user"], password_field=PASSWORD_FIELD, secret=secret,
        )
    ids["session"] = session
    client = authed_client(base_url, session)
    yield {"ids": ids, "client": client}
    client.close()
    with db_conn.cursor() as cur:
        _wipe(cur)


def test_analytics_created_at_axis_actor_timezone(tzworld):
    """``created_at`` month buckets follow the actor zone (Sep/Mar, not Oct/Apr)."""
    client = tzworld["client"]
    resp = client.get(
        f"/api/workspaces/{WS_SLUG}/analytics/?x_axis=created_at&y_axis=issue_count"
    )
    assert resp.status_code == 200, resp.text[:500]
    body = resp.json()
    assert body["total"] == 2
    assert body["distribution"] == {
        "2026-3": [{"dimension": "2026-3", "count": 1}],
        "2026-9": [{"dimension": "2026-9", "count": 1}],
    }


def test_analytics_created_at_filter_actor_timezone(tzworld):
    """``created_at=2026-10-01;after`` excludes A (Sep 30 local), not includes it."""
    client = tzworld["client"]
    resp = client.get(
        f"/api/workspaces/{WS_SLUG}/analytics/"
        "?x_axis=priority&y_axis=issue_count&created_at=2026-10-01;after"
    )
    assert resp.status_code == 200, resp.text[:500]
    body = resp.json()
    assert body["total"] == 0
    assert body["distribution"] == {}


def test_saved_analytic_view_date_axis_actor_timezone(tzworld):
    """Saved-view date axes bucket in the actor zone like ``/analytics/``."""
    client = tzworld["client"]
    view_id = tzworld["ids"]["view"]
    resp = client.get(f"/api/workspaces/{WS_SLUG}/saved-analytic-view/{view_id}/")
    assert resp.status_code == 200, resp.text[:500]
    body = resp.json()
    assert body["total"] == 2
    assert body["distribution"] == {
        "2026-3": [{"dimension": "2026-3", "count": 1}],
        "2026-9": [{"dimension": "2026-9", "count": 1}],
    }


def test_default_analytics_completed_month_actor_timezone(tzworld):
    """``issue_completed_month_wise`` counts C in March (local), not April (UTC)."""
    client = tzworld["client"]
    resp = client.get(f"/api/workspaces/{WS_SLUG}/default-analytics/")
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["issue_completed_month_wise"] == [{"month": 3, "count": 1}]


def test_advance_charts_custom_created_at_actor_timezone(tzworld):
    """Custom ``CREATED_AT`` keys are local dates (Mar 31 / Sep 30)."""
    client = tzworld["client"]
    resp = client.get(
        f"/api/workspaces/{WS_SLUG}/advance-analytics-charts/"
        "?type=custom-work-items&x_axis=CREATED_AT"
    )
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["data"] == [
        {"key": "2026-03-31", "name": "2026-03-31", "count": 1},
        {"key": "2026-09-30", "name": "2026-09-30", "count": 1},
    ]


def test_advance_charts_work_items_monthly_actor_timezone(tzworld):
    """Monthly ``work-items`` buckets count A in Sep and C in Mar (local)."""
    client = tzworld["client"]
    resp = client.get(
        f"/api/workspaces/{WS_SLUG}/advance-analytics-charts/?type=work-items"
    )
    assert resp.status_code == 200, resp.text[:500]
    rows = {row["key"]: row for row in resp.json()["data"]}
    assert rows["2026-09-01"]["created_issues"] == 1
    assert rows["2026-09-01"]["count"] == 1
    assert rows["2026-10-01"]["created_issues"] == 0
    assert rows["2026-03-01"]["created_issues"] == 1
    assert rows["2026-04-01"]["created_issues"] == 0


def test_project_charts_daily_actor_timezone(tzworld):
    """Project daily chart buckets the bridge row on Sep 30 (local)."""
    client = tzworld["client"]
    ids = tzworld["ids"]
    resp = client.get(
        f"/api/workspaces/{WS_SLUG}/projects/{ids['project']}/advance-analytics-charts/"
        f"?type=work-items&cycle_id={ids['cycle']}"
    )
    assert resp.status_code == 200, resp.text[:500]
    rows = {row["key"]: row for row in resp.json()["data"]}
    assert rows["2026-09-30"]["created_issues"] == 1
    assert rows["2026-10-01"]["created_issues"] == 0
