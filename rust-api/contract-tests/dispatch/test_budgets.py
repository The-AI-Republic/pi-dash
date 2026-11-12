"""Budgets and per-run limit counting: NEWPIDASH-54 part 3.

Part 3 of the NEWPIDASH-54 validation series, item 4 of the issue:
per-run usage limits (requests, tokens), ``retries=2``, output
retries and the write budget must be counted per run — never shared
or leaked across runs — and end runs with the same error and state
as Django.

Every case seeds per-run limits in ``tool_plan["limits"]``
(``model_requests`` / ``tool_calls`` / ``input_tokens`` /
``output_tokens`` / ``total_tokens`` — both stacks read them from the
plan identically; ``runtime.py:56-62`` and ``usage_limits_for``).
Over-limit runs end ``FAILED``/``iteration_limit`` (Django classifies
``UsageLimitExceeded`` by type; the Rust port maps
``UsageLimit`` → ``iteration_limit`` the same way). Any leg
divergence is a parity defect and would be filed as its own
NEWPIDASH issue — this module fixes nothing.
"""

from __future__ import annotations

import json
import uuid

import pytest

from .conftest import (
    _MarkerStub,
    COMPLETED,
    FAILED,
    create_agent_run,
    create_member_world,
    final_response,
    publish_run,
    seed_llm_config,
    tool_call_response,
    tool_calls,
    wait_for_run,
    wait_for_terminal,
)

pytestmark = pytest.mark.contract

STUB_MODEL = "oracle-stub"


def _plain_text_response(text):
    return {
        "id": f"stub-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion",
        "created": 1700000000,
        "model": STUB_MODEL,
        "choices": [
            {
                "index": 0,
                "message": {"role": "assistant", "content": text},
                "finish_reason": "stop",
            }
        ],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
    }


def _issue(seeder, world, name):
    return seeder.create_issue(world["workspace"]["id"], world["project"]["id"], name=name)


def _run(db, broker, seeder, world, *, prompt, tool_plan="{}", limits=None):
    plan = json.loads(tool_plan)
    if limits:
        plan["limits"] = limits
    return create_agent_run(
        seeder,
        world["workspace"]["id"],
        world["pod"]["id"],
        world["owner"]["id"],
        prompt=prompt,
        tool_plan=json.dumps(plan),
    )


def test_request_limit_counts_per_run(db, broker, seeder):
    """Item 4: the per-run model-request limit is enforced and never
    shared. Tenant A's ``model_requests=1`` run dies ``iteration_limit``
    after exactly 1 request; tenant B (no tight limit) runs the same
    script to COMPLETED with 2 requests."""
    tag = uuid.uuid4().hex[:8]
    key_a, key_b = f"req-a-{tag}", f"req-b-{tag}"
    stub_a = _MarkerStub(
        {
            f"LIMIT{tag}": [
                tool_call_response("c1", "pidash_search_project_issues", {"query": tag}),
                final_response(summary="a done"),
            ]
        }
    )
    stub_b = _MarkerStub(
        {
            f"FRESH{tag}": [
                tool_call_response("c1", "pidash_search_project_issues", {"query": tag}),
                final_response(summary="b done"),
            ]
        }
    )
    stubs = (stub_a, stub_b)
    try:
        world_a = create_member_world(seeder)
        seed_llm_config(seeder, world_a["owner"]["id"], stub_a.start(), key_a)
        run_a = _run(
            db, broker, seeder, world_a, prompt=f"LIMIT{tag}",
            tool_plan=json.dumps({"tools": ["pidash_search_project_issues"]}),
            limits={"model_requests": 1},
        )
        world_b = create_member_world(seeder)
        seed_llm_config(seeder, world_b["owner"]["id"], stub_b.start(), key_b)
        run_b = _run(
            db, broker, seeder, world_b, prompt=f"FRESH{tag}",
            tool_plan=json.dumps({"tools": ["pidash_search_project_issues"]}),
        )
        publish_run(broker, run_a["id"])
        publish_run(broker, run_b["id"])
        after_a = wait_for_terminal(db, run_a["id"], timeout=60)
        after_b = wait_for_terminal(db, run_b["id"], timeout=60)
        assert after_a["status"] == FAILED
        assert after_a["error_code"] == "iteration_limit"
        assert len(stub_a.requests) == 1
        assert after_b["status"] == COMPLETED
        assert after_b["done_payload"]["summary"] == "b done"
        assert len(stub_b.requests) == 2
        assert stub_a.authorization_headers() == [f"Bearer {key_a}"]
        assert stub_b.authorization_headers() == [f"Bearer {key_b}", f"Bearer {key_b}"]
    finally:
        for stub in stubs:
            stub.stop()


