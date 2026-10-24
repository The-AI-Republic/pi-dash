"""Contract: public intake list at scale (PIDASHCONV-1266).

The list is unpaginated, so its query count must not grow with the row
count. ``test_list_fifty_rows_body`` pins the 50-row body (order, nests,
asset URLs) on both backends; ``test_list_query_count_constant_in_rows``
counts the statements per GET and fails when any lookup runs per row.
"""

import uuid

import pytest

from _harness import dbstats

pytestmark = pytest.mark.contract

BASE = "/api/public"

ROW_KEYS = frozenset(
    [
        "agent_executor", "archived_at", "assigned_pod", "assignee_details",
        "assignees", "completed_at", "complexity_score", "created_at",
        "created_by", "created_via", "deleted_at", "description_binary",
        "description_html", "description_json", "description_stripped",
        "estimate_point", "external_id", "external_source", "git_work_branch",
        "id", "is_draft", "issue_intake", "label_details", "labels", "name",
        "parent", "point", "priority", "project", "project_detail",
        "sequence_id", "sort_order", "start_date", "state", "state_detail",
        "sub_issues_count", "target_date", "type", "updated_at", "updated_by",
        "workspace",
    ]
)


def _list_url(world, intake_id):
    return f"{BASE}/anchor/{world['anchor']}/intakes/{intake_id}/intake-issues/"


def _link_label(db, world, issue_id, label_id):
    db.execute(
        "INSERT INTO issue_labels (id,label_id,issue_id,project_id,workspace_id,"
        "created_at,updated_at) VALUES (%s,%s,%s,%s,%s,now(),now())",
        (str(uuid.uuid4()), label_id, issue_id, world["project"]["id"], world["workspace"]["id"]),
    )


def _link_assignee(db, world, issue_id, user_id):
    db.execute(
        "INSERT INTO issue_assignees (id,assignee_id,issue_id,project_id,workspace_id,"
        "created_at,updated_at) VALUES (%s,%s,%s,%s,%s,now(),now())",
        (str(uuid.uuid4()), user_id, issue_id, world["project"]["id"], world["workspace"]["id"]),
    )


def test_list_fifty_rows_body(user_client, world, seeder, db):
    """A 50-row intake lists every row with the full detail nests,
    ordered by (snoozed_till, status) exactly like Django."""
    state2 = seeder.create_state(
        world["workspace"]["id"], world["project"]["id"], name="In progress", group="started"
    )
    label2 = seeder.create_label(world["workspace"]["id"], world["project"]["id"], name="ui")
    cover = seeder.create_asset(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"],
        entity_type="PROJECT_COVER",
    )
    db.execute(
        "UPDATE projects SET cover_image_asset_id=%s WHERE id=%s",
        (cover["id"], world["project"]["id"]),
    )
    avatar = seeder.create_asset(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"],
        entity_type="USER_AVATAR",
    )
    db.execute(
        "UPDATE users SET avatar_asset_id=%s WHERE id=%s", (avatar["id"], world["owner"]["id"])
    )
    other = seeder.create_user()
    # Distinct (snoozed_till, status) per row: days 0-39 carry status -2,
    # null-snoozed rows 40-49 carry distinct statuses, so the Django
    # ORDER BY (snoozed_till ASC NULLS LAST, status ASC) yields the exact
    # seed sequence on both backends.
    issue_ids = []
    states = []
    for n in range(50):
        state_id = world["state"]["id"] if n % 2 == 0 else state2["id"]
        states.append(state_id)
        issue = seeder.create_issue(
            world["workspace"]["id"], world["project"]["id"], state_id, name=f"Scale idea {n:02d}"
        )
        issue_ids.append(issue["id"])
        bridge = seeder.create_intake_issue(
            world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"],
            status=-2 if n < 40 else n,
        )
        if n < 40:
            db.execute(
                "UPDATE intake_issues SET snoozed_till=now() + make_interval(days=>%s)"
                " WHERE id=%s",
                (n, bridge["id"]),
            )
        if n % 3 != 0:
            _link_label(db, world, issue["id"], world["label"]["id"])
        else:
            _link_label(db, world, issue["id"], label2["id"])
        _link_assignee(
            db, world, issue["id"],
            world["owner"]["id"] if n % 4 != 0 else other["id"],
        )

    response = user_client.get(_list_url(world, world["intake"]["id"]))
    assert response.status_code == 200
    rows = response.json()
    assert [row["id"] for row in rows] == issue_ids
    cover_url = f"/api/assets/v2/static/{cover['id']}/"
    avatar_url = f"/api/assets/v2/static/{avatar['id']}/"
    for n, row in enumerate(rows):
        assert set(row.keys()) == ROW_KEYS, f"row {n} keys changed"
        assert row["name"] == f"Scale idea {n:02d}"
        assert row["state_detail"]["id"] == states[n]
        assert row["project_detail"]["id"] == world["project"]["id"]
        assert row["project_detail"]["cover_image_url"] == cover_url
        assert row["sub_issues_count"] == 0
        assert row["issue_intake"][0]["status"] == (-2 if n < 40 else n)
        want_label = world["label"]["id"] if n % 3 != 0 else label2["id"]
        assert row["labels"] == [want_label]
        assert [label["id"] for label in row["label_details"]] == [want_label]
        want_user = world["owner"]["id"] if n % 4 != 0 else other["id"]
        assert row["assignees"] == [want_user]
        assert [user["id"] for user in row["assignee_details"]] == [want_user]
        assert row["assignee_details"][0]["avatar_url"] == (
            avatar_url if n % 4 != 0 else None
        )


