// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Secondary-destinations disclosure model (parity/specs/shell.md §4,
// SHELL-053/054). Pure: the destination catalogue, role gating, the
// auto-open rule and the persisted open flag. The disclosure lists
// every destination the role permits in catalogue order; pin and order
// writes persist through the sidebar-preferences endpoints, and the
// pin control itself arrives with the cloud extended sidebar. The
// component builds on this; nothing here touches React or the router.

import { ROLE_MEMBER } from "../../core/session/permissions.js";

/** One secondary destination behind the More disclosure. */
export interface MoreDestinationDef {
  /** Stable key; doubles as the sidebar-preferences key when pinned. */
  key: string;
  /** Display label. */
  name: string;
  /** Workspace-relative path below the slug, e.g. "projects/archives". */
  segment: string;
  /** When true, guests never see the destination. */
  memberPlus: boolean;
  /** Landing on the destination expands the disclosure. */
  autoOpen: boolean;
}

/**
 * Destinations in display order (SHELL-053). The projects entry links
 * the workspace projects list; it never auto-opens, so project pages
 * keep the disclosure as the visitor left it.
 */
export const MORE_DESTINATIONS: readonly MoreDestinationDef[] = [
  { key: "projects", name: "Projects", segment: "projects", memberPlus: false, autoOpen: false },
  { key: "runners", name: "AI Workers", segment: "runners", memberPlus: true, autoOpen: true },
  { key: "prompts", name: "Prompts", segment: "prompts", memberPlus: false, autoOpen: true },
  { key: "schedulers", name: "Schedulers", segment: "schedulers", memberPlus: false, autoOpen: true },
  { key: "ai-dev-machines", name: "AI Dev Machines", segment: "ai-dev-machines", memberPlus: true, autoOpen: true },
  { key: "analytics", name: "Analytics", segment: "analytics", memberPlus: true, autoOpen: true },
  { key: "archives", name: "Archives", segment: "projects/archives", memberPlus: true, autoOpen: true },
  { key: "active-cycles", name: "Active cycles", segment: "active-cycles", memberPlus: true, autoOpen: true },
];

/** Sidebar address of one destination (SHELL-053). */
export function moreHref(workspaceSlug: string, segment: string): string {
  return `/${workspaceSlug}/${segment}/`;
}

/** One destination with its address attached. */
export interface MoreDestination extends MoreDestinationDef {
  href: string;
}

/**
 * Destinations the role permits, in catalogue order (SHELL-053/046).
 * Guests see the shared rows only; a null role reads as guest level.
 */
export function selectMoreEntries(args: { workspaceSlug: string; role: number | null }): MoreDestination[] {
  const memberPlus = args.role !== null && args.role >= ROLE_MEMBER;
  return MORE_DESTINATIONS.filter((def) => memberPlus || !def.memberPlus).map((def) => ({
    ...def,
    href: moreHref(args.workspaceSlug, def.segment),
  }));
}

/** Strip one trailing slash so addresses compare with or without it. */
function stripTrailingSlash(value: string): string {
  return value.endsWith("/") && value.length > 1 ? value.slice(0, -1) : value;
}

/**
 * Active rule for one destination (SHELL-054): the current address at
 * or under the destination's address.
 */
export function isMoreEntryActive(pathname: string, href: string): boolean {
  const current = stripTrailingSlash(pathname);
  const entry = stripTrailingSlash(href);
  return current === entry || current.startsWith(`${entry}/`);
}

/**
 * True on a hosted member route (SHELL-054): landing there expands the
 * disclosure so the active row is never hidden behind it. Matching is
 * segment-aware per entry, so project archive pages (which merely
 * contain an "archives" segment) leave the disclosure alone.
 */
export function isMoreRoute(pathname: string, workspaceSlug: string): boolean {
  return MORE_DESTINATIONS.filter((def) => def.autoOpen).some((def) =>
    isMoreEntryActive(pathname, moreHref(workspaceSlug, def.segment))
  );
}

/** localStorage key for the disclosure's persisted open flag. */
export const MORE_SECTION_OPEN_KEY = "web_new.more.open";

function storage(): Storage | null {
  // Both callers run this inside their own try/catch, so a denied
  // store reads closed without throwing here.
  return globalThis.localStorage ?? null;
}

/** Persisted open flag; missing or unreadable storage reads closed. */
export function loadMoreOpen(): boolean {
  try {
    return storage()?.getItem(MORE_SECTION_OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

/** Persist the disclosure's open flag (SHELL-054). */
export function persistMoreOpen(open: boolean): void {
  try {
    const store = storage();
    if (store === null) return;
    if (open) store.setItem(MORE_SECTION_OPEN_KEY, "1");
    else store.removeItem(MORE_SECTION_OPEN_KEY);
  } catch {
    // Private modes deny storage; the disclosure just starts closed.
  }
}
