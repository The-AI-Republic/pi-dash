"""Concurrency + multi-tenant load validation: NEWPIDASH-54 part 2.

Part 2 of the NEWPIDASH-54 validation series: the cloud-agent runtime
must stay correct and tenant-isolated when many tenants run at once on
one worker. Item 3 of the issue (concurrency) plus the operational
halves of item 7's machinery (scan/dispatch backpressure and the stale
sweep, per tenant). Every case runs against Django (oracle) and the
Rust binary unchanged; whichever leg diverges from the other is the
defect, and it gets filed as its own NEWPIDASH issue (this module
itself fixes nothing).

Harness requirements (documented, not approximated):

* Worker concurrency must be raised to >= 20 on BOTH legs, otherwise
  "one slow provider stalls another tenant's run" is unobservable by
  construction: Django celery runs ``--pool=prefork --concurrency=24``
  (the solo pool serialises claims) and the Rust worker runs
  ``--concurrency 24`` (default 4). The two legs are never run with a
  mixed stack: fresh env per leg, publish goes through the detected
  channel (see ``conftest.publish_task``).
* The stack runs with ``CLOUD_AGENT_MODEL_REQUEST_TIMEOUT_SECONDS=5``
  and ``CLOUD_AGENT_TOOL_TIMEOUT_SECONDS=5`` so the timeout mix costs
  ~16s per leg instead of ~3 minutes. Slow stubs sleep 3s — under the
  5s model timeout, so they stall without failing.
* A hanging internal tool cannot be silenced by ``tool_timeout`` on
  either stack: sync function tools have no effective timeout (Django
  runs them in a thread and ``anyio.fail_after`` cannot interrupt the
  wait — proven by pydantic-ai's ``timeoutprobe.json`` and ported
  verbatim in ``model_runtime/agent.rs``: "Sync function tools have no
  effective timeout... The port runs them to completion."). Only MCP
  tools keep the real timeout, and the OSS GitHub MCP toolset needs a
  verified project repository (real GitHub App credentials), so the
  hang case instead pins the testable half: a ``select_for_update``
  write tool blocks past ``tool_timeout`` and runs to completion once
  unblocked, while another tenant's run completes behind it.
"""

from __future__ import annotations

import json
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from .conftest import (
    _MarkerStub,
    CANCELLED,
    COMPLETED,
    FAILED,
    LOCAL_RUNNER,
    QUEUED,
    RUNNING,
    create_agent_run,
    create_member_world,
    final_response,
    get_run,
    publish_run,
    publish_task,
    run_events,
    seed_llm_config,
    tool_call_response,
    tool_calls,
    wait_for_run,
    wait_for_terminal,
)

pytestmark = pytest.mark.contract

SCAN_TASK = "cloud_agent.scan_queued_runs"
SWEEP_TASK = "cloud_agent.sweep_stale_runs"
TWO_HOURS = 2 * 3600

WAVE_TENANTS = 10
WAVE_RUNS_PER_TENANT = 2
HEAVY_TENANT = 7
SLOW_TENANTS = (0, 1)
SLOW_RESPONSE_SECONDS = 3


