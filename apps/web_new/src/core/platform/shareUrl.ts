// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copied in-app links (DESK-019, parity/specs/desktop.md §5). Callers hand
// this the path of the screen they want to share; the active platform
// decides which origin that path belongs to. A path that already carries a
// query or fragment keeps it exactly, and a relative address joins the
// origin without losing either.

export const HOSTED_ORIGIN_REQUIRED = "A hosted web origin is required to create a shareable link.";
export const HOSTED_ORIGIN_SCHEME = "The hosted web origin must use http or https.";

/** Normalise a configured hosted base (trimmed, no trailing slashes). */
function normaliseBase(raw: string): string {
  return raw.trim().replace(/\/+$/, "");
}

/** True when `raw` parses as an absolute http(s) address. */
export function parseHostedOrigin(raw: string | undefined | null): string | undefined {
  if (raw === undefined || raw === null) return undefined;
  const base = normaliseBase(raw);
  if (base === "") return undefined;
  let parsed: URL;
  try {
    parsed = new URL(base);
  } catch {
    return undefined;
  }
  return parsed.protocol === "http:" || parsed.protocol === "https:" ? base : undefined;
}

/**
 * Resolve a shareable address for `path` against `base`.
 *
 * Without a usable base the builder refuses rather than inventing an
 * origin: an empty or non-web address cannot be a share target, and
 * silently falling back to the local bundle origin would hand out a
 * `tauri://` address no browser can open. A base that is set but not a
 * web address is told apart from an absent one so the caller can say so.
 */
export function resolveShareUrl(path: string, base: string | undefined | null): string {
  const configured = base === undefined || base === null ? "" : base.trim();
  if (configured === "") throw new Error(HOSTED_ORIGIN_REQUIRED);
  const origin = parseHostedOrigin(configured);
  if (origin === undefined) throw new Error(HOSTED_ORIGIN_SCHEME);
  return new URL(path, `${origin}/`).toString();
}

declare const __PIDASH_WEB_BASE_URL__: string | undefined;

/** The hosted origin baked into this bundle, when the build set one. */
export function bakedHostedOrigin(): string | undefined {
  return typeof __PIDASH_WEB_BASE_URL__ === "string" ? parseHostedOrigin(__PIDASH_WEB_BASE_URL__) : undefined;
}

/** The current page origin, or undefined outside a browser. */
export function pageOrigin(): string | undefined {
  if (typeof globalThis.window === "undefined") return undefined;
  const origin = globalThis.window.location?.origin;
  return typeof origin === "string" && origin !== "" && origin !== "null" ? origin : undefined;
}

/** Resolve a shareable address on the desktop platform. */
export function desktopShareUrl(path: string, base: string | undefined = bakedHostedOrigin()): string {
  return resolveShareUrl(path, base);
}

/** Resolve a shareable address on the web platform. */
export function webShareUrl(path: string, base: string | undefined = pageOrigin()): string {
  return resolveShareUrl(path, base);
}
