// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail sidebar (parity/specs/cycles.md §14, CYC-035–CYC-040).
// The collapsible panel beside the work-item layouts: the name with
// its lifecycle marker and close control, inline dates, the read-only
// description, the lead and member rows, and the progress section. The
// route mounts it only while expanded and passes the collapse setter
// to the header toggle; a skeleton stands in until the cycle's own
// reads — progress, breakdowns, estimates and the member directory —
// arrive. On narrow viewports the panel floats over the content.

import { useQuery } from "@tanstack/react-query";
import { selectActiveEstimate, type CycleRow } from "@pidash/api-client";
import { Avatar, Badge, IconButton } from "@pidash/kit";
import { X } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePickerWorkspaceMembers } from "../../../shared/pickers/index.js";
import { projectEstimatesQueryOptions } from "../../estimates/index.js";
import { SidebarRow } from "../../issues/index.js";
import { useCycleAnalyticsTab } from "../api/cycleSidebarState.js";
import { cycleAnalyticsQueryOptions, cycleProgressQueryOptions } from "../api/sidebarQueries.js";
import { lifecycleLabel, lifecycleTone } from "../lib/cycleGroups.js";
import { selectCycleMemberDirectory, UNKNOWN_CYCLE_MEMBER, type CycleMemberEntry } from "../lib/cycleMembers.js";
import { CycleSidebarDates } from "./CycleSidebarDates.js";
import { CycleSidebarProgress } from "./CycleSidebarProgress.js";
import { CycleSidebarSkeleton } from "./CycleSidebarSkeleton.js";

export interface CycleDetailSidebarProps {
  workspaceSlug: string;
  projectId: string;
  cycle: CycleRow;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  /** The project carries an estimate system (the measure picker gate). */
  estimatesEnabled: boolean;
  /** Member/admin on a live cycle; everyone else reads dates disabled. */
  editable: boolean;
  /** Collapse the panel (the caller persists the choice). */
  onClose: () => void;
}

/** Close control behind the panel's own collapse (spec §14). */
export const CYCLE_SIDEBAR_CLOSE_LABEL = "Close sidebar";

function LeadAvatar({ entry }: { entry: CycleMemberEntry }): React.ReactElement {
  const image = entry.avatarUrl === null ? {} : { src: entry.avatarUrl };
  return <Avatar name={entry.displayName} initial={entry.displayName.slice(0, 1)} size="small" {...image} />;
}

export function CycleDetailSidebar({
  workspaceSlug,
  projectId,
  cycle,
  projectTimeZone,
  estimatesEnabled,
  editable,
  onClose,
}: CycleDetailSidebarProps): React.ReactElement {
  const client = getAppClient();
  const progress = useQuery(cycleProgressQueryOptions(client, workspaceSlug, projectId, cycle.id));
  const issues = useQuery(cycleAnalyticsQueryOptions(client, workspaceSlug, projectId, cycle.id, "issues"));
  const pointsAvailable = estimatesEnabled || (progress.data?.total_estimate_points ?? 0) > 0;
  const points = useQuery({
    ...cycleAnalyticsQueryOptions(client, workspaceSlug, projectId, cycle.id, "points"),
    enabled: pointsAvailable,
  });
  const estimates = useQuery({
    ...projectEstimatesQueryOptions(client, workspaceSlug, projectId),
    enabled: estimatesEnabled,
  });
  const directoryQuery = usePickerWorkspaceMembers(workspaceSlug, true);
  const [tab, setTab] = useCycleAnalyticsTab(cycle.id);

  const directory = React.useMemo(() => selectCycleMemberDirectory(directoryQuery.data ?? []), [directoryQuery.data]);
  // The skeleton stands until every sidebar read lands. Disabled
  // reads stay pending (idle), so each optional pair joins the gate
  // only while its availability flag holds.
  const pending = [
    progress,
    issues,
    ...(pointsAvailable ? [points] : []),
    ...(estimatesEnabled ? [estimates] : []),
    directoryQuery,
  ].some((read) => read.isPending);
  if (pending) {
    return <CycleSidebarSkeleton testId="cycle-sidebar-skeleton" />;
  }

  const estimateType = estimatesEnabled ? (selectActiveEstimate(estimates.data ?? [])?.type ?? null) : null;
  const lead =
    cycle.owned_by_id === null || cycle.owned_by_id === undefined
      ? null
      : (directory.get(cycle.owned_by_id) ?? UNKNOWN_CYCLE_MEMBER);
  const assigneeIds = cycle.assignee_ids ?? [];
  const description = cycle.description ?? "";

  return (
    <aside
      data-testid="cycle-sidebar"
      aria-label="Cycle details"
      className="max-md:shadow-lg flex w-80 max-w-full shrink-0 flex-col gap-(--space-4) border-l border-(--border) pl-(--space-4) max-md:fixed max-md:inset-y-0 max-md:right-0 max-md:z-40 max-md:overflow-y-auto max-md:bg-(--bg) max-md:p-(--space-4)"
    >
      <div className="flex items-start justify-between gap-(--space-2)">
        <div className="flex min-w-0 flex-col gap-(--space-1)">
          <h2 data-testid="cycle-sidebar-name" className="text-h2 truncate text-(--text)">
            {cycle.name}
          </h2>
          <Badge tone={lifecycleTone(cycle.status)} dot>
            {lifecycleLabel(cycle.status)}
          </Badge>
        </div>
        <IconButton label={CYCLE_SIDEBAR_CLOSE_LABEL} data-testid="cycle-sidebar-close" size="small" onClick={onClose}>
          <X size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
      <CycleSidebarDates
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        cycle={cycle}
        projectTimeZone={projectTimeZone}
        editable={editable}
      />
      {description.trim() === "" ? null : (
        <p data-testid="cycle-sidebar-description" className="text-body whitespace-pre-wrap text-(--text)">
          {description}
        </p>
      )}
      <div className="flex flex-col">
        <SidebarRow label="Lead">
          <span data-testid="cycle-sidebar-lead" className="flex min-w-0 items-center gap-(--space-2)">
            <LeadAvatar entry={lead ?? { displayName: "", avatarUrl: null }} />
            {lead === null ? null : (
              <span data-testid="cycle-sidebar-lead-name" className="text-body truncate text-(--text)">
                {lead.displayName}
              </span>
            )}
          </span>
        </SidebarRow>
        <SidebarRow label="Members">
          {assigneeIds.length === 0 ? (
            <span data-testid="cycle-sidebar-members" className="text-body text-(--text-muted)">
              No assignee
            </span>
          ) : (
            <span data-testid="cycle-sidebar-members" className="flex min-w-0 items-center">
              {assigneeIds.map((id) => (
                <LeadAvatar key={id} entry={directory.get(id) ?? UNKNOWN_CYCLE_MEMBER} />
              ))}
            </span>
          )}
        </SidebarRow>
      </div>
      {progress.data === undefined || issues.isPending ? null : (
        <CycleSidebarProgress
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          cycle={cycle}
          projectTimeZone={projectTimeZone}
          estimatesEnabled={estimatesEnabled}
          estimateType={estimateType}
          progress={progress.data}
          issuesAnalytics={issues.data ?? null}
          pointsAnalytics={points.data ?? null}
          tab={tab}
          onTabChange={setTab}
        />
      )}
    </aside>
  );
}
