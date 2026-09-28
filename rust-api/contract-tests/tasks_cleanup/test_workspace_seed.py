"""D-09 oracle: workspace_seed DB before/after (PIDASHCONV-190).

Django source: pi_dash/bgtasks/workspace_seed_task.py (570 lines).

Seeds one workspace through the real worker — publish
``pi_dash.bgtasks.workspace_seed_task.workspace_seed`` with the workspace
id, then diff the database before/after. Expected row counts are derived
from the seed files under ``pi_dash/seeds/data/`` at runtime (never
Django imports), so the suite tracks seed edits instead of freezing them:

- 1 project named after the workspace (identifier = alnum-first-5 upper,
  seed description/network/cover/logo forwarded, ``is_default`` set,
  timezone copied, bot audit)
- bot user (``bot_user_<wsid>``, ``is_bot``, ``WORKSPACE_SEED``) + role-20
  membership
- project members + user properties fanned out over every workspace
  member *including the bot* (the ``:87`` read runs after the bot insert)
- states / labels / cycles / modules / views with save()-derived
  sequences, sorts, slugs and dates
- issues with sequences (TWO rows each: the ``Issue.save`` auto-row plus
  the explicit create — ported as-is), activities, and label/cycle/module
  links
- pages (both seed rows are PROJECT type) + ``ProjectPage`` links
"""

import json
import uuid
from datetime import datetime, timezone

import pytest
from psycopg.rows import dict_row

from _harness import celery_wire, taskspec
from _harness import seed as seed_helpers
from _harness.db import wait_for_condition as wait_for

M = "pi_dash.bgtasks"
TASK = f"{M}.workspace_seed_task.workspace_seed"


def _seed_rows(name):
    root = taskspec.source_root() / "pi_dash" / "seeds" / "data"
    with open(root / name) as fh:
        return json.load(fh)


def _count(conn, table, where="", params=()):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT count(*) AS n FROM "{table}" {where}', params)
        return cur.fetchone()["n"]


def _one(conn, table, where, params=()):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT * FROM "{table}" {where}', params)
        return cur.fetchone()


def _project_rows(conn, table, project_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT * FROM "{table}" WHERE project_id = %s::uuid', (str(project_id),)
        )
        return cur.fetchall()


