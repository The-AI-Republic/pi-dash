/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export * from "./pod.service";
export * from "./runner.service";
// Named re-export (not `export *`) so future additions to
// runner-transport.ts (per its own "add more seams here" policy) do
// not silently widen the @pi-dash/services public surface. The HTTP
// default fetcher is intentionally module-private — override authors
// who want to compose with the previously-active fetcher should use
// `getRunnerDetailFetcher()` instead.
export {
  getRunnerDetail,
  getRunnerDetailFetcher,
  setRunnerDetailFetcher,
  type RunnerDetailFetcher,
} from "./runner-transport";
