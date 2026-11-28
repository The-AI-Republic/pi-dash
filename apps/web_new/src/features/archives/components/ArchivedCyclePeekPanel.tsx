// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycle quick look (parity/specs/archives.md §4.1 and §5.6,
// ARCH-014, ARCH-025). The shared peek frame over the archived list,
// opened through the ?peekCycle= address key: the name in the frame's
// title heading, and the archived record's details — the state marker,
// dates, lead and members, progress summary and the States,
// Assignees and Labels analytics summaries — read from the archived
// detail endpoint, so opening the peek never depends on the list row
// still being loaded. Every editing affordance the live panel offers
// stays out: no status picker, no add-link control, no date or
// description edit, no member edit, no filter writes from the
// breakdown entries, and no rename commit. A skeleton covers the
// load, and a failed read offers a retry instead of a blank panel.

import { Avatar, Badge, Button, IconButton, Skeleton } from "@pidash/kit";
import type { ArchivedCycleDetail } from "@pidash/api-client";
import { ExternalLink, Link2 } from "lucide-react";
import * as React from "react";

import { useResolvedScheme } from "../../../core/theme/index.js";
import { AreaChart, chartSlotColor, resolveChartScheme } from "../../../shared/charts/index.js";
import { usePickerWorkspaceMembers } from "../../../shared/pickers/index.js";
import { PeekPanel } from "../../../shared/shell/index.js";
import { SidebarRow } from "../../issues/index.js";
import {
  completionChartPoints,
  copyCycleLink,
  CycleDateSpan,
  CYCLE_BREAKDOWN_TABS,
  cycleProgressChartVisible,
  cycleProgressCounts,
  cycleProgressEmptyText,
  cycleProgressPercent,
  CycleProgressRing,
  lifecycleLabel,
  lifecycleTone,
  openCycleInNewTab,
  selectCycleMemberDirectory,
  UNKNOWN_CYCLE_MEMBER,
  type CycleBreakdownTab,
  type CycleDistributionRow,
  type CycleMemberEntry,
} from "../../cycles/index.js";
import { useArchivedCycleDetail } from "../api/archivedCycles.js";
import {
  selectArchivedCycleAssigneeRows,
  selectArchivedCycleChart,
  selectArchivedCycleLabelRows,
  selectArchivedCycleStateRows,
} from "../lib/archivedCycleAnalytics.js";
export interface ArchivedCyclePeekPanelProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  /** Name from the list row; the detail read replaces it once loaded. */
  fallbackName: string;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  onClose: () => void;
}

/** Plot height inside the narrow peek, matching the live sidebar. */
const ANALYTICS_CHART_HEIGHT = 180;

/** Series key behind the remaining-work area. */
const REMAINING_SERIES_KEY = "remaining";

function ArchivedCyclePeekSkeleton(): React.ReactElement {
  return (
    <div data-testid="archived-cycle-peek-skeleton" className="flex flex-col gap-(--space-3)">
      <Skeleton height="14px" width="45%" />
      <Skeleton height="12px" width="75%" />
      <Skeleton height="12px" width="55%" />
      <Skeleton height="12px" width="30%" />
    </div>
  );
}

function MemberAvatar({ entry }: { entry: CycleMemberEntry }): React.ReactElement {
  const image = entry.avatarUrl === null ? {} : { src: entry.avatarUrl };
  return <Avatar name={entry.displayName} initial={entry.displayName.slice(0, 1)} size="small" {...image} />;
}

function AnalyticsEntry({ row, testKey }: { row: CycleDistributionRow; testKey: string }): React.ReactElement {
  return (
    <div
      data-testid={`archived-cycle-peek-entry-${testKey}`}
      className="flex w-full flex-col gap-(--space-1) px-(--space-2) py-(--space-1) text-(--text-muted)"
    >
      <span className="flex w-full items-center justify-between gap-(--space-2)">
        <span className="min-w-0 flex-1 truncate">{row.title}</span>
        <span className="text-meta shrink-0 text-(--text-muted)">{row.percent}%</span>
        <span className="text-meta shrink-0 text-(--text-muted)">of {row.total}</span>
      </span>
      <span aria-hidden="true" className="h-1 w-full overflow-hidden rounded-full bg-(--subtle)">
        <span className="block h-full rounded-full bg-(--accent)" style={{ width: `${row.percent}%` }} />
      </span>
    </div>
  );
}

