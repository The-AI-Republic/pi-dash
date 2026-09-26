"""GET/PATCH/DELETE /api/workspaces/<slug>/users/notifications/<pk>/ — detail."""
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from _harness import db  # noqa: E402
from _harness.config import database_url  # noqa: E402


def _url(slug, pk):
    return f"/api/workspaces/{slug}/users/notifications/{pk}/"


def test_retrieve_shape(ctx, api_owner, mknotif):
    n = mknotif(title="Retrieve me", sender="issue.created")
    r = api_owner.get(_url(ctx.slug_a, n["id"]))
    assert r.status_code == 200
    item = r.json()
    assert item["id"] == n["id"]
    assert item["title"] == "Retrieve me"
    assert item["receiver"] == ctx.owner["id"]


def test_retrieve_foreign_notification_404(ctx, api_member, mknotif):
    n = mknotif(title="Owner only")
    r = api_member.get(_url(ctx.slug_a, n["id"]))
    assert r.status_code == 404


def test_retrieve_unknown_id_404(ctx, api_owner):
    r = api_owner.get(_url(ctx.slug_a, "00000000-0000-0000-0000-000000000000"))
    assert r.status_code == 404


def test_partial_update_snooze(ctx, api_owner, mknotif):
    n = mknotif(title="Snooze me")
    r = api_owner.patch(_url(ctx.slug_a, n["id"]), json={"snoozed_till": "2027-05-05T00:00:00Z"})
    assert r.status_code == 200
    assert r.json()["snoozed_till"] is not None
    assert "2027-05-05" in r.json()["snoozed_till"]
    row = db.fetchone(database_url(), "SELECT snoozed_till FROM notifications WHERE id = %s", (n["id"],))
    assert row["snoozed_till"] is not None


def test_partial_update_ignores_other_fields(ctx, api_owner, mknotif):
    # Only snoozed_till is writable; title changes are dropped, not applied.
    n = mknotif(title="Keep my title")
    r = api_owner.patch(
        _url(ctx.slug_a, n["id"]),
        json={"snoozed_till": None, "title": "Hacked title"},
    )
    assert r.status_code == 200
    assert r.json()["title"] == "Keep my title"


def test_partial_update_foreign_404(ctx, api_member, mknotif):
    n = mknotif(title="Owner only")
    r = api_member.patch(_url(ctx.slug_a, n["id"]), json={"snoozed_till": None})
    assert r.status_code == 404


def test_destroy_is_soft_delete(ctx, api_owner, mknotif):
    n = mknotif(title="Delete me")
    r = api_owner.delete(_url(ctx.slug_a, n["id"]))
    assert r.status_code == 204
    assert r.text == ""
    # Soft-deleted: the row stays with deleted_at set, invisible to the API.
    row = db.fetchone(database_url(), "SELECT deleted_at FROM notifications WHERE id = %s", (n["id"],))
    assert row["deleted_at"] is not None
    assert api_owner.get(_url(ctx.slug_a, n["id"])).status_code == 404
    titles = [i["title"] for i in api_owner.get(f"/api/workspaces/{ctx.slug_a}/users/notifications/").json()]
    assert "Delete me" not in titles


def test_destroy_foreign_404(ctx, api_member, mknotif):
    n = mknotif(title="Owner only")
    assert api_member.delete(_url(ctx.slug_a, n["id"])).status_code == 404
