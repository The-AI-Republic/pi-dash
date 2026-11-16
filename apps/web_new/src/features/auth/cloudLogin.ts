// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud sign-in entry rules (CLOUD-011-014, from parity/specs/cloud.md §3):
// the return target the provider flow carries, the conditions that let a
// returning visitor resume without the card, the loop guard that keeps the
// flow off the login page, and the tab-scoped explicit-logout marker.
// Pure and storage-agnostic so the page and its tests share one definition.
// The desktop handoff below it (CLOUD-017/CLOUD-018, from
// parity/specs/cloud.md §4) starts the same provider flow from the generic
// cards and the shared login card: a web navigation, or the system browser
// with the desktop marker inside the shell.

import { cloudSignInHandoffUrl } from "@pidash/api-client";

import type { ShellSignInApi } from "../../core/platform/index.js";
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
 * `/%6Cogin` and `/LoGiN` cannot slip past the loop guard. A decoded
 * `?` or `#` ends the pathname, so `/%6Cogin%3Fnext_path=%2Fdocs` names
 * the login page too once the escapes are read.
 */
function routerPath(pathname: string): string {
  // `resolveCloudReturnTarget` already proved the path decodes, so this is
  // total for every value that reaches the guard.
  const decoded = decodeURIComponent(pathname);
  const delimiter = decoded.search(/[?#]/);
  return normalizedPath(delimiter === -1 ? decoded : decoded.slice(0, delimiter)).toLowerCase();
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

/** The current page as a handoff target: pathname plus query, never the fragment. */
export function currentPageTarget(location: { pathname: string; search: string }): string {
  return `${location.pathname}${location.search}`;
}

/**
 * Loud refusal when the shell advertised no API origin. The
 * VITE_API_BASE_URL token is pinned by the parity driver (the
 * provider-button and desktop-signin specs assert the refusal names it),
 * so it stays verbatim.
 */
export const DESKTOP_ORIGIN_MISSING =
  "Cloud sign-in refused: the desktop shell advertised no API origin, so no provider handoff can be built (VITE_API_BASE_URL).";

/** Loud refusal when the advertised origin is not a usable web address. */
export const DESKTOP_ORIGIN_UNUSABLE =
  "Cloud sign-in refused: the desktop shell's API origin is not a usable web address.";

/** Logged when the system browser cannot be opened; the caller surfaces it. */
export const DESKTOP_OPEN_FAILED = "Cloud sign-in could not open the system browser.";

/** The baked advertisement as a normalized web origin, or null when unusable. */
function parseBakedOrigin(raw: string): string | null {
  try {
    const parsed = new URL(raw);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    return parsed.origin;
  } catch {
    return null;
  }
}

export interface StartCloudHandoffArgs {
  /** Shell capability; absent on web builds, which navigate instead. */
  shell: Pick<ShellSignInApi, "bakedOrigin" | "openSystemBrowser"> | undefined;
  /** API base for the web branch (the app client's). */
  apiBase: string;
  /** Page origin for the handoff's split-origin check. */
  browserOrigin: string;
  /** Validated return target (app path plus query). */
  returnTo: string;
  /** Web-branch navigation (assign for clicks, replace for silent resume). */
  navigate: (url: string) => void;
  /** Desktop open failure surface; the refusal paths only log. */
  onOpenFailed?: ((error: unknown) => void) | undefined;
}

/**
 * Start the organizational-provider handoff (CLOUD-017/CLOUD-018). Web
 * builds navigate to the handoff URL. Inside the desktop shell the
 * handoff opens in the system browser with the desktop marker and never
 * navigates the embedded view: a shell without a usable baked origin
 * refuses with a logged error, and a blocked browser open is logged and
 * reported through `onOpenFailed` (the login card surfaces it) instead of
 * falling back to an in-window navigation.
 */
export async function startCloudHandoff(args: StartCloudHandoffArgs): Promise<void> {
  const { shell } = args;
  if (shell === undefined) {
    args.navigate(cloudSignInUrl(args.apiBase, args.browserOrigin, args.returnTo));
    return;
  }
  const baked = shell.bakedOrigin;
  if (baked === undefined) {
    console.error(DESKTOP_ORIGIN_MISSING);
    return;
  }
  const origin = parseBakedOrigin(baked);
  if (origin === null) {
    console.error(DESKTOP_ORIGIN_UNUSABLE);
    return;
  }
  try {
    await shell.openSystemBrowser(cloudSignInHandoffUrl(origin, { returnTo: args.returnTo, desktop: true }));
  } catch (error) {
    console.error(DESKTOP_OPEN_FAILED);
    args.onOpenFailed?.(error);
  }
}
