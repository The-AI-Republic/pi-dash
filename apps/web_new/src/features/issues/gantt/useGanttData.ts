// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline data side (parity/specs/issues.md §6 and §14, ISS-044/049/165).
// The chart reads the project's states once (bar tint, later chrome slice
// draws it) and resolves a row click to the shared peek panel: desktop
// docks the panel with the issue's detail query, mobile leaves for the
// canonical browse route. Nothing here refetches issues — the layout root
// owns list freshness.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";
import type { State } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRoles } from "../../auth/index.js";
import { useNarrowViewport } from "../../../shared/shell/index.js";
import { projectsQueryOptions } from "../../projects/index.js";
import type { GanttRow } from "./rows.js";

/** Query key suffix for one project's state list under the workspace. */
export function ganttStatesKey(workspaceSlug: string, projectId: string): readonly string[] {
  return ["ws", workspaceSlug, "issues", "gantt-states", projectId];
}

/** Read the project's workflow states, keyed per workspace and project. */
export function useGanttStates(workspaceSlug: string, projectId: string): State[] {
  const query = useQuery({
    queryKey: ganttStatesKey(workspaceSlug, projectId),
    queryFn: async () => (await import("@pidash/api-client")).listStates(getAppClient(), workspaceSlug, projectId),
    staleTime: 5 * 60_000,
  });
  return query.data ?? [];
}

/**
 * The caller's project role, or null when they hold no membership row.
 * The per-workspace role map is already warmed by the gated route, so the
 * timeline gates its edit affordances without another request pattern.
 */
export function useProjectMemberRole(workspaceSlug: string, projectId: string): number | null {
  const roles = useProjectRoles(workspaceSlug);
  return roles.data?.[projectId] ?? null;
}

/**
 * Whether the project is archived, read from the workspace's project
 * list the gated route already holds (ISS-222). An archive hides every
 * edit affordance; a project the list does not carry reads as live so
 * the timeline never goes read-only on a missing row.
 */
export function useProjectArchived(workspaceSlug: string, projectId: string): boolean {
  const client = getAppClient();
  const { data: projects } = useQuery(projectsQueryOptions(client, workspaceSlug));
  const project = projects?.find((row) => row.id === projectId);
  return project !== undefined && project.archived_at !== null;
}

/** Bulk toolbar field flags for one project (ISS-116). */
export interface GanttBulkFlags {
  hideCycle: boolean;
  hideModule: boolean;
  hideEstimate: boolean;
}

/**
 * The bulk toolbar's field flags from the workspace's project list
 * (ISS-116). The gated route already holds the list, so the timeline
 * gates the cycle, module and estimate entries without another request.
 * A project the list does not carry shows every field.
 */
export function useProjectBulkFlags(workspaceSlug: string, projectId: string): GanttBulkFlags {
  const client = getAppClient();
  const { data: projects } = useQuery(projectsQueryOptions(client, workspaceSlug));
  const project = projects?.find((row) => row.id === projectId);
  if (project === undefined) return { hideCycle: false, hideModule: false, hideEstimate: false };
  return {
    hideCycle: !project.cycle_view,
    hideModule: !project.module_view,
    hideEstimate: project.estimate == null,
  };
}

/** Pick the state that colors a row's bar. */
export function stateColorOf(states: readonly State[], stateId: string | null): string | null {
  if (stateId === null) return null;
  return states.find((state) => state.id === stateId)?.color ?? null;
}

/** The peek panel state the timeline opens; null when closed. */
export interface GanttPeek {
  row: GanttRow;
}

export interface GanttPeekHandle {
  peek: GanttPeek | null;
  openRow: (row: GanttRow) => void;
  closePeek: () => void;
}

/**
 * Row activation (ISS-049/165): desktop opens the shared peek panel with
 * the issue's detail query; a narrow viewport goes to the browse route.
 * Detail is hydrated through its query options so the panel paints from
 * the cache the route already owns. An unconfirmed create has no server
 * row to read yet, so activating it does nothing (ISS-006/049).
 *
 * A host that owns the peek address (the issues route, a saved view) is
 * handed the activated row and mounts the panel itself, so the URL and the
 * panel never disagree; without one the hook keeps the panel locally,
 * which is the standalone timeline mount.
 */
export function useGanttPeek(workspaceSlug: string, onOpen: ((row: GanttRow) => void) | null = null): GanttPeekHandle {
  const [peek, setPeek] = React.useState<GanttPeek | null>(null);
  const narrow = useNarrowViewport();
  const navigate = useNavigate();
  const openRow = React.useCallback(
    (row: GanttRow) => {
      if (row.unconfirmed) return;
      if (narrow) {
        void navigate({ to: "/$ws/browse/$issueKey", params: { ws: workspaceSlug, issueKey: row.identifier } });
        return;
      }
      if (onOpen !== null) {
        onOpen(row);
        return;
      }
      setPeek({ row });
    },
    [narrow, navigate, onOpen, workspaceSlug]
  );
  const closePeek = React.useCallback(() => setPeek(null), []);
  return { peek, openRow, closePeek };
}