def test_wave_20plus_runs_across_tenants(db, broker, seeder):
    """Item 3: 20 runs across 10 tenants on one worker stay isolated.

    Tenants 0/1 are slow (3s per provider response); the rest are
    instant. Tenant 7's second run is tool-heavy (two search calls).
    Asserts per-run history/usage/output separation, per-tenant key
    isolation from the recorded requests, and that instant tenants
    finish before the slow ones (one tenant's slow provider does not
    stall another's run). Worker concurrency on both legs is defined
    by the stack, never by this test.
    """
    tag = uuid.uuid4().hex[:8]
    keys = [f"wave-key-{tag}-{i}" for i in range(WAVE_TENANTS)]
    markers = {
        (i, j): f"WAVE{tag}-{i}-{j}" for i in range(WAVE_TENANTS) for j in range(2)
    }
    stubs: list[_MarkerStub] = []
    worlds = []
    runs = {}
    for i in range(WAVE_TENANTS):
        world = create_member_world(seeder)
        worlds.append(world)
        if i == HEAVY_TENANT:
            script = {
                markers[(i, 0)]: [final_response(summary=f"summary-{i}-0")],
                markers[(i, 1)]: [
                    tool_call_response(
                        f"call_heavy_1", "pidash_search_project_issues",
                        {"query": tag},
                    ),
                    tool_call_response(
                        f"call_heavy_2", "pidash_search_project_issues",
                        {"query": tag},
                    ),
                    final_response(summary=f"summary-{i}-1"),
                ],
            }
        else:
            script = {
                markers[(i, 0)]: [final_response(summary=f"summary-{i}-0")],
                markers[(i, 1)]: [
                    tool_call_response(
                        f"call_search_{i}", "pidash_search_project_issues",
                        {"query": tag},
                    ),
                    final_response(summary=f"summary-{i}-1"),
                ],
            }
        stub = _MarkerStub(script, delay=SLOW_RESPONSE_SECONDS if i in SLOW_TENANTS else 0)
        stubs.append(stub)
        base_url = stub.start()
        seed_llm_config(seeder, world["owner"]["id"], base_url, keys[i])
        for j in range(WAVE_RUNS_PER_TENANT):
            runs[(i, j)] = create_agent_run(
                seeder,
                world["workspace"]["id"],
                world["pod"]["id"],
                world["owner"]["id"],
                prompt=f"run {markers[(i, j)]}",
                tool_plan=json.dumps({"tools": ["pidash_search_project_issues"]}),
            )
    try:
        # Interleave tenants so claims overlap from the first publish.
        for j in range(WAVE_RUNS_PER_TENANT):
            for i in range(WAVE_TENANTS):
                publish_run(broker, runs[(i, j)]["id"])
        terminal_at: dict[tuple, float] = {}
        deadline = time.monotonic() + 120
        pending = set(runs)
        while pending and time.monotonic() < deadline:
            for key in list(pending):
                row = db.fetchone(
                    "SELECT status FROM agent_run WHERE id = %s",
                    (runs[key]["id"],),
                )
                if row is not None and row["status"] in (COMPLETED, FAILED):
                    terminal_at[key] = time.monotonic()
                    pending.remove(key)
            if pending:
                time.sleep(0.1)
        assert not pending, f"runs never reached terminal: {sorted(pending)}"
        after = {
            key: get_run(db, runs[key]["id"]) for key in runs
        }
        assert all(row["status"] == COMPLETED for row in after.values()), [
            (key, row["status"], row["error_code"]) for key, row in after.items()
            if row["status"] != COMPLETED
        ]
        for (i, j), row in after.items():
            assert row["done_payload"]["summary"] == f"summary-{i}-{j}"
            # Usage is per-run: j=0 made 1 model request, j=1 made 2
            # (3 for the heavy tenant). The stub reports
            # {prompt_tokens: 10, completion_tokens: 5, total_tokens: 15}
            # per response, so the run totals must match exactly.
            requests = 1 if j == 0 else (3 if i == HEAVY_TENANT else 2)
            usage = row["usage"] or {}
            assert usage.get("total") == 15 * requests, (i, j, usage)
            assert usage.get("input") == 10 * requests, (i, j, usage)
            assert usage.get("output") == 5 * requests, (i, j, usage)
            if j == 0:
                assert (row["done_payload"].get("tool_calls") or 0) == 0
            else:
                expected = 2 if i == HEAVY_TENANT else 1
                assert (row["done_payload"].get("tool_calls") or 0) == expected
        for i, stub in enumerate(stubs):
            headers = stub.authorization_headers()
            assert headers
            assert all(header == f"Bearer {keys[i]}" for header in headers)
            for k, other_key in enumerate(keys):
                if k != i:
                    stub.assert_key_never_appears(other_key)
            bodies = [json.dumps(req["body"]) for req in stub.requests]
            for (a, b), marker in markers.items():
                if a != i:
                    for text in bodies:
                        assert marker not in text
        instant_times = [
            at for (i, _j), at in terminal_at.items() if i not in SLOW_TENANTS
        ]
        slow_times = [
            at for (i, _j), at in terminal_at.items() if i in SLOW_TENANTS
        ]
        assert max(instant_times) < min(slow_times), (
            "an instant tenant finished after a slow one (stall)",
            max(instant_times),
            min(slow_times),
        )
    finally:
        for stub in stubs:
            stub.stop()