def test_workspace_seed_before_after(db_conn, broker_url):
    tag = uuid.uuid4().hex[:8]
    owner = seed_helpers.user(db_conn, f"contract-seed-owner-{tag}")
    other = seed_helpers.user(db_conn, f"contract-seed-other-{tag}")
    workspace = seed_helpers.workspace(
        db_conn,
        f"contractseed{tag}",
        owner["id"],
        name=f"Seed Contract {tag}!",
    )
    seed_helpers.workspace_member(db_conn, workspace["id"], owner["id"], role=5)
    seed_helpers.workspace_member(db_conn, workspace["id"], other["id"], role=10)

    seed_projects = _seed_rows("projects.json")
    seed_states = _seed_rows("states.json")
    seed_labels = _seed_rows("labels.json")
    seed_cycles = _seed_rows("cycles.json")
    seed_modules = _seed_rows("modules.json")
    seed_issues = _seed_rows("issues.json")
    seed_views = _seed_rows("views.json")
    seed_pages = _seed_rows("pages.json")

    expected_labels = sum(len(r.get("labels", [])) for r in seed_issues)
    expected_cycles = sum(1 for r in seed_issues if r.get("cycle_id"))
    expected_modules = sum(len(r.get("module_ids") or []) for r in seed_issues)
    expected_project_pages = sum(
        1
        for r in seed_pages
        if r.get("project_id") and r.get("type") == "PROJECT"
    )

    before_projects = _count(db_conn, "projects")

    celery_wire.publish(TASK, args=[str(workspace["id"])])

    project = wait_for(
        lambda: _one(
            db_conn,
            "projects",
            "WHERE workspace_id = %s::uuid AND deleted_at IS NULL",
            (str(workspace["id"]),),
        ),
        what="seed project created",
    )
    wait_for(
        lambda: (
            _count(
                db_conn,
                "issues",
                "WHERE project_id = %s::uuid",
                (str(project["id"]),),
            )
            == len(seed_issues)
        )
        or None,
        what="seed issues created",
    )

    # Project shape: workspace name, derived identifier, seed payload,
    # bot audit, first-project default, workspace timezone.
    assert _count(db_conn, "projects") == before_projects + 1
    assert project["name"] == workspace["name"]
    expected_identifier = "".join(
        ch for ch in workspace["name"] if ch.isalnum()
    )[:5].upper()
    assert project["identifier"] == expected_identifier
    assert project["description"] == seed_projects[0]["description"]
    assert project["network"] == seed_projects[0]["network"]
    assert project["cover_image"] == seed_projects[0]["cover_image"]
    assert project["logo_props"] == seed_projects[0]["logo_props"]
    assert project["is_default"] is True
    assert project["timezone"] == workspace["timezone"]
    assert project["cycle_view"] is True
    assert project["module_view"] is True
    assert project["issue_views_view"] is True

    bot = _one(
        db_conn,
        "users",
        "WHERE username = %s",
        (f"bot_user_{workspace['id']}",),
    )
    assert bot is not None
    assert bot["is_bot"] is True
    assert bot["bot_type"] == "WORKSPACE_SEED"
    assert bot["email"] == f"bot_user_{workspace['id']}@example.com"
    assert bot["password"].startswith("pbkdf2_sha256$600000$")
    assert bot["is_password_autoset"] is True
    assert project["created_by_id"] == bot["id"]

    bot_membership = _one(
        db_conn,
        "workspace_members",
        "WHERE workspace_id = %s::uuid AND member_id = %s::uuid",
        (str(workspace["id"]), str(bot["id"])),
    )
    assert bot_membership is not None
    assert bot_membership["role"] == 20
    assert bot_membership["company_role"] == ""

    # Fan-out covers owner + other + bot (3 members).
    members = _project_rows(db_conn, "project_members", project["id"])
    assert {str(m["member_id"]) for m in members} == {
        str(owner["id"]),
        str(other["id"]),
        str(bot["id"]),
    }
    roles = {str(m["member_id"]): m["role"] for m in members}
    assert roles[str(owner["id"])] == 5
    assert roles[str(other["id"])] == 10
    assert all(m["created_by_id"] == bot["id"] for m in members)
    props = _project_rows(db_conn, "project_user_properties", project["id"])
    assert len(props) == 3
    assert props[0]["display_filters"]["group_by"] == "state"
    assert props[0]["display_properties"]["customer_request_count"] is True

    # States / labels keep seed values on the first row of each family.
    states = _project_rows(db_conn, "states", project["id"])
    assert len(states) == len(seed_states)
    by_name = {s["name"]: s for s in states}
    assert by_name["Backlog"]["slug"] == "backlog"
    assert by_name["In Progress"]["slug"] == "in-progress"
    assert by_name["Backlog"]["sequence"] == 15000
    assert by_name["Backlog"]["default"] is True
    assert {s["created_by_id"] for s in states} == {bot["id"]}

    labels = _project_rows(db_conn, "labels", project["id"])
    assert len(labels) == len(seed_labels)
    assert {label["name"] for label in labels} == {
        r["name"] for r in seed_labels
    }

    # Cycles: CURRENT covers now, UPCOMING chains off its end.
    cycles = _project_rows(db_conn, "cycles", project["id"])
    assert len(cycles) == len(seed_cycles)
    current = next(
        c for c in cycles if c["start_date"] <= datetime.now(timezone.utc) <= c["end_date"]
    )
    upcoming = next(c for c in cycles if c["id"] != current["id"])
    assert upcoming["start_date"] >= current["end_date"]
    assert all(c["owned_by_id"] == bot["id"] for c in cycles)

    modules = _project_rows(db_conn, "modules", project["id"])
    assert len(modules) == len(seed_modules)
    assert all(m["target_date"] > m["start_date"] for m in modules)

    # Issues: sequences overwrite the seed values (1-based in file order),
    # stripped descriptions derive from the HTML, completed_at stays null
    # (no seed issue sits in a completed state), and every issue owns TWO
    # sequence rows (save auto-row + explicit create, ported as-is).
    issues = _project_rows(db_conn, "issues", project["id"])
    assert len(issues) == len(seed_issues)
    welcome = next(i for i in issues if i["name"].startswith("Welcome to Pi Dash"))
    assert welcome["sequence_id"] == 1
    assert welcome["priority"] == "urgent"
    assert welcome["description_stripped"] is not None
    assert "<p" not in (welcome["description_stripped"] or "")
    assert all(i["completed_at"] is None for i in issues)
    assert all(i["created_by_id"] == bot["id"] for i in issues)
    for issue in issues:
        seqs = _project_rows(db_conn, "issue_sequences", project["id"])
        mine = [s for s in seqs if str(s["issue_id"]) == str(issue["id"])]
        assert len(mine) == 2, f"issue {issue['id']} must own two sequence rows"
    assert _count(
        db_conn, "issue_sequences", "WHERE project_id = %s::uuid", (str(project["id"]),)
    ) == 2 * len(seed_issues)
    assert (
        _count(
            db_conn,
            "issue_activities",
            "WHERE project_id = %s::uuid AND verb = 'created'",
            (str(project["id"]),),
        )
        == len(seed_issues)
    )
    assert (
        _count(
            db_conn, "issue_labels", "WHERE project_id = %s::uuid", (str(project["id"]),)
        )
        == expected_labels
    )
    assert (
        _count(
            db_conn, "cycle_issues", "WHERE project_id = %s::uuid", (str(project["id"]),)
        )
        == expected_cycles
    )
    assert (
        _count(
            db_conn,
            "module_issues",
            "WHERE project_id = %s::uuid",
            (str(project["id"]),),
        )
        == expected_modules
    )

    # Views: empty seed filters store an empty query; first view keeps its
    # seed sort.
    views = _project_rows(db_conn, "issue_views", project["id"])
    assert len(views) == len(seed_views)
    assert views[0]["query"] == {}
    assert views[0]["sort_order"] == seed_views[0]["sort_order"]
    assert views[0]["owned_by_id"] == bot["id"]

    # Pages: both seed rows are PROJECT type, so both link.
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT * FROM "pages" WHERE workspace_id = %s::uuid AND deleted_at IS NULL',
            (str(workspace["id"]),),
        )
        ws_pages = cur.fetchall()
    assert len(ws_pages) == len(seed_pages)
    assert {p["access"] for p in ws_pages} == {r["access"] for r in seed_pages}
    assert (
        _count(
            db_conn,
            "project_pages",
            "WHERE project_id = %s::uuid",
            (str(project["id"]),),
        )
        == expected_project_pages
    )
