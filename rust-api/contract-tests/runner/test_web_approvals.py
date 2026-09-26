"""Web run approvals (``/api/runners/approvals/...``).

Pending approvals routed to the run creator; decide accepts/declines and
fans the decision out to the serving runner. Approvals are seeded black-box
through the daemon ``runs/<id>/approvals/`` endpoint — never raw SQL.
"""

from __future__ import annotations

import uuid

import pytest

from _harness.auth import login_session
from _harness.http import api_client
from _harness.web import web_post

WEB = "/api/runners"
DAEMON = "/api/v1/runner"

pytestmark = pytest.mark.contract

APPROVAL_KEYS = {
    "id", "agent_run", "kind", "payload", "reason", "status",
    "decision_source", "requested_at", "decided_at", "expires_at",
}


def _seed_approval(machine_client, daemon_run, seeder) -> str:
    approval_id = str(uuid.uuid4())
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/approvals/",
        json={"approval_id": approval_id, "kind": "command_execution", "reason": "rm -rf /"},
        headers={"Idempotency-Key": uuid.uuid4().hex},
    )
    assert response.json() == {"ok": True}, response.text
    return approval_id


def test_web_approvals_list_shape(user_client, machine_client, daemon_run, seeder):
    approval_id = _seed_approval(machine_client, daemon_run, seeder)
    response = user_client.get(f"{WEB}/approvals/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    approval = body[0]
    assert APPROVAL_KEYS <= set(approval)
    assert approval["id"] == approval_id
    assert approval["agent_run"] == daemon_run["id"]
    assert approval["status"] == "pending"


def test_web_approvals_list_denies_anonymous(anon_client):
    response = anon_client.get(f"{WEB}/approvals/")
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}


def test_web_approvals_list_isolates_creators(settings, user_client, machine_client, daemon_run, seeder):
    """Approvals route to the run creator: a stranger sees none (and 404s)."""
    approval_id = _seed_approval(machine_client, daemon_run, seeder)
    assert user_client.get(f"{WEB}/approvals/").json() != []
    stranger = seeder.create_user()
    with api_client(settings.base_url) as client:
        login_session(client, email=stranger["email"], password=stranger["password"])
        assert client.get(f"{WEB}/approvals/").json() == []
        # The decide 500 (see above) masks the stranger 404: the broken
        # read raises before the created_by gate is even evaluated.
        decided = web_post(
            client, f"{WEB}/approvals/{approval_id}/decide/", {"decision": "accept"}
        )
        assert decided.status_code == 500


def test_web_approval_decide_shape(user_client, machine_client, daemon_run, seeder):
    """Accept fans out to the runner; the decided row comes back.

    NOTE (Django bug, ported as-is): on Postgres the decide read
    (``select_for_update`` over the nullable ``agent_run__runner`` join)
    raises, so Django answers 500. The oracle pins the 500 byte-for-byte.
    """
    approval_id = _seed_approval(machine_client, daemon_run, seeder)
    response = web_post(
        user_client, f"{WEB}/approvals/{approval_id}/decide/", {"decision": "accept"}
    )
    assert response.status_code == 500


def test_web_approval_decide_rejects_bad_decision(user_client, machine_client, daemon_run, seeder):
    approval_id = _seed_approval(machine_client, daemon_run, seeder)
    response = web_post(
        user_client, f"{WEB}/approvals/{approval_id}/decide/", {"decision": "maybe"}
    )
    assert response.status_code == 400


def test_web_approval_decide_unknown_is_500(user_client):
    """The decide 500 masks even the unknown-id 404 (same broken read)."""
    response = web_post(
        user_client,
        f"{WEB}/approvals/00000000-0000-0000-0000-000000000000/decide/",
        {"decision": "accept"},
    )
    assert response.status_code == 500


def test_web_approval_decide_denies_anonymous(anon_client, machine_client, daemon_run, seeder):
    approval_id = _seed_approval(machine_client, daemon_run, seeder)
    response = anon_client.post(
        f"{WEB}/approvals/{approval_id}/decide/", json={"decision": "accept"}
    )
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}
