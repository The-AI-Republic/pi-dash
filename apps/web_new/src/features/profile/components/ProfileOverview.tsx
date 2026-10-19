// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Summary tab (parity/specs/profile.md §1, PROF-003/004/005). Three count
// cards link to their issue tabs and stand as placeholder blocks while
// the stats read flies; the workload row always carries all seven state
// groups with zeros not gaps; the priority and state breakdowns render
// from the same payload and fall back to their empty states when the
// member has no assigned work; recent activity closes the page, with a
// compact empty illustration when the feed is quiet.

import type { IssueActivity, UserProfileStats } from "@pidash/api-client";
import { Skeleton } from "@pidash/kit";
import { Activity, CircleSlash } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/CrossAppLink.js";
import { ActivityRow } from "./ProfileActivityList.js";
import {
  chartsAreEmpty,
  overviewCards,
  priorityMax,
  prioritySeries,
  stateLegend,
  stateTotal,
  workloadGroups,
} from "../lib/stats.js";
import { profileBase } from "../lib/tabs.js";

export const PROFILE_OVERVIEW_ACTIVITY_EMPTY = "No Data yet";

export interface ProfileOverviewProps {
  workspaceSlug: string;
  userId: string;
  stats: UserProfileStats | undefined;
  /** First page of the member's activity. */
  activity: IssueActivity[] | undefined;
  currentUserId: string | null;
}

function CardSkeleton(): React.ReactElement {
  return (
    <div
      data-testid="profile-card-placeholder"
      className="rounded-(--radius-panel) border border-(--border) p-(--space-4)"
    >
      <Skeleton className="mb-(--space-2) h-4 w-32" />
      <Skeleton className="h-8 w-16" />
    </div>
  );
}

function CountCards({
  workspaceSlug,
  userId,
  stats,
}: {
  workspaceSlug: string;
  userId: string;
  stats: UserProfileStats;
}): React.ReactElement {
  return (
    <div data-testid="profile-count-cards" className="grid grid-cols-1 gap-(--space-3) sm:grid-cols-3">
      {overviewCards(stats).map((card) => (
        <CrossAppLink
          key={card.tab}
          to={`${profileBase(workspaceSlug, userId)}${card.tab}/`}
          data-profile-card={card.tab}
          className="flex flex-col gap-(--space-1) rounded-(--radius-panel) border border-(--border) bg-(--surface) p-(--space-4) hover:border-(--accent)"
        >
          <span className="text-caption text-(--text-muted)">{card.title}</span>
          <span className="text-h2 text-(--text)" data-profile-card-value={card.tab}>
            {card.value}
          </span>
        </CrossAppLink>
      ))}
    </div>
  );
}

