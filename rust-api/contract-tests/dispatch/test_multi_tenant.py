"""Multi-tenant validation: cloud agent isolation + credentials (NEWPIDASH-54).

Part 1 of the NEWPIDASH-54 validation series: the tenant-isolation and
credential items. Several tenants (separate workspaces, users, projects,
pods, and per-tenant BYOK configs) share one worker; an OpenAI-compatible
tenant points at its own recording stub, one tenant has an Anthropic
config (its requests cannot be recorded — both stacks pin
``api.anthropic.com`` with no base URL — so that lane is pinned by
Django↔Rust outcome parity plus cross-tenant non-contamination), and one
tenant has no config at all.

The N54 stack must run with the private-URL guard **off**
(``ASSISTANT_BLOCK_PRIVATE_URLS`` unset/false, mirroring the existing
dispatch tests) so the 127.0.0.1 stubs are reachable; the guard itself is
pinned by the assistant suite and by the SSRF cases in a later part.

Each test is black-box: seed rows, seed a Fernet-encrypted BYOK config,
publish ``cloud_agent.run_agent_run`` through the live worker's channel,
and assert the terminal rows plus the recorded provider traffic. The
same module runs against Django (oracle) and the Rust binary unchanged.
"""

from __future__ import annotations

import json

import pytest

from .conftest import (
    COMPLETED,
    FAILED,
    ScriptedProvider,
    create_agent_run,
    create_member_world,
    delete_llm_config,
    final_response,
    publish_run,
    rotate_llm_key,
    seed_llm_config,
    tool_call_response,
    tool_calls,
    wait_for_run,
    wait_for_terminal,
)

pytestmark = pytest.mark.contract

KEY_A = "tenant-a-secret-7f3c"
KEY_B = "tenant-b-secret-9d21"
KEY_B_ROTATED = "tenant-b-secret-rotated-51e8"
KEY_D = "tenant-d-secret-anthropic-2b94"

MARKER = "SHAREDMARKER"


@pytest.fixture()
def world_a(seeder):
    return create_member_world(seeder)


@pytest.fixture()
def world_b(seeder):
    return create_member_world(seeder)


@pytest.fixture()
def world_c(seeder):
    return create_member_world(seeder)


def _issue(seeder, world, name):
    return seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], name=name
    )


def _run(db, broker, seeder, world, *, prompt, tool_plan="{}", work_item_id=None):
    return create_agent_run(
        seeder,
        world["workspace"]["id"],
        world["pod"]["id"],
        world["owner"]["id"],
        prompt=prompt,
        tool_plan=tool_plan,
        work_item_id=work_item_id,
    )


def test_each_tenant_run_uses_only_own_key_and_base_url(db, broker, seeder, world_a, world_b):
    """Item 2: A's stub sees only A's key; B's stub sees only B's key."""
    stub_a = ScriptedProvider([final_response(summary="tenant-a done")])
    stub_b = ScriptedProvider([final_response(summary="tenant-b done")])
    base_a = stub_a.start()
    base_b = stub_b.start()
    try:
        assert base_a != base_b
        seed_llm_config(seeder, world_a["owner"]["id"], base_a, KEY_A)
        seed_llm_config(seeder, world_b["owner"]["id"], base_b, KEY_B)
        run_a = _run(db, broker, seeder, world_a, prompt="tenant a run")
        run_b = _run(db, broker, seeder, world_b, prompt="tenant b run")
        publish_run(broker, run_a["id"])
        publish_run(broker, run_b["id"])
        after_a = wait_for_run(db, run_a["id"], status=COMPLETED)
        after_b = wait_for_run(db, run_b["id"], status=COMPLETED)
        # Each stub saw exactly its own tenant's key, nothing else.
        assert stub_a.authorization_headers() == [f"Bearer {KEY_A}"]
        assert stub_b.authorization_headers() == [f"Bearer {KEY_B}"]
        stub_a.assert_key_never_appears(KEY_B)
        stub_b.assert_key_never_appears(KEY_A)
        assert all(req["path"].endswith("/chat/completions") for req in stub_a.requests)
        assert all(req["path"].endswith("/chat/completions") for req in stub_b.requests)
        # Each run's own output stayed its own.
        assert after_a["done_payload"]["summary"] == "tenant-a done"
        assert after_b["done_payload"]["summary"] == "tenant-b done"
        assert after_a["done_payload"]["tool_calls"] == 0
        assert after_b["done_payload"]["tool_calls"] == 0
        # No key appears in either run's stored error/output.
        for after in (after_a, after_b):
            assert KEY_A not in str(after["done_payload"])
            assert KEY_B not in str(after["done_payload"])
    finally:
        stub_a.stop()
        stub_b.stop()


