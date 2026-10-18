// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline chart (parity/specs/issues.md §6, ISS-044–048). One scroll
// container holds a sticky-left sidebar and a day-indexed strip: the
// header carries two tiers, every lane lines up with its sidebar row and
// scrolls vertically with it, and bars sit on their day columns. Today
// draws as a marked column (its week at Month zoom, its month at Quarter
// zoom) and the strip extends endlessly at either horizontal edge.
// Fullscreen renders the same chart inside the full-screen portal.

import { cn } from "@pidash/kit";
import * as React from "react";

import { FULL_SCREEN_PORTAL_ID, OverlayPortal } from "../../../shared/shell/index.js";
import { todayIsoDay } from "../dropdowns/calendar.js";
import { GanttHeader } from "./GanttHeader.js";
import { GanttSidebar } from "./GanttSidebar.js";
import {
  HEADER_HEIGHT,
  ROW_HEIGHT,
  barInView,
  barBox,
  hasBar,
  rowWindow,
  type GanttBarBox,
  type GanttRow,
} from "./rows.js";
import {
  DAY_WIDTHS,
  dayColumns,
  headerTiers,
  timelineWidth,
  windowTodaySegment,
  type GanttSegment,
  type GanttZoom,
} from "./scale.js";
import { useMeasuredSize } from "./useMeasuredSize.js";
import { FALLBACK_VIEWPORT_WIDTH, useGanttViewport, type GanttViewport } from "./useGanttViewport.js";

/** Width of the sticky sidebar column. */
export const SIDEBAR_WIDTH = 288;
/** Distance from an edge that triggers an append. */
const EDGE_TRIGGER_PX = 240;
/** Ignore re-extension for this long after one, so a pinned edge settles. */
const EXTEND_COOLDOWN_MS = 350;

export interface GanttChartProps {
  rows: readonly GanttRow[];
  /** Workspace slug for the sidebar's browse links. */
  workspaceSlug: string;
  /** Server total across all pages (live header count, ISS-044). */
  totalCount: number;
  /** Profile's first day of the week, Sunday-first. */
  weekStart: number;
  /** Bar color per row id; rows without an entry use the accent. */
  barColors?: Readonly<Record<string, string>>;
  peekedId?: string | null;
  onOpenRow: (row: GanttRow) => void;
  /** Rows still arriving from the next page. */
  loadingMore?: boolean;
}

interface SurfaceProps extends GanttChartProps {
  todayIso: string;
  viewport: GanttViewport;
  hoveredId: string | null;
  onHover: (id: string | null) => void;
  /** Why the surface is mounted (drives the page background tone). */
  fullscreen: boolean;
  onToggleFullscreen: () => void;
}

function SegmentRow({
  tier,
  segments,
  width,
  todayKey,
  markedTone,
}: {
  /** Which of the two header rows this renders. */
  tier: "coarse" | "fine";
  segments: readonly GanttSegment[];
  width: number;
  todayKey: string | null;
  markedTone: boolean;
}): React.ReactElement {
  return (
    <div data-tier={tier} className="flex h-6">
      {segments.map((segment) => {
        const marked = markedTone && todayKey === segment.key;
        return (
          <div
            key={segment.key}
            data-segment-key={segment.key}
            data-today-segment={marked ? "true" : undefined}
            title={marked ? "Current" : undefined}
            style={{ width: segment.days * width }}
            className={cn(
              "text-caption flex shrink-0 items-center overflow-hidden border-r border-(--border) px-(--space-1) text-(--text-muted)",
              marked && "bg-(--accent-subtle) font-medium text-(--accent)"
            )}
          >
            <span className="truncate">{segment.label}</span>
          </div>
        );
      })}
    </div>
  );
}

function DayTintLayer({
  range,
  zoom,
  weekStart,
}: {
  range: Parameters<typeof dayColumns>[0];
  zoom: GanttZoom;
  weekStart: number;
}): React.ReactElement {
  const width = DAY_WIDTHS[zoom];
  return (
    <div aria-hidden="true" data-testid="gantt-day-columns" className="pointer-events-none absolute inset-0 flex">
      {dayColumns(range, weekStart).map((day) => (
        <div
          key={day.iso}
          data-day={day.iso}
          data-week-start={day.startsWeek ? "true" : undefined}
          data-weekend={day.weekend ? "true" : undefined}
          style={{ width }}
          className={cn(
            "h-full shrink-0 border-r border-(--border)/40",
            day.weekend && "bg-(--subtle)/70",
            day.startsWeek && "border-l border-(--border)"
          )}
        />
      ))}
    </div>
  );
}

