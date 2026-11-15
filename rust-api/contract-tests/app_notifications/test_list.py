"""GET /api/workspaces/<slug>/users/notifications/ — list shape and filters."""
import datetime
import uuid

import pytest

from _harness import factory as _factory
from _harness import db as _db
from _harness.config import database_url as _database_url
from _harness.diffs import expect


def _get(api, slug, **params):
    return api.get(f"/api/workspaces/{slug}/users/notifications/", params=params or None)


def test_list_empty_for_fresh_member(ctx, api_member):
    r = _get(api_member, ctx.slug_a)
    assert r.status_code == 200
    assert r.json() == []


def test_list_item_shape(ctx, api_owner, mknotif):
    n = mknotif(sender="issue.created", title="Shape check")
    r = _get(api_owner, ctx.slug_a)
    assert r.status_code == 200
    body = r.json()
    assert isinstance(body, list) and len(body) == 1
    item = body[0]
    assert set(item.keys()) == {
        "id", "created_at", "updated_at", "deleted_at",
        "data", "entity_identifier", "entity_name", "title",
        "message", "message_html", "message_stripped", "sender",
        "read_at", "snoozed_till", "archived_at",
        "created_by", "updated_by", "project", "receiver",
        "triggered_by", "workspace", "triggered_by_details",
        "is_inbox_issue", "is_intake_issue", "is_mentioned_notification",
    }
    assert item["id"] == n["id"]
    assert item["entity_name"] == "issue"
    assert item["title"] == "Shape check"
    assert item["receiver"] == ctx.owner["id"]
    assert item["workspace"] == ctx.ws_a["id"]
    assert item["triggered_by"] is None
    assert item["triggered_by_details"] is None
    assert item["is_inbox_issue"] is False
    assert item["is_intake_issue"] is False
    assert item["is_mentioned_notification"] is False


def test_list_entity_name_filter(ctx, api_owner, mknotif):
    mknotif(entity_name="page", title="not-an-issue")
    mknotif(entity_name="issue", title="is-an-issue")
    titles = [i["title"] for i in _get(api_owner, ctx.slug_a).json()]
    assert titles == ["is-an-issue"]


def test_list_mentioned_default_excluded(ctx, api_owner, mknotif):
    mknotif(sender="issue.mentioned", title="mentioned one")
    mknotif(sender="issue.created", title="plain one")
    titles = [i["title"] for i in _get(api_owner, ctx.slug_a).json()]
    assert titles == ["plain one"]


def test_list_mentioned_param(ctx, api_owner, mknotif):
    mknotif(sender="issue.mentioned", title="mentioned one")
    mknotif(sender="issue.created", title="plain one")
    titles = [i["title"] for i in _get(api_owner, ctx.slug_a, mentioned="true").json()]
    assert titles == ["mentioned one"]


def test_list_read_filter(ctx, api_owner, mknotif):
    mknotif(title="unread one")
    mknotif(title="read one", read_at="2026-01-01T00:00:00Z")
    assert [i["title"] for i in _get(api_owner, ctx.slug_a, read="false").json()] == ["unread one"]
    assert [i["title"] for i in _get(api_owner, ctx.slug_a, read="true").json()] == ["read one"]
    assert len(_get(api_owner, ctx.slug_a).json()) == 2


def test_list_archived_filter(ctx, api_owner, mknotif):
    mknotif(title="active one")
    mknotif(title="archived one", archived_at="2026-01-01T00:00:00Z")
    assert [i["title"] for i in _get(api_owner, ctx.slug_a).json()] == ["active one"]
    assert [i["title"] for i in _get(api_owner, ctx.slug_a, archived="true").json()] == [
        "archived one"
    ]


def test_list_snoozed_filter(ctx, api_owner, mknotif):
    past = "2020-01-01T00:00:00Z"
    future = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=30)).isoformat()
    mknotif(title="never snoozed")
    mknotif(title="snooze expired", snoozed_till=past)
    mknotif(title="snoozed ahead", snoozed_till=future)
    default = sorted(i["title"] for i in _get(api_owner, ctx.slug_a).json())
    assert default == ["never snoozed", "snoozed ahead"]
    snoozed = sorted(i["title"] for i in _get(api_owner, ctx.slug_a, snoozed="true").json())
    assert snoozed == ["snooze expired", "snoozed ahead"]


