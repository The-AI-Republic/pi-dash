// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Display options matrix (parity/specs/issues.md §9, ISS-075–081). Which
// panel sections and options the issues surface offers per layout, plus
// the visibility rules layouts share: the kanban cross-hide between the
// grouping axes, cycle/module suppression while their project features
// are off, and the absent-key reads (pills shown, switches off). The
// area gate drives the panel by visible labels; layouts read the same
// matrix to interpret the stored keys.

import type { LayoutKind } from "../layouts/kinds.js";

/** Panel sections in their fixed render order. */
export type DisplaySection = "properties" | "group" | "subGroup" | "order" | "options";

/** Fixed panel order: properties, grouping, sub-grouping, order, switches. */
export const DISPLAY_SECTION_ORDER: readonly DisplaySection[] = ["properties", "group", "subGroup", "order", "options"];

/** Section headings. The switches block keeps its heading; the old app left it bare. */
export const DISPLAY_SECTION_TITLES: Record<DisplaySection, string> = {
  properties: "Display Properties",
  group: "Group by",
  subGroup: "Sub-group by",
  order: "Order by",
  options: "Options",
};

/** The two persisted switches; stored under these display_filters keys. */
export type DisplaySwitchKey = "show_empty_groups" | "sub_issue";

export const DISPLAY_SWITCH_LABELS: Record<DisplaySwitchKey, string> = {
  show_empty_groups: "Show empty groups",
  sub_issue: "Show sub-work items",
};

/** Grouping labels by stored dimension key. */
export const GROUP_DIMENSION_LABELS: Record<string, string> = {
  state: "States",
  priority: "Priority",
  cycle: "Cycle",
  module: "Module",
  labels: "Labels",
  assignees: "Assignees",
  created_by: "Created by",
};

/** Label for the ungrouped choice. */
export const GROUP_NONE_LABEL = "None";

/** Sort labels by stored order key. */
export const ORDER_OPTION_LABELS: Record<string, string> = {
  sort_order: "Manual",
  "-created_at": "Last created",
  "-updated_at": "Last updated",
  start_date: "Start date",
  "-priority": "Priority",
  target_date: "Due date",
};

/** Property pill labels by stored key, in render order. */
export const PROPERTY_PILL_LABELS: Record<string, string> = {
  key: "ID",
  assignee: "Assignee",
  start_date: "Start date",
  due_date: "Due date",
  labels: "Labels",
  priority: "Priority",
  state: "State",
  sub_issue_count: "Sub-work item count",
  attachment_count: "Attachment count",
  link: "Link",
  estimate: "Estimate",
  modules: "Module",
  cycle: "Cycle",
};

/** Epic surfaces relabel the sub-issue count pill. */
export const EPIC_SUB_ISSUE_COUNT_LABEL = "Work item count";

/** One layout's Display panel: null sections stay hidden. */
export interface LayoutDisplayMatrix {
  /** Pill keys in render order. */
  properties: readonly string[];
  /** Group dimensions in render order; a null entry clears the grouping. */
  groupBy: readonly (string | null)[] | null;
  /** Sub-group dimensions; kanban only, and only once grouped. */
  subGroupBy: readonly (string | null)[] | null;
  /** Sort keys in render order. */
  orderBy: readonly string[] | null;
  /** Switches in render order. */
  switches: readonly DisplaySwitchKey[];
}

const LIST_GROUP: readonly (string | null)[] = [
  "state",
  "priority",
  "cycle",
  "module",
  "labels",
  "assignees",
  "created_by",
  null,
];

const KANBAN_GROUP: readonly (string | null)[] = [
  "state",
  "priority",
  "cycle",
  "module",
  "labels",
  "assignees",
  "created_by",
];

const FULL_ORDERS: readonly string[] = [
  "sort_order",
  "-created_at",
  "-updated_at",
  "start_date",
  "-priority",
  "target_date",
];

const SHEET_ORDERS: readonly string[] = ["sort_order", "-created_at", "-updated_at", "start_date", "-priority"];

const ALL_PILLS: readonly string[] = [
  "key",
  "assignee",
  "start_date",
  "due_date",
  "labels",
  "priority",
  "state",
  "sub_issue_count",
  "attachment_count",
  "link",
  "estimate",
  "modules",
  "cycle",
];

/**
 * The issues-surface matrix per layout. The table-like layout keeps its
 * property pills: they edit the shared stored set, and the spreadsheet
 * columns follow them (ISS-016), so hiding the section would strand real
 * function. Other surfaces (archived, global, workspace) extend this
 * module with their slices.
 */
export function layoutDisplayMatrix(layout: LayoutKind): LayoutDisplayMatrix {
  switch (layout) {
    case "list":
      return {
        properties: ALL_PILLS,
        groupBy: LIST_GROUP,
        subGroupBy: null,
        orderBy: FULL_ORDERS,
        switches: ["sub_issue", "show_empty_groups"],
      };
    case "kanban":
      return {
        properties: ALL_PILLS,
        groupBy: KANBAN_GROUP,
        subGroupBy: LIST_GROUP,
        orderBy: FULL_ORDERS,
        switches: ["sub_issue", "show_empty_groups"],
      };
    case "calendar":
      return {
        properties: ["key"],
        groupBy: null,
        subGroupBy: null,
        orderBy: null,
        switches: ["sub_issue"],
      };
    case "spreadsheet":
      return {
        properties: ALL_PILLS,
        groupBy: null,
        subGroupBy: null,
        orderBy: SHEET_ORDERS,
        switches: ["sub_issue"],
      };
    case "gantt_chart":
      return {
        properties: ["key"],
        groupBy: null,
        subGroupBy: null,
        orderBy: SHEET_ORDERS,
        switches: ["sub_issue"],
      };
  }
}

/** Project feature gates behind the cycle/module suppression. */
export interface DisplayFeatureFlags {
  hideCycle: boolean;
  hideModule: boolean;
}

/**
 * Grouping dimensions after the kanban cross-hide and the feature
 * suppression. The hidden axis is the other axis' selection: it drops
 * out only when it names a real dimension, so the ungrouped choice
 * never hides itself.
 */
export function visibleGroupDimensions(
  dimensions: readonly (string | null)[],
  hidden: string | null,
  flags: DisplayFeatureFlags
): (string | null)[] {
  return dimensions.filter((dimension) => {
    if (hidden !== null && dimension === hidden) return false;
    if (dimension === "cycle" && flags.hideCycle) return false;
    if (dimension === "module" && flags.hideModule) return false;
    return true;
  });
}

/** Pill keys after the feature suppression. */
export function visiblePropertyKeys(keys: readonly string[], flags: DisplayFeatureFlags): string[] {
  return keys.filter((key) => {
    if (key === "cycle" && flags.hideCycle) return false;
    if (key === "modules" && flags.hideModule) return false;
    return true;
  });
}

/**
 * Whether a pill reads as shown. Absent keys read as shown: the reset
 * record carries an empty property map with every pill active.
 */
export function isPropertyShown(properties: Record<string, boolean>, key: string): boolean {
  return properties[key] !== false;
}

/** Pill label with the epic relabel applied. */
export function propertyPillLabel(key: string, isEpic: boolean): string {
  if (isEpic && key === "sub_issue_count") return EPIC_SUB_ISSUE_COUNT_LABEL;
  return PROPERTY_PILL_LABELS[key] ?? key;
}

/** Grouping label; the null dimension reads as the ungrouped choice. */
export function groupDimensionLabel(dimension: string | null): string {
  if (dimension === null) return GROUP_NONE_LABEL;
  return GROUP_DIMENSION_LABELS[dimension] ?? dimension;
}
