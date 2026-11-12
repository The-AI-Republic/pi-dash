"""Label shapes (project-scoped issue labels).

Label writes are ADMIN-only; member unsafe methods are rejected by the
viewset permission class (``detail`` body), while the decorator rejects
with the ``error`` body. The suite needs Redis (``REDIS_URL``) because
label writes invalidate the cache via ``cache.keys``.
"""

LABEL_KEYS = {
    "parent", "name", "color", "id", "project_id", "workspace_id",
    "sort_order",
}

VIEWSET_FORBIDDEN = {"detail": "You do not have permission to perform this action."}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_label_list_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, l1, l2 = (
        seed["ws_slug"], seed["project"], seed["label1"], seed["label2"])
    resp = admin.get(f"{_base(ws, pid)}/issue-labels/")
    assert resp.status_code == 200
    rows = resp.json()
    assert {row["id"] for row in rows} >= {l1, l2}
    for row in rows:
        assert set(row) == LABEL_KEYS


def test_label_retrieve_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, l1 = seed["ws_slug"], seed["project"], seed["label1"]
    resp = admin.get(f"{_base(ws, pid)}/issue-labels/{l1}/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == LABEL_KEYS
    assert body["name"] == "L1"
    assert body["color"] == "#ff0000"
    assert body["parent"] is None


def test_label_member_write_denied(clients, seed):
    # The two layers deny differently: the detail route fails the
    # object-level permission (``detail`` body) while the collection
    # route passes the class check and fails the ADMIN decorator.
    member = clients["member"]
    ws, pid, l1 = seed["ws_slug"], seed["project"], seed["label1"]
    resp = member.patch(
        f"{_base(ws, pid)}/issue-labels/{l1}/", json={"description": "x"})
    assert resp.status_code == 403
    assert resp.json() == VIEWSET_FORBIDDEN
    resp = member.post(
        f"{_base(ws, pid)}/issue-labels/",
        json={"name": "L-member", "color": "#123456"})
    assert resp.status_code == 403
    assert resp.json() == {"error": "You don't have the required permissions."}


def test_label_create_patch_delete_cycle(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        f"{_base(ws, pid)}/issue-labels/",
        json={"name": "L-cycle", "color": "#123456"},
    )
    assert resp.status_code == 201
    body = resp.json()
    assert set(body) == LABEL_KEYS
    assert body["name"] == "L-cycle"
    label_id = body["id"]
    try:
        resp = admin.patch(
            f"{_base(ws, pid)}/issue-labels/{label_id}/",
            json={"description": "cycled"},
        )
        assert resp.status_code == 200
        assert resp.json()["id"] == label_id
    finally:
        assert admin.delete(
            f"{_base(ws, pid)}/issue-labels/{label_id}/").status_code == 204
    assert label_id not in {
        row["id"] for row in
        admin.get(f"{_base(ws, pid)}/issue-labels/").json()
    }


def test_label_put_roundtrip(clients, seed):
    admin = clients["admin"]
    ws, pid, l2 = seed["ws_slug"], seed["project"], seed["label2"]
    try:
        resp = admin.put(
            f"{_base(ws, pid)}/issue-labels/{l2}/",
            json={"name": "L2", "color": "#00ff00"},
        )
        assert resp.status_code == 200
        assert set(resp.json()) == LABEL_KEYS
        assert resp.json()["color"] == "#00ff00"
    finally:
        admin.patch(
            f"{_base(ws, pid)}/issue-labels/{l2}/",
            json={"color": "#0000ff"},
        )


def test_label_duplicate_name_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        f"{_base(ws, pid)}/issue-labels/",
        json={"name": "L1", "color": "#123456"},
    )
    assert resp.status_code == 400
    assert resp.json() == {"name": ["LABEL_NAME_ALREADY_EXISTS"]}


