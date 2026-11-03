"""v2 user-asset link/unlink apply the ``User.save()`` column effects (PIDASHCONV-1397).

``entity_asset_save`` / ``entity_asset_delete`` (``app/views/asset/v2.py:40-107``)
funnel avatar/cover links through a full ``user.save()``
(``db/models/user.py:169-187``), which on top of the avatar/cover write:

- bumps ``updated_at`` (``auto_now``);
- rotates ``token`` / ``token_updated_at`` whenever ``token_updated_at``
  is already set (every login sets it);
- normalizes a stored email (lower + strip);
- fills an empty ``display_name`` from the normalized email prefix;
- sets ``is_staff`` for superusers.

Each test below drives one link (``PATCH .../user-assets/<id>/`` with
``{}``) and one unlink (``DELETE``) and pins the row effects of both.
"""

from . import seed_assets as seed_a

COLS = (
    "email, display_name, is_staff, is_superuser, token,"
    " token_updated_at, updated_at, avatar_asset_id, cover_image_asset_id"
)


def base():
    return "/api/assets/v2/user-assets/"


def detail(asset_id):
    return f"/api/assets/v2/user-assets/{asset_id}/"


def _row(conn, uid):
    values = conn.execute(
        f"SELECT {COLS} FROM users WHERE id = %s", (uid,)
    ).fetchone()
    return dict(zip([c.strip() for c in COLS.split(",")], values))


def _rotating_user(org):
    """Give ``org.admin`` login-shaped state: a token in use, an
    unnormalized email, an empty display name, and superuser without
    staff — so every ``save()`` effect is observable."""
    uid = org.admin["id"]
    org.conn.execute(
        "UPDATE users SET token_updated_at = now() - interval '1 hour',"
        " updated_at = now() - interval '1 hour',"
        " email = '  MixedCase-' || %s || '@CT.Example.COM  ',"
        " display_name = '', is_superuser = true, is_staff = false"
        " WHERE id = %s",
        (org.tag, uid),
    )
    return uid


def _post(org, entity_type):
    r = org.request(
        "POST", base(), "admin",
        json={
            "name": "pic.png",
            "type": "image/png",
            "size": 512,
            "entity_type": entity_type,
        },
    )
    assert r.status_code == 200, r.text
    return r.json()["asset_id"]


def _assert_save_effects(before, after):
    assert after["updated_at"] != before["updated_at"], "save() must bump updated_at"
    assert after["token"] != before["token"], "save() must rotate token"
    assert len(after["token"]) == 64, after["token"]
    assert after["token_updated_at"] != before["token_updated_at"]
    assert after["token_updated_at"] > before["token_updated_at"]


def _assert_first_save_normalization(after):
    assert after["email"] == after["email"].lower().strip(), after["email"]
    assert after["display_name"] == after["email"].split("@")[0], after
    assert after["is_staff"] is True


def test_avatar_link_and_unlink_apply_save_effects(org):
    uid = _rotating_user(org)
    asset_id = _post(org, "USER_AVATAR")

    before = _row(org.conn, uid)
    r = org.request("PATCH", detail(asset_id), "admin", json={})
    assert r.status_code == 204, r.text
    after = _row(org.conn, uid)
    assert str(after["avatar_asset_id"]) == asset_id
    assert after["cover_image_asset_id"] is None
    _assert_save_effects(before, after)
    _assert_first_save_normalization(after)

    before = after
    r = org.request("DELETE", detail(asset_id), "admin")
    assert r.status_code == 204, r.text
    after = _row(org.conn, uid)
    assert after["avatar_asset_id"] is None
    assert after["cover_image_asset_id"] is None
    _assert_save_effects(before, after)
    row = seed_a.fetch(org.conn, asset_id)
    assert row["is_deleted"] is True


def test_cover_link_and_unlink_apply_save_effects(org):
    uid = _rotating_user(org)
    asset_id = _post(org, "USER_COVER")

    before = _row(org.conn, uid)
    r = org.request("PATCH", detail(asset_id), "admin", json={})
    assert r.status_code == 204, r.text
    after = _row(org.conn, uid)
    assert after["avatar_asset_id"] is None
    assert str(after["cover_image_asset_id"]) == asset_id
    _assert_save_effects(before, after)
    _assert_first_save_normalization(after)

    before = after
    r = org.request("DELETE", detail(asset_id), "admin")
    assert r.status_code == 204, r.text
    after = _row(org.conn, uid)
    assert after["avatar_asset_id"] is None
    assert after["cover_image_asset_id"] is None
    _assert_save_effects(before, after)
    row = seed_a.fetch(org.conn, asset_id)
    assert row["is_deleted"] is True
