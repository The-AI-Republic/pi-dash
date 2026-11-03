// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics headline totals grid (parity/specs/analytics.md §§5, 8,
// ANL-005, ANL-008). One card per metric in catalogue order with an
// independent placeholder while the totals read is pending; a
// missing metric — or a failed read — settles to zero with no
// error banner. Later slices mount their sections below this grid.

import { Skeleton } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import type { AnalyticsDialogScope } from "../api/keys.js";
import { dialogTotalsQueryOptions } from "../api/projectScope.js";
import { analyticsTotalsQueryOptions } from "../api/totals.js";
import { headlineCardsForTab, selectHeadlineValue } from "../lib/cards.js";
import type { AnalyticsTabId } from "../lib/tabs.js";

export interface HeadlineCardsProps {
  workspaceSlug: string;
  tab: AnalyticsTabId;
  /** Resolved picker scope; enters the query key and the request. */
  projectIds: readonly string[];
  /**
   * Scoped dialog mode (parity/specs/analytics.md §13, ANL-014): read the
   * project endpoint for this scope instead of the workspace read. The tab
   * is always `work-items` there, so the same five cards render.
   */
  dialogScope?: AnalyticsDialogScope | undefined;
}

export function HeadlineCards({ workspaceSlug, tab, projectIds, dialogScope }: HeadlineCardsProps): React.ReactElement {
  // One read runs at a time: the dialog reads the project family, the tab
  // the workspace family, and the other hook stays idle. Both hooks are
  // always called so the order never depends on the mode.
  const dialogTotals = useQuery({
    ...dialogTotalsQueryOptions(
      getAppClient(),
      workspaceSlug,
      dialogScope ?? { projectId: "", cycleId: null, moduleId: null }
    ),
    enabled: dialogScope !== undefined,
  });
  const tabTotals = useQuery({
    ...analyticsTotalsQueryOptions(getAppClient(), workspaceSlug, tab, projectIds),
    enabled: dialogScope === undefined,
  });
  const totals = dialogScope === undefined ? tabTotals : dialogTotals;
  const cards = headlineCardsForTab(dialogScope === undefined ? tab : "work-items");
  return (
    <div
      data-testid="analytics-headline-grid"
      className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-(--space-4)"
    >
      {cards.map((card) => (
        <div
          key={card.key}
          data-testid="analytics-headline-card"
          className="rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)"
        >
          <div data-testid="analytics-headline-label" className="text-caption text-(--text-muted)">
            {card.label}
          </div>
          {totals.isPending ? (
            <div data-testid="analytics-headline-placeholder" className="pt-(--space-2)">
              <Skeleton width="64px" height="28px" />
            </div>
          ) : (
            <div data-testid="analytics-headline-value" className="text-h3 text-(--text)">
              {String(selectHeadlineValue(totals.data, card.key))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
