// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

/**
 * Per-test budget for a full `web_new` suite run.
 *
 * A full run emits ~7 000 tests. On a loaded host the tail of that
 * distribution is dominated by queueing, not by the test's own work: a run
 * measured at load average 163 (8 vCPUs) had real per-test durations of
 * p50 580 ms, p95 2.8 s, p99 4.8 s and p99.9 8.4 s, with a handful of
 * lazy-chunk suites reaching 16.8 s. The old 5 s default cut straight through
 * that tail, so suites that pass in isolation failed with
 * `Test timed out in 5000ms` whenever other slices were running on the same
 * machine.
 *
 * The budget is a load allowance, not a hang allowance: staying under a
 * minute means a genuinely stuck test still fails promptly. It relaxes no
 * assertion; only the wall-clock ceiling moves.
 */
export const FULL_RUN_TEST_TIMEOUT_MS = 30_000;

/** Slowest per-test duration (p99.9) observed in the load-163 full run. */
export const FULL_RUN_OBSERVED_P999_MS = 8_400;

/** One-minute load average on the 8-vCPU host during that measurement. */
export const FULL_RUN_OBSERVED_LOAD = 163;

/**
 * Wall-clock allowance for Testing Library's async queries (`findBy*`,
 * `waitFor`).
 *
 * Testing Library polls for a second before giving up, which is far tighter
 * than the per-test budget above. A suite that waits on a lazy chunk (the
 * token page, a settings section) or on a stubbed read can spend more than
 * that second queueing on a loaded host and fail with "Unable to find ..."
 * even though the element does arrive — the failure NEWFRONT-789 saw on the
 * base head. The wait tracks the per-test budget instead, so the two
 * ceilings agree; it changes only how long a query polls, never what it
 * asserts, and it cannot outlive the test that owns it.
 */
export const ASYNC_QUERY_TIMEOUT_MS = FULL_RUN_TEST_TIMEOUT_MS;
