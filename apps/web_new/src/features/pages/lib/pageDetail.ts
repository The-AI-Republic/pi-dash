// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages detail shell model (parity/specs/pages.md §2.6, §2.7, §4,
// PAGE-022–025,079–080). It owns the shareable pane/version keys, the
// sibling-switcher rows, the read/edit matrix and the loading/panel
// decision; the route and components stay thin over it.

import { PAGE_ACCESS_PRIVATE, type PageRow } from "@pidash/api-client";

import { ROLE_ADMIN, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { pageDetailHref, pageDisplayTitle } from "./pageList.js";

/** Shareable pane tabs; any other value is not a pane. */
export type PagePaneTab = "outline" | "info" | "assets";

export const PAGE_PANE_TABS: ReadonlyArray<{ value: PagePaneTab; label: string }> = [
  { value: "outline", label: "Outline" },
  { value: "info", label: "Info" },
  { value: "assets", label: "Assets" },
];

/** The detail address's two query keys. Both are optional. */
export interface PageDetailSearch {
  paneTab?: PagePaneTab | undefined;
  version?: string | undefined;
}

function single(value: unknown): string | null {
  if (typeof value === "string") return value;
  if (Array.isArray(value) && typeof value[0] === "string") return value[0];
  return null;
}

function isPaneTab(value: string | null): value is PagePaneTab {
  return value === "outline" || value === "info" || value === "assets";
}

/** Read only the two supported keys; unknown pane values are dropped. */
export function parsePageDetailSearch(search: Record<string, unknown>): PageDetailSearch {
  const pane = single(search["paneTab"]);
  const version = single(search["version"]);
  const parsed: PageDetailSearch = {};
  if (isPaneTab(pane)) parsed.paneTab = pane;
  if (version !== null && version.trim() !== "") parsed.version = version;
  return parsed;
}

/** Detail address with no query string; the canonical address copy-link emits. */
export function pageCanonicalHref(workspaceSlug: string, projectId: string, pageId: string): string {
  return pageDetailHref(workspaceSlug, projectId, pageId);
}

/** Detail address carrying whichever deep-link keys are set. */
export function pageDetailDeepHref(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  search: PageDetailSearch
): string {
  const params = new URLSearchParams();
  if (search.paneTab !== undefined) params.set("paneTab", search.paneTab);
  if (search.version !== undefined) params.set("version", search.version);
  const query = params.toString();
  const path = pageCanonicalHref(workspaceSlug, projectId, pageId);
  return query === "" ? path : `${path}?${query}`;
}

/** Sibling navigation drops both keys (PAGE-024, PAGE-080). */
export function siblingHref(workspaceSlug: string, projectId: string, pageId: string): string {
  return pageCanonicalHref(workspaceSlug, projectId, pageId);
}

/** Closing the snapshot keeps the pane but removes the version key. */
export function closeVersionSearch(search: PageDetailSearch): PageDetailSearch {
  const next: PageDetailSearch = {};
  if (search.paneTab !== undefined) next.paneTab = search.paneTab;
  return next;
}

/** The options menu's history shortcut writes the info pane (PAGE-080). */
export function historySearch(search: PageDetailSearch): PageDetailSearch {
  return { ...search, paneTab: "info" };
}

/** Selecting a timeline entry writes both its pane and the version key. */
export function versionSearch(versionId: string, current: PageDetailSearch): PageDetailSearch {
  return { paneTab: current.paneTab ?? "info", version: versionId };
}

/**
 * Picking a pane tab keeps any open snapshot; closing the pane drops only
 * its key (a snapshot can outlive the pane it was opened from).
 */
export function paneSearch(search: PageDetailSearch, tab: PagePaneTab | null): PageDetailSearch {
  const version = search.version === undefined ? {} : { version: search.version };
  return tab === null ? version : { paneTab: tab, ...version };
}

/** Pane-tab label, or null when the address carries no supported tab. */
export function paneTabLabel(tab: PagePaneTab | undefined): string | null {
  return PAGE_PANE_TABS.find((entry) => entry.value === tab)?.label ?? null;
}

/** Document title follows the visible page name, never a blank string. */
export function pageDocumentTitle(name: string): string {
  return pageDisplayTitle(name);
}

/** One switcher row; archived pages never appear (PAGE-024). */
export interface PageSiblingOption {
  id: string;
  name: string;
}

/**
 * Rows the sibling switcher offers: every collection row the server
 * returned (already filtered for visibility), minus archived pages. The
 * open page stays in the list so the switcher always names where you are.
 */
export function pageSiblingOptions(rows: readonly PageRow[]): PageSiblingOption[] {
  return rows
    .filter((row) => row.archived_at === null)
    .map((row) => ({ id: row.id, name: pageDisplayTitle(row.name) }))
    .sort((left, right) => left.name.localeCompare(right.name));
}

/** Narrow a switcher menu by its search field. */
export function filterSiblingOptions(options: readonly PageSiblingOption[], query: string): PageSiblingOption[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...options];
  return options.filter((option) => option.name.toLowerCase().includes(needle));
}

