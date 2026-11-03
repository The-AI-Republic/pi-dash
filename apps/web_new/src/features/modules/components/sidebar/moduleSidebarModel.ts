// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module analytics sidebar rules (parity/specs/modules.md §4.2, §9.1,
// MOD-033). Pure selectors keep the status wording, the points gate, the
// count text and the editability decision out of the component so each
// branch is testable without a browser. The points row is present only
// when the project's active estimate system is the points family; a time
// or categories system is not a points system and renders no row.

import type { EstimateList, ModuleStatus } from "@pidash/api-client";

import { selectActiveSystem } from "../../../estimates/index.js";

export const MODULE_STATUS_LABELS: Record<ModuleStatus, string> = {
  backlog: "Backlog",
  planned: "Planned",
  "in-progress": "In Progress",
  paused: "Paused",
  completed: "Completed",
  cancelled: "Cancelled",
};

/** The pinned success notice for a module field edit (§4). */
export const MODULE_SAVE_SUCCESS = "Module updated successfully.";

/** The fallback sentence when a refusal carries no detail. */
export const MODULE_SAVE_FAILED_BODY = "Check your connection and try again.";

export interface ModuleSidebarCounts {
  completed: number;
  total: number;
}

/** `completed/total`, with missing values read as zero. */
export function moduleCountText(completed: number | undefined, total: number | undefined): string {
  return `${completed ?? 0}/${total ?? 0}`;
}

/**
 * Whether the sidebar may write at all. A guest, an archived module and
 * the peek's own read-only flag all close the same gate; the individual
 * controls continue to render their values.
 */
export function moduleSidebarEditable(input: { readOnly: boolean; archived: boolean; canEdit: boolean }): boolean {
  return input.canEdit && !input.readOnly && !input.archived;
}

/**
 * Whether the points row belongs on this sidebar. Only an active points
 * system turns it on; an absent list, no active system, or a time or
 * categories system keeps it absent (not a zero row).
 */
export function showsModulePoints(estimates: EstimateList | undefined): boolean {
  if (estimates === undefined) return false;
  return selectActiveSystem(estimates)?.type === "points";
}
