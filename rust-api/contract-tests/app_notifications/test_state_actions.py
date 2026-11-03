"""POST/DELETE .../<pk>/read/ and .../<pk>/archive/ — state transitions."""


def _read(slug, pk):
    return f"/api/workspaces/{slug}/users/notifications/{pk}/read/"


def _archive(slug, pk):
    return f"/api/workspaces/{slug}/users/notifications/{pk}/archive/"


def test_mark_read_and_unread(ctx, api_owner, mknotif):
    n = mknotif(title="Read cycle")
    r = api_owner.post(_read(ctx.slug_a, n["id"]))
    assert r.status_code == 200
    assert r.json()["read_at"] is not None
    assert r.json()["id"] == n["id"]
    r = api_owner.delete(_read(ctx.slug_a, n["id"]))
    assert r.status_code == 200
    assert r.json()["read_at"] is None


def test_mark_read_foreign_404(ctx, api_member, mknotif):
    n = mknotif(title="Owner only")
    assert api_member.post(_read(ctx.slug_a, n["id"])).status_code == 404
    assert api_member.delete(_read(ctx.slug_a, n["id"])).status_code == 404


def test_archive_and_unarchive(ctx, api_owner, mknotif):
    n = mknotif(title="Archive cycle")
    r = api_owner.post(_archive(ctx.slug_a, n["id"]))
    assert r.status_code == 200
    assert r.json()["archived_at"] is not None
    # Archived items leave the default list but stay under archived=true.
    titles = [
        i["title"]
        for i in api_owner.get(
            f"/api/workspaces/{ctx.slug_a}/users/notifications/"
        ).json()
    ]
    assert "Archive cycle" not in titles
    titles = [
        i["title"]
        for i in api_owner.get(
            f"/api/workspaces/{ctx.slug_a}/users/notifications/",
            params={"archived": "true"},
        ).json()
    ]
    assert "Archive cycle" in titles
    r = api_owner.delete(_archive(ctx.slug_a, n["id"]))
    assert r.status_code == 200
    assert r.json()["archived_at"] is None


def test_archive_foreign_404(ctx, api_member, mknotif):
    n = mknotif(title="Owner only")
    assert api_member.post(_archive(ctx.slug_a, n["id"])).status_code == 404
    assert api_member.delete(_archive(ctx.slug_a, n["id"])).status_code == 404