/** Project member standing (guest included) may read open pages. */
export type PageDetailStanding = "owner" | "admin" | "member" | "guest" | "outsider";

export interface PageAccessInput {
  ownedBy: string;
  access: number;
  archivedAt: string | null;
  isLocked: boolean;
  viewerId: string | null;
  projectRole: number | null;
}

export interface PageCapabilities {
  standing: PageDetailStanding;
  canRead: boolean;
  canEdit: boolean;
  canManage: boolean;
  readOnly: boolean;
  isShelved: boolean;
  isLocked: boolean;
}

/** Standing is the stronger of ownership and project role. */
export function pageStanding(input: Pick<PageAccessInput, "ownedBy" | "viewerId" | "projectRole">): PageDetailStanding {
  if (input.viewerId !== null && input.ownedBy === input.viewerId) return "owner";
  const role = input.projectRole;
  if (role === null) return "outsider";
  if (role >= ROLE_ADMIN) return "admin";
  if (role >= ROLE_MEMBER) return "member";
  if (role > 0) return "guest";
  return "outsider";
}

/**
 * Read: open pages for members and admins, a restricted page only for its
 * owner, and a guest only the pages it owns (its collection never carries
 * anyone else's). Edit: read plus member-or-owner standing, unlocked and
 * unarchived. A locked or shelved page keeps the shell but has no editable
 * region.
 */
export function pageCapabilities(input: PageAccessInput): PageCapabilities {
  const standing = pageStanding(input);
  const restricted = input.access === PAGE_ACCESS_PRIVATE;
  const isShelved = input.archivedAt !== null;
  const canRead = standing === "owner" || ((standing === "admin" || standing === "member") && !restricted);
  const canManage = standing === "owner" || standing === "admin";
  const memberOrOwner = standing === "owner" || standing === "admin" || standing === "member";
  const canEdit = canRead && !input.isLocked && !isShelved && memberOrOwner;
  return {
    standing,
    canRead,
    canEdit,
    canManage,
    readOnly: canRead && !canEdit,
    isShelved,
    isLocked: input.isLocked,
  };
}

/** What the detail shell renders once the record read settles. */
export type PageDetailBody = "spinner" | "panel" | "editor";

export interface PageDetailBodyInput {
  /** The record read (and the identity it is judged against) is unresolved. */
  pending: boolean;
  /** The record read failed (403, 404, deleted id, transport). */
  recordError: boolean;
  /** The record resolved but the viewer may not read it. */
  canRead: boolean;
}

/**
 * A failed/missing record is the explanatory panel, never an editor flash;
 * a resolved-but-unreadable record is the same panel. Until both the record
 * and the viewer identity settle, the shell keeps its centered spinner
 * (PAGE-023, PAGE-079).
 */
export function resolvePageDetailBody({ pending, recordError, canRead }: PageDetailBodyInput): PageDetailBody {
  if (recordError) return "panel";
  if (pending) return "spinner";
  if (!canRead) return "panel";
  return "editor";
}
