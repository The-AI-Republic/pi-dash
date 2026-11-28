"""Rich ``filters=`` JSON: UUID lists, dates and datetimes (PIDASHCONV-1317).

The web client sends ``{"and": [{"state_id__in": "<uuid>,<uuid>"}]}``
(arrays joined with commas). Django cleans each value into a typed
UUID / date / datetime (naive datetimes interpreted in the request
user's timezone) and returns 200 with the filtered rows; pre-fix Rust
bound them as TEXT params, so Postgres raised 42883 and the list 500d.
"""

import json
import uuid

import pytest


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _filtered(client, base, tree):
    return client.get(f"{base}/issues/", params={"filters": json.dumps(tree)})


def _ids(body):
    return {row["id"] for row in body["results"]}


@pytest.fixture()
def dated_issues(clients, seed, db_conn):
    """Two API-created issues with SQL-pinned dates, deleted on teardown."""
    admin = clients["admin"]
    base = _base(seed["ws_slug"], seed["project"])
    made = []
    try:
        for name in ("RF1 dated", "RF2 dated"):
            resp = admin.post(f"{base}/issues/", json={"name": name})
            assert resp.status_code == 201, resp.text
            made.append(resp.json()["id"])
        rf1, rf2 = made
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE issues SET start_date = %s, target_date = %s,"
                " created_at = %s, updated_at = %s WHERE id = %s",
                ("2026-09-01", "2026-10-01",
                 "2026-05-01T12:00:00+00:00", "2026-05-02T12:00:00+00:00",
                 rf1),
            )
            cur.execute(
                "UPDATE issues SET start_date = %s, target_date = %s,"
                " created_at = %s, updated_at = %s WHERE id = %s",
                ("2026-11-01", "2026-11-15",
                 "2026-07-01T12:00:00+00:00", "2026-07-02T12:00:00+00:00",
                 rf2),
            )
        yield {"rf1": rf1, "rf2": rf2}
    finally:
        for issue_id in made:
            admin.delete(f"{base}/issues/{issue_id}/")


def test_state_id_in(clients, seed):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    backlog, done = seed["state_backlog"], seed["state_done"]
    resp = _filtered(member, base, {"and": [{"state_id__in": backlog}]})
    assert resp.status_code == 200, resp.text
    # Draft I4 shares the backlog state but is excluded from the list.
    assert _ids(resp.json()) == {seed["issue1"], seed["issue2"]}
    resp = _filtered(
        member, base, {"and": [{"state_id__in": f"{backlog},{done}"}]})
    assert resp.status_code == 200, resp.text
    assert _ids(resp.json()) == {
        seed["issue1"], seed["issue2"], seed["issue3"]}


def test_assignee_id_in(clients, seed):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = _filtered(
        member, base, {"and": [{"assignee_id__in": seed["member"]}]})
    assert resp.status_code == 200, resp.text
    assert _ids(resp.json()) == {seed["issue1"]}


def test_assignee_id_in_plus_legacy_assignees(clients, seed, db_conn):
    """NEWPIDASH-195 (project-list twin): a rich assignee leaf AND a
    legacy assignees= key on the same relation must join the link
    table twice (Django applies two .filter() calls), so an issue
    assigned to both users matches. Pre-fix Rust ANDed both
    predicates on one join alias and returned nothing whenever the
    two users differ. I1 is seeded assigned to member; this test
    adds a second (admin) link and queries the pair."""
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO issue_assignees (created_at, updated_at, id,"
            " assignee_id, created_by_id, issue_id, project_id,"
            " updated_by_id, workspace_id)"
            " VALUES (NOW(), NOW(), %s, %s, %s, %s, %s, %s, %s)",
            (str(uuid.uuid4()), seed["admin"], seed["admin"], seed["issue1"],
             seed["project"], seed["admin"], seed["ws"]),
        )
    try:
        resp = member.get(
            f"{base}/issues/",
            params={
                "filters": json.dumps({"assignee_id__in": seed["member"]}),
                "assignees": seed["admin"],
            },
        )
        assert resp.status_code == 200, resp.text
        assert _ids(resp.json()) == {seed["issue1"]}
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "DELETE FROM issue_assignees WHERE issue_id = %s"
                " AND assignee_id = %s",
                (seed["issue1"], seed["admin"]),
            )


def test_target_date_exact(clients, seed, dated_issues):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = _filtered(member, base, {"and": [{"target_date": "2026-10-01"}]})
    assert resp.status_code == 200, resp.text
    ids = _ids(resp.json())
    assert dated_issues["rf1"] in ids
    assert dated_issues["rf2"] not in ids
    assert seed["issue1"] not in ids


def test_start_date_range(clients, seed, dated_issues):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = _filtered(
        member, base,
        {"and": [{"start_date__range": "2026-08-01,2026-10-01"}]})
    assert resp.status_code == 200, resp.text
    ids = _ids(resp.json())
    assert dated_issues["rf1"] in ids
    assert dated_issues["rf2"] not in ids
    assert seed["issue1"] not in ids