def test_label_name_and_color_are_trimmed(clients, seed):
    # NEWPIDASH-139: DRF `CharField(trim_whitespace=True)` strips
    # `name`/`color` before the blank/max-length checks and storage,
    # and `validate_name` dedupes on the stripped value. Same status
    # and body on both backends; fails on Rust before the fix.
    import uuid
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    tag = uuid.uuid4().hex[:8]
    trimmed = f"Padded {tag}"
    resp = admin.post(
        f"{_base(ws, pid)}/issue-labels/",
        json={"name": f"  {trimmed}  ", "color": "  #123456  "},
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["name"] == trimmed
    assert body["color"] == "#123456"
    label_id = body["id"]
    try:
        # Whitespace-only name is blank.
        resp = admin.post(
            f"{_base(ws, pid)}/issue-labels/",
            json={"name": "   ", "color": "#123456"},
        )
        assert resp.status_code == 400
        assert resp.json() == {"name": ["This field may not be blank."]}
        # Padded case-insensitive duplicate is refused.
        resp = admin.post(
            f"{_base(ws, pid)}/issue-labels/",
            json={"name": f" padded {tag} ", "color": "#123456"},
        )
        assert resp.status_code == 400
        assert resp.json() == {"name": ["LABEL_NAME_ALREADY_EXISTS"]}
        # Over-long whitespace is blank alone (blank pre-check runs
        # before the max-length validator), not max-length.
        resp = admin.post(
            f"{_base(ws, pid)}/issue-labels/",
            json={"name": " " * 300, "color": "#123456"},
        )
        assert resp.status_code == 400
        assert resp.json() == {"name": ["This field may not be blank."]}
        # Max-length runs on the stripped value: a padded 255-code-point
        # core passes and is stored trimmed.
        core = "n" * 255
        resp = admin.post(
            f"{_base(ws, pid)}/issue-labels/",
            json={"name": f" {core} ", "color": "#123456"},
        )
        assert resp.status_code == 201
        assert resp.json()["name"] == core
        assert admin.delete(
            f"{_base(ws, pid)}/issue-labels/{resp.json()['id']}/"
        ).status_code == 204
        # PUT and PATCH share the same trim via `validate_label_fields`.
        resp = admin.put(
            f"{_base(ws, pid)}/issue-labels/{label_id}/",
            json={"name": f"  Put {tag}  ", "color": "  #aabbcc  "},
        )
        assert resp.status_code == 200
        assert resp.json()["name"] == f"Put {tag}"
        assert resp.json()["color"] == "#aabbcc"
        resp = admin.patch(
            f"{_base(ws, pid)}/issue-labels/{label_id}/",
            json={"name": f"  Patch {tag}  "},
        )
        assert resp.status_code == 200
        assert resp.json()["name"] == f"Patch {tag}"
        # Whitespace-only color validates as `""` (`allow_blank`).
        resp = admin.patch(
            f"{_base(ws, pid)}/issue-labels/{label_id}/",
            json={"color": "   "},
        )
        assert resp.status_code == 200
        assert resp.json()["color"] == ""
    finally:
        assert admin.delete(
            f"{_base(ws, pid)}/issue-labels/{label_id}/").status_code == 204


def test_bulk_create_labels(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        f"{_base(ws, pid)}/bulk-create-labels/",
        json={"label_data": [{"name": "LB1"}, {"name": "LB2"}]},
    )
    assert resp.status_code == 201
    labels = resp.json()["labels"]
    assert {label["name"] for label in labels} == {"LB1", "LB2"}
    for label in labels:
        assert set(label) == LABEL_KEYS
        assert admin.delete(
            f"{_base(ws, pid)}/issue-labels/{label['id']}/").status_code == 204


def test_bulk_create_labels_rolls_back_on_bad_late_row(
        clients, seed, db_conn):
    # PIDASHCONV-1380: Django wraps every bulk_create batch in one
    # transaction, so a NULL name in batch 2 rolls back batch 1: the
    # whole request writes all labels or none.
    import uuid
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    tag = uuid.uuid4().hex[:8]
    payload = [{"name": "rb%s-%02d" % (tag, n)} for n in range(50)]
    payload.append({"name": None})
    resp = admin.post(
        f"{_base(ws, pid)}/bulk-create-labels/",
        json={"label_data": payload},
    )
    assert resp.status_code == 400
    assert resp.json() == {"error": "The payload is not valid"}
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM labels WHERE project_id = %s AND name LIKE %s",
            (pid, "rb%s-%%" % tag),
        )
        assert cur.fetchone()[0] == 0
