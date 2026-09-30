// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Migrated-route list (F-09). The single source of truth is
// apps/web_new/migrated-routes.json: path prefixes the proxy serves from
// web_new. This module re-exports that list for the app (the link helper
// decides router navigation vs plain anchor from it) with a normalizer
// so "/sign-in", "/sign-in/" and "/sign-in?next=/" all match "/sign-in".

import list from "../../../migrated-routes.json";

/** Path prefixes served by web_new. Empty until area gates add theirs. */
export const MIGRATED_ROUTE_PREFIXES: readonly string[] = list.prefixes;

/** Normalize a path or prefix for comparison: leading slash, no trailing slash, no query/hash. */
export function normalizePathPrefix(value: string): string {
  const bare = value.split(/[?#]/, 1)[0] ?? "";
  const trimmed = bare.trim();
  const leading = trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
  return leading.length > 1 ? leading.replace(/\/+$/, "") : leading;
}

/**
 * True when the path is served by web_new: it equals a migrated prefix
 * or lives under one. Matching is segment-aware, so "/sign-in" never
 * matches "/sign-invites". Defaults to the bundled list; callers
 * (tests, previews) may pass an explicit list instead.
 */
export function isMigratedPath(path: string, prefixes: readonly string[] = MIGRATED_ROUTE_PREFIXES): boolean {
  const normalized = normalizePathPrefix(path);
  return prefixes.some((prefix) => {
    if (prefix.trim() === "") return false;
    const clean = normalizePathPrefix(prefix);
    if (clean === "/") return true;
    return normalized === clean || normalized.startsWith(`${clean}/`);
  });
}

/**
 * Whether a link renders as client-side router navigation ("router") or
 * as a plain anchor ("anchor"). Same-origin either way: unmigrated
 * screens live in the old app behind the same proxy, so a full-page load
 * lands on them with the session cookie intact. Links flip automatically
 * when their prefix joins the list.
 */
export function resolveLinkKind(
  to: string,
  prefixes: readonly string[] = MIGRATED_ROUTE_PREFIXES
): "router" | "anchor" {
  return isMigratedPath(to, prefixes) ? "router" : "anchor";
}
