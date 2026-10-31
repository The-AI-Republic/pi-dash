// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules timeline layout (parity/specs/modules.md §6, MOD-021). The third
// modules-list layout, plotted on the shared timeline chart shell from
// features/issues/gantt (NEWFRONT-624) with the module row model: sidebar
// rows carry a status glyph plus the module name linking to detail, bars
// span the saved start/target dates in the status color with a name-plus-
// dates tooltip, and modules without a complete range keep only their row.
// Bar drags and resizes persist through the module PATCH; row reorder is
// mounted only under manual placement with edit standing, writes
// normalized ranks, and warns outside manual placement. Guests read the
// whole chart but every edit stays a silent no-op.

import { useNavigate } from "@tanstack/react-router";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { notifyInfo } from "../../../../shared/shell/index.js";
import { useProjectRoles } from "../../../auth/index.js";
import { GanttChart, useWeekStart, type DropPlacement, type GanttTimelineEntity } from "../../../issues/index.js";
import { isProjectAuthorized } from "../../../projects/index.js";
import { moduleDetailHref } from "../../lib/moduleList.js";
import { DEFAULT_MODULE_SORT, moduleStatusColor, type ModuleSort } from "../../lib/moduleListControls.js";
import { useModuleListContext } from "../moduleListContext.js";
import {
  buildModuleTimelineRows,
  MODULE_TIMELINE_REORDER_WARNING_BODY,
  MODULE_TIMELINE_REORDER_WARNING_TITLE,
  moduleTimelineBarColors,
  moduleTimelineBarTitle,
  moduleTimelinePrefixStatus,
  warnOnTimelineReorderAttempt,
} from "./moduleTimelineModel.js";
import { useModuleTimelineMutations } from "./useModuleTimelineMutations.js";

/** Status dot ahead of the sidebar name, in the status color (§4.2). */
function ModuleTimelineStatusGlyph({ status }: { status: ModuleRow["status"] }): React.ReactElement {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      aria-hidden="true"
      data-testid="modules-timeline-status-glyph"
      className="shrink-0"
    >
      <circle cx="5" cy="5" r="4" fill={moduleStatusColor(status)} />
    </svg>
  );
}

export interface ModulesTimelineProps {
  workspaceSlug: string;
  projectId: string;
  /** The narrowed, ordered modules to plot. */
  modules: readonly ModuleRow[];
  /** Ordering override; defaults to the list context (manual gates reorder). */
  sort?: ModuleSort | undefined;
}

export function ModulesTimeline({
  workspaceSlug,
  projectId,
  modules,
  sort: sortProp,
}: ModulesTimelineProps): React.ReactElement | null {
  const navigate = useNavigate();
  const context = useModuleListContext();
  const sort = sortProp ?? context?.sort ?? DEFAULT_MODULE_SORT;
  const roles = useProjectRoles(workspaceSlug);
  const canEdit = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const weekStart = useWeekStart();
  const mutations = useModuleTimelineMutations({ client: getAppClient(), workspaceSlug, projectId });

  const rows = React.useMemo(() => buildModuleTimelineRows(modules), [modules]);
  const byId = React.useMemo(() => new Map(modules.map((module) => [module.id, module])), [modules]);
  const barColors = React.useMemo(() => moduleTimelineBarColors(modules), [modules]);

  const reorder = React.useMemo(
    () =>
      canEdit && sort.key === "manual"
        ? {
            onReorder: (sourceId: string, targetId: string, placement: DropPlacement) =>
              mutations.reorder(modules, sourceId, targetId, placement),
          }
        : null,
    [canEdit, modules, mutations, sort.key]
  );

  const handleAttempt = React.useCallback(() => {
    if (warnOnTimelineReorderAttempt(canEdit, sort.key)) {
      notifyInfo(MODULE_TIMELINE_REORDER_WARNING_TITLE, MODULE_TIMELINE_REORDER_WARNING_BODY);
    }
  }, [canEdit, sort.key]);

  const barTooltip = React.useCallback(
    (row: { id: string; name: string }) => moduleTimelineBarTitle(byId, row),
    [byId]
  );

  const entity = React.useMemo<GanttTimelineEntity>(
    () => ({
      rowHref: (row) => moduleDetailHref(workspaceSlug, projectId, row.id),
      rowPrefix: (row) => <ModuleTimelineStatusGlyph status={moduleTimelinePrefixStatus(byId, row.id)} />,
      rowDuration: () => null,
      sidebarTitle: "Module",
      sidebarMeta: null,
      countNoun: { one: "module", many: "modules" },
    }),
    [byId, projectId, workspaceSlug]
  );

  // No rows, no chart: the zero-modules container belongs to slice F
  // (MOD-009), which mounts beside this layout.
  if (modules.length === 0) return null;

  return (
    <div data-testid="modules-timeline" className="flex h-full min-h-0 flex-col">
      <GanttChart
        rows={rows}
        workspaceSlug={workspaceSlug}
        totalCount={rows.length}
        weekStart={weekStart}
        barColors={barColors}
        onOpenRow={(row) => {
          void navigate({ to: moduleDetailHref(workspaceSlug, projectId, row.id) });
        }}
        reorder={reorder}
        onReorderAttempt={handleAttempt}
        entity={entity}
        canEdit={canEdit}
        onDatesChange={(moduleId, dates) => mutations.moveBar(moduleId, dates)}
        barTooltip={barTooltip}
      />
    </div>
  );
}
