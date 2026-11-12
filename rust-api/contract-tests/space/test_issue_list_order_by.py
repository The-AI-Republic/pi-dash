"""Contract: public issue list ``order_by`` validation.

``ProjectIssuesPublicEndpoint.get`` passes the raw ``order_by`` parameter to
Django's ``order_issue_queryset`` (``space/views/issue.py:78,129-130``),
which resolves it against the model (``utils/order_queryset.py:49-57``).
An unknown name raises ``FieldError`` and the generic 500 envelope; no user
text reaches SQL. The Rust port used to interpolate the value into its
``SELECT``/``ORDER BY``. These tests pin the Django behavior and would hang
on the old port (the payload sleeps in the database).
"""

import time

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"
SERVER_ERROR = {"error": "Something went wrong please try again later"}
# The trailing `"issues"."id"` qualification is what makes the injected row
# expression valid in the port's grouped projection; it is still raw SQL from
# an unauthenticated query parameter.
INJECTION = 'id",(SELECT 1 FROM pg_sleep(3)),"issues"."id'


def _issues_url(world):
    return f"{BASE}/anchor/{world['anchor']}/issues/"


def test_issue_list_order_by_rejects_sql_injection(anon_client, world):
    started = time.monotonic()
    response = anon_client.get(_issues_url(world), params={"order_by": INJECTION})
    elapsed = time.monotonic() - started
    assert response.status_code == 500
    assert response.json() == SERVER_ERROR
    assert elapsed < 1.0, f"injected order_by took {elapsed:.2f}s"


@pytest.mark.parametrize("value", ["bogus", "+created_at", "", "name__bogus", "state__bogus"])
def test_issue_list_order_by_unknown_field_is_500(anon_client, world, value):
    response = anon_client.get(_issues_url(world), params={"order_by": value})
    assert response.status_code == 500
    assert response.json() == SERVER_ERROR


def test_issue_list_order_by_valid_fields_still_sort(anon_client, world, seeder, db):
    alpha = seeder.create_state(world["workspace"]["id"], world["project"]["id"], name="Alpha", group="unstarted")
    zeta = seeder.create_state(world["workspace"]["id"], world["project"]["id"], name="Zeta", group="unstarted")
    a = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], alpha["id"],
        name="order-a", priority="urgent", sequence_id=5,
    )
    z = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], zeta["id"],
        name="order-z", priority="low", sequence_id=2,
    )
    b = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], world["state"]["id"],
        name="order-b", priority="high", sequence_id=7,
    )
    # Distinct, stable timestamps: the tie-break is `-created_at`.
    for hours, issue in ((3, a), (2, z), (1, b)):
        db.execute(
            "UPDATE issues SET created_at = now() - make_interval(hours => %s) WHERE id = %s",
            (hours, issue["id"]),
        )

    def names(order_by):
        response = anon_client.get(_issues_url(world), params={"order_by": order_by})
        assert response.status_code == 200, response.text
        return [row["name"] for row in response.json()["results"]]

    assert names("-created_at") == ["Contract issue", "order-b", "order-z", "order-a"]
    assert names("sequence_id") == ["Contract issue", "order-z", "order-a", "order-b"]
    assert names("-sequence_id") == ["order-b", "order-a", "order-z", "Contract issue"]
    assert names("priority") == ["Contract issue", "order-z", "order-b", "order-a"]
    assert names("state__name") == ["order-a", "Contract issue", "order-b", "order-z"]


@pytest.mark.parametrize(
    "value",
    [
        "project__default_assignee",
        "project__workspace",
        "state__project",
        "parent__state",
        "workspace__owner",
    ],
)
def test_issue_list_order_by_relation_fk_field_aliases_are_valid(anon_client, world, value):
    response = anon_client.get(_issues_url(world), params={"order_by": value})
    assert response.status_code == 200, response.text
    assert [row["name"] for row in response.json()["results"]] == ["Contract issue"]


