"""GET/PATCH /api/users/me/notification-preferences/ — own-preferences shape."""


URL = "/api/users/me/notification-preferences/"


def test_preferences_get_shape(ctx, api_owner):
    r = api_owner.get(URL)
    assert r.status_code == 200
    body = r.json()
    assert body["user"] == ctx.owner["id"]
    for key in ("property_change", "state_change", "comment", "mention", "issue_completed"):
        assert body[key] is True


def test_preferences_patch_roundtrip(ctx, api_owner):
    r = api_owner.patch(URL, json={"mention": False, "comment": False})
    assert r.status_code == 200
    assert r.json()["mention"] is False
    assert r.json()["comment"] is False
    body = api_owner.get(URL).json()
    assert body["mention"] is False
    assert body["comment"] is False
    assert body["property_change"] is True
    # Restore defaults so other tests see a clean row.
    api_owner.patch(URL, json={"mention": True, "comment": True})


def test_preferences_are_per_user(ctx, api_member, api_owner):
    mine = api_member.get(URL).json()
    theirs = api_owner.get(URL).json()
    assert mine["user"] == ctx.member["id"]
    assert theirs["user"] == ctx.owner["id"]
    assert mine["id"] != theirs["id"]
    for key in ("property_change", "state_change", "comment", "mention", "issue_completed"):
        assert mine[key] is True


def test_preferences_unauthenticated(ctx, api_anon):
    assert api_anon.get(URL).status_code == 401
    assert api_anon.patch(URL, json={"mention": False}).status_code == 401
