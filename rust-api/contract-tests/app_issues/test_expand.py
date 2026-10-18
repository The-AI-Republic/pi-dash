"""``?expand=`` nested expansions (PIDASHCONV-1143).

Django's ``DynamicBaseSerializer`` (``app/serializers/base.py:12-196``)
drives three of the four reads — the main issue retrieve, the archive
retrieve and the flat ``issues/list/`` branch (``IssueSerializer``) — while
the hand-written ``IssueListDetailSerializer``
(``app/serializers/issue.py:1165-1216``) drives ``issues-detail/``.  These
tests pin Django's behaviour on both backends.

Behaviours that are *bugs in Django* are pinned too (the port carries them):

- a bare ``?expand=issue_attachments`` 500s: the local
  ``IssueAttachmentLiteSerializer`` import only runs inside the per-name
  loop, so a request whose only expand name misses ``self.fields`` reaches
  the attachments leg with the name unbound;
- ``?expand=sub_issues`` 500s on the DynamicBase endpoints (``Issue`` has no
  ``sub_issues`` attribute / ``IssueLiteSerializer`` maps it) but is ignored
  on ``issues-detail/``;
- ``?expand=<declared field>`` clobbers the key with ``getattr(instance,
  "<name>_id", None)`` — ``None`` for every declared name except
  ``estimate_point``;
- unknown expand names are silently ignored.
"""

RELATION_KEYS = {
    "id", "project_id", "sequence_id", "relation_type", "name", "state_id",
    "priority", "created_by", "created_at", "updated_at", "updated_by",
}
# `IssueListDetailSerializer`'s inline dict names the relation_type only
# after `name`; the DynamicBase `IssueRelationSerializer` puts it first.
DETAIL_LIST_RELATION_KEYS = RELATION_KEYS
USER_LITE_KEYS = {
    "id", "first_name", "last_name", "avatar", "avatar_url", "is_bot",
    "display_name",
}
STATE_LITE_KEYS = {"id", "name", "color", "group"}
WORKSPACE_LITE_KEYS = {"name", "slug", "id", "logo_url"}
ATTACHMENT_KEYS = {
    "id", "asset", "attributes", "created_by", "updated_at", "updated_by",
    "asset_url",
}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _get(client, url, expand):
    sep = "&" if "?" in url else "?"
    return client.get(f"{url}{sep}expand={expand}")


def _row_for(body, issue_id):
    if isinstance(body, dict):
        body = body["results"]
    return next(row for row in body if row["id"] == issue_id)


def _endpoints(ws, pid, i1, i5):
    return {
        "detail": f"{_base(ws, pid)}/issues/{i1}/",
        "archive": f"{_base(ws, pid)}/issues/{i5}/archive/",
        "flat": f"{_base(ws, pid)}/issues/list/?issues={i1}",
        "detail_list": f"{_base(ws, pid)}/issues-detail/",
    }


# --- mapper names nest ------------------------------------------------------


def test_detail_relation_expand_nests(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i2, i3 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["issue2"],
        seed["issue3"],
    )
    resp = _get(admin, f"{_base(ws, pid)}/issues/{i1}/", "issue_relation")
    assert resp.status_code == 200
    body = resp.json()
    relations = body["issue_relation"]
    assert isinstance(relations, list) and len(relations) == 2
    assert all(set(entry) == RELATION_KEYS for entry in relations)
    by_type = {entry["relation_type"]: entry for entry in relations}
    assert by_type["relates_to"]["id"] == i2
    assert by_type["relates_to"]["name"] == "I2 child"
    assert by_type["blocked_by"]["id"] == i3
    assert by_type["blocked_by"]["name"] == "I3 done"