def test_issue_list_order_by_view_annotations_sort(anon_client, world, seeder, db):
    """PIDASHCONV-1633: ``order_by`` on the view's annotations sorts (200).

    ``space/views/issue.py:98-125`` annotates the public list queryset with
    ``cycle_id``, ``link_count``, ``attachment_count`` and
    ``sub_issues_count``; ``order_issue_queryset``'s ``else`` branch
    (``utils/order_queryset.py:49-57``) resolves those names and orders by
    ``(annotation, -created_at)``. The Rust port failed closed with a 500.
    """
    ws_id = world["workspace"]["id"]
    proj_id = world["project"]["id"]
    state_id = world["state"]["id"]
    owner_id = world["owner"]["id"]
    cycle_a = seeder.create_cycle(ws_id, proj_id, owner_id, name="Alpha")
    cycle_b = seeder.create_cycle(ws_id, proj_id, owner_id, name="Beta")

    def stamp(issue_id, minutes_ago):
        db.execute(
            "UPDATE issues SET created_at = now() - make_interval(mins => %s) WHERE id = %s",
            (minutes_ago, issue_id),
        )

    def add_issue(name, links, attachments, kids, cycle_ids, minutes_ago):
        issue = seeder.create_issue(ws_id, proj_id, state_id, name=name)
        issue_id = issue["id"]
        for _ in range(links):
            seeder.create_issue_link(ws_id, proj_id, issue_id)
        for _ in range(attachments):
            seeder.create_issue_attachment(ws_id, proj_id, issue_id)
        for num in range(kids):
            kid = seeder.create_issue(ws_id, proj_id, state_id, name=f"{name}-kid{num}")
            db.execute(
                "UPDATE issues SET parent_id = %s WHERE id = %s",
                (issue_id, kid["id"]),
            )
        for cycle_id in cycle_ids:
            db.execute(
                """INSERT INTO cycle_issues
                   (id, workspace_id, project_id, cycle_id, issue_id, created_at, updated_at)
                   VALUES (gen_random_uuid(),%s,%s,%s,%s,now(),now())""",
                (ws_id, proj_id, cycle_id, issue_id),
            )
        stamp(issue_id, minutes_ago)
        return issue_id

    # A: 2 links, 1 attachment, 2 kids; live Beta link. B: 0 links,
    # 3 attachments, 0 kids; live Alpha link plus a *deleted* Beta bridge
    # (which must not affect its pick). C: 1 link, 0 attachments, 1 kid;
    # no cycle.
    add_issue("anno-A", 2, 1, 2, [cycle_b["id"]], 360)
    issue_b = add_issue("anno-B", 0, 3, 0, [cycle_a["id"]], 300)
    db.execute(
        """INSERT INTO cycle_issues
           (id, workspace_id, project_id, cycle_id, issue_id, created_at, updated_at, deleted_at)
           VALUES (gen_random_uuid(),%s,%s,%s,%s,now(),now(),now())""",
        (ws_id, proj_id, cycle_b["id"], issue_b),
    )
    add_issue("anno-C", 1, 0, 1, [], 180)
    # The world's own issue and every kid join the list; pin their
    # timestamps so the `-created_at` tiebreak is deterministic.
    stamp(world["issue"]["id"], 240)
    for minutes_ago, name in ((150, "anno-A-kid0"), (120, "anno-A-kid1"), (90, "anno-C-kid0")):
        db.execute(
            "UPDATE issues SET created_at = now() - make_interval(mins => %s)"
            " WHERE project_id = %s AND name = %s",
            (minutes_ago, proj_id, name),
        )

    # `-created_at` order, newest first: C-kid0, A-kid1, A-kid0, C,
    # Contract, B, A.
    zeros_newest_first = ["anno-C-kid0", "anno-A-kid1", "anno-A-kid0"]
    first, second = (
        ("anno-B", "anno-A") if cycle_a["id"] < cycle_b["id"] else ("anno-A", "anno-B")
    )
    expected = {
        "-link_count": ["anno-A", "anno-C", *zeros_newest_first, "Contract issue", "anno-B"],
        "link_count": [*zeros_newest_first, "Contract issue", "anno-B", "anno-C", "anno-A"],
        "-attachment_count": ["anno-B", "anno-A", *zeros_newest_first, "anno-C", "Contract issue"],
        "attachment_count": [*zeros_newest_first, "anno-C", "Contract issue", "anno-A", "anno-B"],
        "-sub_issues_count": ["anno-A", "anno-C", *zeros_newest_first, "Contract issue", "anno-B"],
        "sub_issues_count": [*zeros_newest_first, "Contract issue", "anno-B", "anno-C", "anno-A"],
        # UUIDs sort ascending; NULL cycles trail by `-created_at`.
        "cycle_id": [first, second, *zeros_newest_first, "anno-C", "Contract issue"],
    }

    for order_by, names in expected.items():
        response = anon_client.get(_issues_url(world), params={"order_by": order_by})
        assert response.status_code == 200, (order_by, response.text)
        body = response.json()
        assert [row["name"] for row in body["results"]] == names, order_by
        assert body["count"] == 7, order_by


