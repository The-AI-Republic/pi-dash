// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics query keys (parity/specs/analytics.md §16.6). Every key
// starts with the workspace scope so a workspace switch or sign-out
// clears one subtree; the picker selection is part of every key so
// each scope caches separately.

import type { AnalyticsChartQuery, AnalyticsXAxis, AnalyticsYAxis } from "@pidash/api-client";

import { workspaceScope } from "../../../core/query/client.js";

import type { AnalyticsTabId } from "../lib/tabs.js";

/** Charts reads, keyed by graph kind (spec §16.6). */
export type AnalyticsChartKind = AnalyticsChartQuery["type"];

export const analyticsKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "analytics"] as const,
  /** One tab's headline totals under one normalized project scope. */
  totals: (workspaceSlug: string, tab: AnalyticsTabId, projectIds: readonly string[]) =>
    [...analyticsKeys.all(workspaceSlug), "totals", tab, projectIds] as const,
  /** One charts read under one normalized project scope. */
  charts: (workspaceSlug: string, kind: AnalyticsChartKind, projectIds: readonly string[]) =>
    [...analyticsKeys.all(workspaceSlug), "charts", kind, projectIds] as const,
  /**
   * One custom-graph read under one normalized project scope and one
   * axis choice (spec §16.6; NEWFRONT-733). The axes ride the key so
   * every re-pick caches separately; an absent split keys as null.
   */
  customCharts: (
    workspaceSlug: string,
    projectIds: readonly string[],
    xAxis: AnalyticsXAxis,
    yAxis: AnalyticsYAxis,
    groupBy: AnalyticsXAxis | null
  ) => [...analyticsKeys.all(workspaceSlug), "charts", "custom-work-items", projectIds, xAxis, yAxis, groupBy] as const,
  /** State breakdown under one normalized project scope. */
  stats: (workspaceSlug: string, projectIds: readonly string[]) =>
    [...analyticsKeys.all(workspaceSlug), "stats", "work-items", projectIds] as const,
  /** Per-project stats under one normalized project scope. */
  projectStats: (workspaceSlug: string, projectIds: readonly string[]) =>
    [...analyticsKeys.all(workspaceSlug), "project-stats", projectIds] as const,
};
