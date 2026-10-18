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
import { useNarrowViewport } from "../../../shared/shell/index.js";
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
 * the cache the route already owns.
 */
export function useGanttPeek(workspaceSlug: string): GanttPeekHandle {
  const [peek, setPeek] = React.useState<GanttPeek | null>(null);
  const narrow = useNarrowViewport();
  const navigate = useNavigate();
  const openRow = React.useCallback(
    (row: GanttRow) => {
      if (narrow) {
        void navigate({ to: "/$ws/browse/$issueKey", params: { ws: workspaceSlug, issueKey: row.identifier } });
        return;
      }
      setPeek({ row });
    },
    [narrow, navigate, workspaceSlug]
  );
  const closePeek = React.useCallback(() => setPeek(null), []);
  return { peek, openRow, closePeek };
}