function Workload({ stats }: { stats: UserProfileStats }): React.ReactElement {
  return (
    <section aria-label="Workload" className="flex flex-col gap-(--space-2)">
      <h2 className="text-h3 text-(--text)">Workload</h2>
      <div data-testid="profile-workload" className="grid grid-cols-2 gap-(--space-2) sm:grid-cols-4 lg:grid-cols-7">
        {workloadGroups(stats.state_distribution).map((group) => (
          <div
            key={group.group}
            data-profile-workload-group={group.group}
            className="flex flex-col gap-(--space-1) rounded-(--radius-panel) border border-(--border) p-(--space-3)"
          >
            <span className="text-caption text-(--text-muted)">{group.label}</span>
            <span className="text-h3 text-(--text)">{group.count}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

function PriorityChart({ stats }: { stats: UserProfileStats }): React.ReactElement {
  const series = prioritySeries(stats.priority_distribution);
  const max = priorityMax(series);
  return (
    <section aria-label="Work items by priority" className="flex flex-1 flex-col gap-(--space-2)">
      <h2 className="text-h3 text-(--text)">Priority</h2>
      {series.length === 0 ? (
        <ChartEmpty />
      ) : (
        <ul
          data-testid="profile-priority-chart"
          className="flex flex-col gap-(--space-2)"
          aria-label="Priority distribution"
        >
          {series.map((row) => (
            <li key={row.label} data-profile-priority={row.label} className="flex items-center gap-(--space-2)">
              <span className="text-caption w-16 shrink-0 text-(--text-muted)">{row.label}</span>
              <span className="h-2 flex-1 overflow-hidden rounded-(--radius-full) bg-(--subtle)">
                <span
                  aria-hidden="true"
                  className="block h-full rounded-(--radius-full) bg-(--accent)"
                  style={{ width: max === 0 ? "0%" : `${Math.max(4, (row.count / max) * 100)}%` }}
                />
              </span>
              <span className="text-caption w-6 shrink-0 text-right text-(--text)">{row.count}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function StateChart({ stats }: { stats: UserProfileStats }): React.ReactElement {
  const legend = stateLegend(stats.state_distribution);
  const total = stateTotal(stats.state_distribution);
  return (
    <section aria-label="Work items by state" className="flex flex-1 flex-col gap-(--space-2)">
      <h2 className="text-h3 text-(--text)">State</h2>
      {legend.length === 0 ? (
        <ChartEmpty />
      ) : (
        <>
          <div aria-hidden="true" className="flex h-2 overflow-hidden rounded-(--radius-full) bg-(--subtle)">
            {legend.map((row, index) => {
              const width = total === 0 ? 0 : (Number(row.count) / total) * 100;
              return (
                <span
                  key={row.group}
                  data-profile-state-slice={row.group}
                  style={{
                    width: `${width}%`,
                    backgroundColor: `color-mix(in srgb, var(--accent) ${100 - index * 12}%, var(--border))`,
                  }}
                />
              );
            })}
          </div>
          <ul data-testid="profile-state-chart" className="grid grid-cols-2 gap-(--space-1)">
            {legend.map((row) => (
              <li
                key={row.group}
                data-profile-state-legend={row.group}
                className="flex items-center justify-between gap-(--space-2)"
              >
                <span className="text-caption text-(--text-muted)">{row.label}</span>
                <span className="text-caption text-(--text)">{row.count}</span>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

function ChartEmpty(): React.ReactElement {
  return (
    <div
      data-testid="profile-chart-empty"
      className="flex flex-1 flex-col items-center justify-center gap-(--space-1) rounded-(--radius-panel) border border-dashed border-(--border) p-(--space-6) text-center"
    >
      <CircleSlash size={20} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
      <p className="text-caption text-(--text-muted)">No work item assigned yet</p>
    </div>
  );
}

function RecentActivity({
  workspaceSlug,
  activity,
  currentUserId,
}: {
  workspaceSlug: string;
  activity: IssueActivity[] | undefined;
  currentUserId: string | null;
}): React.ReactElement {
  return (
    <section aria-label="Recent activity" className="flex flex-col gap-(--space-2)">
      <h2 className="text-h3 text-(--text)">Recent activity</h2>
      {activity === undefined ? (
        <div className="flex flex-col gap-(--space-2)">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : activity.length === 0 ? (
        <div
          data-testid="profile-overview-activity-empty"
          className="flex flex-col items-center gap-(--space-1) rounded-(--radius-panel) border border-dashed border-(--border) p-(--space-6) text-center"
        >
          <Activity size={20} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
          <p className="text-caption text-(--text-muted)">{PROFILE_OVERVIEW_ACTIVITY_EMPTY}</p>
        </div>
      ) : (
        <ul data-testid="profile-overview-activity" className="flex flex-col divide-y divide-(--border)">
          {activity.map((row) => (
            <ActivityRow key={row.id} row={row} workspaceSlug={workspaceSlug} currentUserId={currentUserId} />
          ))}
        </ul>
      )}
    </section>
  );
}

export function ProfileOverview({
  workspaceSlug,
  userId,
  stats,
  activity,
  currentUserId,
}: ProfileOverviewProps): React.ReactElement {
  return (
    <div data-testid="profile-overview" className="flex flex-col gap-(--space-6) p-(--space-4)">
      {stats === undefined ? (
        <div data-testid="profile-count-cards" className="grid grid-cols-1 gap-(--space-3) sm:grid-cols-3">
          <CardSkeleton />
          <CardSkeleton />
          <CardSkeleton />
        </div>
      ) : (
        <CountCards workspaceSlug={workspaceSlug} userId={userId} stats={stats} />
      )}
      {stats === undefined ? null : (
        <>
          <Workload stats={stats} />
          <div className="flex flex-col gap-(--space-4) lg:flex-row">
            <PriorityChart stats={stats} />
            <StateChart stats={stats} />
          </div>
        </>
      )}
      <RecentActivity workspaceSlug={workspaceSlug} activity={activity} currentUserId={currentUserId} />
    </div>
  );
}

/** True when both breakdowns must show their empty states. */
export function overviewChartsEmpty(stats: UserProfileStats | undefined): boolean {
  return stats !== undefined && chartsAreEmpty(stats);
}
