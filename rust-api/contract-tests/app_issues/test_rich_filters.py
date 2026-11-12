"""Rich ``filters=`` JSON: UUID lists, dates and datetimes (PIDASHCONV-1317).

The web client sends ``{"and": [{"state_id__in": "<uuid>,<uuid>"}]}``
(arrays joined with commas). Django cleans each value into a typed
UUID / date / datetime (naive datetimes interpreted in the request
user's timezone) and returns 200 with the filtered rows; pre-fix Rust
bound them as TEXT params, so Postgres raised 42883 and the list 500d.
"""

import json

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
