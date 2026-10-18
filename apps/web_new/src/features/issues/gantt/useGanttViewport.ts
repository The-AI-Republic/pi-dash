// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline viewport state (parity/specs/issues.md §6, ISS-045/046/048).
// Zoom, the rendered day window, the measured strip width and the
// horizontal offset live together here because every operation moves all
// of them: a zoom switch resets the window and re-centers today, Today
// re-centers the current window, and an edge extension grows the window
// (compensating the offset when days are prepended). Nothing persists.

import * as React from "react";

import {
  centeredScroll,
  extendRange,
  initialRange,
  isGanttZoom,
  timelineWidth,
  type GanttRange,
  type GanttZoom,
} from "./scale.js";

/** Strip width assumed until the container has been measured. */
export const FALLBACK_VIEWPORT_WIDTH = 1024;

export interface GanttViewport {
  zoom: GanttZoom;
  range: GanttRange;
  /** Horizontal offset of the strip in pixels. */
  scrollLeft: number;
  /** Zoom switch: new window centered on today (session-local). */
  setZoom: (zoom: GanttZoom) => void;
  /** Scroll today into view at the current zoom. */
  centerToday: () => void;
  /** Record the offset the browser reported. */
  setScrollLeft: (value: number) => void;
  /** Grow the window on one side, keeping the viewport still. */
  extend: (side: "left" | "right") => void;
  /** Record the measured strip width; centers once per zoom. */
  measure: (stripWidth: number) => void;
}

/** Clamp a candidate offset onto the strip. */
export function clampScroll(value: number, total: number, viewportWidth: number): number {
  return Math.max(0, Math.min(value, Math.max(0, total - viewportWidth)));
}

export function useGanttViewport(todayIso: string, weekStart: number): GanttViewport {
  const [zoom, setZoomState] = React.useState<GanttZoom>("week");
  const [range, setRange] = React.useState<GanttRange>(() => initialRange("week", todayIso, weekStart));
  const [scrollLeft, setScrollLeft] = React.useState(0);
  const [viewportWidth, setViewportWidth] = React.useState(FALLBACK_VIEWPORT_WIDTH);
  const settled = React.useRef(false);

  const setZoom = React.useCallback(
    (next: GanttZoom) => {
      if (!isGanttZoom(next)) return;
      setZoomState(next);
      setRange(initialRange(next, todayIso, weekStart));
      settled.current = false;
    },
    [todayIso, weekStart]
  );

  const centerToday = React.useCallback(() => {
    setScrollLeft(centeredScroll(todayIso, range, zoom, viewportWidth));
  }, [range, todayIso, viewportWidth, zoom]);

  const measure = React.useCallback(
    (stripWidth: number) => {
      if (stripWidth <= 0) return;
      // Width drift (a resize) must not yank the view; only the first
      // measurement after a zoom re-centers.
      setViewportWidth(stripWidth);
      if (settled.current) return;
      settled.current = true;
      setScrollLeft(centeredScroll(todayIso, range, zoom, stripWidth));
    },
    [range, todayIso, zoom]
  );

  const extend = React.useCallback(
    (side: "left" | "right") => {
      const next = extendRange(range, side, zoom);
      setRange(next);
      if (side === "left") {
        // Prepend compensation: the strip grows on the left by the
        // appended width, so holding the offset keeps the view still.
        setScrollLeft((current) => current + timelineWidth(next, zoom) - timelineWidth(range, zoom));
      }
    },
    [range, zoom]
  );

  return {
    zoom,
    range,
    scrollLeft,
    setZoom,
    centerToday,
    setScrollLeft,
    extend,
    measure,
  };
}
