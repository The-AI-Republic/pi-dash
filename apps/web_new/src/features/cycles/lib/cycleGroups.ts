// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles list grouping (parity/specs/cycles.md §2, CYC-001). Current
// cycles render through the hero, never inside a group; upcoming and
// draft rows share the upcoming group, finished rows take the completed
// group. Lifecycle markers compare case-insensitively, and a missing
// marker reads as a draft, matching the list's server annotation.

import type { BadgeTone } from "@pidash/kit";

/** The two collapsible row groups on the live list. */
export type CycleListGroup = "upcoming" | "completed";

/**
 * Disclosure-button headings, singular: the parity driver matches each
 * group button by its heading prefix and reads the trailing count.
 */
export const CYCLE_GROUP_HEADINGS: Record<CycleListGroup, string> = {
  upcoming: "Upcoming cycle",
  completed: "Completed cycle",
};

/**
 * The group a row belongs to, or null for current cycles, which the
 * hero renders instead of repeating in a group.
 */
export function groupOfCycle(status: string | undefined): CycleListGroup | null {
  const marker = (status ?? "").toLowerCase();
  if (marker === "completed") return "completed";
  if (marker === "current") return null;
  return "upcoming";
}

const LIFECYCLE_TONE: Record<string, BadgeTone> = {
  current: "accent",
  upcoming: "warning",
  completed: "success",
};

/**
 * The marker tone for a lifecycle marker; drafts and unknown markers
 * read neutral.
 */
export function lifecycleTone(status: string | undefined): BadgeTone {
  return LIFECYCLE_TONE[(status ?? "").toLowerCase()] ?? "neutral";
}

/**
 * The row marker word for a lifecycle marker. The words match the
 * filter labels of spec §4 so rows and filters name each lifecycle
 * the same way; a missing marker reads as a draft.
 */
export function lifecycleLabel(status: string | undefined): string {
  switch ((status ?? "").toLowerCase()) {
    case "current":
      return "In progress";
    case "upcoming":
      return "Yet to start";
    case "completed":
      return "Completed";
    default:
      return "Draft";
  }
}
