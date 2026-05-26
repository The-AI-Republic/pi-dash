/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Pluggable transport for runner-detail fetches.
 *
 * Most callers (cloud web app, self-hosted, browser tests) get the
 * HTTP default — `httpRunnerDetailFetcher` — which hits the same
 * `GET /api/runners/<id>/` endpoint `RunnerService.getDetail` has
 * always used. Nothing in their behavior changes.
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
 * by both Vite (browser) and any future native shell (Tauri), and
 * there is no shared root component the latter could decorate with
 * a context provider before the former bootstraps. A module-level
 * `set...` call from the shell's init script is the lowest-friction
 * way to flip the transport before the first UI mount, and it does
 * not require the OSS web app to grow an awareness of "what
 * environment am I in".
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
 * Default fetcher: `GET /api/runners/<id>/` via the cloud API. The
 * service instance is lazy-created on first call so module load does
 * not perform side effects (e.g., resolving `API_BASE_URL` before the
 * surrounding bundle is fully wired up).
 */
let cachedHttpService: RunnerService | null = null;
export const httpRunnerDetailFetcher: RunnerDetailFetcher = (runnerId) => {
  if (cachedHttpService === null) {
    cachedHttpService = new RunnerService();
  }
  return cachedHttpService.getDetail(runnerId);
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
 * Override the active runner-detail fetcher. Pass `null` to reset to
 * the HTTP default — handy for tests that install a mock and need to
 * tear it down cleanly.
 *
 * Intended for use by environment-specific bootstrap code (Tauri
 * desktop wrapper init, test harnesses). The OSS / cloud / self-
 * hosted web app never calls this.
 */
export function setRunnerDetailFetcher(fetcher: RunnerDetailFetcher | null): void {
  activeFetcher = fetcher ?? httpRunnerDetailFetcher;
}
