// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dependency chrome for one timeline (parity/specs/issues.md §20/§23,
// ISS-237). This module is only mounted when the edition flag is on, so
// the community chart never runs its reads, never asks for a query client
// and never paints an arrow — the oracle's "no dependency" leg. With the
// flag on it reads every drawn row's finish-to-start relations, turns
// them into arrows at the bars' own geometry and lets a drag from one
// bar's handle link it to another.

import * as React from "react";

import { DependencyLayer } from "./DependencyLayer.js";
import { dependencyEnds, drawableEdges } from "./dependencies.js";
import { isEditionFlagOn, GANTT_DEPENDENCIES_FLAG } from "../edition/flags.js";
import { HEADER_HEIGHT, ROW_HEIGHT, type GanttRow } from "./rows.js";
import type { GanttRange, GanttZoom } from "./scale.js";
import { useGanttDependencies } from "./useGanttDependencies.js";

/**
 * Whether this build draws dependency chrome at all. Pure and hook-free,
 * so the chart above can decide without touching React Query.
 */
export function showsGanttDependencies(
  projectId: string | undefined,
  enabled: boolean = isEditionFlagOn(GANTT_DEPENDENCIES_FLAG)
): projectId is string {
  return enabled && projectId !== undefined && projectId !== "";
}

export interface DependencyChromeProps {
  workspaceSlug: string;
  projectId: string;
  rows: readonly GanttRow[];
  range: GanttRange;
  zoom: GanttZoom;
  /** Strip width the arrows are drawn across. */
  width: number;
}

export function DependencyChrome({
  workspaceSlug,
  projectId,
  rows,
  range,
  zoom,
  width,
}: DependencyChromeProps): React.ReactElement | null {
  const dependencies = useGanttDependencies({ workspaceSlug, projectId, rows });
  // The chart only mounts this module behind the same flag; the component
  // repeats the check so no other surface can paint handles or an arrow in
  // a build that has no dependency capability.
  const enabled = dependencies.enabled;
  const ends = React.useMemo(() => dependencyEnds({ rows, range, zoom, rowHeight: ROW_HEIGHT }), [range, rows, zoom]);
  const edges = React.useMemo(() => drawableEdges(dependencies.edges, ends), [dependencies.edges, ends]);
  const canLinkTo = React.useCallback((toId: string) => ends.has(toId), [ends]);
  const { link } = dependencies;
  const onLink = React.useCallback(
    (fromId: string, toId: string) => {
      link({ fromId, toId });
    },
    [link]
  );
  if (!enabled) return null;
  return (
    <DependencyLayer
      ends={ends}
      edges={edges}
      width={width}
      height={rows.length * ROW_HEIGHT}
      top={HEADER_HEIGHT}
      onLink={onLink}
      canLinkTo={canLinkTo}
    />
  );
}
