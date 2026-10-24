// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar progress section (parity/specs/cycles.md §14, CYC-038,
// CYC-039). The Work items and Points pairs, the burndown chart with
// its measure picker, and the States/Assignees/Labels breakdown tabs.
// Clicking an entry toggles the matching rich filter on the cycle's
// work items through the shared filter model; the chart itself has no
// click action. Undated cycles show the empty notice and future-start
// cycles hide the chart while keeping the breakdowns.

import type { CycleDistribution, CycleProgress, CycleRow } from "@pidash/api-client";
import { Combobox, Skeleton } from "@pidash/kit";
import * as React from "react";

import { useResolvedScheme } from "../../../core/theme/index.js";
import { AreaChart, chartSlotColor, resolveChartScheme } from "../../../shared/charts/index.js";
import { SidebarRow, useRichFilters, useUpdateRichFilters } from "../../issues/index.js";
import {
  BREAKDOWN_FILTER_STEMS,
  breakdownEntryActive,
  CYCLE_BREAKDOWN_TABS,
  CYCLE_PROGRESS_MEASURES,
  completionChartPoints,
  cycleProgressChartVisible,
  cycleProgressEmptyText,
  measureLabel,
  pointsPairText,
  progressGrandTotal,
  selectAssigneeRows,
  selectLabelRows,
  selectStateGroupRows,
  showProgressMeasure,
  showPointsRow,
  toggleBreakdownCondition,
  workItemsPairText,
  type CycleBreakdownTab,
  type CycleDistributionRow,
  type CycleProgressMeasure,
  type CycleStateGroupRow,
} from "../lib/cycleSidebar.js";

export interface CycleSidebarProgressProps {
  workspaceSlug: string;
  projectId: string;
  cycle: CycleRow;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  /** The project carries an estimate system (the measure picker gate). */
  estimatesEnabled: boolean;
  /** Active estimate system type behind the Points and measure gates, if any. */
  estimateType: string | null | undefined;
  /** Resolved snapshot counts behind the rows and state entries. */
  progress: CycleProgress;
  /** Resolved Work-items breakdowns and burndown series. */
  issuesAnalytics: CycleDistribution;
  /** Resolved Estimates breakdowns, while the measure read is pending. */
  pointsAnalytics: CycleDistribution | null;
  /** Active breakdown tab, persisted per cycle by the shell. */
  tab: CycleBreakdownTab;
  onTabChange: (tab: CycleBreakdownTab) => void;
}

/** Burndown plot height inside the narrow sidebar. */
const BURNDOWN_HEIGHT = 180;

/** Series key behind the remaining-work area. */
const REMAINING_SERIES_KEY = "remaining";

/** Accessible name of the measure dropdown list. */
const MEASURE_LIST_LABEL = "Progress measure";

function EntryBar({ percent }: { percent: number }): React.ReactElement {
  return (
    <span aria-hidden="true" className="h-1 w-full overflow-hidden rounded-full bg-(--subtle)">
      <span className="block h-full rounded-full bg-(--accent)" style={{ width: `${percent}%` }} />
    </span>
  );
}

interface EntryRowProps {
  title: string;
  testKey: string;
  total: number;
  percent: number;
  /** Null entries (no filter id) and empty groups render inert. */
  interactive: boolean;
  active: boolean;
  /** Defined exactly for interactive rows; inert rows render a div. */
  onToggle: (() => void) | undefined;
}

function EntryRow({
  title,
  testKey,
  total,
  percent,
  interactive,
  active,
  onToggle,
}: EntryRowProps): React.ReactElement {
  const body = (
    <>
      <span className="min-w-0 flex-1 truncate">{title}</span>
      <span className="text-meta shrink-0 text-(--text-muted)">{percent}%</span>
      <span className="text-meta shrink-0 text-(--text-muted)">of {total}</span>
    </>
  );
  const shape = "flex w-full flex-col gap-(--space-1) rounded-(--radius-control) px-(--space-2) py-(--space-1)";
  if (!interactive) {
    return (
      <div data-testid={`cycle-progress-entry-${testKey}`} className={`${shape} text-(--text-muted)`}>
        <span className="flex w-full items-center justify-between gap-(--space-2)">{body}</span>
        <EntryBar percent={percent} />
      </div>
    );
  }
  return (
    <button
      type="button"
      data-testid={`cycle-progress-entry-${testKey}`}
      aria-pressed={active}
      onClick={onToggle}
      className={`${shape} text-left text-(--text) hover:bg-(--subtle)`}
    >
      <span className="flex w-full items-center justify-between gap-(--space-2)">{body}</span>
      <EntryBar percent={percent} />
    </button>
  );
}

function EntriesPending(): React.ReactElement {
  return (
    <div data-testid="cycle-progress-entries-pending" role="status" className="flex flex-col gap-(--space-2)">
      <Skeleton height="12px" width="80%" />
      <Skeleton height="12px" width="65%" />
      <Skeleton height="12px" width="75%" />
    </div>
  );
}

