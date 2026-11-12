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


# --- nested-array ordering --------------------------------------------------
#
# Every `expand` array is ordered by the serialized model's `Meta.ordering`
# (`("-created_at",)`): the joined *target* for the M2M fleets (`Label` /
# `User`) and the related model itself for the reverse-FK fleets
# (`IssueReaction` / `IssueRelation`). The seed has one row of each, so
# without a >=2-row pin a missing `ORDER BY` stays invisible (PIDASHCONV-1143
# review). This test seeds a dedicated issue with distinct `created_at`
# values whose *through/link* timestamps are deliberately reversed, so it
# only passes when the ordering keys on the target model, not the link row.

def test_nested_array_ordering_follows_model_meta(clients, seed, db_conn):
    import uuid as _uuid
    from datetime import datetime, timedelta, timezone

    from app_issues.conftest import ISSUE_COLUMNS, USER_COLUMNS, _issue_row, _user_row

    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    ws_id = seed["ws"]
    i2, i3, i4 = seed["issue2"], seed["issue3"], seed["issue4"]
    T = datetime.now(timezone.utc)
    oid = str(_uuid.uuid4())
    la, lb = str(_uuid.uuid4()), str(_uuid.uuid4())
    ua, ub = str(_uuid.uuid4()), str(_uuid.uuid4())
    uname_a = "ordprobe_a_" + _uuid.uuid4().hex[:8]
    uname_b = "ordprobe_b_" + _uuid.uuid4().hex[:8]
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO issues (%s) VALUES (%s)"
                % (ISSUE_COLUMNS, ",".join(["%s"] * 19)),
                _issue_row(oid, "ordering probe", "low", 900,
                           seed["admin"], pid, seed["state_backlog"], ws_id),
            )
            # Labels: label.created_at new->old, issue_labels the reverse.
            for lid, name, cts in ((la, "ORD_L_A", T - timedelta(hours=3)),
                                   (lb, "ORD_L_B", T - timedelta(hours=2))):
                cur.execute(
                    "INSERT INTO labels (created_at, updated_at, id, name,"
                    " description, created_by_id, project_id, updated_by_id,"
                    " workspace_id, color, sort_order)"
                    " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                    (cts, cts, lid, name, "", seed["admin"], pid, seed["admin"],
                     ws_id, "#123456", 0.0),
                )
            for lid, lts in ((la, T - timedelta(minutes=1)),
                             (lb, T - timedelta(minutes=2))):
                cur.execute(
                    "INSERT INTO issue_labels (created_at, updated_at, id,"
                    " created_by_id, issue_id, label_id, project_id,"
                    " updated_by_id, workspace_id)"
                    " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                    (lts, lts, str(_uuid.uuid4()), seed["admin"], oid, lid,
                     pid, seed["admin"], ws_id),
                )
            # Users: user.created_at new->old, issue_assignees the reverse.
            for uid, uname, uts in ((ua, uname_a, T - timedelta(hours=3)),
                                    (ub, uname_b, T - timedelta(hours=2))):
                cur.execute(
                    "INSERT INTO users (%s) VALUES (%s)"
                    % (USER_COLUMNS, ",".join(["%s"] * 29)),
                    _user_row(uid, uname, uname + "@example.com", uname),
                )
                cur.execute("UPDATE users SET created_at=%s WHERE id=%s", (uts, uid))
            for uid, ats in ((ua, T - timedelta(minutes=10)),
                             (ub, T - timedelta(minutes=20))):
                cur.execute(
                    "INSERT INTO issue_assignees (created_at, updated_at, id,"
                    " assignee_id, created_by_id, issue_id, project_id,"
                    " updated_by_id, workspace_id)"
                    " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                    (ats, ats, str(_uuid.uuid4()), uid, seed["admin"], oid,
                     pid, seed["admin"], ws_id),
                )
            # Reactions: created_at new->old.
            for reaction, rts in (("ORD_R_A", T - timedelta(hours=3)),
                                  ("ORD_R_B", T - timedelta(hours=2))):
                cur.execute(
                    "INSERT INTO issue_reactions (created_at, updated_at, id,"
                    " reaction, actor_id, created_by_id, issue_id, project_id,"
                    " updated_by_id, workspace_id)"
                    " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                    (rts, rts, str(_uuid.uuid4()), reaction, seed["member"],
                     seed["member"], oid, pid, seed["member"], ws_id),
                )
            # Relations: outgoing (issue=oid) and incoming (related_issue=oid),
            # created_at newest-first only under `ORDER BY created_at DESC`.
            for rel_issue, rel_related, rts in (
                (oid, i2, T - timedelta(hours=4)),
                (oid, i3, T - timedelta(hours=5)),
                (i2, oid, T - timedelta(hours=7)),
                (i4, oid, T - timedelta(hours=1)),
            ):
                cur.execute(
                    "INSERT INTO issue_relations (created_at, updated_at, id,"
                    " relation_type, created_by_id, issue_id, project_id,"
                    " related_issue_id, updated_by_id, workspace_id)"
                    " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                    (rts, rts, str(_uuid.uuid4()), "relates_to", seed["admin"],
                     rel_issue, pid, rel_related, seed["admin"], ws_id),
                )

        base = _base(ws, pid)
        detail = f"{base}/issues/{oid}/"

        labels = _get(admin, detail, "labels").json()["labels"]
        assert [row["name"] for row in labels] == ["ORD_L_B", "ORD_L_A"]

        assignees = _get(admin, detail, "assignees").json()["assignees"]
        assert [row["display_name"] for row in assignees] == [uname_b, uname_a]

        reactions = _get(admin, detail, "issue_reactions").json()["issue_reactions"]
        assert [row["reaction"] for row in reactions] == ["ORD_R_B", "ORD_R_A"]

        relations = _get(admin, detail, "issue_relation").json()["issue_relation"]
        assert [row["id"] for row in relations] == [i2, i3]

        related = _get(admin, detail, "issue_related").json()["issue_related"]
        assert [row["id"] for row in related] == [i4, i2]

        # `issues-detail/`'s inline dicts share the same `-created_at` order.
        body = _get(admin, f"{base}/issues-detail/", "issue_relation").json()
        row = _row_for(body, oid)
        assert [entry["id"] for entry in row["issue_relation"]] == [i2, i3]
    finally:
        with db_conn.cursor() as cur:
            for table in ("issue_labels", "issue_assignees", "issue_reactions"):
                cur.execute("DELETE FROM %s WHERE issue_id = %%s" % table, (oid,))
            cur.execute(
                "DELETE FROM issue_relations WHERE issue_id = %s"
                " OR related_issue_id = %s",
                (oid, oid),
            )
            cur.execute("DELETE FROM labels WHERE id IN (%s,%s)", (la, lb))
            cur.execute("DELETE FROM users WHERE id IN (%s,%s)", (ua, ub))
            cur.execute("DELETE FROM issues WHERE id = %s", (oid,))