def test_key_rotation_is_seen_by_next_run(db, broker, seeder, world_b):
    """Item 2: rotating a key (new ciphertext) invalidates the key cache."""
    stub = ScriptedProvider([final_response(summary="before rotation")])
    base_url = stub.start()
    try:
        config = seed_llm_config(seeder, world_b["owner"]["id"], base_url, KEY_B)
        run1 = _run(db, broker, seeder, world_b, prompt="run before rotation")
        publish_run(broker, run1["id"])
        wait_for_run(db, run1["id"], status=COMPLETED)
        assert stub.authorization_headers() == [f"Bearer {KEY_B}"]

        rotate_llm_key(seeder, config["id"], KEY_B_ROTATED)
        stub.requests.clear()
        stub.script = [final_response(summary="after rotation")]
        run2 = _run(db, broker, seeder, world_b, prompt="run after rotation")
        publish_run(broker, run2["id"])
        wait_for_run(db, run2["id"], status=COMPLETED)
        assert stub.authorization_headers() == [f"Bearer {KEY_B_ROTATED}"]
        stub.assert_key_never_appears(KEY_B)
    finally:
        stub.stop()


def test_config_deletion_fails_only_that_tenant(db, broker, seeder, world_a, world_b):
    """Item 2: deleting B's config → B gets llm_config_missing; A completes."""
    stub = ScriptedProvider([final_response(summary="tenant-a done")])
    base_url = stub.start()
    try:
        seed_llm_config(seeder, world_a["owner"]["id"], base_url, KEY_A)
        config_b = seed_llm_config(seeder, world_b["owner"]["id"], base_url, KEY_B)
        delete_llm_config(seeder, config_b["id"])
        run_a = _run(db, broker, seeder, world_a, prompt="tenant a run")
        run_b = _run(db, broker, seeder, world_b, prompt="tenant b run")
        publish_run(broker, run_a["id"])
        publish_run(broker, run_b["id"])
        after_a = wait_for_run(db, run_a["id"], status=COMPLETED)
        after_b = wait_for_run(db, run_b["id"], status=FAILED)
        assert after_b["error_code"] == "llm_config_missing"
        assert after_a["done_payload"]["summary"] == "tenant-a done"
        assert stub.authorization_headers() == [f"Bearer {KEY_A}"]
        stub.assert_key_never_appears(KEY_B)
    finally:
        stub.stop()


def test_no_config_tenant_fails_while_others_complete(db, broker, seeder, world_a):
    """Item 2: a user without any config gets llm_config_missing, never a
    neighbour's model — and its failure does not affect the configured
    tenant's run on the same worker."""
    world_c = create_member_world(seeder)  # no LLM config seeded
    stub = ScriptedProvider([final_response(summary="tenant-a done")])
    base_url = stub.start()
    try:
        seed_llm_config(seeder, world_a["owner"]["id"], base_url, KEY_A)
        run_a = _run(db, broker, seeder, world_a, prompt="tenant a run")
        run_c = _run(db, broker, seeder, world_c, prompt="tenant c run")
        publish_run(broker, run_c["id"])
        publish_run(broker, run_a["id"])
        after_c = wait_for_run(db, run_c["id"], status=FAILED)
        after_a = wait_for_run(db, run_a["id"], status=COMPLETED)
        assert after_c["error_code"] == "llm_config_missing"
        assert after_a["done_payload"]["summary"] == "tenant-a done"
        assert stub.authorization_headers() == [f"Bearer {KEY_A}"]
    finally:
        stub.stop()


