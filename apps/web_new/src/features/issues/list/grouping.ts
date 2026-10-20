// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List grouping model (parity/specs/issues.md §2, ISS-007/009). Maps the
// stored group dimension to the server's group key, then turns a grouped
// envelope's buckets into ordered sections. Section order follows the
// server's declared group list (states in workflow order, priorities in
// the fixed severity order) with the "None" bucket last; the flat list
// always has the single section the oracle names.

import type { GroupedIssueListResponse, GroupedIssueRow, IssueGroupBucket, IssueListItem } from "@pidash/api-client";

import { GROUP_NONE_LABEL } from "../display/matrix.js";

/** One rendered section: key, title, rows, total and add metadata. */
export interface IssueGroupSection {
  /** Server bucket key (`state_id`, `labels__id`, `None`, ...). */
  key: string;
  title: string;
  rows: IssueListItem[];
  /** The group's full count, not just this page. */
  total: number;
  /** Grouping value for quick-add/create prefill, when there is one. */
  value: string | null;
}

/** Display dimension → server group_by key. */
const GROUP_KEY_BY_DIMENSION: Record<string, string> = {
  state: "state_id",
  priority: "priority",
  labels: "labels__id",
  assignees: "assignees__id",
  cycle: "cycle_id",
  module: "issue_module__module_id",
  project: "project_id",
  team_project: "project_id",
  created_by: "created_by",
  "state_detail.group": "state__group",
};

/** The server group key for a display dimension, or null for flat. */
export function serverGroupBy(dimension: string | null): string | null {
  if (dimension === null) return null;
  return GROUP_KEY_BY_DIMENSION[dimension] ?? null;
}

/** The trailing bucket key the server uses for empty values. */
export const NONE_GROUP_KEY = "None";

/** Title for the ungrouped list (the oracle pins this exact text). */
export const FLAT_GROUP_TITLE = "All work items";

/** Human titles for fixed-value dimensions whose buckets carry no names. */
const PRIORITY_TITLES: Record<string, string> = {
  urgent: "Urgent",
  high: "High",
  medium: "Medium",
  low: "Low",
  none: "None",
};

/** Workflow-group titles; the cancelled group reads "Canceled". */
const STATE_GROUP_TITLES: Record<string, string> = {
  backlog: "Backlog",
  unstarted: "Unstarted",
  started: "Started",
  review: "Review",
  test: "Test",
  completed: "Completed",
  cancelled: "Canceled",
};

/** Grouping dimensions whose sections cannot create work items. */
export function creationDisabledForGroup(dimension: string | null): boolean {
  return dimension === "created_by";
}

/** Inputs behind the inline quick-add gate (ISS-011). */
export interface QuickAddGateInput {
  canEdit: boolean;
  completedCycle: boolean;
  /** Completed module contexts are read-only too. */
  completedModule?: boolean | undefined;
  /** The entity's view opts into inline creation. */
  enableQuickAdd: boolean;
  /** Edition workflow rule reason; OSS never supplies one. */
  workflowBlockedReason?: string | undefined;
  groupBy: string | null;
}

/**
 * Why inline quick-add is unavailable, or undefined when it is offered.
 * Permission and completed-cycle reasons win over edition and grouping
 * rules so the reader sees the most fundamental constraint first.
 */
export function quickAddDisabledReason(input: QuickAddGateInput): string | undefined {
  if (!input.canEdit) return "New work items are read-only for you";
  if (input.completedCycle) return "This cycle is complete";
  if (input.completedModule ?? false) return "This module is complete";
  if (!input.enableQuickAdd) return "Quick add is off in this view";
  if (input.workflowBlockedReason !== undefined) return input.workflowBlockedReason;
  if (creationDisabledForGroup(input.groupBy)) return "Creation is off in this grouping";
  return undefined;
}

function isNoneKey(key: string): boolean {
  return key === NONE_GROUP_KEY;
}

/** Fixed titles a dimension's buckets carry when they are not ids. */
export function defaultGroupTitle(dimension: string | null, key: string): string | undefined {
  if (dimension === "priority") return PRIORITY_TITLES[key];
  if (dimension === "state_detail.group") return STATE_GROUP_TITLES[key];
  return undefined;
}

function unknownGroupTitle(key: string, dimension: string | null): string {
  // The server sends "None" for empty values; anything else resolves
  // through the caller's known label map or the dimension's fixed
  // titles, and an unresolved id never renders raw: it reads as an
  // empty-value bucket.
  if (isNoneKey(key)) return GROUP_NONE_LABEL;
  return defaultGroupTitle(dimension, key) ?? key;
}

/**
 * Bucket order. Known order lists carry ids for states/cycles/modules/
 * labels/members, so the server's own group-field order is the truth;
 * `knownOrder` is the explicit id sequence (empty when unavailable),
 * and anything absent follows in envelope order with "None" last.
 */
export function orderGroupKeys(keys: readonly string[]): string[] {
  const named = keys.filter((key) => !isNoneKey(key));
  const none = keys.filter(isNoneKey);
  return [...named, ...none];
}