class _StatusStub:
    """Recording stub answering every request with one status/body.

    ``sleep_seconds`` delays the response (recorded on arrival) for the
    timeout mix: the request timeout (5s in this stack) fires long
    before the 20s sleep ends, and each retry records one more arrival.
    """

    def __init__(self, status, body: bytes, *, sleep_seconds=0):
        self.status = status
        self.body = body
        self.sleep_seconds = sleep_seconds
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
                raw = self.rfile.read(length) if length else b""
                try:
                    payload = json.loads(raw or b"{}")
                except ValueError:
                    payload = {}
                stub.requests.append(
                    {
                        "path": self.path,
                        "headers": {
                            k.lower(): v for k, v in self.headers.items()
                        },
                        "body": payload,
                    }
                )
                if stub.sleep_seconds:
                    time.sleep(stub.sleep_seconds)
                try:
                    self.send_response(stub.status)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(stub.body)))
                    self.end_headers()
                    self.wfile.write(stub.body)
                except (BrokenPipeError, ConnectionResetError):
                    pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._server.daemon_threads = True
        port = self._server.server_address[1]
        self._thread = threading.Thread(
            target=self._server.serve_forever, daemon=True
        )
        self._thread.start()
        return f"http://127.0.0.1:{port}/v1"

    def stop(self):
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None

    def authorization_headers(self):
        return [req["headers"].get("authorization") for req in self.requests]


def test_per_tenant_provider_failures_isolate(db, broker, seeder):
    """Item 3: one tenant's provider failure fails that tenant only.

    429 / 5xx / timeout all retry to 3 attempts on both stacks (openai
    ``_should_retry`` 408/409/429/5xx + connection errors, ported as
    Rust ``MAX_RETRIES=2``); a malformed 200 body ends the run with
    ``provider_error`` after one attempt. The healthy tenant's run
    completes while the slowest failure (timeout, ~16s) is still in
    flight — a failure stalls nothing else.
    """
    tag = uuid.uuid4().hex[:8]
    keys = {name: f"fail-key-{tag}-{name}" for name in ("429", "500", "timeout", "bad", "ok")}
    specs = [
        ("429", _StatusStub(429, json.dumps({"error": {"message": "rate limited"}}).encode())),
        ("500", _StatusStub(500, json.dumps({"error": {"message": "upstream exploded"}}).encode())),
        ("timeout", _StatusStub(200, json.dumps(final_response()).encode(), sleep_seconds=20)),
        ("bad", _StatusStub(200, b"{not json")),
        ("ok", _MarkerStub({f"OK{tag}": [final_response(summary="healthy done")]})),
    ]
    stubs = {}
    runs = {}
    try:
        for name, stub in specs:
            stubs[name] = stub
            world = create_member_world(seeder)
            seed_llm_config(seeder, world["owner"]["id"], stub.start(), keys[name])
            runs[name] = create_agent_run(
                seeder,
                world["workspace"]["id"],
                world["pod"]["id"],
                world["owner"]["id"],
                prompt=f"healthy OK{tag}" if name == "ok" else f"mix run {name} {tag}",
            )
        # Slowest first: the healthy run proves non-stall while the
        # timeout run (3×5s + backoff) is still in flight.
        for name in ("timeout", "429", "500", "bad", "ok"):
            publish_run(broker, runs[name]["id"])
        healthy = wait_for_run(db, runs["ok"]["id"], status=COMPLETED, timeout=60)
        assert healthy["done_payload"]["summary"] == "healthy done"
        slow_row = get_run(db, runs["timeout"]["id"])
        assert slow_row["status"] in (QUEUED, RUNNING), slow_row["status"]
        for name in ("429", "500", "timeout", "bad"):
            after = wait_for_terminal(db, runs[name]["id"], timeout=90)
            assert after["status"] == FAILED, (name, after["status"])
            assert after["error_code"] == "provider_error", (name, after["error_code"])
            headers = stubs[name].authorization_headers()
            if name in ("429", "500", "timeout"):
                assert len(headers) == 3, (name, len(headers))
            else:
                assert len(headers) == 1, (name, len(headers))
            assert all(h == f"Bearer {keys[name]}" for h in headers)
            assert keys[name] not in (after.get("error") or "")
            assert keys[name] not in str(after.get("done_payload") or "")
        assert stubs["ok"].authorization_headers() == [f"Bearer {keys['ok']}"]
    finally:
        for stub in stubs.values():
            stub.stop()


