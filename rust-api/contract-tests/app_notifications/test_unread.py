"""GET .../notifications/unread/ and POST .../notifications/mark-all-read/."""


def _unread(slug):
    return f"/api/workspaces/{slug}/users/notifications/unread/"


def _mark_all(slug):
    return f"/api/workspaces/{slug}/users/notifications/mark-all-read/"


def test_unread_counts_shape(ctx, api_owner, mknotif):
    mknotif(title="plain unread")
    mknotif(title="mentioned unread", sender="issue.mentioned")
    mknotif(title="read one", read_at="2026-01-01T00:00:00Z")
    mknotif(title="archived one", archived_at="2026-01-01T00:00:00Z")
    mknotif(title="snoozed one", snoozed_till="2026-01-01T00:00:00Z")
    r = api_owner.get(_unread(ctx.slug_a))
    assert r.status_code == 200
    body = r.json()
    assert set(body.keys()) == {
        "total_unread_notifications_count",
        "mention_unread_notifications_count",
    }
    assert body["total_unread_notifications_count"] == 1
    assert body["mention_unread_notifications_count"] == 1


def test_mark_all_read(ctx, api_owner, mknotif):
    mknotif(title="one")
    mknotif(title="two")
    r = api_owner.post(_mark_all(ctx.slug_a), json={})
    assert r.status_code == 200
    assert r.json() == {"message": "Successful"}
    body = api_owner.get(_unread(ctx.slug_a)).json()
    assert body["total_unread_notifications_count"] == 0
    assert body["mention_unread_notifications_count"] == 0


def test_mark_all_read_scoped_to_workspace(ctx, api_both, mknotif):
    import sys, os
    sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
    from _harness import factory  # noqa: E402

    a = mknotif(title="in A", receiver_id=ctx.both["id"])
    b_ids = []
    n = factory.create_notification(ctx.ws_b["id"], ctx.both["id"], title="in B")
    b_ids.append(n["id"])
    try:
        api_both.post(_mark_all(ctx.slug_a), json={})
        assert api_both.get(_unread(ctx.slug_a)).json()["total_unread_notifications_count"] == 0
        # Workspace B is untouched.
        assert api_both.get(_unread(ctx.slug_b)).json()["total_unread_notifications_count"] == 1
    finally:
        for nid in b_ids:
            factory.delete_notification(nid)