def test_input_token_limit_counts_per_run(db, broker, seeder):
    """Item 4: the per-run input-token limit is enforced and never
    shared. With ``input_tokens=15`` and 10 input tokens per response,
    the second request's cumulative total (20) trips the limit; the
    twin tenant (no tight limit) completes, and a second run for the
    limited tenant trips again (counting restarts per run)."""
    tag = uuid.uuid4().hex[:8]
    key_a, key_b = f"tok-a-{tag}", f"tok-b-{tag}"
    stub_a = _MarkerStub(
        {
            f"TOK{tag}": [
                tool_call_response("c1", "pidash_search_project_issues", {"query": tag}),
                final_response(summary="a done"),
            ],
            f"TOK2{tag}": [
                tool_call_response("c1", "pidash_search_project_issues", {"query": tag}),
                final_response(summary="a2 done"),
            ],
        }
    )
    stub_b = _MarkerStub(
        {
            f"SOK{tag}": [
                tool_call_response("c1", "pidash_search_project_issues", {"query": tag}),
                final_response(summary="b done"),
            ]
        }
    )
    stubs = (stub_a, stub_b)
    try:
        world_a = create_member_world(seeder)
        seed_llm_config(seeder, world_a["owner"]["id"], stub_a.start(), key_a)
        limits = {"input_tokens": 15}
        run_a1 = _run(
            db, broker, seeder, world_a, prompt=f"TOK{tag}",
            tool_plan=json.dumps({"tools": ["pidash_search_project_issues"]}),
            limits=limits,
        )
        world_b = create_member_world(seeder)
        seed_llm_config(seeder, world_b["owner"]["id"], stub_b.start(), key_b)
        run_b = _run(
            db, broker, seeder, world_b, prompt=f"SOK{tag}",
            tool_plan=json.dumps({"tools": ["pidash_search_project_issues"]}),
        )
        run_a2 = _run(
            db, broker, seeder, world_a, prompt=f"TOK2{tag} again",
            tool_plan=json.dumps({"tools": ["pidash_search_project_issues"]}),
            limits=limits,
        )
        publish_run(broker, run_a1["id"])
        publish_run(broker, run_b["id"])
        publish_run(broker, run_a2["id"])
        after_a1 = wait_for_terminal(db, run_a1["id"], timeout=60)
        after_a2 = wait_for_terminal(db, run_a2["id"], timeout=60)
        after_b = wait_for_terminal(db, run_b["id"], timeout=60)
        for after in (after_a1, after_a2):
            assert after["status"] == FAILED
            assert after["error_code"] == "iteration_limit"
        assert after_b["status"] == COMPLETED
        assert len(stub_b.requests) == 2
        # Both limited runs consumed 2 requests each (the trip lands
        # after the second response) — so counting restarts per run:
        # a shared counter would have tripped run a2 after 1 request
        # (10 + 10 = 20 > 15).
        per_marker = {}
        for req in stub_a.requests:
            per_marker[req["marker"]] = per_marker.get(req["marker"], 0) + 1
        assert per_marker == {f"TOK{tag}": 2, f"TOK2{tag}": 2}, per_marker
        assert stub_a.authorization_headers() == [f"Bearer {key_a}"] * 4
        assert after_b["done_payload"]["summary"] == "b done"
    finally:
        for stub in stubs:
            stub.stop()