def test_hanging_tool_runs_to_completion_past_tool_timeout(db, broker, seeder, settings):
    """Item 3: a tool blocked past ``tool_timeout`` never stalls others.

    Sync function tools have no effective timeout on either stack
    (``timeoutprobe.json``; ``model_runtime/agent.rs`` header): a
    ``select_for_update`` write tool blocked on a held row lock runs
    to completion once the lock releases, even though the block far
    exceeds ``CLOUD_AGENT_TOOL_TIMEOUT_SECONDS``. The healthy
    tenant's run completes while this run is still blocked.
    """
    import psycopg

    tag = uuid.uuid4().hex[:8]
    key_hang = f"hang-key-{tag}"
    key_ok = f"ok-key-{tag}"
    world_hang = create_member_world(seeder)
    state_a = seeder.create_state(
        world_hang["workspace"]["id"], world_hang["project"]["id"],
        name="Backlog", group="backlog",
    )
    state_b = seeder.create_state(
        world_hang["workspace"]["id"], world_hang["project"]["id"],
        name="In Progress", group="started",
    )
    issue = seeder.create_issue(
        world_hang["workspace"]["id"], world_hang["project"]["id"],
        state_a["id"], name=f"HANG-{tag}",
    )
    stub_hang = _MarkerStub(
        {
            f"HANG{tag}": [
                tool_call_response(
                    "call_transition",
                    "pidash_transition_current_issue",
                    {"state_id": state_b["id"]},
                ),
                final_response(summary="moved"),
            ]
        }
    )
    stub_ok = _MarkerStub({f"OK{tag}": [final_response(summary="healthy done")]})
    base_hang = stub_hang.start()
    base_ok = stub_ok.start()
    lock_conn = None
    try:
        seed_llm_config(seeder, world_hang["owner"]["id"], base_hang, key_hang)
        run_hang = create_agent_run(
            seeder,
            world_hang["workspace"]["id"],
            world_hang["pod"]["id"],
            world_hang["owner"]["id"],
            prompt=f"move the issue HANG{tag}",
            tool_plan=json.dumps({"tools": ["pidash_transition_current_issue"]}),
            work_item_id=issue["id"],
        )
        world_ok = create_member_world(seeder)
        seed_llm_config(seeder, world_ok["owner"]["id"], base_ok, key_ok)
        run_ok = create_agent_run(
            seeder,
            world_ok["workspace"]["id"],
            world_ok["pod"]["id"],
            world_ok["owner"]["id"],
            prompt=f"healthy OK{tag}",
        )
        # Hold the issue row's lock FIRST so the tool's
        # select_for_update blocks deterministically.
        lock_conn = psycopg.connect(settings.database_url)
        lock_conn.autocommit = False
        lock_cur = lock_conn.cursor()
        lock_cur.execute(
            "SELECT id FROM issues WHERE id = %s FOR UPDATE", (issue["id"],)
        )
        publish_run(broker, run_hang["id"])
        blocked = wait_for_run(db, run_hang["id"], status=RUNNING, timeout=30)
        publish_run(broker, run_ok["id"])
        healthy = wait_for_run(db, run_ok["id"], status=COMPLETED, timeout=30)
        assert healthy["done_payload"]["summary"] == "healthy done"
        # The blocked run has not been touched by the healthy tenant's
        # completion: still running, no transition yet.
        still = get_run(db, run_hang["id"])
        assert still["status"] == RUNNING
        state_now = db.fetchval(
            "SELECT state_id FROM issues WHERE id = %s", (issue["id"],)
        )
        assert state_now is None or str(state_now) == str(state_a["id"])
        lock_conn.rollback()
        lock_conn.close()
        lock_conn = None
        after = wait_for_run(db, run_hang["id"], status=COMPLETED, timeout=60)
        assert after["done_payload"]["summary"] == "moved"
        calls = tool_calls(db, run_hang["id"])
        assert len(calls) == 1
        assert calls[0]["status"] == "succeeded"
        state_after = db.fetchval(
            "SELECT state_id FROM issues WHERE id = %s", (issue["id"],)
        )
        assert str(state_after) == str(state_b["id"])
        # Two model requests (tool call, then final) both carry the
        # same tenant key — and nothing else did.
        assert stub_hang.authorization_headers() == [
            f"Bearer {key_hang}",
            f"Bearer {key_hang}",
        ]
        assert stub_ok.authorization_headers() == [f"Bearer {key_ok}"]
    finally:
        if lock_conn is not None:
            try:
                lock_conn.rollback()
                lock_conn.close()
            except Exception:
                pass
        stub_hang.stop()
        stub_ok.stop()