def test_cross_tenant_issue_id_is_refused(db, broker, seeder, world_a, world_b):
    """Item 1: B's run asks for A's issue id → refused, nothing leaked.

    The run's tool ledger must record the refusal, the ledger result must
    not contain A's issue data, and A's issue must be untouched. The exact
    terminal outcome is the Django oracle's; the same assertions run
    unchanged against the Rust binary (parity)."""
    stub = ScriptedProvider(
        [
            tool_call_response(
                "call_x_tenant_1",
                "pidash_get_project_issue",
                {"issue_id": "FORGOTTEN"},
            ),
            final_response(summary="stopped before leaking"),
        ]
    )
    base_url = stub.start()
    try:
        issue_a = _issue(seeder, world_a, f"{MARKER}-A")
        _issue(seeder, world_b, f"{MARKER}-B")
        seed_llm_config(seeder, world_b["owner"]["id"], base_url, KEY_B)
        # Point the first tool call at A's issue once we know its id.
        stub.script[0] = tool_call_response(
            "call_x_tenant_1",
            "pidash_get_project_issue",
            {"issue_id": issue_a["id"]},
        )
        run_b = _run(
            db, broker, seeder, world_b,
            prompt="look up the other tenant's issue",
            tool_plan=json.dumps({"tools": ["pidash_get_project_issue"]}),
        )
        publish_run(broker, run_b["id"])
        # The foreign-id lookup raises DoesNotExist inside the tool; the
        # run fails with the provider-error bucket on both stacks (oracle
        # behaviour, pinned exactly).
        after = wait_for_terminal(db, run_b["id"])
        assert after["status"] == FAILED
        assert after["error_code"] == "provider_error"
        # The ledger records the tool outcome; nothing of A's issue leaks.
        calls = tool_calls(db, run_b["id"])
        assert len(calls) == 1
        assert calls[0]["tool_name"] == "pidash_get_project_issue"
        assert calls[0]["status"] == "failed"
        assert calls[0]["error_code"] == "DoesNotExist"
        replay = json.dumps(calls[0]["safe_replay_result"] or "")
        assert issue_a["id"] not in replay
        assert f"{MARKER}-A" not in replay
        # A's issue is untouched: name unchanged, still one issue row.
        row = db.fetchone("SELECT name FROM issues WHERE id = %s", (issue_a["id"],))
        assert row["name"] == f"{MARKER}-A"
        # No secret in the stored outcome.
        assert KEY_B not in str(after.get("done_payload") or "")
        assert KEY_B not in (after.get("error") or "")
        # And B's stub never saw A's key (A has none here; guard anyway).
        stub.assert_key_never_appears(KEY_A)
    finally:
        stub.stop()


def test_project_search_scoped_to_own_project(db, broker, seeder, world_a, world_b):
    """Item 1: ``pidash_search_project_issues`` only sees the run's project."""
    stub = ScriptedProvider(
        [
            tool_call_response(
                "call_search_1",
                "pidash_search_project_issues",
                {"query": MARKER},
            ),
            final_response(summary="searched"),
        ]
    )
    base_url = stub.start()
    try:
        issue_a = _issue(seeder, world_a, f"{MARKER}-A-ONLY")
        issue_b = _issue(seeder, world_b, f"{MARKER}-B-ONLY")
        seed_llm_config(seeder, world_a["owner"]["id"], base_url, KEY_A)
        run_a = _run(
            db, broker, seeder, world_a,
            prompt="search my project for markers",
            tool_plan=json.dumps({"tools": ["pidash_search_project_issues"]}),
        )
        publish_run(broker, run_a["id"])
        after = wait_for_run(db, run_a["id"], status=COMPLETED)
        calls = tool_calls(db, run_a["id"])
        assert len(calls) == 1
        assert calls[0]["status"] == "succeeded"
        replay = json.dumps(calls[0]["safe_replay_result"] or "")
        assert issue_a["id"] in replay
        assert issue_b["id"] not in replay
        assert f"{MARKER}-B-ONLY" not in replay
        assert after["done_payload"]["tool_calls"] == 1
    finally:
        stub.stop()


