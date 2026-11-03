-- D13-F5 (part 5): Runner.revoke() cascade SQL. models.py:597-651.
-- Method: hand-composed from cited ORM calls. Tx boundary: everything
-- between BEGIN/COMMIT below is ONE transaction.atomic(); the on_commit
-- hooks (handoff/drain/stream-cleanup scheduling) run post-commit.

-- Pre-tx (pure Python): reason validation — unknown reason -> logger.warning;
-- len > 32 -> logger.warning + truncate stored_reason = reason[:32]
-- (models.py:575-590). revoked_at already set -> RETURN (no-op, no SQL).

-- BEGIN;
-- S1. Mark revoked (:599-603). NOTE: QuerySet.update -> updated_at UNCHANGED.
UPDATE "runner"
SET "status" = 'revoked', "revoked_at" = $now, "revoked_reason" = $reason
WHERE "id" = $runner;

-- S2. Revoke active sessions (:618-620). Next daemon poll sees the row gone
-- and reacts 409 session_evicted; the 409 body echoes revoked_reason.
UPDATE "runner_session"
SET "revoked_at" = $now, "revoked_reason" = $reason
WHERE "runner_id" = $runner AND "revoked_at" IS NULL;

-- S3. Lock + list non-terminal runs (:622-626):
SELECT "id", "pod_id" FROM "agent_run"
WHERE "runner_id" = $runner AND "status" IN
  ('queued','assigned','waiting_for_worktree','running','cancel_requested',
   'awaiting_approval','awaiting_reauth','paused_awaiting_input')
FOR UPDATE;
-- Per run (:631-640): finalize_agent_run(run_id, 'cancelled',
--   updates={error: 'runner revoked', error_code: 'runner_revoked',
--            cancel_reason: stored_reason}, expected_runner_id=runner)
-- (finalize SQL owned by D-14; pinned in wire_pins.json — first-writer-wins
-- terminal transition + terminal AgentRunEvent for cloud runs).
-- affected_pod_ids collects DISTINCT pod_ids (non-null).

-- S4. Unpin QUEUED follow-ups (:644-651):
SELECT "pod_id" FROM "agent_run"
WHERE "pinned_runner_id" = $runner AND "status" = 'queued';
UPDATE "agent_run" SET "pinned_runner_id" = NULL
WHERE "pinned_runner_id" = $runner AND "status" = 'queued';
-- (The SELECT runs first; the UPDATE only when the list is non-empty.
-- affected_pod_ids gains these pod_ids too.)
-- COMMIT.

-- Post-commit hooks (transaction.on_commit, models.py:673-685):
--   per affected run_id: complete_project_move_handoff(run_id) — exceptions
--     swallowed after logger.exception (revocation already committed).
--   per affected pod_id: drain_pod_by_id(pod_id).
--   always: schedule_stream_cleanup_for_runner(runner_id) — Redis ZSET add
--     (grace = 2 * ACCESS_TOKEN_TTL_SECS), NOT SQL.
-- Hook ORDER: handoffs registered first (single on_commit closure over ALL
-- run ids), then one drain closure per pod, then stream cleanup. on_commit
-- hooks run in registration order.
-- When zero affected runs: NO handoff hook; drains only for pinned pod ids;
-- stream cleanup ALWAYS scheduled.
