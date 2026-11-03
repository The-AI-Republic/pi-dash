"""Oracle: ``cloud_agent.run_agent_run`` (PIDASHCONV-22, D-11).

Source: ``apps/api/pi_dash/cloud_agent/tasks.py`` ``run_cloud_agent`` —
``acks_late=False``, ``max_retries=0``, soft/hard time limits. Each test
seeds one ``agent_run`` row (the "before"), publishes the job in Celery wire
format, lets the live worker execute it, and asserts the row "after".

Covered branches (in source order):

- unknown id → ``"ignored"``: no row appears, nothing else moves.
- non-QUEUED row → ``"ignored"``: row and events untouched.
- ``CLOUD_AGENT_ENABLED`` off → ``"disabled"``: NOT covered here — the
  suite runs under the declared ``CLOUD_AGENT_ENABLED=true`` regime (see
  ``README.md``); the kill switch is a global setting with no per-run
  override, so no black-box test can flip it. Pinned by unit tests
  (``test_master_switch_is_rechecked_after_model_boundary``).
- bot/inactive/non-member creator → ``"unauthorized"`` (``FAILED``,
  ``actor_no_longer_authorized``).
- member creator without LLM config → ``"llm_config_missing"``.
- oversized prompt → ``"prompt_too_large"`` (limit
  ``CLOUD_AGENT_MAX_PROMPT_BYTES``, default 262144).
- ``cancel_requested_at`` set → ``"cancelled"`` (``CANCELLED``).
- redelivery (same job twice) collapses to one effect: exactly one
  ``run_started`` + one ``terminal`` event — the observable half of
  ``acks_late=False`` (at-most-once) parity.
- a failed execution is never requeued — the observable half of
  ``max_retries=0`` parity.

The full-LLM ``completed``/``blocked``/``refused`` path needs provider
credentials no black-box suite can provision; it is pinned by unit tests
(``test_task_completes_structured_result_and_duplicate_message_is_ignored``,
``test_blocked_model_outcome_uses_blocked_lifecycle_state``,
``test_provider_refusal_has_distinct_terminal_state``).
"""

import uuid

import pytest

from .conftest import (
    CANCELLED,
    CLOUD_AGENT,
    FAILED,
    QUEUED,
    RUNNING,
    create_agent_run,
    create_llm_config,
    get_run,
    run_events,
    wait_for_queue_drain,
    wait_for_run,
)

pytestmark = pytest.mark.contract

RUN_TASK = "cloud_agent.run_agent_run"


def _publish(broker, run_id):
    baseline = broker.queue_length()
    broker.publish(RUN_TASK, [run_id])
    return baseline


def test_unknown_run_id_is_ignored(db, broker):
    """Publishing an id no run has: the worker claims nothing, creates no
    row, and the queue drains."""
    missing = str(uuid.uuid4())
    baseline = _publish(broker, missing)
    wait_for_queue_drain(broker, baseline=baseline)
    assert get_run(db, missing) is None


def test_non_queued_run_is_ignored(db, broker, seeder, world):
    """A RUNNING row published again: ``_claim`` finds no QUEUED row, the
    worker returns ``ignored`` and touches neither the row nor events."""
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        status=RUNNING,
    )
    baseline = _publish(broker, run["id"])
    wait_for_queue_drain(broker, baseline=baseline)
    after = get_run(db, run["id"])
    assert after["status"] == RUNNING
    assert after["error_code"] == ""
    assert run_events(db, run["id"]) == []


def test_bot_creator_is_unauthorized(db, broker, seeder, world):
    """Bot creator: claimed (QUEUED → RUNNING → FAILED), ``run_started``
    event, then ``actor_no_longer_authorized`` with no LLM call."""
    seeder.db.execute("UPDATE users SET is_bot = true WHERE id = %s", (world["owner"]["id"],))
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"]
    )
    baseline = _publish(broker, run["id"])
    after = wait_for_run(db, run["id"], status=FAILED)
    assert after["error_code"] == "actor_no_longer_authorized"
    assert after["ended_at"] is not None
    kinds = [row["kind"] for row in run_events(db, run["id"])]
    assert kinds[0] == "run_started"
    assert "terminal" in kinds
    wait_for_queue_drain(broker, baseline=baseline)


def test_member_without_llm_config_fails(db, broker, seeder, world):
    """Full member + project role but no LLM config: ``llm_config_missing``.
    Seeded users never have an assistant LLM config, so this is deterministic."""
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"]
    )
    _publish(broker, run["id"])
    after = wait_for_run(db, run["id"], status=FAILED)
    assert after["error_code"] == "llm_config_missing"


def test_oversized_prompt_fails(db, broker, seeder, world):
    """A prompt past ``CLOUD_AGENT_MAX_PROMPT_BYTES`` (default 262144):
    ``prompt_too_large`` before any model call. The creator carries an LLM
    marker so the earlier LLM-config gate passes first (source order)."""
    create_llm_config(seeder, world["owner"]["id"])
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        prompt="p" * 300_000,
    )
    _publish(broker, run["id"])
    after = wait_for_run(db, run["id"], status=FAILED)
    assert after["error_code"] == "prompt_too_large"


def test_cancel_requested_run_is_cancelled(db, broker, seeder, world):
    """``cancel_requested_at`` set before execution: ``CANCELLED`` with the
    request reason preserved. The creator carries an LLM marker so the
    earlier gates pass first (source order)."""
    create_llm_config(seeder, world["owner"]["id"])
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        cancel_reason="user changed their mind",
    )
    _publish(broker, run["id"])
    after = wait_for_run(db, run["id"], status=CANCELLED)
    assert after["error_code"] == "cancelled"
    assert after["error"] == "user changed their mind"


def test_redelivery_collapses_to_single_effect(db, broker, seeder, world):
    """No double execution under redelivery: the same job published twice
    claims once — exactly one ``run_started`` and one ``terminal`` event,
    first-writer-wins on the terminal state."""
    seeder.db.execute("UPDATE users SET is_bot = true WHERE id = %s", (world["owner"]["id"],))
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"]
    )
    baseline = broker.queue_length()
    broker.publish(RUN_TASK, [run["id"]])
    broker.publish(RUN_TASK, [run["id"]])
    after = wait_for_run(db, run["id"], status=FAILED)
    assert after["error_code"] == "actor_no_longer_authorized"
    wait_for_queue_drain(broker, baseline=baseline)
    kinds = [row["kind"] for row in run_events(db, run["id"])]
    assert kinds.count("run_started") == 1
    assert kinds.count("terminal") == 1


def test_failed_execution_is_not_retried(db, broker, seeder, world):
    """``max_retries=0`` parity: after the terminal state lands and the
    queue goes quiet, no retry appears — status and events stay put."""
    seeder.db.execute("UPDATE users SET is_bot = true WHERE id = %s", (world["owner"]["id"],))
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"]
    )
    baseline = _publish(broker, run["id"])
    wait_for_run(db, run["id"], status=FAILED)
    wait_for_queue_drain(broker, baseline=baseline)
    # Quiescence: nothing the worker does afterwards may move this run.
    wait_for_queue_drain(broker, baseline=baseline)
    after = get_run(db, run["id"])
    assert after["status"] == FAILED
    assert after["error_code"] == "actor_no_longer_authorized"
    assert len(run_events(db, run["id"])) == 2