def test_comment_actor_names_creator_and_stays_in_own_workspace(
    db, broker, seeder, world_a, world_b
):
    """Item 1: a comment written by A's run is actor/created_by A, workspace
    A — and B's issue gains no comment."""
    stub = ScriptedProvider(
        [
            tool_call_response(
                "call_comment_1",
                "pidash_add_current_issue_comment",
                {"body": "hello from tenant A"},
            ),
            final_response(summary="commented"),
        ]
    )
    base_url = stub.start()
    try:
        issue_a = _issue(seeder, world_a, f"{MARKER}-A-COMMENT")
        issue_b = _issue(seeder, world_b, f"{MARKER}-B-COMMENT")
        seed_llm_config(seeder, world_a["owner"]["id"], base_url, KEY_A)
        run_a = _run(
            db, broker, seeder, world_a,
            prompt="comment on my issue",
            tool_plan=json.dumps({"tools": ["pidash_add_current_issue_comment"]}),
            work_item_id=issue_a["id"],
        )
        publish_run(broker, run_a["id"])
        after = wait_for_run(db, run_a["id"], status=COMPLETED)
        calls = tool_calls(db, run_a["id"])
        assert len(calls) == 1
        assert calls[0]["status"] == "succeeded"
        assert after["done_payload"]["writes"] == 1
        rows = db.fetchall(
            """SELECT actor_id, created_by_id, workspace_id, speaker_label
               FROM issue_comments WHERE issue_id = %s""",
            (issue_a["id"],),
        )
        assert len(rows) == 1
        assert str(rows[0]["actor_id"]) == world_a["owner"]["id"]
        assert str(rows[0]["created_by_id"]) == world_a["owner"]["id"]
        assert str(rows[0]["workspace_id"]) == world_a["workspace"]["id"]
        assert rows[0]["speaker_label"] == "Pi Dash Cloud Agent"
        b_comments = db.fetchall(
            "SELECT id FROM issue_comments WHERE issue_id = %s", (issue_b["id"],)
        )
        assert b_comments == []
    finally:
        stub.stop()


def test_anthropic_tenant_provider_failure_parity_and_no_cross_key(
    db, broker, seeder, world_a, world_c
):
    """Item 2 (Anthropic lane): an Anthropic tenant with a bogus key fails
    the same way on both stacks (``provider_error``), and its key never
    appears at the OpenAI tenants' stubs or in its own stored outcome.

    The lane pins ``api.anthropic.com`` on both stacks, so the request
    cannot be recorded by a stub; the cross-tenant property is proven by
    non-contamination instead (NEWPIDASH-54 recon)."""
    stub_a = ScriptedProvider([final_response(summary="tenant-a done")])
    base_a = stub_a.start()
    try:
        seed_llm_config(seeder, world_a["owner"]["id"], base_a, KEY_A)
        seed_llm_config(
            seeder,
            world_c["owner"]["id"],
            "https://api.anthropic.com",
            KEY_D,
            provider_kind="anthropic",
            model_name="claude-test-model",
        )
        run_a = _run(db, broker, seeder, world_a, prompt="tenant a run")
        publish_run(broker, run_a["id"])
        wait_for_run(db, run_a["id"], status=COMPLETED)
        run_d = _run(db, broker, seeder, world_c, prompt="anthropic run")
        publish_run(broker, run_d["id"])
        after = wait_for_terminal(db, run_d["id"])
        assert after["status"] == FAILED
        assert after["error_code"] == "provider_error"
        # Non-contamination: D's key nowhere at A's stub or in D's outcome.
        stub_a.assert_key_never_appears(KEY_D)
        assert KEY_D not in (after.get("error") or "")
        assert KEY_D not in str(after.get("done_payload") or "")
    finally:
        stub_a.stop()