# --- M2M through-table filtering -------------------------------------------
#
# Django's forward M2M manager builds a queryset over the *target* model
# (`django/db/models/fields/related_descriptors.py:create_forward_many_to_many_manager`
# sets `self.model = rel.model`), so it applies the target model's manager
# filter — `labels.deleted_at IS NULL` for `labels` — and nothing on the
# through/link row.  `users` has no `deleted_at`, so `assignees` filters
# nothing at all.  The API's remove-label / remove-assignee path soft-deletes
# the *link* row (`issue_labels` / `issue_assignees`.`deleted_at`), so the
# item keeps appearing under `?expand=`.  This seeds a live target with a
# soft-deleted link row to pin it.

def test_m2m_expand_ignores_through_soft_delete(clients, seed, db_conn):
    import uuid as _uuid
    from datetime import datetime, timezone

    from app_issues.conftest import ISSUE_COLUMNS, USER_COLUMNS, _issue_row, _user_row

    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    ws_id = seed["ws"]
    T = datetime.now(timezone.utc)
    oid = str(_uuid.uuid4())
    lid = str(_uuid.uuid4())
    uid = str(_uuid.uuid4())
    lname = "SD_L_" + _uuid.uuid4().hex[:8]
    uname = "sd_u_" + _uuid.uuid4().hex[:8]
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO issues (%s) VALUES (%s)"
                % (ISSUE_COLUMNS, ",".join(["%s"] * 19)),
                _issue_row(oid, "through soft-delete probe", "low", 901,
                           seed["admin"], pid, seed["state_backlog"], ws_id),
            )
            # Live label; its issue_labels link row is soft-deleted.
            cur.execute(
                "INSERT INTO labels (created_at, updated_at, id, name,"
                " description, created_by_id, project_id, updated_by_id,"
                " workspace_id, color, sort_order)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (T, T, lid, lname, "", seed["admin"], pid, seed["admin"],
                 ws_id, "#123456", 0.0),
            )
            cur.execute(
                "INSERT INTO issue_labels (created_at, updated_at, id,"
                " created_by_id, issue_id, label_id, project_id,"
                " updated_by_id, workspace_id, deleted_at)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (T, T, str(_uuid.uuid4()), seed["admin"], oid, lid, pid,
                 seed["admin"], ws_id, T),
            )
            # Live user; its issue_assignees link row is soft-deleted.
            cur.execute(
                "INSERT INTO users (%s) VALUES (%s)"
                % (USER_COLUMNS, ",".join(["%s"] * 29)),
                _user_row(uid, uname, uname + "@example.com", uname),
            )
            cur.execute(
                "INSERT INTO issue_assignees (created_at, updated_at, id,"
                " assignee_id, created_by_id, issue_id, project_id,"
                " updated_by_id, workspace_id, deleted_at)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (T, T, str(_uuid.uuid4()), uid, seed["admin"], oid, pid,
                 seed["admin"], ws_id, T),
            )

        detail = f"{_base(ws, pid)}/issues/{oid}/"

        labels = _get(admin, detail, "labels").json()["labels"]
        assert lid in {row["id"] for row in labels}, labels

        assignees = _get(admin, detail, "assignees").json()["assignees"]
        assert uid in {row["id"] for row in assignees}, assignees
    finally:
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM issue_labels WHERE issue_id = %s", (oid,))
            cur.execute("DELETE FROM issue_assignees WHERE issue_id = %s", (oid,))
            cur.execute("DELETE FROM labels WHERE id = %s", (lid,))
            cur.execute("DELETE FROM users WHERE id = %s", (uid,))
            cur.execute("DELETE FROM issues WHERE id = %s", (oid,))


