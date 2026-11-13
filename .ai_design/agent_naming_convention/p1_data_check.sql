-- P1 data check (spec section 5.1, item 1.4) — READ-ONLY.
--
-- Run on each environment (test, then production) BEFORE P1 deploys.
-- P1 deploys to an environment only after this returns zero rows there.
-- Rerun immediately before P3 deploys for the same reason (5.1 ordering).
-- Do NOT run this against a shared database from CI. Manual, operator-run.
--
-- 1.5 clean-up (only where 1.4 finds rows, on that environment, before P1
-- deploys):
--   1. Cancel each non-terminal cloud agent_run through the existing cancel
--      endpoint (its cloud arms still exist until P3 step C).
--   2. Delete the rust_job_queue rows in check 4.
--   3. Set the project/issue rows in checks 2-3 to 'local_runner'.
--   Rerun this file until it returns zero rows.
--
-- Terminal statuses (apps/api/pi_dash/runner/services/agent_run_finalization.py
-- TERMINAL_STATUSES): completed, failed, cancelled, blocked, refused.
-- Non-terminal = everything else: queued, assigned, waiting_for_worktree,
-- running, cancel_requested, awaiting_approval, awaiting_reauth,
-- paused_awaiting_input. A BLOCKED row is terminal for finalize; the check
-- below excludes the five terminal values, so BLOCKED is treated as terminal.

-- Check 1: non-terminal cloud runs (must be zero before deploy).
SELECT id, status, executor_kind, created_at
FROM agent_run
WHERE executor_kind = 'cloud_agent'
  AND status NOT IN ('completed', 'failed', 'cancelled', 'blocked', 'refused')
ORDER BY created_at
LIMIT 100;

SELECT COUNT(*) AS non_terminal_cloud_runs
FROM agent_run
WHERE executor_kind = 'cloud_agent'
  AND status NOT IN ('completed', 'failed', 'cancelled', 'blocked', 'refused');

-- Check 2: projects defaulting to cloud (must be zero before deploy).
SELECT id, workspace_id, name, default_agent_executor
FROM projects
WHERE default_agent_executor = 'cloud_agent'
ORDER BY id
LIMIT 100;

SELECT COUNT(*) AS cloud_default_projects
FROM projects
WHERE default_agent_executor = 'cloud_agent';

-- Check 3: issues set to cloud (must be zero before deploy).
SELECT id, workspace_id, project_id, agent_executor
FROM issues
WHERE agent_executor = 'cloud_agent'
ORDER BY id
LIMIT 100;

SELECT COUNT(*) AS cloud_issues
FROM issues
WHERE agent_executor = 'cloud_agent';

-- Check 4: queued cloud jobs (must be zero before deploy; P1 leaves the
-- handlers registered until P3, so a leftover row would still execute).
SELECT id, queue, task, status, created_at
FROM rust_job_queue
WHERE queue = 'cloud_agent' OR task LIKE 'cloud_agent.%'
ORDER BY id
LIMIT 100;

SELECT COUNT(*) AS cloud_queued_jobs
FROM rust_job_queue
WHERE queue = 'cloud_agent' OR task LIKE 'cloud_agent.%';