function GanttLanes({
  rows,
  zoom,
  range,
  barColors,
  peekedId,
  hoveredId,
  onHover,
  onOpenRow,
  onScrollToBar,
  windowStart,
  windowEnd,
  viewportLeft,
  viewportWidth,
}: {
  rows: readonly GanttRow[];
  zoom: GanttZoom;
  range: Parameters<typeof dayColumns>[0];
  barColors: Readonly<Record<string, string>> | undefined;
  peekedId: string | null;
  hoveredId: string | null;
  onHover: (id: string | null) => void;
  onOpenRow: (row: GanttRow) => void;
  /** Bring an off-viewport bar into the visible slice. */
  onScrollToBar: (id: string, box: GanttBarBox) => void;
  windowStart: number;
  windowEnd: number;
  /** Visible strip slice, in strip coordinates. */
  viewportLeft: number;
  viewportWidth: number;
}): React.ReactElement {
  const visible = rows.slice(windowStart, windowEnd);
  return (
    <div data-testid="gantt-lanes">
      <div style={{ height: windowStart * ROW_HEIGHT }} aria-hidden="true" />
      {visible.map((row) => {
        const box = hasBar(row) ? barBox(row, range, zoom) : null;
        return (
          <div
            key={row.id}
            data-lane={row.id}
            data-hovered={hoveredId === row.id ? "true" : undefined}
            data-peeked={peekedId === row.id ? "true" : undefined}
            style={{ height: ROW_HEIGHT }}
            onMouseEnter={() => onHover(row.id)}
            onMouseLeave={() => onHover(null)}
            className={cn(
              "relative border-b border-(--border)",
              hoveredId === row.id && "bg-(--subtle)/60",
              peekedId === row.id && "bg-(--accent-subtle)/50"
            )}
          >
            {box === null ? (
              <span className="text-caption absolute inset-y-0 left-0 flex items-center px-(--space-3) text-(--text-muted)/70">
                No dates
              </span>
            ) : (
              <button
                type="button"
                id={`gantt-block-${row.id}`}
                data-testid="gantt-bar"
                aria-label={row.name}
                title={row.name}
                style={{
                  left: box.left,
                  width: box.width,
                  backgroundColor: barColors?.[row.id] ?? "var(--accent)",
                }}
                onClick={() => onOpenRow(row)}
                className="absolute top-1/2 h-6 -translate-y-1/2 cursor-pointer overflow-hidden rounded-(--radius-control) opacity-90 hover:opacity-100 focus-visible:outline-2 focus-visible:outline-(--accent)"
              >
                <span className="text-caption block truncate px-(--space-2) leading-6 text-(--on-accent)">
                  {row.name}
                </span>
              </button>
            )}
            {box !== null && !barInView(box, viewportLeft, viewportWidth) ? (
              <button
                type="button"
                data-testid="gantt-scroll-arrow"
                aria-label={`Scroll to ${row.name}`}
                onClick={() => onScrollToBar(row.id, box)}
                className="sticky left-0 z-20 mx-(--space-1) my-(--space-1) grid size-6 place-items-center rounded-(--radius-control) border border-(--border) bg-(--bg) text-(--text-muted)"
              >
                {box.left + box.width <= viewportLeft ? "←" : "→"}
              </button>
            ) : null}
          </div>
        );
      })}
      <div style={{ height: Math.max(0, rows.length - windowEnd) * ROW_HEIGHT }} aria-hidden="true" />
    </div>
  );
}