def test_output_validation_retries_per_run(db, broker, seeder):
    """Item 4: invalid output is retried under ``retries=2`` (3 model
    requests), then ends the run — without affecting the twin tenant's
    valid run."""
    tag = uuid.uuid4().hex[:8]
    key_a, key_b = f"out-a-{tag}", f"out-b-{tag}"
    stub_a = _MarkerStub({f"TEXT{tag}": [_plain_text_response("not an outcome")]})
    stub_b = _MarkerStub({f"VALID{tag}": [final_response(summary="b done")]})
    stubs = (stub_a, stub_b)
    try:
        world_a = create_member_world(seeder)
        seed_llm_config(seeder, world_a["owner"]["id"], stub_a.start(), key_a)
        run_a = _run(db, broker, seeder, world_a, prompt=f"TEXT{tag}")
        world_b = create_member_world(seeder)
        seed_llm_config(seeder, world_b["owner"]["id"], stub_b.start(), key_b)
        run_b = _run(db, broker, seeder, world_b, prompt=f"VALID{tag}")
        publish_run(broker, run_a["id"])
        publish_run(broker, run_b["id"])
        after_a = wait_for_terminal(db, run_a["id"], timeout=60)
        after_b = wait_for_terminal(db, run_b["id"], timeout=60)
        assert after_a["status"] == FAILED
        assert after_a["error_code"] == "provider_error"
        assert len(stub_a.requests) == 3, len(stub_a.requests)
        assert after_b["status"] == COMPLETED
        assert after_b["done_payload"]["summary"] == "b done"
        assert len(stub_b.requests) == 1
        assert stub_a.authorization_headers() == [f"Bearer {key_a}"] * 3
    finally:
        for stub in stubs:
            stub.stop()


def test_write_budget_counts_per_run(db, broker, seeder):
    """Item 4: the write budget (3 writes) is counted per run. Tenant A
    keeps relating past the budget — the over-budget run ends FAILED
    while tenant B's 3 writes stay within budget and complete."""
    tag = uuid.uuid4().hex[:8]
    key_a, key_b = f"wri-a-{tag}", f"wri-b-{tag}"
    world_a = create_member_world(seeder)
    issue_a = _issue(seeder, world_a, f"WRI-A-{tag}")
    target_a = _issue(seeder, world_a, f"WRI-A-T-{tag}")
    relate_args = {
        "issue": issue_a["id"],
        "relation_type": "relates_to",
        "related_issues": [target_a["id"]],
    }
    stub_a = _MarkerStub(
        {
            f"WIDE{tag}": [
                tool_call_response(
                    f"w{i}", "pidash_relate_issues", relate_args
                )
                for i in range(1, 7)
            ]
        }
    )
    world_b = create_member_world(seeder)
    issue_b = _issue(seeder, world_b, f"WRI-B-{tag}")
    target_b = _issue(seeder, world_b, f"WRI-B-T-{tag}")
    relate_args_b = {
        "issue": issue_b["id"],
        "relation_type": "relates_to",
        "related_issues": [target_b["id"]],
    }
    stub_b = _MarkerStub(
        {
            f"NARROW{tag}": [
                tool_call_response(
                    f"w{i}", "pidash_relate_issues", relate_args_b
                )
                for i in range(1, 4)
            ]
            + [final_response(summary="b done")]
        }
    )
    stubs = (stub_a, stub_b)
    try:
        seed_llm_config(seeder, world_a["owner"]["id"], stub_a.start(), key_a)
        run_a = _run(
            db, broker, seeder, world_a, prompt=f"WIDE{tag}",
            tool_plan=json.dumps({"tools": ["pidash_relate_issues"]}),
        )
        seed_llm_config(seeder, world_b["owner"]["id"], stub_b.start(), key_b)
        run_b = _run(
            db, broker, seeder, world_b, prompt=f"NARROW{tag}",
            tool_plan=json.dumps({"tools": ["pidash_relate_issues"]}),
        )
        publish_run(broker, run_a["id"])
        publish_run(broker, run_b["id"])
        after_a = wait_for_terminal(db, run_a["id"], timeout=90)
        after_b = wait_for_terminal(db, run_b["id"], timeout=60)
        assert after_b["status"] == COMPLETED
        assert (after_b["done_payload"].get("writes") or 0) == 3
        assert after_a["status"] == FAILED
        assert after_a["error_code"] == "provider_error"
        # Exactly 3 writes succeeded in the overshooting run too (the
        # 4th was denied) — the budget stopped it, not the finalizer.
        succeeded = [
            c for c in tool_calls(db, run_a["id"]) if c["status"] == "succeeded"
        ]
        assert len(succeeded) == 3
        assert len(tool_calls(db, run_b["id"])) == 3
    finally:
        for stub in stubs:
            stub.stop()
