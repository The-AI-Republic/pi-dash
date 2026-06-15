/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Recovery for stale-deploy chunk errors.
 *
 * A new release swaps the bundle's content-hashed chunks, but a user may still
 * hold a pre-deploy index.html (cached tab, kept-open session). A lazy() import
 * then requests a chunk the new build no longer serves, which throws. Reloading
 * once pulls the current index + assets and resolves it.
 */

// A stale-deploy failure. Vite/Rollup and each browser word this differently,
// and a CSS preload failure has its own wording — match them all.
export function isChunkLoadError(error: unknown): boolean {
  const err = error as { name?: string; message?: string } | null;
  const message = err?.message ?? "";
  return (
    err?.name === "ChunkLoadError" ||
    /failed to fetch dynamically imported module/i.test(message) ||
    /error loading dynamically imported module/i.test(message) ||
    /importing a module script failed/i.test(message) ||
    /unable to preload css/i.test(message)
  );
}

// Only reload once per short window so a genuinely-missing asset (build/CDN
// gap, not just stale) falls through instead of looping.
const RELOAD_GUARD_KEY = "pi-dash:chunk-reload-at";
const RELOAD_GUARD_MS = 10_000;

/**
 * Reload once to recover from a stale-deploy chunk error.
 *
 * Returns `true` if a reload was triggered (the page is about to navigate away),
 * or `false` if we reloaded too recently — the caller should then surface the
 * real error instead of looping.
 */
export function reloadOnceForStaleChunk(): boolean {
  try {
    const last = Number(sessionStorage.getItem(RELOAD_GUARD_KEY) ?? 0);
    if (Date.now() - last <= RELOAD_GUARD_MS) return false;
    sessionStorage.setItem(RELOAD_GUARD_KEY, String(Date.now()));
  } catch {
    // sessionStorage unavailable (private mode / disabled). Reload anyway: the
    // common case (stale index, working storage) is fully guarded above; this
    // path only loops for the rare combination of storage-off AND a truly
    // missing asset, which one reload still resolves in the normal sub-case.
  }
  window.location.reload();
  return true;
}
