"""Runner outbox/poll big-int rendering (PIDASHCONV-1332).

Python ints are unbounded: Django's outbox ``json.dumps`` and the poll
``JsonResponse`` echo the exact digits of integers past u64/i64. The
Rust renderer must do the same, both when it enqueues the payload and
when the poll re-renders a stored entry.
"""

from __future__ import annotations

import pytest

from _harness.web import web_post

from .conftest import DAEMON
from .test_daemon_sessions import _open_runner_session, _post_poll

pytestmark = pytest.mark.contract

WEB = "/api/runners"

BIG_POS = 18446744073709551616  # 2**64 == u64::MAX + 1
BIG_NEG = -9223372036854775809  # i64::MIN - 1


def test_runner_poll_echoes_big_integers_exactly(
    user_client, machine_client, machine_flow, chat_world, seeder
):
    """A chat message's big-int content_parts survive enqueue + poll exactly."""
    seeder.db.execute(
        "UPDATE runner SET status = 'online' WHERE id = %s", (machine_flow["runner_id"],)
    )
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    sent = web_post(
        user_client,
        f"{WEB}/chat/sessions/{chat_world['session']['id']}/messages/",
        {
            "content": "bigint probe",
            "content_parts": [{"type": "x", "n": BIG_POS}, {"type": "y", "n": BIG_NEG}],
        },
    )
    assert sent.status_code == 201, sent.text
    poll_url = (
        f"{DAEMON}/runners/{machine_flow['runner_id']}"
        f"/sessions/{opened['session_id']}/poll"
    )
    # First poll replays the (empty) PEL without blocking; the second
    # reads the newly enqueued entry.
    first = _post_poll(
        machine_client, poll_url, {"ack": [], "status": {"status": "idle"}}
    )
    assert first.status_code == 200, first.text
    poll = _post_poll(
        machine_client, poll_url, {"ack": [], "status": {"status": "idle"}}
    )
    assert poll.status_code == 200, poll.text
    # Exact integer text on the wire: no float spelling, no lost precision.
    assert f'"n": {BIG_POS}' in poll.text
    assert f'"n": {BIG_NEG}' in poll.text
    chats = [m for m in poll.json()["messages"] if m.get("type") == "chat_user_message"]
    assert len(chats) == 1
    parts = chats[0]["body"]["content_parts"]
    assert parts[0]["n"] == BIG_POS and isinstance(parts[0]["n"], int)
    assert parts[1]["n"] == BIG_NEG and isinstance(parts[1]["n"], int)
