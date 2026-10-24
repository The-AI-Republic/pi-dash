// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module links section rules (parity/specs/modules.md §9.3, MOD-035).
// Pure selectors keep the scheme default, the title fallback, the icon
// source, the author join and the meta sentence out of the components so
// each branch is testable without a browser. Live and archived detail
// records both feed the section, so the row shape is the structural
// intersection the two link contracts share.

import type { WorkspaceMemberList } from "@pidash/api-client";

import { timeAgo } from "../../../../shared/lib/timeAgo.js";

/** The link fields the section reads; live and archived rows satisfy it. */
export interface ModuleLinkSummary {
  id: string;
  title: string | null;
  url: string;
  created_at?: string | undefined;
  created_by?: string | null | undefined;
}

/** Section header; the disclosure toggle carries this name. */
export const MODULE_LINKS_TITLE = "Links";

/** Pinned notice while the open section holds no links (§9.3). */
export const MODULE_LINKS_EMPTY = "No links added yet";

/** Pinned dialog titles for adding and editing (§9.3). */
export const MODULE_LINK_ADD_TITLE = "Add link";
export const MODULE_LINK_UPDATE_TITLE = "Update link";

/** Pinned success notices for the three link writes (§9.3). */
export const MODULE_LINK_CREATED = "Module link created successfully.";
export const MODULE_LINK_UPDATED = "Module link updated successfully.";
export const MODULE_LINK_DELETED = "Module link deleted successfully.";

/** Pinned notice after copying a link URL (§9.3). */
export const MODULE_LINK_COPIED = "Copied to clipboard";

/** Failure titles; the server sentence (or the shared fallback) follows. */
export const MODULE_LINK_CREATE_FAILED_TITLE = "Couldn't create the module link";
export const MODULE_LINK_UPDATE_FAILED_TITLE = "Couldn't update the module link";
export const MODULE_LINK_DELETE_FAILED_TITLE = "Couldn't delete the module link";
export const MODULE_LINK_COPY_FAILED_TITLE = "Couldn't copy the link";

/**
 * Default a scheme-less address to http:// (§9.3). Anything already
 * starting with http(s) is kept untouched; anything else is left for
 * the server to judge after the default is applied.
 */
export function defaultModuleLinkScheme(raw: string): string {
  const trimmed = raw.trim();
  return /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
}

/** One row's display text: the title when set, else the URL (§9.3). */
export function moduleLinkTitle(link: Pick<ModuleLinkSummary, "title" | "url">): string {
  const title = link.title?.trim() ?? "";
  return title === "" ? link.url : title;
}

/** Favicon for an absolute URL; null when the address will not parse. */
export function moduleLinkFavicon(url: string): string | null {
  try {
    return `${new URL(url).origin}/favicon.ico`;
  } catch {
    return null;
  }
}

/**
 * Author display name for a link creator, joined against the workspace
 * directory. Null when no creator is recorded or the member is gone; the
 * row then shows the age alone, as the old app does.
 */
export function resolveModuleLinkAuthor(
  members: WorkspaceMemberList | undefined,
  createdBy: string | null | undefined
): string | null {
  if (createdBy === null || createdBy === undefined) return null;
  return members?.find((entry) => entry.member.id === createdBy)?.member.display_name ?? null;
}

/**
 * Row meta sentence (§9.3): the relative age and the author's display
 * name. Either half may be absent; with neither (archived rows) there is
 * no meta line at all.
 */
export function describeModuleLinkMeta(
  createdAt: string | undefined,
  author: string | null,
  nowMs: number = Date.now()
): string | null {
  const age = createdAt === undefined ? null : `Added ${timeAgo(createdAt, nowMs)}`;
  if (age !== null && author !== null) return `${age} by ${author}`;
  if (age !== null) return age;
  if (author !== null) return `Added by ${author}`;
  return null;
}

/** True when at least one row records a creator worth resolving. */
export function moduleLinksNeedAuthors(links: readonly Pick<ModuleLinkSummary, "created_by">[]): boolean {
  return links.some((link) => link.created_by !== null && link.created_by !== undefined);
}
