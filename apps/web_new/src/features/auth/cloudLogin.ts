// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud sign-in entry rules (CLOUD-011-014, from parity/specs/cloud.md §3):
// the return target the provider flow carries, the conditions that let a
// returning visitor resume without the card, the loop guard that keeps the
// flow off the login page, and the tab-scoped explicit-logout marker.
// Pure and storage-agnostic so the page and its tests share one definition.

import { cloudSignInHandoffUrl } from "@pidash/api-client";

import { safeNextPath } from "../../core/session/guards.js";

/** Tab-scoped marker: this tab signed out explicitly (shared with CLOUD-016). */
export const EXPLICIT_LOGOUT_KEY = "pidash:explicitly-logged-out";

/** The provider handoff path, also the card's static (unenhanced) target. */
export const CLOUD_SIGN_IN_HREF = "/api/auth/sign-in/";

/** Paths that would send the visitor straight back to sign-in. */
const SIGN_IN_PATHS = new Set(["/login", "/sign-in", "/signin"]);

export interface CloudLoginSearch {
  /** Raw `next_path`, absent when the visit carried none. */
  nextPath: string | null;
  /** Any non-empty `error` keeps the card on screen with its alert. */
  error: string | null;
}

/** Read the two login parameters from a raw query string. */
export function parseCloudLoginSearch(search: string): CloudLoginSearch {
  const params = new URLSearchParams(search.startsWith("?") ? search.slice(1) : search);
  const nextPath = params.get("next_path");
  const error = params.get("error");
  return {
    nextPath: nextPath !== null && nextPath.length > 0 ? nextPath : null,
    error: error !== null && error.length > 0 ? error : null,
  };
}

/**
 * Accept only an app-local target and normalize it to path + query.
 * A single leading slash is required (protocol-relative values leave the
 * origin), backslashes are rejected because browsers read them as
 * separators, and the value must still resolve to the app's own origin.
 * The fragment never survives: provider URLs and logs must not carry
 * one-time tokens. Values that fail any rule resolve to null.
 */
export function resolveCloudReturnTarget(raw: string | null | undefined, origin: string): string | null {
  if (safeNextPath(raw) === undefined) return null;
  if ((raw as string).includes("\\")) return null;
  let parsed: URL;
  try {
    parsed = new URL(raw as string, origin);
  } catch {
    return null;
  }
  if (parsed.origin !== origin) return null;
  const normalized = parsed.pathname;
  // Dot segments are decoded before URL normalization, so a value such as
  // `/%2e%2e//evil.example/x` arrives here as a protocol-relative path.
  // Re-apply the app-local rules to what the parser produced.
  let decoded: string;
  try {
    decoded = decodeURIComponent(normalized);
  } catch {
    return null;
  }
  if (!normalized.startsWith("/") || normalized.startsWith("//") || normalized.includes("\\")) return null;
  if (decoded.includes("\\") || decoded.startsWith("//")) return null;
  return `${normalized}${parsed.search}`;
}

/** Trim trailing slashes so `/login/` guards the same as `/login`. */
function normalizedPath(pathname: string): string {
  return pathname.replace(/\/+$/, "") || "/";
}

/**
 * Fold a pathname to the spelling the router actually matches: percent
 * escapes are decoded and the comparison is case-insensitive, so
 * `/%6Cogin` and `/LoGiN` cannot slip past the loop guard.
 */
function routerPath(pathname: string): string {
  // `resolveCloudReturnTarget` already proved the path decodes, so this is
  // total for every value that reaches the guard.
  return normalizedPath(decodeURIComponent(pathname)).toLowerCase();
}

/**
 * The target the handoff carries. A missing or unsafe `next_path` falls
 * back to the app root, and so does a target that resolves to the login
 * page itself: returning there would read as "sign-in did nothing".
 */
export function cloudHandoffTarget(raw: string | null | undefined, origin: string): string {
  const resolved = resolveCloudReturnTarget(raw, origin);
  if (resolved === null) return "/";
  const pathname = new URL(resolved, origin).pathname;
  return SIGN_IN_PATHS.has(routerPath(pathname)) ? "/" : resolved;
}

/**
 * Whether this visit resumes the provider flow without showing the card.
 * A pending (safe) target is required — a bare login visit stays on the
 * card — and both an error landing and the explicit-logout marker stop it.
 */
export function shouldAttemptSilentSignIn(args: {
  pendingTarget: string | null;
  error: string | null;
  explicitlyLoggedOut: boolean;
}): boolean {
  return args.pendingTarget !== null && args.error === null && !args.explicitlyLoggedOut;
}

/** Whether the tab carries the explicit-logout marker; unreadable storage reads false. */
export function readExplicitLogout(storage: Pick<Storage, "getItem"> | null): boolean {
  if (storage === null) return false;
  try {
    return storage.getItem(EXPLICIT_LOGOUT_KEY) === "1";
  } catch {
    return false;
  }
}

/** Record the marker for this tab; storage failures are swallowed. */
export function markExplicitLogout(storage: Pick<Storage, "setItem"> | null): void {
  if (storage === null) return;
  try {
    storage.setItem(EXPLICIT_LOGOUT_KEY, "1");
  } catch {
    // Privacy-restricted contexts have no usable storage; the marker is a convenience.
  }
}

/** Clear the marker before a manual sign-in; storage failures are swallowed. */
export function clearExplicitLogout(storage: Pick<Storage, "removeItem"> | null): void {
  if (storage === null) return;
  try {
    storage.removeItem(EXPLICIT_LOGOUT_KEY);
  } catch {
    // Privacy-restricted contexts have no usable storage; the marker is a convenience.
  }
}

/** The full provider handoff the browser leaves for. */
export function cloudSignInUrl(apiBase: string, browserOrigin: string, returnTo: string): string {
  return cloudSignInHandoffUrl(apiBase, { returnTo, returnOrigin: browserOrigin });
}