def test_list_type_branches_without_relations(ctx, api_owner, api_guest, mknotif):
    mknotif(title="plain one")
    for t in ("assigned", "subscribed", "created"):
        assert _get(api_owner, ctx.slug_a, type=t).json() == []
    # Guests see nothing under type=created (role < 15 short-circuits to none()).
    assert _get(api_guest, ctx.slug_a, type="created").json() == []


def test_list_pagination_envelope(ctx, api_owner, mknotif):
    mknotif(title="one")
    mknotif(title="two")
    r = _get(api_owner, ctx.slug_a, per_page="1", cursor="1:0:0")
    assert r.status_code == 200
    body = r.json()
    assert set(body.keys()) == {
        "grouped_by", "sub_grouped_by", "total_count", "next_cursor", "prev_cursor",
        "next_page_results", "prev_page_results", "count", "total_pages",
        "total_results", "extra_stats", "results",
    }
    assert body["count"] == 1
    assert body["total_count"] == 2
    assert body["next_page_results"] is True
    assert len(body["results"]) == 1


def test_list_invalid_cursor(ctx, api_owner, mknotif):
    mknotif(title="one")
    r = _get(api_owner, ctx.slug_a, per_page="1", cursor="bogus")
    assert r.status_code == 400


def _insert_project(workspace_id: str, owner_id: str, tag: str):
    pid = str(uuid.uuid4())
    _db.execute(
        _database_url(),
        """INSERT INTO projects (id, name, description, network, identifier,
            workspace_id, cycle_view, module_view, issue_views_view, page_view,
            intake_view, archive_in, close_in, logo_props,
            is_time_tracking_enabled, is_issue_type_enabled,
            guest_view_all_features, timezone, members_can_edit_states,
            repo_url, base_branch, agent_default_interval_seconds,
            agent_default_max_ticks, agent_ticking_enabled, is_default,
            agent_review_default_interval_seconds, default_agent_executor,
            dedicated_runner_lost_action,
            agent_test_default_interval_seconds, created_at, updated_at,
            created_by_id)
        VALUES (%s,%s,'contract project',1,%s,%s,
            true,true,true,true,true,30,30,'{}',
            true,true,false,'UTC',true,'','main',60,10,true,false,
            60,'local_runner','fail_to_backlog',60,now(),now(),%s)""",
        (pid, f"Project {tag}", tag, workspace_id, owner_id),
    )
    return pid


def _insert_issue(workspace_id: str, project_id: str, name: str, created_by_id=None):
    iid = str(uuid.uuid4())
    params = [iid, workspace_id, project_id, name, "none", 1, 65535]
    # Minimal issue insert mirrors seed.create_issue but we add created_by_id via UPDATE
    _db.execute(
        _database_url(),
        """INSERT INTO issues
           (id, workspace_id, project_id, name, description_json,
            description_html, priority, complexity_score, sequence_id, sort_order,
            is_draft, git_work_branch, workpad, created_at, updated_at)
           VALUES (%s,%s,%s,%s,'{}','<p></p>',%s,%s,%s,%s,false,'','',now(),now())""",
        (iid, workspace_id, project_id, name, "none", 0, int(uuid.uuid4().hex[:6], 16) % 100000, 65535),
    )
    if created_by_id:
        _db.execute(
            _database_url(),
            "UPDATE issues SET created_by_id=%s WHERE id=%s",
            (created_by_id, iid),
        )
    return iid


def _insert_subscriber(workspace_id: str, project_id: str, issue_id: str, subscriber_id: str):
    row_id = str(uuid.uuid4())
    _db.execute(
        _database_url(),
        "INSERT INTO issue_subscribers (id, workspace_id, project_id, issue_id, subscriber_id, created_at, updated_at) VALUES (%s,%s,%s,%s,%s,now(),now())",
        (row_id, workspace_id, project_id, issue_id, subscriber_id),
    )
    return row_id


def _insert_assignee(workspace_id: str, project_id: str, issue_id: str, assignee_id: str):
    row_id = str(uuid.uuid4())
    _db.execute(
        _database_url(),
        "INSERT INTO issue_assignees (id, workspace_id, project_id, issue_id, assignee_id, created_at, updated_at) VALUES (%s,%s,%s,%s,%s,now(),now())",
        (row_id, workspace_id, project_id, issue_id, assignee_id),
    )
    return row_id


