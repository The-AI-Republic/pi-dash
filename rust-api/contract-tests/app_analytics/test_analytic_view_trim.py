"""Analytic-view name/description follow DRF ``CharField`` input rules (PIDASHCONV-1455).

``AnalyticViewSerializer`` is a ``ModelSerializer`` with ``fields="__all__"``,
so ``name`` (``CharField(max_length=255)``) and ``description``
(``TextField(blank=True)``) both get ``trim_whitespace=True``: the value is
stripped before the blank and length checks, ``int``/``float`` inputs are
coerced via ``str()``, and only ``bool`` and other non-string types are
rejected.

Each PATCH test creates its own view first so the session-shared seed view
(``AV1``, asserted on by other suites) is never mutated.
"""

BLANK = {"name": ["This field may not be blank."]}


def _stored(db_conn, view_id):
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT name, description FROM analytic_views WHERE id = %s",
            (view_id,),
        )
        row = cur.fetchone()
    assert row is not None
    return row[0], row[1]


def _create_view(admin, ws, name):
    created = admin.post(
        f"/api/workspaces/{ws}/analytic-view/", json={"name": name}
    )
    assert created.status_code == 201
    return created.json()["id"]


def test_create_blank_name_rejected(clients, seed):
    resp = clients["admin"].post(
        f"/api/workspaces/{seed['ws_slug']}/analytic-view/",
        json={"name": "   "},
    )
    assert resp.status_code == 400
    assert resp.json() == BLANK


def test_create_trims_name_and_description(clients, seed, db_conn):
    resp = clients["admin"].post(
        f"/api/workspaces/{seed['ws_slug']}/analytic-view/",
        json={"name": "  Q3 burn  ", "description": "  note  "},
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["name"] == "Q3 burn"
    assert body["description"] == "note"
    assert _stored(db_conn, body["id"]) == ("Q3 burn", "note")


def test_create_numeric_name_coerced(clients, seed, db_conn):
    resp = clients["admin"].post(
        f"/api/workspaces/{seed['ws_slug']}/analytic-view/",
        json={"name": 2024},
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["name"] == "2024"
    assert _stored(db_conn, body["id"])[0] == "2024"


def test_patch_blank_name_rejected(clients, seed, db_conn):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    vid = _create_view(admin, ws, "trim-patch-blank-src")
    resp = admin.patch(
        f"/api/workspaces/{ws}/analytic-view/{vid}/", json={"name": "   "}
    )
    assert resp.status_code == 400
    assert resp.json() == BLANK
    assert _stored(db_conn, vid)[0] == "trim-patch-blank-src"


def test_patch_trims_name_and_description(clients, seed, db_conn):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    vid = _create_view(admin, ws, "trim-patch-padded-src")
    resp = admin.patch(
        f"/api/workspaces/{ws}/analytic-view/{vid}/",
        json={"name": "  Q3 burn  ", "description": "  note  "},
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["name"] == "Q3 burn"
    assert body["description"] == "note"
    assert _stored(db_conn, vid) == ("Q3 burn", "note")


def test_patch_numeric_name_coerced(clients, seed, db_conn):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    vid = _create_view(admin, ws, "trim-patch-numeric-src")
    resp = admin.patch(
        f"/api/workspaces/{ws}/analytic-view/{vid}/", json={"name": 2024}
    )
    assert resp.status_code == 200
    assert resp.json()["name"] == "2024"
    assert _stored(db_conn, vid)[0] == "2024"