# --- NULL-FK created_by expansion -------------------------------------------
#
# `?expand=created_by` on an issue whose `created_by_id` is NULL renders the
# `UserLiteSerializer` initial object, not `{}`: DRF's unbound `.data` falls
# back to `get_initial()`, which keeps every writable field in field order,
# and `first_name`, `last_name`, `avatar`, `display_name` are writable
# CharFields with `initial = ''` (`app/serializers/user.py`, `read_only_fields
# = ["id", "is_bot"]`, `avatar_url` a model property).  `state`/`parent` keep
# rendering `{}` (every field read-only).  (PIDASHCONV-1381.)

CREATED_BY_INITIAL_KEYS = ["first_name", "last_name", "avatar", "display_name"]
CREATED_BY_INITIAL = {key: "" for key in CREATED_BY_INITIAL_KEYS}


def test_expand_created_by_null(clients, seed, db_conn):
    import uuid as _uuid
    from datetime import datetime, timezone

    from app_issues.conftest import ISSUE_COLUMNS, _issue_row

    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    ws_id = seed["ws"]
    T = datetime.now(timezone.utc)
    oid = str(_uuid.uuid4())
    aid = str(_uuid.uuid4())
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO issues (%s) VALUES (%s)"
                % (ISSUE_COLUMNS, ",".join(["%s"] * 19)),
                _issue_row(oid, "null creator probe", "low", 904,
                           None, pid, seed["state_backlog"], ws_id),
            )
            cur.execute(
                "INSERT INTO issues (%s) VALUES (%s)"
                % (ISSUE_COLUMNS, ",".join(["%s"] * 19)),
                _issue_row(aid, "null creator archived probe", "low", 905,
                           None, pid, seed["state_done"], ws_id),
            )
            cur.execute(
                "UPDATE issues SET archived_at = %s WHERE id = %s", (T, aid),
            )

        base = _base(ws, pid)

        # The issue retrieve.
        body = _get(admin, f"{base}/issues/{oid}/", "created_by").json()
        assert list(body["created_by"]) == CREATED_BY_INITIAL_KEYS
        assert body["created_by"] == CREATED_BY_INITIAL

        # The archived-issue retrieve.
        archived = _get(admin, f"{base}/issues/{aid}/archive/", "created_by").json()
        assert list(archived["created_by"]) == CREATED_BY_INITIAL_KEYS
        assert archived["created_by"] == CREATED_BY_INITIAL

        # The flat `issues/list/` row (`fields=id`, same shared kernel).
        flat = admin.get(
            f"{base}/issues/list/?issues={oid}&fields=id&expand=created_by"
        ).json()
        row = _row_for(flat, oid)
        assert list(row["created_by"]) == CREATED_BY_INITIAL_KEYS
        assert row["created_by"] == CREATED_BY_INITIAL
    finally:
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM issues WHERE id IN (%s,%s)", (oid, aid))
