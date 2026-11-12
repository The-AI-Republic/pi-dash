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


# Valid JSON that is not an object reaches the preference serializer directly
# (``UserNotificationPreferenceEndpoint.patch`` passes ``request.data`` to the
# serializer), so DRF answers 400 ``non_field_errors`` naming the Python type
# (``rest_framework/serializers.py``). The workspace notification detail PATCH
# instead treats ``request.data`` as a mapping and 500s on the same bodies.
NON_OBJECT_BODIES = [
    (b"[]", "list"),
    (b'"x"', "str"),
    (b"5", "int"),
    (b"true", "bool"),
]


def test_preferences_patch_non_object_body_400(ctx, api_owner):
    for raw, drf_type in NON_OBJECT_BODIES:
        r = api_owner.patch(
            URL, content=raw, headers={"Content-Type": "application/json"}
        )
        assert r.status_code == 400, (raw, r.status_code, r.text)
        assert r.json() == {
            "non_field_errors": [
                f"Invalid data. Expected a dictionary, but got {drf_type}."
            ]
        }, (raw, r.text)


def test_preferences_patch_null_body_400(ctx, api_owner):
    r = api_owner.patch(URL, content=b"null", headers={"Content-Type": "application/json"})
    assert r.status_code == 400, (r.status_code, r.text)
    assert r.json() == {"non_field_errors": ["No data provided"]}


def _detail_url(slug, pk):
    return f"/api/workspaces/{slug}/users/notifications/{pk}/"


def test_workspace_notification_patch_non_object_body_500(ctx, api_owner, mknotif):
    n = mknotif(title="non-object body")
    for raw, _ in NON_OBJECT_BODIES:
        r = api_owner.patch(
            _detail_url(ctx.slug_a, n["id"]),
            content=raw,
            headers={"Content-Type": "application/json"},
        )
        assert r.status_code == 500, (raw, r.status_code, r.text)
        assert r.json() == {"error": "Something went wrong please try again later"}, (
            raw,
            r.text,
        )
