"""Contract: public intake endpoints.

Covers ``space/urls/intake.py``: the intake-issue list/create/retrieve/patch/
delete quartet (plus the ``inbox-issues`` alias, which shares the view) and
``workspaces/<slug>/project-boards/``.

Every action here requires a session (``BaseViewSet`` default): the
unauthenticated cases are part of the suite's denied-permission tripwire.
"""

import uuid
from concurrent.futures import ThreadPoolExecutor

import pytest

from _harness.http import api_client

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _list_url(world):
    return f"{BASE}/anchor/{world['anchor']}/intakes/{world['intake']['id']}/intake-issues/"


def _detail_url(world, pk):
    return f"{_list_url(world)}{pk}/"


def _inbox_url(world):
    return f"{BASE}/anchor/{world['anchor']}/intakes/{world['intake']['id']}/inbox-issues/"


def test_list_requires_auth(anon_client, world):
    assert anon_client.get(_list_url(world)).status_code in (401, 403)


def test_list_shape(user_client, world, seeder, db):
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    response = user_client.get(_list_url(world))
    assert response.status_code == 200
    rows = response.json()
    assert isinstance(rows, list) and len(rows) == 1
    row = rows[0]
    for key in ("id", "name", "priority", "state_detail", "project_detail", "sub_issues_count", "issue_intake"):
        assert key in row, f"intake row missing {key}"
    assert row["project_detail"]["id"] == world["project"]["id"]


def test_inbox_alias_matches_list(user_client, world):
    listed = user_client.get(_list_url(world))
    inbox = user_client.get(_inbox_url(world))
    assert inbox.status_code == 200
    assert inbox.json() == listed.json()


def test_create_shape(user_client, world):
    payload = {"issue": {"name": "Intake idea", "priority": "low", "description_html": "<p>why</p>"}}
    response = user_client.post(_list_url(world), json=payload)
    assert response.status_code == 200
    body = response.json()
    assert body["name"] == "Intake idea"
    assert body["state_detail"] is not None


def test_create_without_description_stores_empty_stripped(user_client, world, db):
    # PIDASHCONV-882: a missing description_html defaults to "<p></p>"
    # (views/intake.py:148) and Issue.save stores strip_tags verbatim —
    # None only when the input html itself is ""/None
    # (db/models/issue.py:330-334) — so the stored and returned
    # description_stripped is "", not null.
    response = user_client.post(_inbox_url(world), json={"issue": {"name": "nodesc idea"}})
    assert response.status_code == 200
    body = response.json()
    assert body["description_html"] == "<p></p>"
    assert body["description_stripped"] == ""
    row = db.fetchone("SELECT description_stripped FROM issues WHERE id=%s", (body["id"],))
    assert row["description_stripped"] == ""


def test_create_requires_name(user_client, world):
    response = user_client.post(_list_url(world), json={"issue": {"priority": "low"}})
    assert response.status_code == 400
    assert response.json() == {"error": "Name is required"}


def test_create_rejects_bad_priority(user_client, world):
    response = user_client.post(_list_url(world), json={"issue": {"name": "x", "priority": "eventual"}})
    assert response.status_code == 400
    assert response.json() == {"error": "Invalid priority"}


def test_create_null_description_json_is_400(user_client, world, db):
    # PIDASHCONV-1637: an explicit null reaches Issue.objects.create as
    # None (views/intake.py:147); description_json is NOT NULL
    # (db/models/issue.py:138), so the insert raises IntegrityError and
    # BaseViewSet.handle_exception answers 400 (views/base.py:74-78)
    # with no issue/intake rows written. The triage State row IS
    # written first (:133-142 runs before the failing insert), so only
    # the issue and intake_issues tables are counted here.
    before_issues = db.fetchone("SELECT count(*) AS c FROM issues")["c"]
    before_intakes = db.fetchone("SELECT count(*) AS c FROM intake_issues")["c"]
    response = user_client.post(
        _list_url(world), json={"issue": {"name": "x", "description_json": None}}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "The payload is not valid"}
    after_issues = db.fetchone("SELECT count(*) AS c FROM issues")["c"]
    after_intakes = db.fetchone("SELECT count(*) AS c FROM intake_issues")["c"]
    assert (after_issues, after_intakes) == (before_issues, before_intakes)