def test_single_active_run_rule_per_issue(db, broker, seeder):
    """Item 3: one active run per issue, enforced by the DB constraint.

    A second QUEUED run for the same work item violates
    ``agent_run_one_active_per_work_item`` (migration 0019/0023) and
    cannot even be seeded while the first is active; once the first
    completes, the second inserts and runs. The published first run
    reaches COMPLETED on its own stub.
    """
    world = create_member_world(seeder)
    stub = _MarkerStub(
        {
            "FIRST": [final_response(summary="first done")],
            "SECOND": [final_response(summary="second done")],
        }
    )
    base_url = stub.start()
    try:
        seed_llm_config(seeder, world["owner"]["id"], base_url, "single-key")
        issue = seeder.create_issue(
            world["workspace"]["id"], world["project"]["id"], name="single-active"
        )
        first = create_agent_run(
            seeder,
            world["workspace"]["id"],
            world["pod"]["id"],
            world["owner"]["id"],
            prompt="FIRST",
            work_item_id=issue["id"],
        )
        with pytest.raises(Exception) as excinfo:
            create_agent_run(
                seeder,
                world["workspace"]["id"],
                world["pod"]["id"],
                world["owner"]["id"],
                prompt="SECOND",
                work_item_id=issue["id"],
            )
        message = str(excinfo.value)
        assert "agent_run_one_active_per_work_item" in message, message
        publish_run(broker, first["id"])
        after = wait_for_run(db, first["id"], status=COMPLETED, timeout=30)
        assert after["done_payload"]["summary"] == "first done"
        second = create_agent_run(
            seeder,
            world["workspace"]["id"],
            world["pod"]["id"],
            world["owner"]["id"],
            prompt="SECOND",
            work_item_id=issue["id"],
        )
        publish_run(broker, second["id"])
        after2 = wait_for_run(db, second["id"], status=COMPLETED, timeout=30)
        assert after2["done_payload"]["summary"] == "second done"
    finally:
        stub.stop()


