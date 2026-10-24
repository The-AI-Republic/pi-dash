// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

/**
 * Resource limits for a full test run.
 *
 * Vitest's worker-to-main RPC gives the main process 60 seconds to answer an
 * `onTaskUpdate` call, and that deadline is hardcoded in birpc: Vitest 3
 * passes no `timeout`, so it cannot be raised from configuration. On a large
 * run the main process also carries the reporters and the v8 coverage work,
 * and on a shared host it can miss the deadline while every assertion passes.
 * The run then exits 1 with an infrastructure error instead of a test result.
 *
 * Bounding the pool is the available lever. Vitest's defaults here are three
 * forks (`cpus - 1` on a four-vCPU host) and a coverage processing
 * concurrency of `min(20, cpus)` (four). The caps below leave the main
 * process room to answer the RPC without changing what any test asserts.
 */
export const FULL_RUN_MAX_WORKERS = 2;

export const FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY = 2;
