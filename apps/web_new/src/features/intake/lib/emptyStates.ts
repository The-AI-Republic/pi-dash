// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List empty-state selection (parity/specs/intake.md INT-008, bug
// NEWFRONT-262). Three situations must render differently: filters that
// match nothing, an untouched Open tab with no requests, and an
// untouched Closed tab with nothing resolved. The tab's default status
// set alone never counts as a user filter — it is a real query value
// and chip, but the decision below reads the user's own choices.

import type { IntakeTab } from "./tabs.js";

export type IntakeEmptyState = "filtered" | "open-guidance" | "closed-informational";

export interface IntakeEmptyInput {
  tab: IntakeTab;
  /** The tab's list finished with no rows. */
  rowCount: number;
  /** Filters the user added or removed on top of the tab default. */
  userFilterCount: number;
  /** A filter panel selection replaced the tab default status set. */
  statusNarrowed: boolean;
}

/** Which empty state the list pane shows while `rowCount` is zero. */
export function selectIntakeEmptyState({ tab, userFilterCount, statusNarrowed }: IntakeEmptyInput): IntakeEmptyState {
  if (userFilterCount > 0 || statusNarrowed) return "filtered";
  return tab === "open" ? "open-guidance" : "closed-informational";
}
