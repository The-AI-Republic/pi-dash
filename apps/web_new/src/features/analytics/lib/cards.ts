// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics headline cards (parity/specs/analytics.md §§5, 8,
// ANL-005, ANL-008). Pure: the per-tab card catalogues in display
// order and the missing-metric rule. Components build on these;
// nothing here touches React, the router, or the network.

import type { AnalyticsTabId } from "./tabs.js";

/** One headline card: its totals key and rendered label. */
export interface HeadlineCardDef {
  key: string;
  label: string;
}

/** Overview grid in display order: people, projects, work items, cycles, intake, agent tokens. */
export const OVERVIEW_HEADLINE_CARDS: readonly HeadlineCardDef[] = [
  { key: "total_users", label: "Total Users" },
  { key: "total_admins", label: "Total Admins" },
  { key: "total_members", label: "Total Members" },
  { key: "total_guests", label: "Total Guests" },
  { key: "total_projects", label: "Total Projects" },
  { key: "total_work_items", label: "Total Work items" },
  { key: "total_cycles", label: "Total Cycles" },
  { key: "total_intake", label: "Total Intake" },
  { key: "agent_run_input_tokens", label: "Agent input tokens" },
  { key: "agent_run_output_tokens", label: "Agent output tokens" },
  { key: "agent_run_total_tokens", label: "Agent total tokens" },
];

/** Work-items grid in display order: overall, then workflow progress. */
export const WORK_ITEMS_HEADLINE_CARDS: readonly HeadlineCardDef[] = [
  { key: "total_work_items", label: "Total Work items" },
  { key: "started_work_items", label: "Started Work items" },
  { key: "backlog_work_items", label: "Backlog Work items" },
  { key: "un_started_work_items", label: "Unstarted Work items" },
  { key: "completed_work_items", label: "Completed Work items" },
];

/** The headline grid one tab renders. */
export function headlineCardsForTab(tab: AnalyticsTabId): readonly HeadlineCardDef[] {
  return tab === "work-items" ? WORK_ITEMS_HEADLINE_CARDS : OVERVIEW_HEADLINE_CARDS;
}

/**
 * One card's value from a totals payload. Each metric is shaped
 * `{ count: number }`; a missing key or a non-numeric count reads
 * as zero rather than crashing the grid.
 */
export function selectHeadlineValue(totals: unknown, key: string): number {
  if (typeof totals !== "object" || totals === null) return 0;
  const metric = (totals as Record<string, unknown>)[key];
  if (typeof metric !== "object" || metric === null) return 0;
  const count = (metric as Record<string, unknown>)["count"];
  return typeof count === "number" && Number.isFinite(count) ? count : 0;
}