def test_list_subscribed_excludes_assigned_and_created(ctx, api_owner, mknotif):
    """NEWPIDASH-270 (DIFF-31 old bug fix): subscribed must exclude assigned/created.

    Old Django used pk=OuterRef("issue_id") so assigned never excluded and
    leaked into subscribed; the fixed Rust behaviour excludes via issue_id,
    so type=subscribed returns only issues neither created nor assigned.
    The assigned leg pins both behaviours per backend via ``expect``: the
    django arm asserts the leak is real, the rust arm asserts the fix.
    The created leg is excluded on both backends (Django's created Exists
    is correct), so it asserts directly.
    """
    ws_id = ctx.ws_a["id"]
    owner_id = ctx.owner["id"]
    member_id = ctx.member["id"]
    tag = uuid.uuid4().hex[:8]
    proj_id = _insert_project(ws_id, owner_id, f"270-{tag}")  # projects.identifier is varchar(12)
    try:
        # subscribed-only: created by member, subscribed by owner, not assigned
        subscribed_issue = _insert_issue(ws_id, proj_id, f"subscribed-only-{tag}", created_by_id=member_id)
        _insert_subscriber(ws_id, proj_id, subscribed_issue, owner_id)

        # assigned + subscribed: created by member, assigned to owner, subscribed by owner
        assigned_issue = _insert_issue(ws_id, proj_id, f"assigned-both-{tag}", created_by_id=member_id)
        _insert_subscriber(ws_id, proj_id, assigned_issue, owner_id)
        _insert_assignee(ws_id, proj_id, assigned_issue, owner_id)

        # created + subscribed: created by owner, subscribed by owner
        created_issue = _insert_issue(ws_id, proj_id, f"created-both-{tag}", created_by_id=owner_id)
        _insert_subscriber(ws_id, proj_id, created_issue, owner_id)

        # Notifications for each issue (receiver = owner)
        mknotif(entity_identifier=subscribed_issue, title=f"notif-subscribed-{tag}")
        mknotif(entity_identifier=assigned_issue, title=f"notif-assigned-{tag}")
        mknotif(entity_identifier=created_issue, title=f"notif-created-{tag}")

        # Verify assigned filter still works
        assigned_ids = [r["entity_identifier"] for r in _get(api_owner, ctx.slug_a, type="assigned").json()]
        assert assigned_issue in assigned_ids
        assert subscribed_issue not in assigned_ids
        assert created_issue not in assigned_ids

        # Verify created filter still works (created by owner only)
        created_ids = [r["entity_identifier"] for r in _get(api_owner, ctx.slug_a, type="created").json()]
        # Note: created guard excludes guest role but owner is admin, so should see created
        assert created_issue in created_ids
        assert assigned_issue not in created_ids
        assert subscribed_issue not in created_ids

        # Core assertion: subscribed excludes both assigned and created.
        # Django leaks the assigned issue (pk=OuterRef bug, NTF-015);
        # Rust excludes it via issue_id (DIFF-31). The created leg is
        # excluded on both backends.
        subscribed_ids = [r["entity_identifier"] for r in _get(api_owner, ctx.slug_a, type="subscribed").json()]
        assert subscribed_issue in subscribed_ids, "subscribed-only must be listed under type=subscribed"
        assert (assigned_issue in subscribed_ids) == expect("DIFF-31", django=True, rust=False), (
            "DIFF-31: Django leaks assigned into subscribed; Rust excludes via issue_id"
        )
        assert created_issue not in subscribed_ids, "created issue must NOT be listed under type=subscribed"
    finally:
        # Cleanup in reverse order
        _db.execute(_database_url(), "DELETE FROM notifications WHERE workspace_id=%s AND entity_identifier IN (%s,%s,%s)", (ws_id, subscribed_issue, assigned_issue, created_issue))
        _db.execute(_database_url(), "DELETE FROM issue_assignees WHERE issue_id=%s", (assigned_issue,))
        _db.execute(_database_url(), "DELETE FROM issue_subscribers WHERE issue_id IN (%s,%s,%s)", (subscribed_issue, assigned_issue, created_issue))
        _db.execute(_database_url(), "DELETE FROM issues WHERE id IN (%s,%s,%s)", (subscribed_issue, assigned_issue, created_issue))
        _db.execute(_database_url(), "DELETE FROM projects WHERE id=%s", (proj_id,))