def test_create_requires_auth(anon_client, world):
    response = anon_client.post(_list_url(world), json={"issue": {"name": "anon idea"}})
    assert response.status_code in (401, 403)


def test_create_without_intake_is_400(user_client, world, seeder):
    # One live project board per project (unique constraint on
    # deploy_boards(entity_name, entity_identifier) where deleted_at is null),
    # so the intake-less board lives on a fresh project, not world's.
    # The fresh project also needs a distinct name: project names are unique
    # per workspace (project_unique_name_workspace_when_deleted_at_null).
    project = seeder.create_project(
        world["workspace"]["id"], name=f"Contract Project {seeder.tag} no-intake"
    )
    board = seeder.create_board(world["workspace"]["id"], project["id"])
    url = f"{BASE}/anchor/{board['anchor']}/intakes/{world['intake']['id']}/intake-issues/"
    response = user_client.post(url, json={"issue": {"name": "no intake"}})
    assert response.status_code == 400
    assert response.json() == {"error": "Intake is not enabled for this Project Board"}


def test_retrieve_patch_delete_by_creator(user_client, world, seeder, db):
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    db.execute("UPDATE issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], issue["id"]))
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    db.execute(
        "UPDATE intake_issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], bridge["id"])
    )

    got = user_client.get(_detail_url(world, bridge["id"]))
    assert got.status_code == 200
    assert got.json()["id"] == issue["id"]

    patched = user_client.patch(
        _detail_url(world, bridge["id"]),
        json={"issue": {"name": "Renamed idea", "description_html": "<p>new</p>"}},
    )
    assert patched.status_code == 200

    deleted = user_client.delete(_detail_url(world, bridge["id"]))
    assert deleted.status_code == 204


def test_patch_textless_html_stores_empty_stripped(user_client, world, seeder, db):
    # PIDASHCONV-1264: PATCHing description_html with no text (the editor's
    # normal "cleared" value "<p></p>") stores "" — not NULL — in
    # issues.description_stripped (Issue.save update branch,
    # db/models/issue.py:344-349: None only when the input html itself is
    # ""/None, else strip_tags verbatim).
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    db.execute("UPDATE issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], issue["id"]))
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    db.execute(
        "UPDATE intake_issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], bridge["id"])
    )
    patched = user_client.patch(
        _detail_url(world, bridge["id"]),
        json={"issue": {"description_html": "<p></p>"}},
    )
    assert patched.status_code == 200
    body = patched.json()
    assert body["description_html"] == "<p></p>"
    assert body["description_stripped"] == ""
    row = db.fetchone("SELECT description_stripped FROM issues WHERE id=%s", (body["id"],))
    assert row["description_stripped"] == ""


def test_patch_by_other_user_is_rejected(other_client, world, seeder, db):
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    db.execute(
        "UPDATE intake_issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], bridge["id"])
    )
    response = other_client.patch(
        _detail_url(world, bridge["id"]), json={"issue": {"name": "hijack"}}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "You cannot edit intake issues"}


def test_delete_by_other_user_is_rejected(other_client, world, seeder, db):
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    db.execute(
        "UPDATE intake_issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], bridge["id"])
    )
    response = other_client.delete(_detail_url(world, bridge["id"]))
    assert response.status_code == 400
    assert response.json() == {"error": "You cannot delete intake issue"}


def test_cross_board_detail_is_not_found(user_client, world, seeder):
    # The detail lookup is scoped to the anchor's workspace+project+intake:
    # another board's bridge id must not resolve here.
    owner2 = seeder.create_user()
    workspace2 = seeder.create_workspace(owner2["id"])
    project2 = seeder.create_project(workspace2["id"])
    intake2 = seeder.create_intake(workspace2["id"], project2["id"])
    board2 = seeder.create_board(workspace2["id"], project2["id"], intake_id=intake2["id"])
    state2 = seeder.create_state(workspace2["id"], project2["id"])
    issue2 = seeder.create_issue(workspace2["id"], project2["id"], state2["id"])
    bridge2 = seeder.create_intake_issue(workspace2["id"], project2["id"], intake2["id"], issue2["id"])
    assert board2["anchor"] != world["anchor"]

    response = user_client.get(_detail_url(world, bridge2["id"]))
    assert response.status_code == 404


def test_workspace_project_boards_broken_contract(anon_client, world):
    # NOTE (ported bug): ``WorkspaceProjectDeployBoardEndpoint.get`` takes an
    # ``anchor`` kwarg the ``workspaces/<slug>/project-boards/`` route never
    # provides (and then indexes ``.values_list`` without calling it), so this
    # endpoint 500s on Django today. Pinned as 5xx so the Rust port reproduces
    # the failure instead of silently "fixing" it; the real fix is a follow-up.
    response = anon_client.get(f"{BASE}/workspaces/{world['workspace']['slug']}/project-boards/")
    assert response.status_code >= 500


def _link_label(db, seeder, world, issue_id, label_id):
    row_id = uuid.uuid4().hex
    db.execute(
        "INSERT INTO issue_labels (id, workspace_id, project_id, issue_id, label_id, created_at, updated_at)"
        " VALUES (%s,%s,%s,%s,%s,now(),now())",
        (row_id, world["workspace"]["id"], world["project"]["id"], issue_id, label_id),
    )
    seeder.track.add("issue_labels", row_id)


def _link_assignee(db, seeder, world, issue_id, user_id):
    row_id = uuid.uuid4().hex
    db.execute(
        "INSERT INTO issue_assignees (id, workspace_id, project_id, issue_id, assignee_id, created_at, updated_at)"
        " VALUES (%s,%s,%s,%s,%s,now(),now())",
        (row_id, world["workspace"]["id"], world["project"]["id"], issue_id, user_id),
    )
    seeder.track.add("issue_assignees", row_id)


def test_soft_deleted_label_hidden_from_intake_labels(user_client, world, seeder, db):
    # PIDASHCONV-1263: the labels M2M is built from Label's default manager
    # (SoftDeletionManager, db/mixins.py), so a soft-deleted label appears in
    # neither `labels` nor `label_details`, on list or detail.
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    live = seeder.create_label(world["workspace"]["id"], world["project"]["id"], name="live label")
    dead = seeder.create_label(world["workspace"]["id"], world["project"]["id"], name="deleted label")
    db.execute("UPDATE labels SET deleted_at=now() WHERE id=%s", (dead["id"],))
    _link_label(db, seeder, world, issue["id"], live["id"])
    _link_label(db, seeder, world, issue["id"], dead["id"])

    listed = user_client.get(_list_url(world))
    assert listed.status_code == 200
    rows = listed.json()
    assert len(rows) == 1
    assert rows[0]["labels"] == [live["id"]]
    assert [d["id"] for d in rows[0]["label_details"]] == [live["id"]]

    got = user_client.get(_detail_url(world, bridge["id"]))
    assert got.status_code == 200
    assert got.json()["labels"] == [live["id"]]
    assert [d["id"] for d in got.json()["label_details"]] == [live["id"]]


def test_intake_labels_newest_first(user_client, world, seeder, db):
    # Label.Meta.ordering = ("-created_at",) rides along on the prefetch
    # queryset, so both arrays come back newest-first. created_at is pinned
    # (not insertion order) so the expectation is deterministic.
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    older = seeder.create_label(world["workspace"]["id"], world["project"]["id"], name="older label")
    newer = seeder.create_label(world["workspace"]["id"], world["project"]["id"], name="newer label")
    db.execute("UPDATE labels SET created_at='2024-01-01T00:00:00Z' WHERE id=%s", (older["id"],))
    db.execute("UPDATE labels SET created_at='2025-01-01T00:00:00Z' WHERE id=%s", (newer["id"],))
    _link_label(db, seeder, world, issue["id"], older["id"])
    _link_label(db, seeder, world, issue["id"], newer["id"])

    listed = user_client.get(_list_url(world))
    assert listed.status_code == 200
    rows = listed.json()
    assert len(rows) == 1
    assert rows[0]["labels"] == [newer["id"], older["id"]]
    assert [d["name"] for d in rows[0]["label_details"]] == ["newer label", "older label"]

    got = user_client.get(_detail_url(world, bridge["id"]))
    assert got.status_code == 200
    assert got.json()["labels"] == [newer["id"], older["id"]]
    assert [d["name"] for d in got.json()["label_details"]] == ["newer label", "older label"]


def test_intake_assignees_newest_first(user_client, world, seeder, db):
    # User.Meta.ordering = ("-created_at",), same newest-first rule for the
    # assignees M2M.
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    older = seeder.create_user()
    newer = seeder.create_user()
    db.execute("UPDATE users SET created_at='2024-01-01T00:00:00Z' WHERE id=%s", (older["id"],))
    db.execute("UPDATE users SET created_at='2025-01-01T00:00:00Z' WHERE id=%s", (newer["id"],))
    _link_assignee(db, seeder, world, issue["id"], older["id"])
    _link_assignee(db, seeder, world, issue["id"], newer["id"])

    listed = user_client.get(_list_url(world))
    assert listed.status_code == 200
    rows = listed.json()
    assert len(rows) == 1
    assert rows[0]["assignees"] == [newer["id"], older["id"]]
    assert [d["id"] for d in rows[0]["assignee_details"]] == [newer["id"], older["id"]]

    got = user_client.get(_detail_url(world, bridge["id"]))
    assert got.status_code == 200
    assert got.json()["assignees"] == [newer["id"], older["id"]]
    assert [d["id"] for d in got.json()["assignee_details"]] == [newer["id"], older["id"]]


def test_concurrent_creates_get_distinct_sequence_ids(user_client, world, seeder, db, settings):
    # PIDASHCONV-1265: Issue.save takes a per-project advisory lock and
    # assigns MAX(sequence)+1 inside one transaction
    # (db/models/issue.py:313-343), so N concurrent intake creates must
    # yield N distinct sequence_ids. The triage state is pre-seeded so
    # every create takes the reuse path — the triage get-or-create race
    # is separate Django behavior, out of scope here.
    seeder.create_state(
        world["workspace"]["id"], world["project"]["id"], name="Triage", group="triage"
    )
    url = _list_url(world)
    cookies = dict(user_client.cookies)
    count = 20

    def _one(n):
        with api_client(settings.base_url) as client:
            client.cookies.update(cookies)
            response = client.post(url, json={"issue": {"name": f"race idea {n}"}})
            assert response.status_code == 200, response.text[:200]
            return response.json()["sequence_id"]

    with ThreadPoolExecutor(max_workers=count) as pool:
        sequence_ids = list(pool.map(_one, range(count)))
    assert len(set(sequence_ids)) == count, f"shared sequence_ids: {sorted(sequence_ids)}"
    # The world's seeded issue bypasses Issue.save (no issue_sequences
    # row), so the dupe scan covers only the created rows.
    dupes = db.fetchall(
        "SELECT sequence_id, count(*) AS c FROM issues WHERE project_id=%s AND id != %s"
        " GROUP BY 1 HAVING count(*) > 1",
        (world["project"]["id"], world["issue"]["id"]),
    )
    assert dupes == []


def test_create_triage_autocreate_ignores_deleted_and_triage_sequences(
    user_client, world, seeder, db
):
    # PIDASHCONV-1638: the auto-created Triage row's sequence comes from
    # State.save, which reads MAX(sequence) over State.objects — live,
    # non-triage project states (db/models/state.py:79-83,131-139) — plus
    # 15000. Soft-deleted rows (any group) and triage-group rows must not
    # inflate it: live max 50000 here, so Triage lands on 65000 even with
    # a deleted backlog state at 100000 and a deleted triage state (the
    # highest row) at 200000.
    db.execute("UPDATE states SET sequence=50000 WHERE id=%s", (world["state"]["id"],))
    dead = seeder.create_state(
        world["workspace"]["id"], world["project"]["id"], name="Deleted", group="backlog"
    )
    db.execute("UPDATE states SET sequence=100000, deleted_at=now() WHERE id=%s", (dead["id"],))
    dead_triage = seeder.create_state(
        world["workspace"]["id"], world["project"]["id"], name="Old triage", group="triage"
    )
    db.execute(
        "UPDATE states SET sequence=200000, deleted_at=now() WHERE id=%s", (dead_triage["id"],)
    )
    response = user_client.post(_list_url(world), json={"issue": {"name": "triage scope idea"}})
    assert response.status_code == 200
    row = db.fetchone(
        'SELECT sequence FROM states WHERE project_id=%s AND "group"=%s AND deleted_at IS NULL',
        (world["project"]["id"], "triage"),
    )
    assert float(row["sequence"]) == 65000.0
