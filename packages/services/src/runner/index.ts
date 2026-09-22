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
// Same policy as runner-transport above: a named re-export keeps the
// chat-transport seam's public surface explicit. The cloud default
// transport is intentionally module-private — overrides go through
// `setChatTransport`, and consumers read the active transport via
// `getChatTransport`.
export {
  getChatTransport,
  setChatTransport,
  type ChatEventErrorHandler,
  type ChatEventHandler,
  type ChatEventUnsubscribe,
  type ChatTransport,
} from "./chat-transport";
