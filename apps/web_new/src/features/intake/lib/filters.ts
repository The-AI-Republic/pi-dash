// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The intake filter selection (parity/specs/intake.md INT-009, INT-010).
// One selection per tab session: the tab's default status set plus the
// user's own choices. Every toggle returns a new selection; the screen
// holds it, the panel edits it, and the list read derives its query
// from it. The status section never empties: unchecking the last
// remaining status is refused on both tabs.

import type { IntakeStatus } from "@pidash/api-client";

import type { IntakeListFilters } from "../api/keys.js";
import { intakeDateToken, todayUtcIsoDay, type IntakeDatePresetId, type IntakeDateValue } from "./dates.js";
import { orderStatusesForTab } from "./statuses.js";
import type { IntakeTab } from "./tabs.js";
import { INTAKE_TAB_STATUSES } from "./tabsModel.js";

/** Filter sections in panel order. */
export const INTAKE_FILTER_SECTIONS = [
  "status",
  "priority",
  "assignees",
  "createdBy",
  "labels",
  "createdAt",
  "updatedAt",
] as const;
export type IntakeFilterSection = (typeof INTAKE_FILTER_SECTIONS)[number];

/** Section titles in the panel and above the chip groups. */
export const INTAKE_FILTER_SECTION_TITLES: Readonly<Record<IntakeFilterSection, string>> = {
  status: "Status",
  priority: "Priority",
  assignees: "Assignees",
  createdBy: "Created by",
  labels: "Labels",
  createdAt: "Created date",
  updatedAt: "Updated date",
};

/** The two date sections, the only single-select ones. */
export type IntakeDateSection = "createdAt" | "updatedAt";

/** One tab session's filter selection. */
export interface IntakeFilterSelection {
  statuses: readonly IntakeStatus[];
  priorities: readonly string[];
  assignees: readonly string[];
  createdBy: readonly string[];
  labels: readonly string[];
  createdAt: IntakeDateValue | null;
  updatedAt: IntakeDateValue | null;
}

/**
 * A tab session's starting selection (INT-012): the tab's default
 * status set, everything else empty.
 */
export function defaultSelectionForTab(tab: IntakeTab): IntakeFilterSelection {
  return {
    statuses: [...INTAKE_TAB_STATUSES[tab]],
    priorities: [],
    assignees: [],
    createdBy: [],
    labels: [],
    createdAt: null,
    updatedAt: null,
  };
}

function toggleId(ids: readonly string[], id: string): readonly string[] {
  return ids.includes(id) ? ids.filter((kept) => kept !== id) : [...ids, id];
}

/**
 * Toggle a status. Refuses to empty the section: unchecking the last
 * remaining status returns the selection unchanged (INT-009).
 */
export function toggleFilterStatus(selection: IntakeFilterSelection, status: IntakeStatus): IntakeFilterSelection {
  if (selection.statuses.includes(status)) {
    if (selection.statuses.length === 1) return selection;
    return { ...selection, statuses: selection.statuses.filter((kept) => kept !== status) };
  }
  return { ...selection, statuses: [...selection.statuses, status] };
}

/** Toggle a priority, assignee, creator or label id. */
export function toggleFilterId(
  selection: IntakeFilterSelection,
  section: "priorities" | "assignees" | "createdBy" | "labels",
  id: string
): IntakeFilterSelection {
  return { ...selection, [section]: toggleId(selection[section], id) };
}

/**
 * Pick a date preset. Choosing the checked preset clears the section;
 * any other pick replaces the section's value (INT-009/010).
 */
export function pickDatePreset(
  selection: IntakeFilterSelection,
  section: IntakeDateSection,
  preset: IntakeDatePresetId
): IntakeFilterSelection {
  const current = selection[section];
  const next: IntakeDateValue | null =
    current !== null && current.kind === "preset" && current.preset === preset ? null : { kind: "preset", preset };
  return { ...selection, [section]: next };
}

/** Apply an explicit range, replacing the section's value. */
export function applyCustomDate(
  selection: IntakeFilterSelection,
  section: IntakeDateSection,
  value: IntakeDateValue
): IntakeFilterSelection {
  return { ...selection, [section]: value };
}

/** Clear one group through its chips. Status has no clear-all. */
export function clearFilterGroup(
  selection: IntakeFilterSelection,
  section: Exclude<IntakeFilterSection, "status">
): IntakeFilterSelection {
  switch (section) {
    case "priority":
      return { ...selection, priorities: [] };
    case "assignees":
      return { ...selection, assignees: [] };
    case "createdBy":
      return { ...selection, createdBy: [] };
    case "labels":
      return { ...selection, labels: [] };
    case "createdAt":
      return { ...selection, createdAt: null };
    case "updatedAt":
      return { ...selection, updatedAt: null };
  }
}

/**
 * The running applied count: the selected values of every section,
 * including the tab-default status (INT-009). Display state only; the
 * empty-state decision reads `filterActivity` instead.
 */
export function appliedFilterCount(selection: IntakeFilterSelection): number {
  return (
    selection.statuses.length +
    selection.priorities.length +
    selection.assignees.length +
    selection.createdBy.length +
    selection.labels.length +
    (selection.createdAt === null ? 0 : 1) +
    (selection.updatedAt === null ? 0 : 1)
  );
}

export interface IntakeFilterActivity {
  /** Values the user chose outside the status section. */
  userFilterCount: number;
  /** Whether the status set still equals the tab default. */
  statusNarrowed: boolean;
}

/**
 * The "filters are active" signal the empty states read (INT-008): the
 * user's own choices, never the tab default. An untouched tab reports
 * no activity even though its status section counts one.
 */
export function filterActivity(selection: IntakeFilterSelection, tab: IntakeTab): IntakeFilterActivity {
  const defaults = INTAKE_TAB_STATUSES[tab];
  const narrowed =
    selection.statuses.length !== defaults.length || selection.statuses.some((status) => !defaults.includes(status));
  return {
    userFilterCount:
      selection.priorities.length +
      selection.assignees.length +
      selection.createdBy.length +
      selection.labels.length +
      (selection.createdAt === null ? 0 : 1) +
      (selection.updatedAt === null ? 0 : 1),
    statusNarrowed: narrowed,
  };
}

/**
 * The list read's filters for a selection: the status set in the tab's
 * wire order, the multi-value sections comma-ready, and each date as
 * its encoded range. Empty sections stay absent so the read sends no
 * empty parameter.
 */
export function selectionToListFilters(
  tab: IntakeTab,
  selection: IntakeFilterSelection,
  today: string = todayUtcIsoDay()
): IntakeListFilters {
  return {
    statuses: orderStatusesForTab(tab, selection.statuses),
    ...(selection.priorities.length === 0 ? {} : { priorities: [...selection.priorities] }),
    ...(selection.assignees.length === 0 ? {} : { assignees: [...selection.assignees] }),
    ...(selection.createdBy.length === 0 ? {} : { createdBy: [...selection.createdBy] }),
    ...(selection.labels.length === 0 ? {} : { labels: [...selection.labels] }),
    ...(selection.createdAt === null ? {} : { createdAt: intakeDateToken(selection.createdAt, today) }),
    ...(selection.updatedAt === null ? {} : { updatedAt: intakeDateToken(selection.updatedAt, today) }),
  };
}
