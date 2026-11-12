"""Oracle: empty tool results on the OpenAI-compatible lane (PIDASHCONV-1489).

pydantic-ai 1.107.0 (``messages.py`` ``ToolReturnPart._unwrap_data``,
``model_response_str``) renders an empty tool result — an empty list or
``None`` — as the empty string in the follow-up OpenAI tool message
(``models/openai.py`` ``_map_messages``), not as ``"[]"``/``"null"``.

The test seeds one ``agent_run`` row plus a Fernet-encrypted BYOK
``openai_compatible`` config pointing at the scripted mock below,
publishes ``cloud_agent.run_agent_run`` through the live worker's
channel (Celery wire when a Django worker answers inspect, else a
``rust_job_queue`` row for ``pidash-api worker``), and asserts the
terminal row plus the recorded follow-up request body:

- the stub answers request 1 with a ``pidash_search_project_issues``
  tool call whose query matches nothing, and request 2 with the final
  JSON output;
- the run completes, and the second request's ``{"role": "tool"}``
  message carries ``"content": ""`` — not ``"[]"``.

The mock binds 127.0.0.1, so runs need ``ASSISTANT_BLOCK_PRIVATE_URLS``
off (the guard itself is pinned by the assistant suite).
"""

from __future__ import annotations

import json
import os
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from _harness import broker_probe, rust_queue

from .conftest import COMPLETED, create_agent_run, wait_for_run

pytestmark = pytest.mark.contract

RUN_TASK = "cloud_agent.run_agent_run"
MODEL_NAME = "oracle-stub"
API_KEY = "test-key-123"
FINAL_JSON = {"outcome": "completed", "summary": "empty search done"}
NO_MATCH_QUERY = "zzz-no-such-issue-1489"

_channel = None


def _publish(run_id, broker):
    """Publish through the live worker's channel, detected once per session.

    A Django worker answers Celery inspect and consumes the redis wire;
    otherwise the Rust worker claims ``rust_job_queue`` rows. Exactly one
    worker runs in any contract run, so exactly one channel delivers.
    """
    global _channel
    if _channel is None:
        try:
            live = bool(broker_probe.registered_tasks(timeout=5.0))
        except Exception:
            live = False
        _channel = "celery" if live else "rust_queue"
        if _channel == "rust_queue":
            rust_queue.ensure_schema()
    if _channel == "celery":
        broker.publish(RUN_TASK, [run_id])
    else:
        rust_queue.publish(RUN_TASK, [run_id])


def _completion(message, *, finish_reason="stop"):
    return {
        "id": f"stub-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion",
        "created": 1700000000,
        "model": MODEL_NAME,
        "choices": [
            {"index": 0, "message": message, "finish_reason": finish_reason}
        ],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
    }


class _ScriptedProvider:
    """Mock OpenAI-compatible endpoint serving one scripted message per request.

    ``script`` is ``[(message, finish_reason), ...]``; request ``n`` gets
    entry ``n`` (the last entry repeats, so output-retry loops observe a
    stable provider instead of hanging the worker). Every request body is
    recorded for follow-up assertions.
    """

    def __init__(self, script):
        self.script = list(script)
        self.requests = []
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
                raw = self.rfile.read(length) if length else b""
                try:
                    payload = json.loads(raw or b"{}")
                except ValueError:
                    payload = {}
                stub.requests.append({"path": self.path, "body": payload})
                index = min(len(stub.requests) - 1, len(stub.script) - 1)
                message, finish = stub.script[index]
                content = json.dumps(_completion(message, finish_reason=finish)).encode()
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


def _seed_llm_config(seeder, user_id, base_url):
    """Seed a decryptable BYOK config pointing at the mock endpoint."""
    from cryptography.fernet import Fernet

    key = os.environ["ASSISTANT_ENCRYPTION_KEY"].encode("utf-8")
    token = Fernet(key).encrypt(API_KEY.encode("utf-8"))
    seeder.db.execute(
        """INSERT INTO assistant_user_llm_config
           (provider_kind, base_url, model_name, api_key_encrypted,
            last_verified_at, created_at, updated_at, user_id)
           VALUES ('openai_compatible',%s,%s,%s,NULL,now(),now(),%s)""",
        (base_url, MODEL_NAME, token, user_id),
    )
    row_id = seeder.db.fetchval(
        "SELECT id FROM assistant_user_llm_config WHERE user_id = %s", (user_id,)
    )
    seeder.track.add("assistant_user_llm_config", row_id)
    return {"id": row_id}


def _messages(stub, index):
    req = stub.requests[index]
    assert req["path"].endswith("/chat/completions"), req["path"]
    body = req["body"]
    assert body.get("stream") is False, body
    return body["messages"]


def test_empty_search_tool_result_is_empty_string(db, broker, seeder, world):
    """A search with no matches sends ``"content": ""`` on the follow-up."""
    call_id = "call_empty_search_1"
    arguments = json.dumps({"query": NO_MATCH_QUERY}, separators=(",", ":"))
    stub = _ScriptedProvider(
        [
            (
                {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [
                        {
                            "id": call_id,
                            "type": "function",
                            "function": {
                                "name": "pidash_search_project_issues",
                                "arguments": arguments,
                            },
                        }
                    ],
                },
                "tool_calls",
            ),
            ({"role": "assistant", "content": json.dumps(FINAL_JSON)}, "stop"),
        ]
    )
    base_url = stub.start()
    try:
        _seed_llm_config(seeder, world["owner"]["id"], base_url)
        run = create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            prompt="contract empty search run",
            tool_plan=json.dumps({"tools": ["pidash_search_project_issues"]}),
        )
        _publish(run["id"], broker)
        after = wait_for_run(db, run["id"], status=COMPLETED)
        assert after["error_code"] == ""
        assert after["done_payload"]["status"] == "completed"
        assert len(stub.requests) == 2
        second = _messages(stub, 1)
        assistant = next(m for m in second if m["role"] == "assistant")
        assert assistant["content"] is None, assistant
        assert assistant["tool_calls"] == [
            {
                "id": call_id,
                "type": "function",
                "function": {
                    "name": "pidash_search_project_issues",
                    "arguments": arguments,
                },
            }
        ], assistant
        tool = next(m for m in second if m["role"] == "tool")
        assert tool["tool_call_id"] == call_id, tool
        # `_unwrap_data` (`messages.py:1179-1187`): the empty match list
        # renders as the empty string, not `"[]"` — on both backends.
        assert tool["content"] == "", tool
    finally:
        stub.stop()
