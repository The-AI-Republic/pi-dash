// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Migrated-route list (F-09, workspace-scoped form NEWFRONT-528). The single
// source of truth is apps/web_new/migrated-routes.json: routes the proxy
// serves from web_new. This module re-exports that list for the app (the link
// helper decides router navigation vs plain anchor from it) and matches paths
// with exactly the rules the Caddy generator applies:
//
//   /a/b   literal path; matches it and everything below it
//   /{ws}  one variable segment, exactly that depth
//   /{ws}/projects  variable then literals, exact depth
//   /{ws}/x/*       the same plus everything below
//
// A variable first segment never matches a reserved top-level name
// (case-insensitive) or a name that owns a literal entry, and never matches a
// segment containing a dot — workspace slugs cannot contain one. The shared
// fixtures in migrated-routes.test.ts and checks/coexistence-routes.test.mjs
// keep this side and the generator honest.

import list from "../../../migrated-routes.json";

/** Route entries served by web_new, in list order. */
export const MIGRATED_ROUTE_ENTRIES: readonly string[] = list.entries;

/** Top-level names a workspace-scoped entry never claims. */
export const MIGRATED_RESERVED_TOP_LEVEL: readonly string[] = list.reservedTopLevel;

const VARIABLE_SEGMENT = /^\{([A-Za-z][A-Za-z0-9]*)\}$/;

/** Normalize a path or entry for comparison: leading slash, no trailing slash, no query/hash. */
export function normalizePathPrefix(value: string): string {
  const bare = value.split(/[?#]/, 1)[0] ?? "";
  const trimmed = bare.trim();
  const leading = trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
  return leading.length > 1 ? leading.replace(/\/+$/, "") : leading;
}

type Segment = { kind: "literal"; value: string } | { kind: "variable" } | { kind: "wildcard" };

/** A segment that can appear in a compiled entry: the trailing wildcard is sliced off at compile time. */
type FixedSegment = Exclude<Segment, { kind: "wildcard" }>;

interface CompiledEntry {
  /** Normalized entry; "/" is the catch-all. */
  entry: string;
  catchAll: boolean;
  /** Literal entries match their whole subtree, as they always have. */
  subtree: boolean;
  segments: readonly FixedSegment[];
  /** True when the first segment is a variable (a workspace scope). */
  workspaceScoped: boolean;
}

function splitEntry(entry: string): Segment[] {
  // The caller passes a normalized entry, so the leading slash is guaranteed.
  if (entry === "/") return [];
  return entry
    .slice(1)
    .split("/")
    .map((segment) => compileSegment(segment, entry));
}

function compileSegment(segment: string, entry: string): Segment {
  if (segment === "*") return { kind: "wildcard" };
  if (VARIABLE_SEGMENT.test(segment)) return { kind: "variable" };
  if (segment === "" || /[{}*]/.test(segment)) {
    throw new Error(`bad segment ${JSON.stringify(segment)} in migrated route ${entry}`);
  }
  return { kind: "literal", value: segment };
}

/** Compile one entry. Throws on malformed syntax (the generator validates the same list). */
export function compileEntry(entry: string): CompiledEntry {
  if (!entry.startsWith("/")) throw new Error(`migrated route must start with a slash: ${entry}`);
  const normalized = normalizePathPrefix(entry);
  const segments = splitEntry(normalized);
  const wildcard = segments.findIndex((segment) => segment.kind === "wildcard");
  if (wildcard !== -1 && wildcard !== segments.length - 1) {
    throw new Error(`wildcard "*" must be the last segment: ${entry}`);
  }
  // A mid-list wildcard threw above, so slicing the end leaves no wildcard.
  const fixed = (wildcard === -1 ? segments : segments.slice(0, -1)) as FixedSegment[];
  return {
    entry: normalized,
    catchAll: normalized === "/",
    // Literal entries have always meant "this path and everything below";
    // workspace-scoped entries are exact unless they end in `*`.
    subtree: wildcard !== -1 || segments[0]?.kind === "literal",
    segments: fixed,
    workspaceScoped: segments[0]?.kind === "variable",
  };
}

/**
 * Names a workspace-scoped entry must not claim: the reserved list plus every
 * literal entry's first segment, lowercased. Literal entries own their name
 * even when it is also reserved.
 */
export function reservedTopLevelOf(
  entries: readonly string[] = MIGRATED_ROUTE_ENTRIES,
  reserved: readonly string[] = MIGRATED_RESERVED_TOP_LEVEL
): readonly string[] {
  const names = new Set(reserved.map((name) => name.toLowerCase()));
  for (const compiled of entries.map(compileEntry)) {
    const first = compiled.segments[0];
    if (first?.kind === "literal") names.add(first.value.toLowerCase());
  }
  return [...names].sort();
}

const DEFAULT_COMPILED = MIGRATED_ROUTE_ENTRIES.map(compileEntry);
const DEFAULT_RESERVED = reservedTopLevelOf();

function matchesSegment(segment: FixedSegment, value: string): boolean {
  if (segment.kind === "variable") return value.length > 0 && !value.includes(".");
  return segment.value === value;
}

function matchesEntry(compiled: CompiledEntry, segments: readonly string[], reserved: readonly string[]): boolean {
  if (compiled.catchAll) return true;
  const fixed = compiled.segments.length;
  if (segments.length < fixed) return false;
  if (!compiled.subtree && segments.length !== fixed) return false;
  return compiled.segments.every((segment, index) => {
    // The length checks above guarantee the path holds this index.
    const value = segments[index] as string;
    if (segment.kind === "variable" && index === 0 && reserved.includes(value.toLowerCase())) return false;
    return matchesSegment(segment, value);
  });
}

function compileList(
  entries: readonly string[],
  reserved: readonly string[]
): { compiled: CompiledEntry[]; names: readonly string[] } {
  if (entries === MIGRATED_ROUTE_ENTRIES && reserved === MIGRATED_RESERVED_TOP_LEVEL) {
    return { compiled: DEFAULT_COMPILED, names: DEFAULT_RESERVED };
  }
  const listed = entries.filter((entry) => entry.trim() !== "");
  return { compiled: listed.map(compileEntry), names: reservedTopLevelOf(listed, reserved) };
}

/**
 * True when the path is served by web_new: a literal entry matches it or a
 * path below it; a workspace-scoped entry matches its exact depth (or its
 * subtree with a trailing `*`), never a reserved top-level name. Defaults to
 * the bundled list; callers (tests, previews) may pass an explicit list.
 */
export function isMigratedPath(
  path: string,
  entries: readonly string[] = MIGRATED_ROUTE_ENTRIES,
  reserved: readonly string[] = MIGRATED_RESERVED_TOP_LEVEL
): boolean {
  const normalized = normalizePathPrefix(path);
  const segments = normalized === "/" ? [] : normalized.slice(1).split("/");
  if (segments.some((segment) => segment === "")) return false;
  const { compiled, names } = compileList(entries, reserved);
  return compiled.some((entry) => matchesEntry(entry, segments, names));
}

/**
 * Whether a link renders as client-side router navigation ("router") or
 * as a plain anchor ("anchor"). Same-origin either way: unmigrated
 * screens live in the old app behind the same proxy, so a full-page load
 * lands on them with the session cookie intact. Links flip automatically
 * when their route joins the list.
 */
export function resolveLinkKind(to: string, entries: readonly string[] = MIGRATED_ROUTE_ENTRIES): "router" | "anchor" {
  return isMigratedPath(to, entries) ? "router" : "anchor";
}
