"""Oracle: reasoning output on the OpenAI-compatible lane (PIDASHCONV-1488).

pydantic-ai 1.107.0 (``models/openai.py``, ``_agent_graph.py``) treats
reasoning as first-class: ``<think>`` tags split out of ``content`` into
``ThinkingPart`` (default profile tags ``('<think>','</think>')``), the
``reasoning`` / ``reasoning_content`` fields map to ``ThinkingPart``,
thinking-only responses run the thinking arm (text recovery, else the
retry prompt), and thinking replays on the next request (field-sourced
as ``message[field]``, tag-sourced as a ``<think>`` block).

Each test seeds one ``agent_run`` row plus a Fernet-encrypted BYOK
``openai_compatible`` config pointing at the scripted mock below,
publishes ``cloud_agent.run_agent_run`` through the live worker's
channel (Celery wire when a Django worker answers inspect, else a
``rust_job_queue`` row for ``pidash-api worker``), and asserts the
terminal row plus the recorded follow-up request bodies:

- (a) ``<think>plan</think>{"outcome":...}`` completes: the text output
  is the JSON after the tags.
- (b) empty content with only ``reasoning_content`` runs the
  thinking-only arm: no recoverable text, so the follow-up request
  replays the reasoning and carries the retry prompt.
- (c) ``reasoning_content`` plus a tool call: the follow-up request's
  assistant message carries ``reasoning_content``.

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
RETRY_PROMPT = "Please return text or include your response in a tool call."
RETRY_USER_TEXT = f"Validation feedback:\n{RETRY_PROMPT}\n\nFix the errors and try again."
FINAL_JSON = {"outcome": "completed", "summary": "reasoned and done"}

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


def test_think_tags_split_from_text(db, broker, seeder, world):
    """(a) tags in content split out; the JSON after them validates."""
    stub = _ScriptedProvider(
        [({"role": "assistant", "content": "<think>consider alternatives</think>" + json.dumps(FINAL_JSON)}, "stop")]
    )
    base_url = stub.start()
    try:
        _seed_llm_config(seeder, world["owner"]["id"], base_url)
        run = create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            prompt="contract reasoning run",
        )
        _publish(run["id"], broker)
        after = wait_for_run(db, run["id"], status=COMPLETED)
        assert after["error_code"] == ""
        assert after["done_payload"]["status"] == "completed"
        assert after["done_payload"]["summary"] == FINAL_JSON["summary"]
        assert len(stub.requests) == 1
    finally:
        stub.stop()


def test_reasoning_only_runs_thinking_arm(db, broker, seeder, world):
    """(b) thinking-only response: replay + retry prompt, not empty resubmit."""
    stub = _ScriptedProvider(
        [
            ({"role": "assistant", "content": "", "reasoning_content": "let me think"}, "stop"),
            ({"role": "assistant", "content": json.dumps(FINAL_JSON)}, "stop"),
        ]
    )
    base_url = stub.start()
    try:
        _seed_llm_config(seeder, world["owner"]["id"], base_url)
        run = create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            prompt="contract reasoning run",
        )
        _publish(run["id"], broker)
        after = wait_for_run(db, run["id"], status=COMPLETED)
        assert after["error_code"] == ""
        assert len(stub.requests) == 2
        first, second = _messages(stub, 0), _messages(stub, 1)
        assert first[0]["role"] == "system", first
        assert first[0]["content"].startswith("You are Pi Dash Cloud Agent."), first
        assert first[1] == {"role": "user", "content": "contract reasoning run"}
        assistant = next(m for m in second if m["role"] == "assistant")
        assert assistant["content"] is None, assistant
        assert assistant["reasoning_content"] == "let me think", assistant
        assert "reasoning" not in assistant, assistant
        assert second[-1] == {"role": "user", "content": RETRY_USER_TEXT}, second
    finally:
        stub.stop()


def test_reasoning_replayed_after_tool_call(db, broker, seeder, world):
    """(c) reasoning beside a tool call replays on the follow-up request."""
    call_id = "call_reason_1"
    stub = _ScriptedProvider(
        [
            (
                {
                    "role": "assistant",
                    "content": None,
                    "reasoning_content": "plan the lookup",
                    "tool_calls": [
                        {
                            "id": call_id,
                            "type": "function",
                            "function": {
                                "name": "pidash_list_project_states",
                                "arguments": "{}",
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
        state = seeder.create_state(
            world["workspace"]["id"], world["project"]["id"],
            name="Contract Todo", group="unstarted",
        )
        run = create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            prompt="contract reasoning run",
            tool_plan=json.dumps({"tools": ["pidash_list_project_states"]}),
        )
        _publish(run["id"], broker)
        after = wait_for_run(db, run["id"], status=COMPLETED)
        assert after["error_code"] == ""
        assert len(stub.requests) == 2
        second = _messages(stub, 1)
        assistant = next(m for m in second if m["role"] == "assistant")
        assert assistant["content"] is None, assistant
        assert assistant["reasoning_content"] == "plan the lookup", assistant
        assert assistant["tool_calls"] == [
            {
                "id": call_id,
                "type": "function",
                "function": {"name": "pidash_list_project_states", "arguments": "{}"},
            }
        ], assistant
        tool = next(m for m in second if m["role"] == "tool")
        assert tool["tool_call_id"] == call_id, tool
        # The seeded state row renders through the tool-result
        # serializer on both backends (compact JSON, id/name/group order).
        assert tool["content"] == json.dumps(
            [{"id": state["id"], "name": "Contract Todo", "group": "unstarted"}],
            separators=(",", ":"),
        ), tool
    finally:
        stub.stop()
