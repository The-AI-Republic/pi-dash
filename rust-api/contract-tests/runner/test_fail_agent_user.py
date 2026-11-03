"""Daemon ``fail`` creates the agent system user on a fresh database (PIDASHCONV-1480).

``_post_failure_comment`` authors a SYSTEM comment as the ``pi_dash_agent`` bot
user, resolved via ``get_agent_system_user`` (get-or-create). On a database
where Django never created the bot row, the Rust inline get-or-create issued a
7-column ``users`` INSERT that violated the table's NOT NULL columns, so the
comment was dropped inside its savepoint while the hooks marker still stamped.
This test deletes any bot row first, fails an issue-attached run, and asserts
the bot row plus the SYSTEM comment exist — on Django (the oracle) and Rust.
"""

from __future__ import annotations

import uuid

import pytest

from .conftest import DAEMON

pytestmark = pytest.mark.contract

AGENT_USERNAME = "pi_dash_agent"


@pytest.fixture()
def fail_world(seeder, daemon_world, machine_flow):
    """An issue plus an assigned daemon run attached to it; no bot user.

    The bot row is deleted up front so the test is hermetic within a shared
    contract database, and the server-created rows (comment, description, bot
    user) are removed afterwards — the seed tracker only knows seeded rows.
    """
    seeder.db.execute("DELETE FROM users WHERE username = %s", (AGENT_USERNAME,))
    state = seeder.create_state(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"]
    )
    issue = seeder.create_issue(
        daemon_world["workspace"]["id"],
        daemon_world["project"]["id"],
        state["id"],
        name="fail comment issue",
    )
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="assigned",
    )
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s WHERE id = %s",
        (issue["id"], run["id"]),
    )
    world = {"issue_id": issue["id"], "run_id": run["id"]}
    yield world
    seeder.db.execute("DELETE FROM run_message_dedupe WHERE run_id = %s", (run["id"],))
    seeder.db.execute("DELETE FROM agent_run_event WHERE agent_run_id = %s", (run["id"],))
    seeder.db.execute(
        "DELETE FROM agent_run_approval WHERE agent_run_id = %s", (run["id"],)
    )
    descriptions = seeder.db.fetchall(
        "SELECT description_id FROM issue_comments WHERE issue_id = %s",
        (issue["id"],),
    )
    seeder.db.execute("DELETE FROM issue_comments WHERE issue_id = %s", (issue["id"],))
    for row in descriptions:
        if row["description_id"] is not None:
            seeder.db.execute(
                "DELETE FROM descriptions WHERE id = %s", (row["description_id"],)
            )
    seeder.db.execute("DELETE FROM users WHERE username = %s", (AGENT_USERNAME,))


def test_fail_creates_agent_user_and_posts_comment(
    machine_client, seeder, fail_world
):
    machine_client.post(f"{DAEMON}/runs/{fail_world['run_id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{fail_world['run_id']}/fail/",
        json={"detail": "boom"},
        headers={"Idempotency-Key": uuid.uuid4().hex},
    )
    assert response.json() == {"ok": True}
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval(
            "SELECT status FROM agent_run WHERE id = %s", (fail_world["run_id"],)
        )
        == "failed"
    )
    bot = seeder.db.fetchone(
        "SELECT id, email, first_name, last_name, display_name, is_bot,"
        " is_active, user_timezone, password"
        " FROM users WHERE username = %s",
        (AGENT_USERNAME,),
    )
    assert bot is not None
    assert bot["is_bot"] is True
    assert bot["email"] == "agent@example.com"
    assert bot["first_name"] == "Pi Dash"
    assert bot["last_name"] == "Agent"
    assert bot["display_name"] == "agent"
    assert bot["is_active"] is True
    assert bot["user_timezone"] == "UTC"
    assert bot["password"].startswith("!")
    comment = seeder.db.fetchone(
        "SELECT actor_id, speaker_type, speaker_agent_run_id, comment_html"
        " FROM issue_comments"
        " WHERE issue_id = %s AND deleted_at IS NULL",
        (fail_world["issue_id"],),
    )
    assert comment is not None
    assert comment["actor_id"] == bot["id"]
    assert comment["speaker_type"] == "system"
    assert str(comment["speaker_agent_run_id"]) == fail_world["run_id"]
    assert "Run failed." in comment["comment_html"]
    assert "boom" in comment["comment_html"]