def test_created_at_range(clients, seed, dated_issues):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = _filtered(
        member, base,
        {"and": [{
            "created_at__range":
                "2026-04-01T00:00:00Z,2026-06-01T00:00:00Z"}]})
    assert resp.status_code == 200, resp.text
    ids = _ids(resp.json())
    assert dated_issues["rf1"] in ids
    assert dated_issues["rf2"] not in ids
    assert seed["issue1"] not in ids


def test_naive_datetime_range_uses_user_timezone(clients, seed, db_conn,
                                                  dated_issues):
    """Naive `__range` bounds resolve in the request user's timezone too.
    Pre-fix Rust inlined the bounds as untyped literals, leaving them to
    the Postgres session zone (wrong rows whenever it differs from the
    user's zone); post-fix both backends resolve them in the user's zone.
    RF1 was created at 12:00 UTC, inside 07:00-09:00 New York wall."""
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE users SET user_timezone = %s WHERE id = %s",
            ("America/New_York", seed["member"]),
        )
    try:
        resp = _filtered(
            member, base,
            {"and": [{
                "created_at__range":
                    "2026-05-01 07:00:00,2026-05-01 09:00:00"}]})
        assert resp.status_code == 200, resp.text
        ids = _ids(resp.json())
        assert dated_issues["rf1"] in ids
        assert dated_issues["rf2"] not in ids
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET user_timezone = %s WHERE id = %s",
                ("UTC", seed["member"]),
            )


def test_naive_datetime_uses_user_timezone(clients, seed, db_conn,
                                           dated_issues):
    """Naive datetimes resolve in the request user's timezone (Django
    ``TimezoneMixin`` activates ``user_timezone``). RF1 was created at
    12:00 UTC, which is 08:00 on the wall clock in America/New_York
    (EDT), so the naive ``08:00`` filter matches it there."""
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE users SET user_timezone = %s WHERE id = %s",
            ("America/New_York", seed["member"]),
        )
    try:
        resp = _filtered(
            member, base,
            {"and": [{"created_at": "2026-05-01 08:00:00"}]})
        assert resp.status_code == 200, resp.text
        ids = _ids(resp.json())
        assert dated_issues["rf1"] in ids
        assert dated_issues["rf2"] not in ids
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET user_timezone = %s WHERE id = %s",
                ("UTC", seed["member"]),
            )


def test_state_id_in_empty_string_matches_nothing(clients, seed):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = _filtered(member, base, {"and": [{"state_id__in": ""}]})
    assert resp.status_code == 200, resp.text
    assert _ids(resp.json()) == set()


def test_state_id_in_blank_part_dropped(clients, seed):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = _filtered(
        member, base,
        {"and": [{"state_id__in": f"{seed['state_backlog']},"}]})
    assert resp.status_code == 200, resp.text
    assert _ids(resp.json()) == {seed["issue1"], seed["issue2"]}


def test_us_datetime_long_fraction_rejected(clients, seed):
    """US datetimes with 7-9 digit fractions are a 400 on both backends
    (PIDASHCONV-1479). Django's ``parse_datetime`` returns None for `/`
    strings, and the ``strptime`` ``%f`` fallback takes at most 6 digits
    (``unconverted data remains``); pre-fix Rust took up to 9 via
    chrono ``%.f`` and truncated to micros (200)."""
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    for raw in ("05/01/2026 08:00:00.1234567",
                "05/01/2026 08:00:00.12345678",
                "05/01/2026 08:00:00.123456789",
                "05/01/26 08:00:00.1234567"):
        resp = _filtered(member, base, {"and": [{"created_at": raw}]})
        assert resp.status_code == 400, (raw, resp.text)
        assert resp.json()["code"] == "invalid_filterset", (raw, resp.text)
    # A long-fraction bound 400s a range too (same cleaning path).
    resp = _filtered(
        member, base,
        {"and": [{
            "created_at__range":
                "05/01/2026 08:00:00.1234567,05/02/2026 08:00:00"}]})
    assert resp.status_code == 400, resp.text
    assert resp.json()["code"] == "invalid_filterset", resp.text


def test_us_datetime_six_digit_fraction_accepted(clients, seed):
    """The 6-digit boundary still cleans on both backends (PIDASHCONV-1479):
    6 digits is exactly what Django's ``strptime`` ``%f`` accepts."""
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = _filtered(
        member, base,
        {"and": [{"created_at": "05/01/2026 08:00:00.123456"}]})
    assert resp.status_code == 200, resp.text


