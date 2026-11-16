// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop sign-in rules (AUTH-022, AUTH-023, from parity/specs/auth.md):
// the failure landing reads ?error= / ?error_description= above the card,
// and the unavailable card resolves its browser destination from the baked
// web base URL with the API origin as the fallback.

import { asString, safeNextPath } from "./entrySearch.js";

/** The desktop entry search: failure landing plus a pending destination. */
export interface DesktopEntrySearch {
  error?: string | undefined;
  errorDescription?: string | undefined;
  nextPath?: string | undefined;
}

/** Lenient parse: unknown or invalid params fall back to absent. */
export function parseDesktopEntrySearch(search: Record<string, unknown>): DesktopEntrySearch {
  const error = asString(search["error"]);
  const errorDescription = asString(search["error_description"]);
  const nextPath = safeNextPath(search["next_path"]) ?? safeNextPath(search["next"]);
  return {
    ...(error === undefined ? {} : { error }),
    ...(errorDescription === undefined ? {} : { errorDescription }),
    ...(nextPath === undefined ? {} : { nextPath }),
  };
}

/** The failure alert for a provider-side or aborted desktop flow. */
export interface DesktopSignInFailure {
  heading: string;
  body: string;
}

const DESKTOP_FAILURE_HEADING = "Could not finish signing in";

/**
 * The alert above the desktop card: the provider description when the
 * callback carried one, else a retry line naming the error. Null without
 * ?error= (a plain visit shows the card alone).
 */
export function describeDesktopSignInFailure(search: DesktopEntrySearch): DesktopSignInFailure | null {
  if (search.error === undefined) return null;
  return {
    heading: DESKTOP_FAILURE_HEADING,
    body: search.errorDescription ?? `The sign-in attempt stopped (${search.error}). Try again.`,
  };
}

/** The baked web base URL out of a build define; empty builds carry none. */
export function parseBakedWebBaseUrl(raw: unknown): string | undefined {
  return typeof raw === "string" && raw.length > 0 ? raw : undefined;
}

declare const __PIDASH_WEB_BASE_URL__: string | undefined;

/** The baked web base URL for this bundle, or undefined when unset. */
export function bakedWebBaseUrl(): string | undefined {
  return typeof __PIDASH_WEB_BASE_URL__ === "string" ? parseBakedWebBaseUrl(__PIDASH_WEB_BASE_URL__) : undefined;
}

/** Plain same-tab navigation for when no native bridge answers. */
export function fallbackToPlainNavigation(url: string): void {
  globalThis.window.location.assign(url);
}

/**
 * Where the unavailable card sends the browser: the baked web base URL
 * first, else the API origin (which serves the web app in a default
 * deployment). An unparseable API base passes through verbatim.
 */
export function resolveBrowserAppUrl(bakedWebBaseUrl: string | undefined, apiBaseUrl: string): string {
  if (bakedWebBaseUrl !== undefined) return bakedWebBaseUrl;
  try {
    return new URL(apiBaseUrl).origin;
  } catch {
    return apiBaseUrl;
  }
}