function GanttSurface({
  rows,
  workspaceSlug,
  totalCount,
  weekStart,
  barColors,
  peekedId = null,
  onOpenRow,
  loadingMore = false,
  todayIso,
  viewport,
  hoveredId,
  onHover: setHoveredId,
  fullscreen,
  onToggleFullscreen,
}: SurfaceProps): React.ReactElement {
  const [viewportWidth, setViewportWidth] = React.useState(FALLBACK_VIEWPORT_WIDTH);
  const [viewportHeight, setViewportHeight] = React.useState(640);
  const [scrollTop, setScrollTop] = React.useState(0);
  // The scroll host itself is state, so every handler and effect that
  // needs it can run only once it exists (the first effect pass sees
  // null, the next sees the element).
  const [host, setHost] = React.useState<HTMLDivElement | null>(null);
  const [scrollRequest, setScrollRequest] = React.useState<number | null>(null);
  const cooldown = React.useRef(false);
  const { zoom, range, scrollLeft, setZoom, centerToday, setScrollLeft, extend, measure } = viewport;
  const width = DAY_WIDTHS[zoom];
  const total = timelineWidth(range, zoom);

  const handleScroll = React.useCallback(
    (event: React.UIEvent<HTMLDivElement>) => {
      const target = event.currentTarget;
      setScrollLeft(target.scrollLeft);
      setScrollTop(target.scrollTop);
      // The sidebar is sticky inside the same scroller, so the content
      // width includes it; only the strip's own width decides "past edge".
      const overflowing = total + SIDEBAR_WIDTH > target.clientWidth;
      if (!overflowing) return;
      const atRight = target.scrollLeft + target.clientWidth >= total + SIDEBAR_WIDTH - EDGE_TRIGGER_PX;
      const atLeft = target.scrollLeft <= EDGE_TRIGGER_PX;
      if (cooldown.current || (!atLeft && !atRight)) return;
      cooldown.current = true;
      extend(atLeft ? "left" : "right");
      globalThis.setTimeout(() => {
        cooldown.current = false;
      }, EXTEND_COOLDOWN_MS);
    },
    [extend, setScrollLeft, total]
  );

  /** Queue a smooth scroll that brings an off-viewport bar into view. */
  const scrollToBar = React.useCallback((_id: string, box: GanttBarBox) => {
    setScrollRequest(Math.max(0, box.left - SIDEBAR_WIDTH - EDGE_TRIGGER_PX / 2));
  }, []);

  React.useEffect(() => {
    if (host === null || scrollRequest === null) return;
    host.scrollTo({ left: scrollRequest, behavior: "smooth" });
    setScrollLeft(scrollRequest);
    setScrollRequest(null);
  }, [host, scrollRequest, setScrollLeft]);

  // Track the viewport for the row window, and center today once the
  // strip has a measured width (mount and every zoom change).
  const measured = useMeasuredSize(host);
  React.useEffect(() => {
    if (measured.width <= 0) return;
    setViewportWidth(measured.width);
    setViewportHeight(measured.height);
    measure(Math.max(0, measured.width - SIDEBAR_WIDTH));
  }, [measure, measured.height, measured.width]);

  React.useEffect(() => {
    if (host === null) return;
    host.scrollLeft = scrollLeft;
  }, [host, scrollLeft]);

  React.useEffect(() => {
    if (host === null) return;
    host.scrollTop = 0;
    setScrollTop(0);
  }, [host, zoom]);

  const today = windowTodaySegment(zoom, range, todayIso);
  const tiers = headerTiers(zoom, range);
  const empty = rows.length === 0;
  const window = rowWindow(scrollTop, viewportHeight, rows.length, ROW_HEIGHT);
  const stripLeft = Math.max(0, scrollLeft - SIDEBAR_WIDTH);
  const stripViewport = Math.max(0, viewportWidth - SIDEBAR_WIDTH);

  return (
    <div className={cn("flex min-h-0 flex-1 flex-col", fullscreen && "h-full bg-(--bg)")}>
      <GanttHeader
        rowCount={Math.max(totalCount, rows.length)}
        zoom={zoom}
        onZoom={setZoom}
        onToday={centerToday}
        fullscreen={fullscreen}
        onToggleFullscreen={onToggleFullscreen}
      />
      <div
        id="gantt-container"
        ref={setHost}
        data-testid="gantt-container"
        onScroll={handleScroll}
        className="min-h-0 flex-1 overflow-auto"
      >
        <div className="relative flex" style={{ width: SIDEBAR_WIDTH + total }}>
          <div
            data-testid="gantt-sidebar-column"
            className="sticky left-0 z-20 shrink-0 bg-(--surface)"
            style={{ width: SIDEBAR_WIDTH }}
          >
            <div
              aria-hidden="true"
              className="sticky top-0 z-30 flex items-end justify-between border-r border-b border-(--border) bg-(--surface) px-(--space-3)"
              style={{ height: HEADER_HEIGHT }}
            >
              <span className="text-caption font-medium text-(--text-muted)">Issue</span>
              <span className="text-caption text-(--text-muted)">Duration</span>
            </div>
            {empty ? (
              <div data-testid="gantt-sidebar-skeleton" className="flex flex-col gap-(--space-3) p-(--space-3)">
                <span className="h-4 animate-pulse rounded bg-(--subtle)" />
                <span className="h-4 animate-pulse rounded bg-(--subtle)" />
                <span className="h-4 animate-pulse rounded bg-(--subtle)" />
              </div>
            ) : (
              <GanttSidebar
                rows={rows}
                workspaceSlug={workspaceSlug}
                peekedId={peekedId}
                hoveredId={hoveredId}
                onHover={setHoveredId}
                onOpenRow={onOpenRow}
                loadingMore={loadingMore}
                windowStart={window.start}
                windowEnd={window.end}
              />
            )}
          </div>
          <div className="relative" style={{ width: total }}>
            <DayTintLayer range={range} zoom={zoom} weekStart={weekStart} />
            <div
              className="sticky top-0 z-10 border-b border-(--border) bg-(--surface)"
              style={{ height: HEADER_HEIGHT }}
            >
              {empty ? (
                <div className="flex h-full items-center px-(--space-3)">
                  <span className="h-3 w-40 animate-pulse rounded bg-(--subtle)" />
                </div>
              ) : (
                <div className="flex h-full flex-col justify-center">
                  <SegmentRow tier="coarse" segments={tiers.coarse} width={width} todayKey={null} markedTone={false} />
                  <SegmentRow tier="fine" segments={tiers.fine} width={width} todayKey={today.key} markedTone />
                </div>
              )}
            </div>
            <div
              aria-hidden="true"
              data-testid="gantt-today-highlight"
              className="pointer-events-none absolute top-0 z-0 bg-(--accent)/20"
              style={{
                left: today.startIndex * width,
                width: today.days * width,
                height: HEADER_HEIGHT + rows.length * ROW_HEIGHT,
              }}
            />
            {empty ? (
              <div data-testid="gantt-strip-skeleton" className="flex flex-col gap-(--space-3) p-(--space-3)">
                <span className="h-4 animate-pulse rounded bg-(--subtle)" />
                <span className="h-4 animate-pulse rounded bg-(--subtle)" />
              </div>
            ) : (
              <GanttLanes
                rows={rows}
                zoom={zoom}
                range={range}
                barColors={barColors}
                peekedId={peekedId}
                hoveredId={hoveredId}
                onHover={setHoveredId}
                onOpenRow={onOpenRow}
                onScrollToBar={scrollToBar}
                windowStart={window.start}
                windowEnd={window.end}
                viewportLeft={stripLeft}
                viewportWidth={stripViewport}
              />
            )}
          </div>
        </div>
      </div>
      <span className="sr-only" data-testid="gantt-range">
        {`${range.startIso} + ${range.dayCount} days`}
      </span>
    </div>
  );
}

export function GanttChart(props: GanttChartProps): React.ReactElement {
  const { weekStart } = props;
  const [fullscreen, setFullscreen] = React.useState(false);
  const [hoveredId, setHoveredId] = React.useState<string | null>(null);
  // Zoom and range stay above the fullscreen branch: moving the chart into
  // the portal re-parents the surface, and a viewport owned by the surface
  // would remount to Week/initial range on every expand (ISS-047).
  const todayIso = React.useMemo(() => todayIsoDay(), []);
  const viewport = useGanttViewport(todayIso, weekStart);
  const toggle = React.useCallback(() => setFullscreen((open) => !open), []);
  const shared = {
    ...props,
    todayIso,
    viewport,
    hoveredId,
    onHover: setHoveredId,
    onToggleFullscreen: toggle,
  };
  if (!fullscreen) return <GanttSurface {...shared} fullscreen={false} />;
  return (
    <OverlayPortal rootId={FULL_SCREEN_PORTAL_ID}>
      <div data-testid="gantt-fullscreen-root" className="fixed inset-0 z-40 flex flex-col bg-(--bg)">
        <GanttSurface {...shared} fullscreen />
      </div>
    </OverlayPortal>
  );
}
