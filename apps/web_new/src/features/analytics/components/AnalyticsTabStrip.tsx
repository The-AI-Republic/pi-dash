// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics tab strip (parity/specs/analytics.md §1, ANL-001). The two
// tabs as address links with tab roles; the active marker comes from
// the path segment, and an unknown segment selects nothing while the
// strip keeps offering both tabs. Tab switches carry the picker scope.

import { Link } from "@tanstack/react-router";
import * as React from "react";

import { ANALYTICS_TAB_IDS, ANALYTICS_TAB_LABELS, analyticsTabHref, selectAnalyticsTab } from "../lib/tabs.js";

export interface AnalyticsTabStripProps {
  workspaceSlug: string;
  /** Raw path segment; unknown values select nothing. */
  tabId: string;
  /** Picker scope the tab links carry along. */
  scopeIds: readonly string[];
}

export function AnalyticsTabStrip({ workspaceSlug, tabId, scopeIds }: AnalyticsTabStripProps): React.ReactElement {
  const selected = selectAnalyticsTab(tabId);
  const search = scopeIds.length === 0 ? {} : { project_ids: [...scopeIds] };
  return (
    <div
      role="tablist"
      aria-label="Analytics"
      data-testid="analytics-tabs"
      className="flex min-w-0 flex-1 items-center gap-(--space-1) overflow-x-auto border-b border-(--border)"
    >
      {ANALYTICS_TAB_IDS.map((id) => {
        const active = selected === id;
        return (
          <Link
            key={id}
            to={analyticsTabHref(workspaceSlug, id)}
            search={search}
            role="tab"
            id={`analytics-tab-${id}`}
            aria-selected={active}
            aria-controls={`analytics-panel-${id}`}
            data-tab-id={id}
            data-testid={`analytics-tab-${id}`}
            className={`text-body flex h-10 shrink-0 items-center border-b-2 px-(--space-3) font-medium ${
              active ? "border-(--accent) text-(--text)" : "border-transparent text-(--text-muted) hover:text-(--text)"
            }`}
          >
            {ANALYTICS_TAB_LABELS[id]}
          </Link>
        );
      })}
    </div>
  );
}
