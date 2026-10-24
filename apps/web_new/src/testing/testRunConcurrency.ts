// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

/**
 * Resource limits for a test run.
 *
 * Vitest's worker-to-main RPC gives the main process 60 seconds to answer an
 * `onTaskUpdate` call. A full run on a loaded host can keep the main process
 * busy long enough to miss that deadline while every assertion passes, which
 * turns the run's exit code into an infrastructure failure. Fork workers keep
 * the test isolates out of the main process, and the caps keep both test
 * execution and v8 coverage processing within the host's capacity instead of
 * queueing work the reporter then has to wait for.
 */
export const FULL_RUN_MAX_WORKERS = 2;

export const FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY = 2;
