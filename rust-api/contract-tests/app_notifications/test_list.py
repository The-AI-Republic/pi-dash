"""GET /api/workspaces/<slug>/users/notifications/ — list shape and filters."""
import datetime


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
