// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

/**
 * Resource limit for coverage runs.
 *
 * Vitest's worker-to-main RPC gives the main process 60 seconds to answer an
 * `onTaskUpdate` call, and that deadline is hardcoded in birpc: Vitest 3
 * passes no `timeout`, so it cannot be raised from configuration. On a large
 * run the main process also carries the reporters and the v8 coverage work,
 * and a saturated main process misses the deadline after every test has
 * passed — the run then exits 1 with an infrastructure error instead of a
 * test result.
 *
 * The coverage lever is the one that costs nothing elsewhere. v8 remapping
 * runs inside the main process with a default concurrency of
 * `min(20, cpus)` (four here); capping it keeps the main process responsive
 * without touching how many test workers run. Capping the worker pool was
 * measured at roughly half the throughput on a loaded host — a cost paid by
 * every run, not just coverage runs — so the pool keeps its default.
 */
export const FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY = 2;
