// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Version timeline model (parity/specs/pages.md §6.9, PAGE-051). The info
// tab's history is one ordered list: the live document first as an
// id-less row, then every stored snapshot newest-first, each carrying the
// moment it was saved and who saved it. Exactly one row is the active
// one — the snapshot named by the address, or the live row when the
// address names none — so the reader always knows which document the
// overlay is showing.

import type { PageVersion } from "@pidash/api-client";

import { PAGE_DEACTIVATED_USER, type PageActorMember } from "./pagePane.js";

/** The pinned label of the id-less live row; it never links anywhere. */
export const PAGE_CURRENT_VERSION_LABEL = "Current version";

/** One timeline row. */
export interface PageVersionEntry {
  /** The snapshot's id, or null for the live document. */
  id: string | null;
  /** The row's primary text: a saved-at timestamp, or the pinned label. */
  label: string;
  /**
   * Who saved the snapshot. Empty for the live row and while the member
   * directory has not answered yet, so no fallback label flashes first.
   */
  author: string;
  /** True for the one row the open overlay (or the live document) shows. */
  active: boolean;
}

/** Saved-at text in the reader's locale, with the year the oracle reads. */
export function pageVersionTime(isoTimestamp: string): string {
  const atMs = Date.parse(isoTimestamp);
  if (Number.isNaN(atMs)) return isoTimestamp;
  return new Date(atMs).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

/** The snapshot owner's display name, or the neutral label once they left. */
function versionAuthor(members: readonly PageActorMember[], ownedBy: string): string {
  return members.find((member) => member.id === ownedBy)?.name ?? PAGE_DEACTIVATED_USER;
}

/** Newest first; an unparseable timestamp sorts last instead of garbage. */
function newestFirst(versions: readonly PageVersion[]): PageVersion[] {
  const rank = (version: PageVersion): number => {
    const atMs = Date.parse(version.last_saved_at);
    return Number.isNaN(atMs) ? Number.NEGATIVE_INFINITY : atMs;
  };
  return [...versions].sort((left, right) => rank(right) - rank(left));
}

export interface PageVersionEntriesInput {
  /** The history read; undefined while it is in flight. */
  versions: readonly PageVersion[] | undefined;
  /** The snapshot the address names, when the overlay is open. */
  activeVersionId: string | undefined;
  /** The workspace directory, for authors' display names. */
  members: readonly PageActorMember[] | undefined;
}

/**
 * The live row plus the dated snapshots. The live row leads and is the
 * active one whenever no snapshot is open; a snapshot row is active only
 * while its id is the one in the address (PAGE-051).
 */
export function pageVersionEntries(input: PageVersionEntriesInput): PageVersionEntry[] {
  const live: PageVersionEntry = {
    id: null,
    label: PAGE_CURRENT_VERSION_LABEL,
    author: "",
    active: input.activeVersionId === undefined,
  };
  if (input.versions === undefined) return [live];
  const snapshots = newestFirst(input.versions).map((version) => ({
    id: version.id,
    label: pageVersionTime(version.last_saved_at),
    author: input.members === undefined ? "" : versionAuthor(input.members, version.owned_by),
    active: version.id === input.activeVersionId,
  }));
  return [live, ...snapshots];
}
