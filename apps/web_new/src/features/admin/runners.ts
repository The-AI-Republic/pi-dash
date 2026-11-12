// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners page pure helpers (ADM-080, from parity/specs/admin.md
// §11). The runners table, the provisioning filter, the total count,
// and the pager read from here. The old app has no runners surface
// (bug: NEWFRONT-494 — no sidebar entry, no page, endpoint 404), so
// no string here is oracle-pinned: the copy is fresh, and the
// NEWFRONT-521 gate pins it against the new app.
import { RUNNER_PROVISIONINGS } from "@pidash/api-client";

/** Rows per runners page, fixed by the endpoint (spec §11.1). */
export const RUNNERS_PAGE_SIZE = 50;

/** Page heading; the suite pins its presence, not its wording. */
export const RUNNERS_HEADING = "Runners";

/** Page description; the suite pins its presence, not its wording. */
export const RUNNERS_DESCRIPTION = "Every runner enrolled on this instance, newest first.";

/** Table column headers, left to right (spec §11.1). */
export const RUNNERS_HEADERS: readonly string[] = [
  "Name",
  "Workspace",
  "Owner",
  "Provisioning",
  "Status",
  "Runner version",
  "Codex version",
  "Last heartbeat",
];

/** Empty-table message row (spec §11.1). */
export const RUNNERS_EMPTY_TEXT = "No runners found.";

/** Cell text for an absent version, slug, owner, or heartbeat (spec §11.1). */
export const RUNNERS_MISSING_TEXT = "—";

/** Provisioning filter label (spec §11.1). */
export const RUNNERS_FILTER_LABEL = "Provisioning";

/** Provisioning option label for the blank (unfiltered) value (spec §11.1). */
export const RUNNERS_FILTER_ALL_LABEL = "all";

/**
 * Provisioning filter option values, blank first for all. The fixed
 * set rides the api-client runner contract; the blank is the page's
 * unfiltered state, which the contract drops from the URL (the
 * backend rejects a present-but-empty provisioning with 400).
 */
export const RUNNERS_FILTER_OPTIONS: readonly string[] = ["", ...RUNNER_PROVISIONINGS];

/** Skeleton bar count and height while first loading (spec §11.1). */
export const RUNNERS_SKELETON_ROWS = 2;
export const RUNNERS_SKELETON_BAR_HEIGHT = "48px";

/** Pager button labels (spec §11.1). */
export const RUNNERS_PREV_LABEL = "Previous";
export const RUNNERS_NEXT_LABEL = "Next";

/** Provisioning option label: the blank value reads as all (spec §11.1). */
export function formatProvisioningOptionLabel(provisioning: string): string {
  return provisioning === "" ? RUNNERS_FILTER_ALL_LABEL : provisioning;
}

/**
 * Version, slug, or owner cell: the value, or the missing marker when
 * absent. An empty version string counts as absent — the backend
 * stores an unset runner version as "".
 */
export function formatRunnerCellText(value: string | null): string {
  return value ? value : RUNNERS_MISSING_TEXT;
}

/** Heartbeat cell: the localized timestamp, or the missing marker when unset (spec §11.1). */
export function formatRunnerHeartbeat(lastHeartbeatAt: string | null): string {
  return lastHeartbeatAt ? new Date(lastHeartbeatAt).toLocaleString() : RUNNERS_MISSING_TEXT;
}

/** Total-count line beside the filter (spec §11.1). */
export function formatRunnerTotal(total: number): string {
  return total === 1 ? "1 runner" : `${total} runners`;
}

/** Page count for a total at fifty rows per page. */
export function runnersPageCount(total: number): number {
  return Math.ceil(total / RUNNERS_PAGE_SIZE);
}

/** Pager label carrying the current page number (spec §11.1). */
export function formatRunnersPageLabel(page: number): string {
  return `Page ${page}`;
}
