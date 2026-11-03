// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row and card identity rules (parity/specs/modules.md §3.5, §3.6, §5,
// MOD-012, MOD-019, MOD-020). Pure selectors keep three decisions out of
// the components: which marker the progress ring carries, how the work-item
// mix splits into bar segments, and whether the lead slot shows an avatar
// or the no-lead marker. Nothing here touches the network.

import type { ModuleRow, WorkspaceMember } from "@pidash/api-client";

/** The three shapes a row's progress marker can take (§3.5). */
export type ModuleProgressMarker = { kind: "check" } | { kind: "alert" } | { kind: "percent"; percent: number };

/**
 * The row's progress marker. A wholly complete module shows a check; a
 * module marked completed while its work items fall short shows the alert;
 * cancelled is deliberately not an alert — it reports its percentage.
 * The percentage counts completed and cancelled items over the total, the
 * same share the ordering uses.
 */
export function moduleProgressMarker(row: ModuleRow): ModuleProgressMarker {
  const total = row.total_issues;
  const done = row.completed_issues + row.cancelled_issues;
  if (total > 0 && done >= total) return { kind: "check" };
  if (row.status === "completed") return { kind: "alert" };
  return { kind: "percent", percent: modulePercentFloor(row) };
}

/** Floored completion share, 0 when the module has no work items. */
export function modulePercentFloor(row: Pick<ModuleRow, "completed_issues" | "cancelled_issues" | "total_issues">) {
  if (row.total_issues <= 0) return 0;
  const done = row.completed_issues + row.cancelled_issues;
  return Math.min(100, Math.max(0, Math.floor((done / row.total_issues) * 100)));
}

/** One colored slice of the card's segmented progress bar. */
export interface ModuleProgressSegment {
  group: ModuleStateSegmentGroup;
  label: string;
  count: number;
  /** Width as a percent of the module's whole work-item set. */
  width: number;
  color: string;
}

/** The state groups the segmented bar can show, in reading order. */
export const MODULE_SEGMENT_GROUPS = ["completed", "started", "unstarted", "backlog", "cancelled"] as const;
export type ModuleStateSegmentGroup = (typeof MODULE_SEGMENT_GROUPS)[number];

/**
 * Fixed hues per segment group. The card bar is a small decorative chart,
 * so it keeps its own palette instead of borrowing status or theme hues
 * (a module's lifecycle status is a separate signal above it).
 */
export const MODULE_SEGMENT_COLORS: Record<ModuleStateSegmentGroup, string> = {
  completed: "#16a34a",
  started: "#f59e0b",
  unstarted: "#6b7280",
  backlog: "#a3a3a3",
  cancelled: "#ef4444",
};

export const MODULE_SEGMENT_LABELS: Record<ModuleStateSegmentGroup, string> = {
  completed: "Completed",
  started: "Started",
  unstarted: "Unstarted",
  backlog: "Backlog",
  cancelled: "Cancelled",
};

type SegmentCountField =
  | "completed_issues"
  | "started_issues"
  | "unstarted_issues"
  | "backlog_issues"
  | "cancelled_issues";

const SEGMENT_COUNT_FIELD: Record<ModuleStateSegmentGroup, SegmentCountField> = {
  completed: "completed_issues",
  started: "started_issues",
  unstarted: "unstarted_issues",
  backlog: "backlog_issues",
  cancelled: "cancelled_issues",
};

/**
 * Non-zero slices of the card's segmented bar, in reading order, with
 * widths as a share of the module's total work items. A group with no work
 * items is omitted entirely — an empty module renders an empty bar rather
 * than a row of zero-width slivers. Counts are clamped to the total so a
 * stale annotation can never overflow the bar.
 */
export function moduleProgressSegments(
  row: Pick<
    ModuleRow,
    "total_issues" | "completed_issues" | "started_issues" | "unstarted_issues" | "backlog_issues" | "cancelled_issues"
  >
): ModuleProgressSegment[] {
  const total = row.total_issues;
  if (total <= 0) return [];
  const segments: ModuleProgressSegment[] = [];
  for (const group of MODULE_SEGMENT_GROUPS) {
    const raw = row[SEGMENT_COUNT_FIELD[group]];
    const count = typeof raw === "number" && Number.isFinite(raw) ? Math.max(0, raw) : 0;
    if (count <= 0) continue;
    segments.push({
      group,
      label: MODULE_SEGMENT_LABELS[group],
      count,
      width: Math.min(100, (count / total) * 100),
      color: MODULE_SEGMENT_COLORS[group],
    });
  }
  return segments;
}

/** What the card's lead slot holds (§3.6): a person, or the no-lead mark. */
export type ModuleLeadSlot = { kind: "avatar"; memberId: string } | { kind: "none" };

export function moduleLeadSlot(row: Pick<ModuleRow, "lead_id">): ModuleLeadSlot {
  return row.lead_id === null ? { kind: "none" } : { kind: "avatar", memberId: row.lead_id };
}

/** The lead as a card can show it: the person's name and picture. */
export interface ModuleLeadPerson {
  name: string;
  avatarUrl: string | null;
}

/** A resolved lead slot: the person, or the reason the mark is the no-lead one. */
export type ResolvedModuleLead = { kind: "avatar"; person: ModuleLeadPerson } | { kind: "none" };

/**
 * Resolve a module's lead against the workspace directory (§4.3). A module
 * without a lead, or one whose lead is no longer in the directory the viewer
 * can read, shows the no-lead mark — the card never renders an empty avatar
 * or an id as a name.
 */
export function resolveModuleLead(
  row: Pick<ModuleRow, "lead_id">,
  members: readonly WorkspaceMember[] | undefined
): ResolvedModuleLead {
  const slot = moduleLeadSlot(row);
  if (slot.kind === "none") return { kind: "none" };
  const entry = members?.find((membership) => membership.member.id === slot.memberId);
  if (entry === undefined) return { kind: "none" };
  return { kind: "avatar", person: { name: entry.member.display_name, avatarUrl: entry.member.avatar_url } };
}

/** The card's done-over-total count text, e.g. `1/2 work items`. */
export function moduleWorkItemCountText(row: Pick<ModuleRow, "completed_issues" | "total_issues">): string {
  if (row.total_issues <= 0) return "0 work items";
  const noun = row.total_issues === 1 ? "work item" : "work items";
  return `${row.completed_issues}/${row.total_issues} ${noun}`;
}
