/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Pluggable transport for runner-detail fetches.
 *
 * Most callers (cloud web app, self-hosted, browser tests) get the
 * HTTP default — built on `RunnerService.getDetail` — which hits the
 * same `GET /api/runners/<id>/` endpoint as before. Nothing in their
 * behavior changes.
 *
 * The reason this seam exists: a Tauri desktop wrapper that bundles
 * the OSS web app and runs alongside a same-machine `pidash` daemon
 * can answer the same query from local IPC, skipping the cloud
 * round-trip. The wrapper registers an override at app startup via
 * `setRunnerDetailFetcher`; the UI code keeps calling
 * `getRunnerDetail` and is agnostic about which path served the
 * request.
 *
 * Why a module-level registry rather than DI: the OSS UI is loaded
 * by Vite (browser-only — apps/web has `ssr: false`), and there is
 * no shared root component a native shell could decorate with a
 * context provider before the React tree mounts. A module-level
 * `set...` call from the shell's init script is the lowest-friction
 * way to flip the transport before the first UI mount, and it does
 * not require the OSS web app to grow an awareness of "what
 * environment am I in".
 *
 * Scope: this module is intended for browser-side consumption.
 * `apps/live` (Node) does not import `@pi-dash/services`; if a
 * server-side consumer is added later, that consumer must not rely
 * on `setRunnerDetailFetcher` for per-request behavior — module
 * state is process-global on Node.
 *
 * Adding more seams: each pluggable surface gets its own
 * `<Verb>Fetcher` type + `set<Verb>Fetcher` setter + `<verb>` caller
 * in this file. Resist the temptation to generalize to one mega-
 * "transport" object — per-method override + clean function
 * signatures is easier to reason about than a registry of opaque
 * handlers keyed by string.
 */

import type { IRunner } from "@pi-dash/types";

import { RunnerService } from "./runner.service";

export type RunnerDetailFetcher = (runnerId: string) => Promise<IRunner>;

/**
 * Default fetcher — `GET /api/runners/<id>/` via the cloud API.
 *
 * Constructed eagerly at module load. `RunnerService`'s constructor
 * reads `API_BASE_URL` and runs `applyAxiosSetups`; doing this at
 * module load means every axios interceptor that's registered up
 * through this module's evaluation is captured. Interceptors that
 * register LATER will not be picked up by this singleton — register
 * them before `@pi-dash/services` is first imported.
 *
 * Kept module-private to avoid a footgun where consumers import this
 * fetcher directly and bypass any registered override. Override
 * authors who want to compose with the previously-active fetcher
 * (e.g. to wrap the HTTP path with logging) should snapshot it via
 * `getRunnerDetailFetcher()` BEFORE calling `setRunnerDetailFetcher`.
 */
const httpService = new RunnerService();
const httpRunnerDetailFetcher: RunnerDetailFetcher = (runnerId) => {
  return httpService.getDetail(runnerId);
};

let activeFetcher: RunnerDetailFetcher = httpRunnerDetailFetcher;

/**
 * Fetch a runner's full detail (status, observability snapshot, pod,
 * connection, …). UI code should prefer this over instantiating
 * `RunnerService` directly so non-HTTP environments can override
 * transport without the call site changing.
 */
export function getRunnerDetail(runnerId: string): Promise<IRunner> {
  return activeFetcher(runnerId);
}

/**
 * Read the currently-active runner-detail fetcher. Intended for two
 * narrow use cases:
 *
 *  1. Override authors composing with the previously-active fetcher
 *     (snapshot via this getter, then `setRunnerDetailFetcher` to a
 *     wrapper that calls the snapshot).
 *  2. Tests asserting "is my override actually installed?" without
 *     having to call `getRunnerDetail` and observe.
 *
 * Normal UI code should NOT call this — call `getRunnerDetail`.
 */
export function getRunnerDetailFetcher(): RunnerDetailFetcher {
  return activeFetcher;
}

/**
 * Override the active runner-detail fetcher. Pass `null` or
 * `undefined` to reset to the HTTP default — convenient for test
 * `afterEach` teardown and for optional-fetcher config patterns.
 *
 * Intended for use by environment-specific bootstrap code (Tauri
 * desktop wrapper init, test harnesses). The OSS / cloud / self-
 * hosted web app never calls this.
 *
 * Register before the first UI mount. SWR (and similar) caches by
 * key only — swapping transport after a fetch has populated the
 * cache will not invalidate that entry. If a transport swap mid-
 * session is needed, follow it with an explicit `mutate(...)` for
 * the affected keys.
 *
 * Composition: an override can wrap the previously active fetcher
 * by snapshotting it via `getRunnerDetailFetcher()` before calling
 * `setRunnerDetailFetcher`, e.g.
 *
 * ```ts
 * import {
 *   getRunnerDetail,
 *   getRunnerDetailFetcher,
 *   setRunnerDetailFetcher,
 * } from "@pi-dash/services";
 *
 * const next = getRunnerDetailFetcher();
 * setRunnerDetailFetcher(async (id) => {
 *   const result = await next(id);
 *   logRunnerDetail(id, result);
 *   return result;
 * });
 * ```
 *
 * Footguns:
 *  - In-flight requests issued by a previously-active fetcher are
 *    NOT aborted by reset. Their Promises still resolve into the
 *    consumer's cache. Use `AbortController` inside your fetcher if
 *    you need cancellable swaps.
 *  - Class-method references lose `this` when passed bare. Use
 *    `myClient.getDetail.bind(myClient)` or an arrow wrapper.
 *  - For consistency with the HTTP default's error shape, overrides
 *    should reject with response-data-shaped errors so downstream
 *    `onError` handlers see the same field names. `RunnerService`'s
 *    `getDetail` rethrows `e?.response?.data`; mirror that contract.
 *
 * Vite HMR resets module state on hot reload. If you register an
 * override during dev and a save causes this module to be re-
 * evaluated, the override is lost. The HMR hook below preserves the
 * active fetcher across reloads.
 */
export function setRunnerDetailFetcher(fetcher: RunnerDetailFetcher | null | undefined): void {
  activeFetcher = fetcher ?? httpRunnerDetailFetcher;
}

// Vite HMR: preserve the active fetcher across hot reloads in dev so
// a desktop shell that registered an override at boot doesn't silently
// lose it when a save triggers re-evaluation of this module. The
// `import.meta.hot` API is a Vite-only no-op elsewhere; tsdown builds
// it out cleanly.
//
// Minimal local interface for Vite's HMR API — keeps the seam from
// adding a dev-only dependency.
interface ViteHotContext {
  readonly data: { activeFetcher?: RunnerDetailFetcher };
  accept(cb: (mod: unknown) => void): void;
}

interface ImportMetaWithHot extends ImportMeta {
  hot?: ViteHotContext;
}

const _meta = import.meta as ImportMetaWithHot;
if (_meta.hot) {
  // Stash the current active fetcher on hot.data so the next module
  // version can pick it up.
  _meta.hot.data.activeFetcher = activeFetcher;
  _meta.hot.accept((mod) => {
    const previous = _meta.hot?.data.activeFetcher;
    if (mod && previous) {
      // The new module instance has its own `activeFetcher` binding;
      // ask it to re-install whatever was active before.
      (
        mod as {
          setRunnerDetailFetcher: typeof setRunnerDetailFetcher;
        }
      ).setRunnerDetailFetcher(previous);
    }
  });
}
