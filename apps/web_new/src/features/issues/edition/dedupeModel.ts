// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Duplicate row shaping for the detail popover (ISS-231). Pure: the
// detail excludes the viewed item and caps the list, and the address is
// built from the hit's own project and sequence.

import type { IssueSearchHit } from "@pidash/api-client";

/** Most rows the popover lists; the rest stay behind the count. */
export const DUPLICATE_ROW_CAP = 5;

/** Near-duplicate rows for a viewed item, capped and without itself. */
export function duplicateRows(hits: readonly IssueSearchHit[], issueId: string): IssueSearchHit[] {
  return hits.filter((hit) => hit.id !== issueId).slice(0, DUPLICATE_ROW_CAP);
}

/** Browse address for a duplicate hit. */
export function duplicateHref(
  workspaceSlug: string,
  hit: Pick<IssueSearchHit, "project__identifier" | "sequence_id">
): string {
  return `/${workspaceSlug}/browse/${hit.project__identifier}-${hit.sequence_id}`;
}