export function CycleSidebarProgress({
  workspaceSlug,
  projectId,
  cycle,
  projectTimeZone,
  estimatesEnabled,
  estimateType,
  progress,
  issuesAnalytics,
  pointsAnalytics,
  tab,
  onTabChange,
}: CycleSidebarProgressProps): React.ReactElement {
  const [measure, setMeasure] = React.useState<CycleProgressMeasure>("issues");
  const { scheme, customDark } = useResolvedScheme();
  const entity = React.useMemo(
    () => ({ scope: "cycle", projectId, cycleId: cycle.id }) as const,
    [projectId, cycle.id]
  );
  const { conditions } = useRichFilters(workspaceSlug, entity);
  const { updateRichFilters } = useUpdateRichFilters(workspaceSlug, entity);

  const chart = cycleProgressChartVisible(cycle.start_date, cycle.end_date, projectTimeZone);
  const emptyText = cycleProgressEmptyText(cycle.start_date, cycle.end_date);
  const points = showPointsRow({
    estimateType,
    status: cycle.status,
    totalEstimatePoints: progress.total_estimate_points,
    snapshot: { progress_snapshot: cycle.progress_snapshot ?? null },
  });
  const measureAvailable = showProgressMeasure({ estimatesEnabled, estimateType, progress });
  // Estimates-off projects always read back to counts, even when point
  // data keeps the picker visible (spec §6.3, EST-019).
  const activeMeasure: CycleProgressMeasure = estimatesEnabled && measureAvailable ? measure : "issues";
  const analytics = activeMeasure === "points" ? pointsAnalytics : issuesAnalytics;
  const grandTotal = progressGrandTotal(progress, activeMeasure);
  const stem = BREAKDOWN_FILTER_STEMS[tab];

  function toggleEntry(id: string): void {
    updateRichFilters(toggleBreakdownCondition(conditions, stem, id));
  }

  function stateEntries(): React.ReactElement[] {
    return selectStateGroupRows(progress, activeMeasure).map((row: CycleStateGroupRow) => (
      <EntryRow
        key={row.key}
        title={row.title}
        testKey={row.key}
        total={row.total}
        percent={row.percent}
        interactive={row.count > 0}
        active={breakdownEntryActive(conditions, stem, row.key)}
        onToggle={() => toggleEntry(row.key)}
      />
    ));
  }

  function distributionEntries(rows: CycleDistributionRow[]): React.ReactElement[] {
    return rows.map((row, index) => {
      const id = row.id;
      const interactive = id !== null && row.count > 0;
      // Only buttons fire it, and only entries with an id render as
      // buttons, so the toggle exists exactly where it can run.
      const action = id === null ? undefined : () => toggleEntry(id);
      return (
        <EntryRow
          key={id ?? `entry-${index}`}
          title={row.title}
          testKey={id ?? `entry-${index}`}
          total={row.total}
          percent={row.percent}
          interactive={interactive}
          active={id !== null && breakdownEntryActive(conditions, stem, id)}
          onToggle={interactive ? action : undefined}
        />
      );
    });
  }

  const entries =
    tab === "States"
      ? stateEntries()
      : analytics === null
        ? null
        : distributionEntries(
            tab === "Labels"
              ? selectLabelRows(analytics, activeMeasure, grandTotal)
              : selectAssigneeRows(analytics, activeMeasure, grandTotal)
          );

  return (
    <section aria-label="Progress" className="flex flex-col gap-(--space-3)">
      <h3 className="text-h3 text-(--text)">Progress</h3>
      <SidebarRow label="Work items">
        <span data-testid="cycle-progress-work-items" className="text-body text-(--text)">
          {workItemsPairText(progress)}
        </span>
      </SidebarRow>
      {points ? (
        <SidebarRow label="Points">
          <span data-testid="cycle-progress-points" className="text-body text-(--text)">
            {pointsPairText(progress)}
          </span>
        </SidebarRow>
      ) : null}
      {chart && analytics !== null ? (
        <div data-testid="cycle-progress-chart">
          <AreaChart
            points={completionChartPoints(analytics.completion_chart)}
            series={[
              {
                key: REMAINING_SERIES_KEY,
                name: "Remaining",
                color: chartSlotColor(0, resolveChartScheme(scheme, customDark)),
              },
            ]}
            legend={false}
            height={BURNDOWN_HEIGHT}
          />
        </div>
      ) : emptyText === null ? null : (
        <p data-testid="cycle-progress-empty" className="text-body text-(--text-muted)">
          {emptyText}
        </p>
      )}
      {measureAvailable ? (
        <Combobox
          trigger={
            <button
              type="button"
              data-testid="cycle-progress-measure"
              className="text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle)"
            >
              {measureLabel(activeMeasure)}
            </button>
          }
          options={CYCLE_PROGRESS_MEASURES.map((entry) => ({ id: entry.value, label: entry.label }))}
          value={activeMeasure}
          onSelect={(id) => setMeasure(id === "points" ? "points" : "issues")}
          listLabel={MEASURE_LIST_LABEL}
          emptyText="No measures"
          searchEnabled={false}
        />
      ) : null}
      <div
        role="tablist"
        aria-label="Progress breakdown"
        data-testid="cycle-progress-tabs"
        className="flex gap-(--space-1)"
      >
        {CYCLE_BREAKDOWN_TABS.map((name) => (
          <button
            key={name}
            type="button"
            role="tab"
            aria-selected={tab === name}
            data-testid={`cycle-progress-tab-${name}`}
            onClick={() => onTabChange(name)}
            className={
              tab === name
                ? "text-body rounded-(--radius-control) bg-(--subtle) px-(--space-3) py-(--space-1) font-medium text-(--text)"
                : "text-body rounded-(--radius-control) px-(--space-3) py-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
            }
          >
            {name}
          </button>
        ))}
      </div>
      <div role="tabpanel" data-testid="cycle-progress-entries" className="flex flex-col gap-(--space-1)">
        {entries === null ? <EntriesPending /> : entries}
      </div>
    </section>
  );
}
