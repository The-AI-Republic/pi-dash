// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline chart (parity/specs/issues.md §6, ISS-044–048/056–059/116). One
// scroll container holds a sticky-left sidebar and a day-indexed strip:
// the header carries two tiers, every lane lines up with its sidebar row
// and scrolls vertically with it, and bars sit on their day columns.
// Today draws as a marked column (its week at Month zoom, its month at
// Quarter zoom) and the strip extends endlessly at either horizontal
// edge. Fullscreen renders the same chart inside the full-screen portal.
// Edit standing (ISS-050–055/058) turns on the bar gestures, the add-block
// control on undated lanes and the sticky quick-add; without it the chart
// is read-only. Bars fade toward a missing end with the name pinned
// visible on scroll, off-screen bars offer a scroll-to-block arrow, and
// the sidebar carries bulk checkboxes plus the load-more sentinel.

import { cn } from "@pidash/kit";
import * as React from "react";

import { FULL_SCREEN_PORTAL_ID, OverlayPortal } from "../../../shared/shell/index.js";
import { GroupSelectionCheckbox, type GroupSelectionState } from "../bulk/index.js";
import { formatDay, todayIsoDay } from "../dropdowns/calendar.js";
import {
  addBlockDate,
  DRAG_THRESHOLD_PX,
  movedDates,
  plantedDates as plantedDatesFor,
  resizedDates,
  snappedDayDelta,
  type GestureDates,
  type ResizeResult,
} from "./drag.js";
import { DependencyChrome, showsGanttDependencies } from "./DependencyChrome.js";
import { GanttHeader } from "./GanttHeader.js";
import { GanttSidebar, type GanttTimelineEntity, type ReorderBinding } from "./GanttSidebar.js";
import {
  HEADER_HEIGHT,
  ROW_HEIGHT,
  barBox,
  barInView,
  barMask,
  datedBarBox,
  hasBar,
  rowWindow,
  type GanttBarBox,
  type GanttRow,
} from "./rows.js";
import {
  DAY_WIDTHS,
  addDays,
  dayColumns,
  growRangeToCover,
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

/**
 * Bulk selection binding for the sidebar (ISS-116). The layout owns the
 * selection state; the chart only renders the header tri-state and hands
 * the row toggles to the sidebar. Null hides every checkbox.
 */
export interface GanttSelectionBinding {
  isSelected: (id: string) => boolean;
  onRowToggle: (id: string, shiftKey: boolean) => void;
  groupState: GroupSelectionState;
  onGroupToggle: () => void;
  /** False when no row can be selected (all epics, all unconfirmed). */
  hasSelectables: boolean;
}

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
  /** Bar tooltip text; default the row name (NEWFRONT-801: name + dates). */
  barTooltip?: ((row: GanttRow) => string) | undefined;
  peekedId?: string | null;
  onOpenRow: (row: GanttRow) => void;
  /** Rows still arriving from the next page. */
  loadingMore?: boolean;
  /** The server holds more rows than rendered (ISS-059). */
  hasMore?: boolean;
  /** Bulk selection binding; null hides every checkbox (ISS-116). */
  selection?: GanttSelectionBinding | null;
  /** Manual-sort reorder seam; null disables row dragging (ISS-050). */
  reorder?: ReorderBinding | null;
  /**
   * Fires when a row press travels without a reorder binding; never while
   * one is mounted (NEWFRONT-801: the modules timeline gates on manual sort).
   */
  onReorderAttempt?: (() => void) | undefined;
  /** Another entity's row vocabulary; absent keeps the issue chart. */
  entity?: GanttTimelineEntity | undefined;
  /** Project admin/member standing: bars drag and lanes plant (ISS-058). */
  canEdit?: boolean;
  /** Persist a moved or resized bar's dates (ISS-051–053). */
  onDatesChange?: (issueId: string, dates: { startDate: string | null; endDate: string | null }) => void;
  /**
   * Plant a block on an undated row (ISS-054). Absent, undated lanes keep
   * only their no-dates reading (NEWFRONT-801: modules add from the header).
   */
  onPlantBlock?: (issueId: string, dates: { startDate: string | null; endDate: string | null }) => void;
  /** Sticky quick-add entry (ISS-055); absent hides the control. */
  quickAdd?: React.ReactNode;
  /**
   * Project the dependency reads belong to. Absent (or the edition flag
   * off) leaves the timeline with no dependency chrome at all (ISS-237).
   */
  projectId?: string | undefined;
  /**
   * Hover preview for a reference surface (ISS-133). Receives the bar or
   * sidebar link to wrap; absent leaves the surfaces plain.
   */
  renderPreview?:
    | ((row: GanttRow, trigger: React.ReactElement<Record<string, unknown>>) => React.ReactNode)
    | undefined;
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

/** A live bar gesture, with the dates it will commit. */
interface BarDrag {
  issueId: string;
  side: "move" | "left" | "right";
  startX: number;
  origin: GestureDates;
  /** The window drawn under the pointer, including a filled missing end. */
  dates: GestureDates;
  /** The ends to persist; the non-dragged end may stay missing. */
  write: GestureDates;
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

function Bar({
  row,
  box,
  drag,
  colors,
  barTooltip,
  canEdit,
  openable,
  onOpen,
  onBeginDrag,
  consumeGestureClick,
  range,
  zoom,
  renderPreview,
}: {
  row: GanttRow;
  box: GanttBarBox;
  drag: BarDrag | null;
  colors: Readonly<Record<string, string>> | undefined;
  barTooltip: ((row: GanttRow) => string) | undefined;
  canEdit: boolean;
  /** False for an unconfirmed create: the bar draws but never opens (ISS-055). */
  openable: boolean;
  onOpen: () => void;
  onBeginDrag: (event: React.MouseEvent, side: "move" | "left" | "right") => void;
  /** True for the click that ends a bar gesture, which must not open (ISS-051–053). */
  consumeGestureClick: () => boolean;
  range: Parameters<typeof dayColumns>[0];
  zoom: GanttZoom;
  renderPreview?: GanttChartProps["renderPreview"];
}): React.ReactElement {
  // While a gesture runs the bar follows the pointer; on release the
  // persisted dates flow back through the list cache (ISS-051).
  const [hoveredSide, setHoveredSide] = React.useState<"left" | "right" | null>(null);
  const previewIso = React.useMemo(() => {
    if (!canEdit) return null;
    if (drag !== null) {
      return drag.side === "left" ? drag.dates.startDate : drag.side === "right" ? drag.dates.targetDate : null;
    }
    if (hoveredSide === null) return null;
    return hoveredSide === "left" ? row.startIso : row.targetIso;
  }, [canEdit, drag, hoveredSide, row.startIso, row.targetIso]);
  const boxNow =
    drag !== null && drag.dates.startDate !== null && drag.dates.targetDate !== null
      ? datedBarBox(drag.dates.startDate, drag.dates.targetDate, range, zoom)
      : box;
  // The fade follows the committed dates, not the live gesture: while a
  // resize fills a missing end the bar keeps its half-dated look until
  // the release persists (ISS-051/056).
  const mask = barMask(row);
  const bar = (
    <div
      id={`gantt-block-${row.id}`}
      data-testid="gantt-bar"
      role="button"
      aria-label={row.name}
      title={barTooltip === undefined ? row.name : barTooltip(row)}
      style={{
        left: boxNow.left,
        width: boxNow.width,
      }}
      onClick={() => {
        // A handle press or a move still dispatches a click after mouseup;
        // it belongs to the gesture, not to the row (ISS-051–053). Only the
        // gesture's own click is consumed, so a plain click still opens.
        if (consumeGestureClick()) return;
        if (drag === null && openable) onOpen();
      }}
      onKeyDown={(event) => {
        if (openable && (event.key === "Enter" || event.key === " ")) onOpen();
      }}
      onMouseDown={(event) => onBeginDrag(event, "move")}
      aria-disabled={openable ? undefined : true}
      tabIndex={openable ? 0 : -1}
      className={cn(
        "group/bar absolute top-1/2 h-6 -translate-y-1/2 overflow-visible opacity-90 hover:opacity-100 focus-visible:outline-2 focus-visible:outline-(--accent)",
        openable ? "cursor-pointer" : "cursor-default"
      )}
    >
      <div
        aria-hidden="true"
        data-testid="gantt-bar-body"
        style={{
          backgroundColor: colors?.[row.id] ?? "var(--accent)",
          maskImage: mask ?? undefined,
          WebkitMaskImage: mask ?? undefined,
        }}
        className="absolute inset-0 rounded-(--radius-control)"
      />
      <div
        data-testid="gantt-bar-name"
        style={{ left: SIDEBAR_WIDTH, maskImage: mask ?? undefined, WebkitMaskImage: mask ?? undefined }}
        className="text-caption sticky w-fit max-w-full truncate px-(--space-2) leading-6 text-(--on-accent)"
      >
        {row.name}
      </div>
      {canEdit ? (
        <>
          <span
            data-testid="gantt-resize-left"
            aria-label={row.startIso === null ? undefined : formatDay(row.startIso)}
            onMouseDown={(event) => onBeginDrag(event, "left")}
            onMouseEnter={() => setHoveredSide("left")}
            onMouseLeave={() => setHoveredSide(null)}
            className="absolute top-0 -left-1.5 h-full w-3 cursor-col-resize"
          />
          <span
            data-testid="gantt-resize-right"
            aria-label={row.targetIso === null ? undefined : formatDay(row.targetIso)}
            onMouseDown={(event) => onBeginDrag(event, "right")}
            onMouseEnter={() => setHoveredSide("right")}
            onMouseLeave={() => setHoveredSide(null)}
            className="absolute top-0 -right-1.5 h-full w-3 cursor-col-resize"
          />
          {previewIso !== null ? (
            <span
              data-testid="gantt-resize-preview"
              className="text-caption shadow pointer-events-none absolute -top-6 left-1/2 -translate-x-1/2 rounded-(--radius-control) bg-(--surface) px-(--space-1) whitespace-nowrap text-(--text)"
            >
              {formatDay(previewIso)}
            </span>
          ) : null}
        </>
      ) : null}
    </div>
  );
  // The hover preview wraps the bar as its reference surface (ISS-133).
  return <>{renderPreview === undefined ? bar : renderPreview(row, bar)}</>;
}

function UndatedLane({
  canEdit,
  rangeStartIso,
  zoom,
  onPlant,
}: {
  canEdit: boolean;
  rangeStartIso: string;
  zoom: GanttZoom;
  /** Absent, the lane keeps only its no-dates reading (no add-block). */
  onPlant?: ((dates: { startDate: string | null; endDate: string | null }) => void) | undefined;
}): React.ReactElement {
  const [hover, setHover] = React.useState<{ x: number; date: string } | null>(null);
  if (!canEdit || onPlant === undefined) {
    return (
      <span className="text-caption absolute inset-y-0 left-0 flex items-center px-(--space-3) text-(--text-muted)/70">
        No dates
      </span>
    );
  }
  return (
    <div
      data-testid="gantt-undated-lane"
      className="absolute inset-0"
      onMouseMove={(event) => {
        const box = event.currentTarget.getBoundingClientRect();
        const x = event.clientX - box.left;
        const dayIndex = Math.max(0, Math.floor(x / DAY_WIDTHS[zoom]));
        setHover({ x, date: addBlockDate({ startIso: rangeStartIso }, dayIndex) });
      }}
      onMouseLeave={() => setHover(null)}
    >
      <span className="text-caption pointer-events-none absolute inset-y-0 left-0 flex items-center px-(--space-3) text-(--text-muted)/70">
        No dates
      </span>
      {hover !== null ? (
        <button
          type="button"
          data-testid="gantt-add-block"
          aria-label={`Add a block on ${formatDay(hover.date)}`}
          title={formatDay(hover.date)}
          style={{ left: hover.x }}
          onClick={() => {
            const dayIndex = Math.max(0, Math.floor(hover.x / DAY_WIDTHS[zoom]));
            const dates = plantedDatesFor(rangeStartIso, dayIndex, zoom);
            onPlant({ startDate: dates.startDate, endDate: dates.targetDate });
          }}
          className="absolute top-1/2 grid size-7 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-(--radius-control) border border-(--border) bg-(--bg) text-(--text)"
        >
          +
        </button>
      ) : null}
    </div>
  );
}

function GanttLanes({
  rows,
  zoom,
  range,
  barColors,
  barTooltip,
  peekedId,
  hoveredId,
  onHover,
  onOpenRow,
  onScrollToBar,
  windowStart,
  windowEnd,
  viewportLeft,
  viewportWidth,
  canEdit,
  onDatesChange,
  onPlantBlock,
  rangeStartIso,
  renderPreview,
}: {
  rows: readonly GanttRow[];
  zoom: GanttZoom;
  range: Parameters<typeof dayColumns>[0];
  barColors: Readonly<Record<string, string>> | undefined;
  barTooltip: ((row: GanttRow) => string) | undefined;
  peekedId: string | null;
  hoveredId: string | null;
  onHover: (id: string | null) => void;
  onOpenRow: (row: GanttRow) => void;
  /** Bring an off-viewport bar into the visible slice. */
  onScrollToBar: (row: GanttRow) => void;
  windowStart: number;
  windowEnd: number;
  /** Visible strip slice, in strip coordinates. */
  viewportLeft: number;
  viewportWidth: number;
  canEdit: boolean;
  onDatesChange: (issueId: string, dates: { startDate: string | null; endDate: string | null }) => void;
  onPlantBlock?: ((issueId: string, dates: { startDate: string | null; endDate: string | null }) => void) | undefined;
  rangeStartIso: string;
  renderPreview?: GanttChartProps["renderPreview"];
}): React.ReactElement {
  const visible = rows.slice(windowStart, windowEnd);
  const [drag, setDrag] = React.useState<BarDrag | null>(null);
  const suppressed = React.useRef(false);
  const consumeGestureClick = React.useCallback(() => {
    const wasSuppressed = suppressed.current;
    suppressed.current = false;
    return wasSuppressed;
  }, []);
  const dayWidth = DAY_WIDTHS[zoom];

  const beginDrag = (event: React.MouseEvent, row: GanttRow, side: "move" | "left" | "right") => {
    if (!canEdit || row.unconfirmed || event.button !== 0) return;
    // A press on a resize handle is always a gesture, even without travel.
    // A body press is only a gesture once it moves past the click threshold.
    suppressed.current = side !== "move";
    event.preventDefault();
    event.stopPropagation();
    const origin = { startDate: row.startIso, targetDate: row.targetIso };
    setDrag({ issueId: row.id, side, startX: event.clientX, origin, dates: origin, write: origin });
  };

  // The listener set is re-registered for every gesture state, so the
  // handlers below always see the live gesture.
  React.useEffect(() => {
    if (drag === null) return;
    const onMove = (event: MouseEvent) => {
      if (Math.abs(event.clientX - drag.startX) >= DRAG_THRESHOLD_PX) suppressed.current = true;
      const delta = snappedDayDelta(event.clientX - drag.startX, dayWidth);
      if (drag.side === "move") {
        // Half-dated rows draw a bar but do not move (ISS-051).
        const dates = movedDates(drag.origin, delta);
        if (dates !== null) setDrag({ ...drag, dates, write: dates });
        return;
      }
      // A bar only mounts on a row with at least one date, so a resize
      // always has a window to draw.
      const resized = resizedDates(drag.origin, drag.side, delta) as ResizeResult;
      setDrag({ ...drag, dates: resized.window, write: resized.write });
    };
    const onUp = () => {
      setDrag(null);
      const changed =
        drag.write.startDate !== drag.origin.startDate || drag.write.targetDate !== drag.origin.targetDate;
      if (changed) onDatesChange(drag.issueId, { startDate: drag.write.startDate, endDate: drag.write.targetDate });
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setDrag(null);
      }
    };
    globalThis.addEventListener("mousemove", onMove);
    globalThis.addEventListener("mouseup", onUp);
    globalThis.addEventListener("keydown", onKey);
    return () => {
      globalThis.removeEventListener("mousemove", onMove);
      globalThis.removeEventListener("mouseup", onUp);
      globalThis.removeEventListener("keydown", onKey);
    };
  }, [dayWidth, drag, onDatesChange, setDrag]);

  return (
    <div data-testid="gantt-lanes">
      <div style={{ height: windowStart * ROW_HEIGHT }} aria-hidden="true" />
      {visible.map((row) => {
        const box = hasBar(row) ? barBox(row, range, zoom) : null;
        return (
          <div
            key={row.id}
            data-lane={row.id}
            data-gantt-lane="true"
            data-lane-id={row.id}
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
            {row.unconfirmed && box === null ? (
              // An unconfirmed create with no dates is not a real issue
              // yet: its lane pulses instead of offering dates (ISS-006/049).
              <span
                aria-hidden="true"
                data-testid="gantt-lane-unconfirmed"
                className="absolute inset-x-(--space-3) top-1/2 h-4 -translate-y-1/2 animate-pulse rounded-(--radius-control) bg-(--subtle)/60"
              />
            ) : box === null ? (
              <UndatedLane
                canEdit={canEdit}
                rangeStartIso={rangeStartIso}
                zoom={zoom}
                onPlant={onPlantBlock === undefined ? undefined : (dates) => onPlantBlock(row.id, dates)}
              />
            ) : (
              <Bar
                row={row}
                box={box}
                drag={drag?.issueId === row.id ? drag : null}
                colors={barColors}
                barTooltip={barTooltip}
                // A quick-add bar shows immediately but stays inert until
                // the server confirms the row (ISS-055).
                canEdit={canEdit && !row.unconfirmed}
                openable={!row.unconfirmed}
                onOpen={() => onOpenRow(row)}
                onBeginDrag={(event, side) => beginDrag(event, row, side)}
                consumeGestureClick={consumeGestureClick}
                range={range}
                zoom={zoom}
                renderPreview={renderPreview}
              />
            )}
            {box !== null && !barInView(box, viewportLeft, viewportWidth) ? (
              <button
                type="button"
                data-testid="gantt-scroll-arrow"
                aria-label={`Scroll to ${row.name}`}
                onClick={() => onScrollToBar(row)}
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
  barTooltip,
  peekedId = null,
  onOpenRow,
  loadingMore = false,
  hasMore = false,
  selection = null,
  reorder = null,
  onReorderAttempt,
  entity,
  canEdit = false,
  onDatesChange,
  onPlantBlock,
  quickAdd,
  projectId,
  renderPreview,
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
  const { zoom, range, scrollLeft, setZoom, centerToday, setScrollLeft, extend, cover, measure } = viewport;
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
  const scrollToBar = React.useCallback(
    (row: GanttRow) => {
      // A bar far outside the rendered window has no columns to land on,
      // so the range grows to cover its span first (ISS-057). A partial
      // bar draws two days from its known end; the covered span mirrors
      // that geometry so the whole bar lands on rendered columns. The
      // lane renders an arrow only for a dated row, so one end — and the
      // box below — is always set.
      const from = row.startIso ?? addDays(row.targetIso!, -1);
      const to = row.targetIso ?? addDays(row.startIso!, 1);
      const grown = growRangeToCover(range, zoom, from, to);
      cover(from, to);
      const box = barBox(row, grown, zoom)!;
      setScrollRequest(Math.max(0, box.left - SIDEBAR_WIDTH - EDGE_TRIGGER_PX / 2));
    },
    [cover, range, zoom]
  );

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

  // Dependency chrome belongs to the cloud edition (ISS-237). The flag
  // alone decides; with it off nothing below mounts, so the community
  // chart draws exactly the lanes it always has.
  const dependencyProject = showsGanttDependencies(projectId) ? projectId : null;

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
        countNoun={entity?.countNoun}
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
              className="sticky top-0 z-30 flex items-end justify-between border-r border-b border-(--border) bg-(--surface) px-(--space-3)"
              style={{ height: HEADER_HEIGHT }}
            >
              <span className="flex items-center gap-(--space-2)">
                {selection !== null && selection.hasSelectables ? (
                  <GroupSelectionCheckbox
                    state={selection.groupState}
                    label="Select all work items"
                    onToggle={selection.onGroupToggle}
                  />
                ) : null}
                <span aria-hidden="true" className="text-caption font-medium text-(--text-muted)">
                  {entity?.sidebarTitle ?? "Issue"}
                </span>
              </span>
              {entity?.sidebarMeta === null ? null : (
                <span aria-hidden="true" className="text-caption text-(--text-muted)">
                  {entity?.sidebarMeta ?? "Duration"}
                </span>
              )}
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
                hasMore={hasMore}
                selection={selection}
                windowStart={window.start}
                windowEnd={window.end}
                reorder={reorder}
                onReorderAttempt={onReorderAttempt}
                entity={entity}
                renderPreview={renderPreview}
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
                barTooltip={barTooltip}
                peekedId={peekedId}
                hoveredId={hoveredId}
                onHover={setHoveredId}
                onOpenRow={onOpenRow}
                onScrollToBar={scrollToBar}
                windowStart={window.start}
                windowEnd={window.end}
                viewportLeft={stripLeft}
                viewportWidth={stripViewport}
                canEdit={canEdit}
                onDatesChange={onDatesChange ?? (() => undefined)}
                onPlantBlock={onPlantBlock}
                rangeStartIso={range.startIso}
                renderPreview={renderPreview}
              />
            )}
            {dependencyProject === null ? null : (
              <DependencyChrome
                workspaceSlug={workspaceSlug}
                projectId={dependencyProject}
                rows={rows}
                range={range}
                zoom={zoom}
                width={total}
              />
            )}
          </div>
        </div>
      </div>
      {quickAdd ?? null}
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
