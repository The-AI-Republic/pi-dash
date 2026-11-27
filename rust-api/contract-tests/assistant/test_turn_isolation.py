# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant-turn credential isolation: NEWPIDASH-54 part 4.

Part 4 of the NEWPIDASH-54 validation series, the assistant half of
item 2 (Credentials): two users run assistant turns on one worker;
each turn's provider traffic carries only its own user's BYOK key,
and each thread's answer is written by its own provider. The same
cases run against Django and the Rust binary unchanged; whichever leg
diverges is a parity defect and gets filed as its own issue.
"""

from __future__ import annotations

import json
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _harness.celery import Broker, get_broker_url
from _harness.config import get_settings
from _harness.db import LazyDatabase
from _harness.seed import Seeder, SeedTracker
from dispatch.conftest import publish_task, seed_llm_config

RUN_TURN = "assistant.run_turn"


@pytest.fixture(scope="session")
def settings():
    return get_settings()


@pytest.fixture()
def db(settings):
    database = LazyDatabase(settings.database_url)
    yield database
    database.close()


@pytest.fixture()
def broker():
    broker = Broker(get_broker_url())
    yield broker
    broker.close()


@pytest.fixture()
def seeder(db):
    tracker = SeedTracker(db)
    seeder = Seeder(db, tracker, tag=uuid.uuid4().hex[:10])
    yield seeder
    tracker.cleanup()


def create_member_world(seeder, *, role=20):
    from dispatch.conftest import create_member_world as _create

    return _create(seeder, role=role)


class _StreamStub:
    """Recording OpenAI-compatible streaming stub (assistant turn shape).

    Serves ``POST /v1/chat/completions``: on a streaming request it
    sends one SSE chunk whose content is the tenant's own tag, then
    ends the stream cleanly (``[DONE]``). Non-streaming requests get
    the JSON final form. Every request is recorded with its headers
    (``Authorization`` carries the per-user key proof) and body.
    """

    def __init__(self, tag):
        self.tag = tag
        self.requests: list[dict] = []
        self._server = None
        self._thread = None

    def start(self):
        stub = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.0"

            def log_message(self, *args):
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                body = self.rfile.read(length) if length else b""
                try:
                    payload = json.loads(body or b"{}")
                except ValueError:
                    payload = {}
                stub.requests.append(
                    {
                        "path": self.path,
                        "headers": {k.lower(): v for k, v in self.headers.items()},
                        "body": payload,
                        "stream": payload.get("stream"),
                    }
                )
                if self.path.endswith("/chat/completions") and payload.get("stream"):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.end_headers()
                    self.wfile.write(
                        (
                            'data: {"id":"stub","object":"chat.completion.chunk",'
                            '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                            '"delta":{"content":'
                            + json.dumps(f"answer-from-{stub.tag}")
                            + '},"finish_reason":null}]}\n\n'
                        ).encode()
                    )
                    self.wfile.flush()
                    self.wfile.write(b"data: [DONE]\n\n")
                    self.wfile.flush()
                else:
                    content = json.dumps(
                        {
                            "id": "stub",
                            "object": "chat.completion",
                            "created": 0,
                            "model": "oracle-stub",
                            "choices": [
                                {
                                    "index": 0,
                                    "message": {
                                        "role": "assistant",
                                        "content": f"answer-from-{stub.tag}",
                                    },
                                    "finish_reason": "stop",
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 1,
                                "completion_tokens": 1,
                                "total_tokens": 2,
                            },
                        }
                    ).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(content)))
                    self.end_headers()
                    self.wfile.write(content)

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._server.daemon_threads = True
        port = self._server.server_address[1]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        return f"http://127.0.0.1:{port}/v1"

    def stop(self):
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None

    def authorization_headers(self):
        return [req["headers"].get("authorization") for req in self.requests]


def _seed_turn(db, world, question):
    """Seed one thread + queued turn + user message (same shape as the
    live oracle in ``test_turn_errors._seed_turn_trio``)."""
    thread_id, turn_id, message_id = (str(uuid.uuid4()) for _ in range(3))
    now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    db.execute(
        "INSERT INTO assistant_thread "
        "(id, workspace_id, user_id, title, kind, is_archived, "
        "created_at, updated_at) "
        "VALUES (%s, %s, %s, %s, 'chat', FALSE, %s, %s)",
        (thread_id, world["workspace"]["id"], world["owner"]["id"], "turn-isolation", now, now),
    )
    db.execute(
        "INSERT INTO assistant_turn (id, thread_id, status, model_used, error_code, "
        "error_detail, created_at, started_at) "
        "VALUES (%s, %s, 'queued', '', '', '', %s, %s)",
        (turn_id, thread_id, now, None),
    )
    db.execute(
        "INSERT INTO assistant_message "
        "(id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) "
        "VALUES (%s, %s, %s, 1, 'user', %s, '{}', 'completed', %s)",
        (message_id, thread_id, turn_id, question, now),
    )
    db.execute(
        "UPDATE assistant_turn SET user_message_id = %s WHERE id = %s",
        (message_id, turn_id),
    )
    db.execute(
        "UPDATE assistant_thread SET active_turn_id = %s WHERE id = %s",
        (turn_id, thread_id),
    )
    return thread_id, turn_id


def _wait_turn(db, turn_id, *, timeout=60):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        last = db.fetchone("SELECT * FROM assistant_turn WHERE id = %s", (turn_id,))
        if last is not None and last["status"] in ("completed", "failed"):
            return last
        time.sleep(0.2)
    raise AssertionError(f"timed out waiting for turn {turn_id}; last: {last}")


def _assistant_answer(db, thread_id):
    row = db.fetchone(
        "SELECT display_content FROM assistant_message "
        "WHERE thread_id = %s AND kind = 'assistant' ORDER BY created_at DESC LIMIT 1",
        (thread_id,),
    )
    return row["display_content"] if row else None


def test_turns_carry_only_own_users_key(db, broker, seeder):
    """Item 2 (assistant turns): two users run turns on one worker —
    each provider request carries only its own user's key, and each
    thread's answer is written by its own provider (no cross-tenant
    contamination)."""
    tag = uuid.uuid4().hex[:8]
    key_a, key_b = f"turn-a-{tag}", f"turn-b-{tag}"
    stub_a = _StreamStub("userA")
    stub_b = _StreamStub("userB")
    stubs = (stub_a, stub_b)
    try:
        world_a = create_member_world(seeder)
        seed_llm_config(seeder, world_a["owner"]["id"], stub_a.start(), key_a)
        thread_a, turn_a = _seed_turn(db, world_a, f"userA question {tag}")
        world_b = create_member_world(seeder)
        seed_llm_config(seeder, world_b["owner"]["id"], stub_b.start(), key_b)
        thread_b, turn_b = _seed_turn(db, world_b, f"userB question {tag}")
        publish_task(broker, RUN_TURN, [turn_a])
        publish_task(broker, RUN_TURN, [turn_b])
        row_a = _wait_turn(db, turn_a)
        row_b = _wait_turn(db, turn_b)
        assert row_a["status"] == "completed", row_a
        assert row_b["status"] == "completed", row_b
        # Each provider saw only its own user's key — every request.
        assert stub_a.authorization_headers() == [f"Bearer {key_a}"] * len(
            stub_a.authorization_headers()
        )
        assert stub_b.authorization_headers() == [f"Bearer {key_b}"] * len(
            stub_b.authorization_headers()
        )
        assert stub_a.authorization_headers(), "user A's turn hit no provider"
        assert stub_b.authorization_headers(), "user B's turn hit no provider"
        for stub, other in ((stub_a, key_b), (stub_b, key_a)):
            for req in stub.requests:
                assert other not in (
                    json.dumps(req["headers"]) + json.dumps(req["body"])
                ), f"other tenant's key {other} leaked: {req}"
        # Answers stay in their own threads.
        assert _assistant_answer(db, thread_a) == "answer-from-userA"
        assert _assistant_answer(db, thread_b) == "answer-from-userB"
        # No key leaks into either answer or turn state.
        for turn, thread in ((row_a, thread_a), (row_b, thread_b)):
            answer = _assistant_answer(db, thread) or ""
            assert key_a not in answer and key_b not in answer
    finally:
        for stub in stubs:
            stub.stop()


def test_turn_without_config_fails(db, broker, seeder):
    """Item 2 (assistant turns): a turn whose user has no BYOK config
    fails closed (``llm_config_missing`` turn failure), while the
    configured user's turn on the same worker completes."""
    tag = uuid.uuid4().hex[:8]
    key_a = f"turn-ok-{tag}"
    stub_a = _StreamStub("userA")
    try:
        world_a = create_member_world(seeder)
        seed_llm_config(seeder, world_a["owner"]["id"], stub_a.start(), key_a)
        thread_a, turn_a = _seed_turn(db, world_a, f"userA question {tag}")
        world_c = create_member_world(seeder)  # no LLM config
        thread_c, turn_c = _seed_turn(db, world_c, f"userC question {tag}")
        publish_task(broker, RUN_TURN, [turn_c])
        publish_task(broker, RUN_TURN, [turn_a])
        row_c = _wait_turn(db, turn_c)
        row_a = _wait_turn(db, turn_a)
        assert row_c["status"] == "failed", row_c
        assert row_c["error_code"] == "llm_config_missing", row_c
        assert row_a["status"] == "completed", row_a
        assert _assistant_answer(db, thread_a) == "answer-from-userA"
        assert stub_a.authorization_headers() == [f"Bearer {key_a}"] * len(
            stub_a.authorization_headers()
        )
    finally:
        stub_a.stop()
