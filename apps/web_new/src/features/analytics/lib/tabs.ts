// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics tab model (parity/specs/analytics.md §1, ANL-001). Pure:
// the two tab ids in display order, their strip labels, address
// helpers, and unknown-segment resolution. Components and routes
// build on these; nothing here touches React, the router, or the
// network.

/** Tab ids in strip order; the first is the default landing. */
export const ANALYTICS_TAB_IDS = ["overview", "work-items"] as const;

/** One known analytics tab id. */
export type AnalyticsTabId = (typeof ANALYTICS_TAB_IDS)[number];

/** The tab the bare analytics address forwards to. */
export const DEFAULT_ANALYTICS_TAB: AnalyticsTabId = "overview";

/** Strip labels for the known tabs. */
export const ANALYTICS_TAB_LABELS: Record<AnalyticsTabId, string> = {
  overview: "Overview",
  "work-items": "Work items",
};

/** True for the two ids the strip offers. */
export function isAnalyticsTabId(value: string): value is AnalyticsTabId {
  return (ANALYTICS_TAB_IDS as readonly string[]).includes(value);
}

/**
 * The tab a path segment selects, or null for an unknown segment.
 * Unknown segments stay in the address and select nothing (ANL-001).
 */
export function selectAnalyticsTab(tabId: string): AnalyticsTabId | null {
  return isAnalyticsTabId(tabId) ? tabId : null;
}

/** The shareable address of one tab. */
export function analyticsTabHref(workspaceSlug: string, tabId: string): string {
  return `/${workspaceSlug}/analytics/${tabId}`;
}

/** The default tab's address, carrying the query string verbatim. */
export function defaultAnalyticsTabHref(workspaceSlug: string, search: string): string {
  return `${analyticsTabHref(workspaceSlug, DEFAULT_ANALYTICS_TAB)}${search}`;
}
