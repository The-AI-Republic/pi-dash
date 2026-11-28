-- P1 clean-up (spec section 5.1, item 1.5). WRITES DATA. Operator-run only.
--
-- Use only on an environment where p1_data_check.sql returned rows, and
-- only before P1 deploys there. Never run it from CI or against a database
-- you have not just checked.
--
-- Step 1 is not SQL. Cancel each non-terminal cloud agent_run (check 1 of
-- p1_data_check.sql) through the existing cancel endpoint, so the run's
-- cloud cancel arm (still present until P3 step C) finalizes it and
-- releases its capacity:
--
--   POST /api/runners/runs/<run_id>/cancel/
--
-- Then rerun check 1; it must return zero rows before you go on.
--
-- Steps 2 and 3 below run in one transaction that ends in ROLLBACK, so a
-- first run only shows the row counts. Read the counts, compare them with
-- p1_data_check.sql, then change the last line to COMMIT and run again.
-- Afterwards rerun p1_data_check.sql until every check returns zero rows.

BEGIN;

-- Step 2: delete the queued cloud jobs (check 4). This includes
-- dead-letter rows (status 'failed'), which check 4 also counts.
DELETE FROM rust_job_queue
WHERE queue = 'cloud_agent' OR task LIKE 'cloud_agent.%';

-- Step 3: move the project and issue defaults to the local runner
-- (checks 2 and 3).
UPDATE projects
SET default_agent_executor = 'local_runner'
WHERE default_agent_executor = 'cloud_agent';

UPDATE issues
SET agent_executor = 'local_runner'
WHERE agent_executor = 'cloud_agent';

ROLLBACK;