def _is_bridge_refetch(query):
    # Django's own per-bridge refetch (the `.only(...)` prefetch leaves
    # `issue_id` deferred and the nest touches it once per row): out of
    # scope here — Rust already serves bridges in one query — so the
    # growth assertion below ignores this one shape on both backends.
    return 'FROM "intake_issues"' in query and 'WHERE "intake_issues"."id" =' in query


def test_list_query_count_constant_in_rows(user_client, world, seeder, db):
    """5-row and 50-row lists issue the same queries: no lookup may run
    per row. Plain rows (labels + assignees, no cover/avatar assets) so
    the bound holds on Django too — Django resolves cover/avatar assets
    per row, a Django-side quirk outside this issue's scope."""
    if not dbstats.available(db):
        pytest.skip("pg_stat_statements not preloaded: statement counting impossible")
    label = seeder.create_label(
        world["workspace"]["id"], world["project"]["id"], name=f"count-{seeder.tag}"
    )
    small = seeder.create_intake(
        world["workspace"]["id"], world["project"]["id"], name=f"Count small {seeder.tag}"
    )
    large = seeder.create_intake(
        world["workspace"]["id"], world["project"]["id"], name=f"Count large {seeder.tag}"
    )
    for intake, count in ((small, 5), (large, 50)):
        for n in range(count):
            issue = seeder.create_issue(
                world["workspace"]["id"], world["project"]["id"], world["state"]["id"]
            )
            seeder.create_intake_issue(
                world["workspace"]["id"], world["project"]["id"], intake["id"], issue["id"]
            )
            _link_label(db, world, issue["id"], label["id"])
            _link_assignee(db, world, issue["id"], world["owner"]["id"])

    def _measured(intake_id):
        before = dbstats.snapshot(db)
        response = user_client.get(_list_url(world, intake_id))
        assert response.status_code == 200
        return response.json(), dbstats.delta(db, before)

    small_rows, small_delta = _measured(small["id"])
    large_rows, large_delta = _measured(large["id"])
    assert len(small_rows) == 5
    assert len(large_rows) == 50
    for query, calls in sorted(large_delta.items()):
        growth = calls - small_delta.get(query, 0)
        assert growth <= 2 or _is_bridge_refetch(query), (
            f"query runs per row (+{growth} from 5 to 50 rows): {query[:160]}"
        )
    assert sum(large_delta.values()) <= 120, (
        f"50-row list issued {sum(large_delta.values())} statements"
    )