function ArchivedCycleAnalytics({
  cycle,
  projectTimeZone,
}: {
  cycle: ArchivedCycleDetail;
  projectTimeZone: string;
}): React.ReactElement {
  const [tab, setTab] = React.useState<CycleBreakdownTab>("States");
  const { scheme, customDark } = useResolvedScheme();

  const stateRows = React.useMemo(() => selectArchivedCycleStateRows(cycle), [cycle]);
  const assigneeRows = React.useMemo(() => selectArchivedCycleAssigneeRows(cycle), [cycle]);
  const labelRows = React.useMemo(() => selectArchivedCycleLabelRows(cycle), [cycle]);
  const chart = React.useMemo(() => selectArchivedCycleChart(cycle), [cycle]);
  const chartVisible = cycleProgressChartVisible(cycle.start_date, cycle.end_date, projectTimeZone);
  const emptyText = cycleProgressEmptyText(cycle.start_date, cycle.end_date);
  const rows = tab === "States" ? stateRows : tab === "Labels" ? labelRows : assigneeRows;

  return (
    <section aria-label="Analytics" className="flex flex-col gap-(--space-3)">
      {chartVisible && chart !== null ? (
        <div data-testid="archived-cycle-peek-chart">
          <AreaChart
            points={completionChartPoints(chart)}
            series={[
              {
                key: REMAINING_SERIES_KEY,
                name: "Remaining",
                color: chartSlotColor(0, resolveChartScheme(scheme, customDark)),
              },
            ]}
            legend={false}
            height={ANALYTICS_CHART_HEIGHT}
          />
        </div>
      ) : emptyText === null ? null : (
        <p data-testid="archived-cycle-peek-empty" className="text-body text-(--text-muted)">
          {emptyText}
        </p>
      )}
      <div
        role="tablist"
        aria-label="Progress breakdown"
        data-testid="archived-cycle-peek-tabs"
        className="flex gap-(--space-1)"
      >
        {CYCLE_BREAKDOWN_TABS.map((name) => (
          <button
            key={name}
            type="button"
            role="tab"
            aria-selected={tab === name}
            data-testid={`archived-cycle-peek-tab-${name}`}
            onClick={() => setTab(name)}
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
      <div role="tabpanel" data-testid="archived-cycle-peek-entries" className="flex flex-col gap-(--space-1)">
        {rows.map((row) => (
          <AnalyticsEntry key={row.id ?? row.title} row={row} testKey={row.id ?? row.title} />
        ))}
      </div>
    </section>
  );
}

function ArchivedCyclePeekBody({
  cycle,
  directory,
  projectTimeZone,
}: {
  cycle: ArchivedCycleDetail;
  directory: Map<string, CycleMemberEntry>;
  projectTimeZone: string;
}): React.ReactElement {
  const counts = cycleProgressCounts(cycle);
  const lead = directory.get(cycle.owned_by_id) ?? UNKNOWN_CYCLE_MEMBER;
  const assigneeIds = cycle.assignee_ids ?? [];
  return (
    <div data-testid="archived-cycle-peek-body" className="flex flex-col gap-(--space-4)">
      <Badge tone={lifecycleTone(cycle.status)} dot>
        {lifecycleLabel(cycle.status)}
      </Badge>
      <CycleDateSpan startDate={cycle.start_date} endDate={cycle.end_date} projectTimeZone={projectTimeZone} />
      <div data-testid="archived-cycle-peek-summary" className="flex items-center gap-(--space-3)">
        <CycleProgressRing percent={cycleProgressPercent(cycle)} />
        <span className="text-body text-(--text)">
          {counts.completed} of {counts.total} {counts.total === 1 ? "work item" : "work items"}
        </span>
      </div>
      <div className="flex flex-col">
        <SidebarRow label="Lead">
          <span data-testid="archived-cycle-peek-lead" className="flex min-w-0 items-center gap-(--space-2)">
            <MemberAvatar entry={lead} />
            <span data-testid="archived-cycle-peek-lead-name" className="text-body truncate text-(--text)">
              {lead.displayName}
            </span>
          </span>
        </SidebarRow>
        <SidebarRow label="Members">
          {assigneeIds.length === 0 ? (
            <span data-testid="archived-cycle-peek-members" className="text-body text-(--text-muted)">
              No assignee
            </span>
          ) : (
            <span data-testid="archived-cycle-peek-members" className="flex min-w-0 items-center">
              {assigneeIds.map((id) => (
                <MemberAvatar key={id} entry={directory.get(id) ?? UNKNOWN_CYCLE_MEMBER} />
              ))}
            </span>
          )}
        </SidebarRow>
      </div>
      <ArchivedCycleAnalytics cycle={cycle} projectTimeZone={projectTimeZone} />
    </div>
  );
}

export function ArchivedCyclePeekPanel({
  workspaceSlug,
  projectId,
  cycleId,
  fallbackName,
  projectTimeZone,
  onClose,
}: ArchivedCyclePeekPanelProps): React.ReactElement {
  const detail = useArchivedCycleDetail(workspaceSlug, projectId, cycleId);
  const members = usePickerWorkspaceMembers(workspaceSlug, true);
  const cycle = detail.data ?? null;
  const directory = React.useMemo(() => selectCycleMemberDirectory(members.data ?? []), [members.data]);
  return (
    <PeekPanel
      label="Archived cycle preview"
      testId="archived-cycle-preview"
      eyebrow="Archived cycle"
      title={cycle?.name ?? fallbackName}
      titleLabel="Archived cycle name"
      titleAsHeading
      onClose={onClose}
      actions={
        <>
          <IconButton
            label="Copy cycle link"
            size="small"
            onClick={() => {
              void copyCycleLink(workspaceSlug, projectId, cycleId);
            }}
          >
            <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
          <IconButton
            label="Open cycle in new tab"
            size="small"
            onClick={() => {
              void openCycleInNewTab(workspaceSlug, projectId, cycleId);
            }}
          >
            <ExternalLink size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </>
      }
    >
      {detail.isPending || members.isPending ? (
        <ArchivedCyclePeekSkeleton />
      ) : cycle === null ? (
        <div data-testid="archived-cycle-peek-error" className="flex flex-col items-start gap-(--space-3)">
          <p className="text-body text-(--text)">Couldn&apos;t load this archived cycle.</p>
          <Button
            variant="secondary"
            size="small"
            onClick={() => {
              void detail.refetch();
            }}
          >
            Try again
          </Button>
        </div>
      ) : (
        <ArchivedCyclePeekBody cycle={cycle} directory={directory} projectTimeZone={projectTimeZone} />
      )}
    </PeekPanel>
  );
}