def test_detail_related_expand_nests(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    # I1 blocked_by I3, so I3's incoming related list holds I1.
    resp = _get(admin, f"{_base(ws, pid)}/issues/{seed['issue3']}/", "issue_related")
    assert resp.status_code == 200
    related = resp.json()["issue_related"]
    assert [entry["id"] for entry in related] == [seed["issue1"]]
    assert all(set(entry) == RELATION_KEYS for entry in related)


def test_detail_mapper_names_nest(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    for name, expected_keys in (
        ("state", STATE_LITE_KEYS),
        ("created_by", USER_LITE_KEYS),
        ("workspace", WORKSPACE_LITE_KEYS),
    ):
        resp = _get(admin, f"{_base(ws, pid)}/issues/{i1}/", name)
        assert resp.status_code == 200, name
        value = resp.json()[name]
        assert set(value) == expected_keys, name
    # labels / assignees are `many=True` fleets.
    labels = _get(admin, f"{_base(ws, pid)}/issues/{i1}/", "labels").json()["labels"]
    assert [entry["name"] for entry in labels] == ["L1"]
    assignees = _get(admin, f"{_base(ws, pid)}/issues/{i1}/", "assignees").json()[
        "assignees"
    ]
    assert [entry["display_name"] for entry in assignees] == ["is_member"]
    # issue_link is an array of link lites.
    links = _get(admin, f"{_base(ws, pid)}/issues/{i1}/", "issue_link").json()[
        "issue_link"
    ]
    assert [entry["title"] for entry in links] == ["Seed spec"]
    # parent is the empty object (DRF `get_initial`) when the FK is null.
    assert _get(admin, f"{_base(ws, pid)}/issues/{seed['issue1']}/", "parent").json()[
        "parent"
    ] == {}


def test_archive_relation_expand_nests(clients, seed):
    admin = clients["admin"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    body = _get(admin, f"{_base(ws, pid)}/issues/{i5}/archive/", "issue_relation").json()
    assert body["issue_relation"] == []
    state = _get(admin, f"{_base(ws, pid)}/issues/{i5}/archive/", "state").json()["state"]
    assert set(state) == STATE_LITE_KEYS


def test_flat_relation_expand_nests(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    body = _get(admin, f"{_base(ws, pid)}/issues/list/?issues={i1}", "issue_relation").json()
    row = _row_for(body, i1)
    assert {entry["relation_type"] for entry in row["issue_relation"]} == {
        "relates_to", "blocked_by",
    }
    assert all(set(entry) == RELATION_KEYS for entry in row["issue_relation"])


def test_detail_list_relation_expand_nests(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i3 = seed["ws_slug"], seed["project"], seed["issue1"], seed["issue3"]
    body = _get(admin, f"{_base(ws, pid)}/issues-detail/", "issue_relation").json()
    row = _row_for(body, i1)
    assert len(row["issue_relation"]) == 2
    assert all(set(entry) == DETAIL_LIST_RELATION_KEYS for entry in row["issue_relation"])
    # The `issue_related` direction on the same endpoint.
    body = _get(admin, f"{_base(ws, pid)}/issues-detail/", "issue_related").json()
    row = _row_for(body, i3)
    assert [entry["id"] for entry in row["issue_related"]] == [i1]


def test_detail_list_ignores_every_other_name(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    for name in ("state", "sub_issues", "issue_attachments", "user"):
        body = _get(admin, f"{_base(ws, pid)}/issues-detail/", name).json()
        row = _row_for(body, i1)
        assert name not in row, name
    # A declared-field expand is ignored here too: `name` stays the base value.
    body = _get(admin, f"{_base(ws, pid)}/issues-detail/", "name").json()
    assert _row_for(body, i1)["name"] == "I1 parent"


# --- issue_attachments ------------------------------------------------------


def _insert_attachment(db_conn, seed, name):
    """A fresh live attachment on I1, so the assertion is order-independent
    (``test_attachments`` soft-deletes the seeded ones at the end of its run)."""
    import uuid
    from datetime import datetime, timezone

    asset_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO file_assets (created_at, updated_at, id, attributes,"
            " asset, created_by_id, updated_by_id, workspace_id, is_deleted,"
            " is_archived, entity_type, is_uploaded, issue_id, project_id,"
            " size, user_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (now, now, asset_id, '{"name": "%s"}' % name, name,
             seed["admin"], seed["admin"], seed["ws"], False, False,
             "ISSUE_ATTACHMENT", True, seed["issue1"], seed["project"], 4,
             seed["admin"]),
        )
    db_conn.commit()
    return asset_id


def test_issue_attachments_appends_array(clients, seed, db_conn):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    asset_id = _insert_attachment(db_conn, seed, "expand-test.txt")
    resp = _get(admin, f"{_base(ws, pid)}/issues/{i1}/", "state,issue_attachments")
    assert resp.status_code == 200
    body = resp.json()
    # The mapper name still nests alongside the attachment array.
    assert set(body["state"]) == STATE_LITE_KEYS
    attachments = body["issue_attachments"]
    assert all(set(entry) == ATTACHMENT_KEYS for entry in attachments)
    assert asset_id in {entry["id"] for entry in attachments}
    mine = next(entry for entry in attachments if entry["id"] == asset_id)
    assert mine["asset"] == "expand-test.txt"
    assert mine["attributes"] == {"name": "expand-test.txt"}


def test_bare_issue_attachments_500s_like_django(clients, seed):
    """Django's import-scope bug: the attachments leg needs a loop iteration."""
    admin = clients["admin"]
    ws, pid, i1, i5 = seed["ws_slug"], seed["project"], seed["issue1"], seed["issue5"]
    eps = _endpoints(ws, pid, i1, i5)
    for name in ("detail", "archive", "flat"):
        resp = _get(admin, eps[name], "issue_attachments")
        assert resp.status_code == 500, name
        assert resp.json() == {"error": "Something went wrong please try again later"}
    # issues-detail/'s serializer ignores the name entirely.
    assert _get(admin, eps["detail_list"], "issue_attachments").status_code == 200


# --- sub_issues 500 ---------------------------------------------------------


def test_sub_issues_500s_on_dynamic_endpoints(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i5 = seed["ws_slug"], seed["project"], seed["issue1"], seed["issue5"]
    eps = _endpoints(ws, pid, i1, i5)
    for name in ("detail", "archive", "flat"):
        resp = _get(admin, eps[name], "sub_issues")
        assert resp.status_code == 500, name
        assert resp.json() == {"error": "Something went wrong please try again later"}
    # `IssueListDetailSerializer` never touches it.
    assert _get(admin, eps["detail_list"], "sub_issues").status_code == 200


def test_non_mapper_names_500(clients, seed):
    """Mapped names that are not `Issue` attributes raise in the base render."""
    admin = clients["admin"]
    ws, pid, i1, i5 = seed["ws_slug"], seed["project"], seed["issue1"], seed["issue5"]
    eps = _endpoints(ws, pid, i1, i5)
    for name in ("user", "default_assignee", "project_lead", "actor", "members"):
        for endpoint in ("detail", "archive", "flat"):
            assert _get(admin, eps[endpoint], name).status_code == 500, (name, endpoint)


# --- declared-field clobber -------------------------------------------------


def test_declared_field_expand_clobbers_to_null(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i5 = seed["ws_slug"], seed["project"], seed["issue1"], seed["issue5"]
    eps = _endpoints(ws, pid, i1, i5)
    for endpoint in ("detail", "archive", "flat"):
        body = _get(admin, eps[endpoint], "name").json()
        if endpoint == "flat":
            body = body[0]
        assert body["name"] is None, endpoint
    # `created_by` is declared *and* mapped: it nests instead of clobbering.
    nested = _get(admin, eps["detail"], "created_by").json()["created_by"]
    assert set(nested) == USER_LITE_KEYS


# --- unknown names ----------------------------------------------------------


def test_unknown_expand_name_is_ignored(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i5 = seed["ws_slug"], seed["project"], seed["issue1"], seed["issue5"]
    eps = _endpoints(ws, pid, i1, i5)
    for endpoint in eps:
        resp = _get(admin, eps[endpoint], "unknown_xyz")
        assert resp.status_code == 200, endpoint
        body = resp.json()
        row = _row_for(body, i1) if endpoint in ("flat", "detail_list") else body
        assert "unknown_xyz" not in row, endpoint
