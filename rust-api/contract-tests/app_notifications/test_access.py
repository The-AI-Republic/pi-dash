"""Access control: denied-permission and tenant-isolation cases.

The 403 tests below are the permission-removal guard: ``@allow_permission``
on the guarded views is the only thing turning an outsider's requests into
403s (the querysets alone would return 200s with the outsider's own rows),
so deleting any of those decorators flips these tests red. Demonstrated in
the PR with a one-line local patch, reverted. retrieve/destroy carry no
decorator; their 404 tests guard the receiver-scoped queryset instead.
"""
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from _harness import factory  # noqa: E402


DENIED = {"error": "You don't have the required permissions."}


def _detail(slug, pk):
    return f"/api/workspaces/{slug}/users/notifications/{pk}/"


def test_outsider_denied_on_guarded_views(ctx, api_outsider, mknotif):
    # Views carrying @allow_permission answer 403 to non-members.
    n = mknotif(title="Owner only")
    base = f"/api/workspaces/{ctx.slug_a}/users/notifications/"
    cases = [
        ("GET", base, None),
        ("GET", base + "unread/", None),
        ("POST", base + "mark-all-read/", {}),
        ("PATCH", _detail(ctx.slug_a, n["id"]), {"snoozed_till": None}),
        ("POST", _detail(ctx.slug_a, n["id"]) + "read/", None),
        ("DELETE", _detail(ctx.slug_a, n["id"]) + "read/", None),
        ("POST", _detail(ctx.slug_a, n["id"]) + "archive/", None),
        ("DELETE", _detail(ctx.slug_a, n["id"]) + "archive/", None),
    ]
    for method, url, payload in cases:
        r = api_outsider.request(method, url, json=payload)
        assert r.status_code == 403, (method, url, r.status_code, r.text[:200])
        assert r.json() == DENIED


def test_outsider_gets_404_on_undecorated_views(ctx, api_outsider, mknotif):
    # retrieve/destroy carry no @allow_permission (upstream shape); the
    # receiver-scoped queryset is the only guard, so outsiders see 404.
    n = mknotif(title="Owner only")
    assert api_outsider.get(_detail(ctx.slug_a, n["id"])).status_code == 404
    assert api_outsider.delete(_detail(ctx.slug_a, n["id"])).status_code == 404


def test_unauthenticated_denied(ctx, api_anon, mknotif):
    n = mknotif(title="Owner only")
    base = f"/api/workspaces/{ctx.slug_a}/users/notifications/"
    assert api_anon.get(base).status_code == 401
    assert api_anon.get(base).json() == {"detail": "Authentication credentials were not provided."}
    assert api_anon.get(_detail(ctx.slug_a, n["id"])).status_code == 401


def test_cross_workspace_member_cannot_enter(ctx, api_bmember, mknotif):
    # b_member belongs to B only: workspace A answers 403 on guarded views
    # and 404 on retrieve (undecorated, queryset-scoped) — never data.
    n = mknotif(title="Owner only")
    base = f"/api/workspaces/{ctx.slug_a}/users/notifications/"
    assert api_bmember.get(base).status_code == 403
    assert api_bmember.get(_detail(ctx.slug_a, n["id"])).status_code == 404


def test_workspace_lists_are_isolated(ctx, api_both, mknotif):
    # `both` is a member of A and B: each workspace list shows only its own rows.
    b_ids = []
    mknotif(title="in A", receiver_id=ctx.both["id"])
    n = factory.create_notification(ctx.ws_b["id"], ctx.both["id"], title="in B")
    b_ids.append(n["id"])
    try:
        a_titles = [
            i["title"]
            for i in api_both.get(f"/api/workspaces/{ctx.slug_a}/users/notifications/").json()
        ]
        b_titles = [
            i["title"]
            for i in api_both.get(f"/api/workspaces/{ctx.slug_b}/users/notifications/").json()
        ]
        assert a_titles == ["in A"]
        assert b_titles == ["in B"]
        # A-row fetched through the B slug does not resolve (permission or 404 —
        # either way no cross-tenant read). B members only: use b_member client.
    finally:
        for nid in b_ids:
            factory.delete_notification(nid)


def test_retrieve_via_wrong_workspace_slug_404(ctx, api_both, mknotif):
    # `both` passes B's permission check, but the A-row is invisible under B's slug.
    n = mknotif(title="in A", receiver_id=ctx.both["id"])
    r = api_both.get(_detail(ctx.slug_b, n["id"]))
    assert r.status_code == 404
    # ...while the same row resolves fine under its own workspace.
    assert api_both.get(_detail(ctx.slug_a, n["id"])).status_code == 200