/** Known label for a group key, falling back to the empty-value label. */
export function groupTitle(
  key: string,
  labels: ReadonlyMap<string, string>,
  options: { titleFallback?: string; dimension?: string | null } = {}
): string {
  if (key === NONE_GROUP_KEY) return GROUP_NONE_LABEL;
  const known = labels.get(key);
  if (known !== undefined) return known;
  return options.titleFallback ?? unknownGroupTitle(key, options.dimension ?? null);
}

/**
 * Grouped rows replace the m2m arrays with the raw join key for the
 * grouping dimension, and omit the other arrays. The property strip and
 * row key read the list shape, so fold the join key back into its array
 * and default absent arrays to empty.
 */
export function normalizeGroupedRow(row: GroupedIssueRow, dimension: string | null): IssueListItem {
  const labelIds = row.label_ids ?? (row.labels__id === null || row.labels__id === undefined ? [] : [row.labels__id]);
  const assigneeIds =
    row.assignee_ids ?? (row.assignees__id === null || row.assignees__id === undefined ? [] : [row.assignees__id]);
  const moduleIds =
    row.module_ids ??
    (row.issue_module__module_id === null || row.issue_module__module_id === undefined
      ? []
      : [row.issue_module__module_id]);
  // A team-project bucket carries the plain `project_id`; fold it into the
  // same shape so the strip and row keys read it like any other list row.
  const projectId = dimension === "team_project" && row.project_id !== undefined ? row.project_id : undefined;
  return {
    ...row,
    label_ids: labelIds,
    assignee_ids: assigneeIds,
    module_ids: moduleIds,
    ...(projectId === undefined ? {} : { project_id: projectId }),
  };
}

/** One flat section holding every row. */
export function flatSection(rows: readonly IssueListItem[], total: number): IssueGroupSection {
  return {
    key: "all",
    title: FLAT_GROUP_TITLE,
    rows: [...rows],
    total,
    value: null,
  };
}

/**
 * Convert a grouped envelope into sections. `labels` resolves ids to
 * display names; unknown ids stay out of the title fallback so a
 * deleted label never renders a raw uuid.
 */
export function groupedSections(
  response: Pick<GroupedIssueListResponse, "results">,
  labels: ReadonlyMap<string, string>,
  options: { showEmptyGroups: boolean; dimension?: string | null }
): IssueGroupSection[] {
  const keys = orderGroupKeys(Object.keys(response.results));
  const sections: IssueGroupSection[] = [];
  for (const key of keys) {
    const bucket = response.results[key] as IssueGroupBucket | undefined;
    if (bucket === undefined) continue;
    const total = bucket.total_results;
    if (!options.showEmptyGroups && total === 0) continue;
    sections.push({
      key,
      title: groupTitle(key, labels, { dimension: options.dimension ?? null }),
      rows: bucket.results.map((row) => normalizeGroupedRow(row, options.dimension ?? null)),
      total,
      value: isNoneKey(key) ? null : key,
    });
  }
  return sections;
}

/** Merge the next grouped page into the current sections. */
export function mergeGroupedPage(
  current: readonly IssueGroupSection[],
  incoming: readonly IssueGroupSection[]
): IssueGroupSection[] {
  const byKey = new Map(current.map((section) => [section.key, section]));
  for (const section of incoming) {
    const existing = byKey.get(section.key);
    if (existing === undefined) {
      byKey.set(section.key, section);
      continue;
    }
    const seen = new Set(existing.rows.map((row) => row.id));
    byKey.set(section.key, {
      ...existing,
      rows: [...existing.rows, ...section.rows.filter((row) => !seen.has(row.id))],
      total: section.total,
    });
  }
  // Keep the server's discovered order: incoming keys extend the tail.
  const order = orderGroupKeys([...byKey.keys()]);
  return order.map((key) => byKey.get(key)!).filter(Boolean);
}

/** Whether a section still has unseen rows. */
export function sectionHasMore(section: IssueGroupSection): boolean {
  return section.rows.length < section.total;
}

/** Cursor that asks the server for one section's next page. */
export function sectionNextCursor(section: IssueGroupSection, pageSize: number): string | null {
  if (!sectionHasMore(section)) return null;
  return `${pageSize}:${Math.floor(section.rows.length / pageSize)}:0`;
}

/** A section's grouping value for quick-add/create payloads. */
export function sectionPrefill(
  dimension: string | null,
  section: IssueGroupSection
): Record<string, string | string[]> {
  if (dimension === null || section.value === null) return {};
  switch (dimension) {
    case "state":
      return { state_id: section.value };
    case "priority":
      return { priority: section.value };
    case "labels":
      return { label_ids: [section.value] };
    case "assignees":
      return { assignee_ids: [section.value] };
    case "cycle":
      return { cycle_id: section.value };
    case "module":
      return { module_ids: [section.value] };
    default:
      return {};
  }
}