def test_scan_dispatch_backpressure_per_tenant(db, broker, seeder):
    """Item 3: scan backpressure is per tenant.

    Workspace W1 is at capacity (2 RUNNING): its QUEUED run survives
    the scan untouched. Tenant W2 (fresh) dispatches its QUEUED run
    through the full scan → lease → publish → claim → execute chain
    (no LLM config → ``llm_config_missing``, ``dispatch_attempts == 1``).
    Each tenant's world is its own; neither bleeds into the other.
    """
    world_full = create_member_world(seeder)
    world_open = create_member_world(seeder)
    for _ in range(2):
        create_agent_run(
            seeder,
            world_full["workspace"]["id"],
            world_full["pod"]["id"],
            world_full["owner"]["id"],
            status=RUNNING,
        )
    blocked_run = create_agent_run(
        seeder,
        world_full["workspace"]["id"],
        world_full["pod"]["id"],
        world_full["owner"]["id"],
    )
    open_run = create_agent_run(
        seeder,
        world_open["workspace"]["id"],
        world_open["pod"]["id"],
        world_open["owner"]["id"],
    )
    publish_task(broker, SCAN_TASK)
    after_open = wait_for_run(db, open_run["id"], status=FAILED, timeout=60)
    assert after_open["error_code"] == "llm_config_missing"
    assert after_open["dispatch_attempts"] == 1
    # Give any stray scan a beat to act, then prove W1 never moved.
    time.sleep(2)
    blocked_after = get_run(db, blocked_run["id"])
    assert blocked_after["status"] == QUEUED
    assert blocked_after["dispatch_attempts"] == 0
    assert blocked_after["lease_expires_at"] is None


def test_sweep_stale_runs_per_tenant(db, broker, seeder):
    """Item 3: the stale sweep is per tenant and exact per the oracle.

    Both tenants' stale RUNNING rows are FAILED with ``run_timeout``
    and a terminal event; fresh RUNNING, other-executor RUNNING, and
    terminal rows are untouched — independently per tenant.
    """
    world_a = create_member_world(seeder)
    world_b = create_member_world(seeder)
    stale_a = create_agent_run(
        seeder,
        world_a["workspace"]["id"], world_a["pod"]["id"], world_a["owner"]["id"],
        status=RUNNING, started_ago_seconds=TWO_HOURS,
    )
    stale_b = create_agent_run(
        seeder,
        world_b["workspace"]["id"], world_b["pod"]["id"], world_b["owner"]["id"],
        status=RUNNING, started_ago_seconds=TWO_HOURS,
    )
    fresh = create_agent_run(
        seeder,
        world_a["workspace"]["id"], world_a["pod"]["id"], world_a["owner"]["id"],
        status=RUNNING,
    )
    local = create_agent_run(
        seeder,
        world_b["workspace"]["id"], world_b["pod"]["id"], world_b["owner"]["id"],
        executor_kind=LOCAL_RUNNER, status=RUNNING, started_ago_seconds=TWO_HOURS,
    )
    cancelled = create_agent_run(
        seeder,
        world_b["workspace"]["id"], world_b["pod"]["id"], world_b["owner"]["id"],
        status=RUNNING, started_ago_seconds=TWO_HOURS,
        cancel_reason="stop it",
    )
    terminal = create_agent_run(
        seeder,
        world_a["workspace"]["id"], world_a["pod"]["id"], world_a["owner"]["id"],
        status=COMPLETED, started_ago_seconds=TWO_HOURS,
    )
    publish_task(broker, SWEEP_TASK)
    for stale in (stale_a, stale_b):
        after = wait_for_run(db, stale["id"], status=FAILED, timeout=60)
        assert after["error_code"] == "run_timeout", after["error_code"]
        kinds = [row["kind"] for row in run_events(db, stale["id"])]
        assert "terminal" in kinds
    cancel_after = wait_for_run(db, cancelled["id"], status=CANCELLED, timeout=60)
    assert cancel_after["error_code"] == "cancelled"
    assert cancel_after["error"] == "stop it"
    time.sleep(1)
    assert get_run(db, fresh["id"])["status"] == RUNNING
    assert get_run(db, local["id"])["status"] == RUNNING
    terminal_after = get_run(db, terminal["id"])
    assert terminal_after["status"] == COMPLETED
    assert run_events(db, terminal["id"]) == []
