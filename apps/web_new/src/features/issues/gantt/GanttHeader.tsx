// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline chrome above the chart (parity/specs/issues.md §6, ISS-044–047).
// A live row count, the three zoom levels as a segmented switcher, Today,
// and the expand/shrink control that moves the chart in and out of the
// full-screen portal. Purely presentational: actions arrive as callbacks.

import { Button, cn, IconButton } from "@pidash/kit";
import { Maximize2, Minimize2 } from "lucide-react";
import * as React from "react";

import { GANTT_ZOOM_LABELS, GANTT_ZOOMS, type GanttZoom } from "./scale.js";

export interface GanttHeaderProps {
  rowCount: number;
  zoom: GanttZoom;
  onZoom: (zoom: GanttZoom) => void;
  onToday: () => void;
  fullscreen: boolean;
  onToggleFullscreen: () => void;
  /** Rendered beside the count, e.g. later slices' bulk actions. */
  actions?: React.ReactNode;
}

export function GanttHeader({
  rowCount,
  zoom,
  onZoom,
  onToday,
  fullscreen,
  onToggleFullscreen,
  actions,
}: GanttHeaderProps): React.ReactElement {
  return (
    <div
      data-testid="gantt-header"
      className="flex shrink-0 flex-wrap items-center gap-(--space-2) border-b border-(--border) bg-(--surface) px-(--space-3) py-(--space-2)"
    >
      <p data-testid="gantt-count" className="text-caption text-(--text-muted)">
        {rowCount} {rowCount === 1 ? "work item" : "work items"}
      </p>
      <div className="ml-auto flex flex-wrap items-center gap-(--space-2)">
        {actions}
        <div role="group" aria-label="Timeline zoom" className="flex items-center gap-(--space-1)">
          {GANTT_ZOOMS.map((level) => {
            const active = level === zoom;
            return (
              <Button
                key={level}
                variant="ghost"
                size="small"
                data-zoom-option={level}
                aria-pressed={active}
                data-zoom-active={active ? "true" : undefined}
                onClick={() => {
                  if (!active) onZoom(level);
                }}
                className={cn(active && "bg-(--subtle) font-semibold text-(--text)")}
              >
                {GANTT_ZOOM_LABELS[level]}
              </Button>
            );
          })}
        </div>
        <Button variant="ghost" size="small" data-testid="gantt-today" onClick={onToday}>
          Today
        </Button>
        <IconButton
          label={fullscreen ? "Exit full screen" : "Expand timeline"}
          variant="secondary"
          size="small"
          data-testid="gantt-fullscreen"
          onClick={onToggleFullscreen}
        >
          {fullscreen ? (
            <Minimize2 size={14} strokeWidth={1.5} aria-hidden="true" />
          ) : (
            <Maximize2 size={14} strokeWidth={1.5} aria-hidden="true" />
          )}
        </IconButton>
      </div>
    </div>
  );
}