@pytest.fixture()
def negated_relation_issues(clients, seed, db_conn):
    """NEWPIDASH-194: U (no links), Xonly (X), XY (X+Y + L2), Lonly (L2).

    Project IS holds seed I1 (X) + I2/I3 (bare) plus these four, so NOT
    assertions are subset-based (Xonly/XY excluded, U/Lonly kept).
    Issues are API-created like `dated_issues`; assignee/label rows are
    SQL so the test controls live links exactly. Deleted on teardown.
    """
    admin = clients["admin"]
    base = _base(seed["ws_slug"], seed["project"])
    x = seed["member"]
    y = seed["admin"]
    made = []
    label_id = str(uuid.uuid4())
    try:
        for name in ("NR U", "NR X", "NR XY", "NR L"):
            resp = admin.post(f"{base}/issues/", json={"name": name})
            assert resp.status_code == 201, resp.text
            made.append(resp.json()["id"])
        u, xonly, xy, lonly = made
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO labels (created_at, updated_at, id, name,"
                " description, created_by_id, project_id, updated_by_id,"
                " workspace_id, color, sort_order)"
                " VALUES (NOW(), NOW(), %s, %s, %s, %s, %s, %s, %s, %s, %s)",
                (
                    label_id,
                    "NR L2",
                    "",
                    seed["admin"],
                    seed["project"],
                    seed["admin"],
                    seed["ws"],
                    "#ff0000",
                    0.0,
                ),
            )
            # Xonly: X only. XY: X + Y + L2. Lonly: L2 only. U: none.
            for issue_id, assignee in ((xonly, x), (xy, x), (xy, y)):
                cur.execute(
                    "INSERT INTO issue_assignees (created_at, updated_at, id,"
                    " assignee_id, created_by_id, issue_id, project_id,"
                    " updated_by_id, workspace_id)"
                    " VALUES (NOW(), NOW(), %s, %s, %s, %s, %s, %s, %s)",
                    (
                        str(uuid.uuid4()),
                        assignee,
                        seed["admin"],
                        issue_id,
                        seed["project"],
                        seed["admin"],
                        seed["ws"],
                    ),
                )
            for issue_id in (xy, lonly):
                cur.execute(
                    "INSERT INTO issue_labels (created_at, updated_at, id,"
                    " created_by_id, issue_id, label_id, project_id,"
                    " updated_by_id, workspace_id)"
                    " VALUES (NOW(), NOW(), %s, %s, %s, %s, %s, %s, %s)",
                    (
                        str(uuid.uuid4()),
                        seed["admin"],
                        issue_id,
                        label_id,
                        seed["project"],
                        seed["admin"],
                        seed["ws"],
                    ),
                )
        yield {"u": u, "xonly": xonly, "xy": xy, "lonly": lonly,
               "x": x, "y": y, "label": label_id}
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "DELETE FROM issue_labels WHERE issue_id = ANY(%s)",
                (made,),
            )
            cur.execute(
                "DELETE FROM issue_assignees WHERE issue_id = ANY(%s)",
                (made,),
            )
            cur.execute("DELETE FROM labels WHERE id = %s", (label_id,))
        for issue_id in made:
            admin.delete(f"{base}/issues/{issue_id}/")


def test_not_assignee_keeps_unassigned_excludes_x_and_xy(
    clients, seed, negated_relation_issues
):
    """NEWPIDASH-194: NOT assignee X is Django split_exclude (NOT EXISTS):
    U and Lonly (no X link) stay, Xonly and XY (both carry X) drop out.
    Pre-fix Rust applied NOT per INNER-joined row, returning XY's Y row
    and dropping U/Lonly."""
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    ids = _ids(_filtered(
        member, base,
        {"not": {"assignee_id__in": negated_relation_issues["x"]}},
    ).json())
    assert negated_relation_issues["u"] in ids
    assert negated_relation_issues["lonly"] in ids
    assert negated_relation_issues["xonly"] not in ids
    assert negated_relation_issues["xy"] not in ids


def test_not_label_keeps_unlabelled_excludes_labelled(
    clients, seed, negated_relation_issues
):
    """NEWPIDASH-194: NOT label L2 keeps U + Xonly, excludes XY + Lonly."""
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    ids = _ids(_filtered(
        member, base,
        {"not": {"label_id__in": negated_relation_issues["label"]}},
    ).json())
    assert negated_relation_issues["u"] in ids
    assert negated_relation_issues["xonly"] in ids
    assert negated_relation_issues["xy"] not in ids
    assert negated_relation_issues["lonly"] not in ids


def test_or_assignee_label_matches_either_branch(
    clients, seed, negated_relation_issues
):
    """NEWPIDASH-194: OR over two relations is Django LEFT OUTER joins:
    Xonly (assignee only, no label) and Lonly (label only) both match.
    Pre-fix Rust INNER-joined both tables, so only XY (both) matched."""
    # Drive through the member client: PROJECT list is DISTINCT, so each
    # matching issue appears once; Xonly has no label row and Lonly has
    # no assignee row, so either INNER join drops one of them.
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = _filtered(
        member, base,
        {"or": [
            {"assignee_id": negated_relation_issues["x"]},
            {"label_id": negated_relation_issues["label"]},
        ]},
    )
    assert resp.status_code == 200, resp.text
    ids = _ids(resp.json())
    assert negated_relation_issues["xonly"] in ids
    assert negated_relation_issues["lonly"] in ids
    assert negated_relation_issues["xy"] in ids
    assert negated_relation_issues["u"] not in ids