def test_issue_list_order_by_desc_nullable_keys_nulls_last(anon_client, world, seeder, db):
    """PIDASHCONV-1643: nullable DESC keys sort NULLs last on both backends.

    The issue alleged Rust renders ``NULLS LAST`` while Django's plain
    ``order_by`` in ``order_queryset.py:49-57`` leaves the Postgres default
    (``DESC`` -> ``NULLS FIRST``). Live Django disproves it: the paginator
    re-orders with explicit ``nulls_last=True`` in both directions
    (``OffsetPaginator.get_result``, ``paginator.py:136-139``, and the
    grouped variants at ``:253-265`` / ``:461-470``), superseding the
    ``order_issue_queryset`` ordering — so both backends sort NULLs last
    on ``-start_date``, ``-target_date`` and ``-cycle_id``, ungrouped and
    grouped. This test pins that agreed row order.
    """
    ws_id = world["workspace"]["id"]
    proj_id = world["project"]["id"]
    state_id = world["state"]["id"]
    cycle = seeder.create_cycle(ws_id, proj_id, world["owner"]["id"], name="Nulls cycle")

    def stamp(issue_id, start, target, minutes_ago):
        db.execute(
            "UPDATE issues SET start_date = %s, target_date = %s,"
            " created_at = now() - make_interval(mins => %s) WHERE id = %s",
            (start, target, minutes_ago, issue_id),
        )

    def add_issue(name, start, target, minutes_ago, link_cycle=False):
        issue = seeder.create_issue(ws_id, proj_id, state_id, name=name)
        stamp(issue["id"], start, target, minutes_ago)
        if link_cycle:
            db.execute(
                """INSERT INTO cycle_issues
                   (id, workspace_id, project_id, cycle_id, issue_id, created_at, updated_at)
                   VALUES (gen_random_uuid(),%s,%s,%s,%s,now(),now())""",
                (ws_id, proj_id, cycle["id"], issue["id"]),
            )
        return issue["id"]

    # Dated rows sort among themselves; the two NULL rows expose the
    # placement (NULLS LAST both directions, `-created_at` within).
    stamp(world["issue"]["id"], "2026-03-15", "2026-04-15", 100)
    add_issue("nulls-A", None, None, 300)
    add_issue("nulls-B", None, None, 20)
    add_issue("nulls-old", "2026-01-10", "2026-02-10", 200)
    add_issue("nulls-new", "2026-06-01", "2026-07-01", 10, link_cycle=True)

    nulls_by_created = ["nulls-B", "Contract issue", "nulls-old", "nulls-A"]
    expected = {
        "-start_date": ["nulls-new", "Contract issue", "nulls-old", "nulls-B", "nulls-A"],
        "start_date": ["nulls-old", "Contract issue", "nulls-new", "nulls-B", "nulls-A"],
        "-target_date": ["nulls-new", "Contract issue", "nulls-old", "nulls-B", "nulls-A"],
        "-cycle_id": ["nulls-new", *nulls_by_created],
        "cycle_id": ["nulls-new", *nulls_by_created],
    }

    for order_by, names in expected.items():
        response = anon_client.get(_issues_url(world), params={"order_by": order_by})
        assert response.status_code == 200, (order_by, response.text)
        body = response.json()
        assert [row["name"] for row in body["results"]] == names, order_by
        assert body["count"] == 5, order_by

    # Grouped path: the window/outer orderings carry the same placement.
    response = anon_client.get(
        _issues_url(world), params={"order_by": "-start_date", "group_by": "priority"}
    )
    assert response.status_code == 200, response.text
    grouped = response.json()["results"]["none"]["results"]
    assert [row["name"] for row in grouped] == expected["-start_date"]
