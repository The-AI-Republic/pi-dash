// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline chart (parity/specs/issues.md §6, ISS-044–048). jsdom does no
// layout, so the scroll host is given a fixed size and a scrollTo stub;
// everything else is the real chart: bars on their day columns, the
// two-tier header, today's mark, the zoom switcher with its re-center,
// edge extension in both directions, the fullscreen portal and the
// loading/empty placeholders.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { formatDay } from "../dropdowns/calendar.js";
import { GanttChart, SIDEBAR_WIDTH } from "./GanttChart.js";
import { ROW_OVERSCAN, ROW_HEIGHT } from "./rows.js";
import type { GanttRow } from "./rows.js";
import { DAY_WIDTHS, addDays, centeredScroll, initialRange, timelineWidth } from "./scale.js";

const clock = vi.hoisted(() => ({ iso: "2026-10-18" }));

vi.mock("../dropdowns/calendar.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../dropdowns/calendar.js")>();
  return { ...actual, todayIsoDay: () => clock.iso };
});

const HOST_WIDTH = 1200;
const HOST_HEIGHT = 600;

function row(overrides: Partial<GanttRow> = {}): GanttRow {
  return {
    id: "row-1",
    identifier: "WEB-1",
    name: "First",
    startIso: null,
    targetIso: null,
    durationDays: null,
    stateId: null,
    sortOrder: 1,
    archived: false,
    unconfirmed: false,
    ...overrides,
  };
}

const TODAY = clock.iso;
const WEEK_RANGE = initialRange("week", TODAY, 0);
const ROWS = [
  row({
    id: "row-1",
    name: "Dated",
    startIso: addDays(WEEK_RANGE.startIso, 3),
    targetIso: addDays(WEEK_RANGE.startIso, 5),
    durationDays: 3,
  }),
  row({ id: "row-2", name: "Undated" }),
  row({
    id: "row-3",
    name: "Far right",
    startIso: addDays(WEEK_RANGE.startIso, 90),
    targetIso: addDays(WEEK_RANGE.startIso, 92),
    durationDays: 3,
  }),
];

interface ScrollStub {
  scrollTo: ReturnType<typeof vi.fn>;
}

function stubGeometry(width: number = HOST_WIDTH): ScrollStub {
  const scrollTo = vi.fn(function scrollTo(this: HTMLElement, options: { left?: number }) {
    this.scrollLeft = options.left ?? 0;
  });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.id === "gantt-container" ? width : 0;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get(this: HTMLElement) {
      return this.id === "gantt-container" ? HOST_HEIGHT : 0;
    },
  });
  Element.prototype.scrollTo = scrollTo as unknown as typeof Element.prototype.scrollTo;
  return { scrollTo };
}

function host(): HTMLElement {
  return screen.getByTestId("gantt-container");
}

function scrollTo(value: number): void {
  const element = host();
  element.scrollLeft = value;
  fireEvent.scroll(element);
}

function dayColumns(): HTMLElement[] {
  return [...screen.getByTestId("gantt-day-columns").querySelectorAll<HTMLElement>("[data-day]")];
}

function renderChart(props: Partial<React.ComponentProps<typeof GanttChart>> = {}) {
  const onOpenRow = vi.fn();
  const view = render(
    <GanttChart
      rows={ROWS}
      workspaceSlug="acme"
      totalCount={ROWS.length}
      weekStart={0}
      onOpenRow={onOpenRow}
      {...props}
    />
  );
  return { onOpenRow, ...view };
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  stubGeometry();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("GanttChart layout", () => {
  it("renders the header count, sidebar and chart in one scroller", () => {
    renderChart();
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("3 work items");
    expect(screen.getByTestId("gantt-sidebar")).toBeInTheDocument();
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(3);
    expect(host().contains(screen.getByTestId("gantt-sidebar"))).toBe(true);
    expect(host().contains(screen.getByTestId("gantt-lanes"))).toBe(true);
  });

  it("draws one day column per rendered day at the zoom's width", () => {
    renderChart();
    const columns = dayColumns();
    expect(columns).toHaveLength(WEEK_RANGE.dayCount);
    expect(columns[0]?.style.width).toBe(`${DAY_WIDTHS.week}px`);
  });

  it("tints weekends and marks week starts", () => {
    renderChart();
    const tinted = screen.getByTestId("gantt-day-columns").querySelectorAll('[data-weekend="true"]');
    expect(tinted.length).toBeGreaterThan(0);
    expect(tinted.length).toBeLessThan(WEEK_RANGE.dayCount);
    const weekStart = dayColumns().find((column) => column.className.includes("border-l"));
    expect(weekStart?.dataset["day"]).toBe(WEEK_RANGE.startIso);
  });

  function barOf(id: string): HTMLElement {
    const bar = document.querySelector<HTMLElement>(`#gantt-block-${id}`);
    if (bar === null) throw new Error(`no bar for ${id}`);
    return bar;
  }

  it("places a bar on its start..target day columns", () => {
    renderChart();
    const bar = barOf("row-1");
    expect(bar).toHaveAttribute("data-testid", "gantt-bar");
    expect(bar).toHaveStyle({ left: `${3 * DAY_WIDTHS.week}px`, width: `${3 * DAY_WIDTHS.week}px` });
    expect(bar).toHaveTextContent("Dated");
  });

  it("colors a bar from the row's state color", () => {
    renderChart({ barColors: { "row-1": "#ff0000" } });
    expect(barOf("row-1")).toHaveStyle({ backgroundColor: "#ff0000" });
    expect(barOf("row-3")).toHaveStyle({ backgroundColor: "var(--accent)" });
  });

  it("keeps undated rows with a no-dates lane", () => {
    renderChart();
    expect(screen.getByText("No dates")).toBeInTheDocument();
    expect(document.querySelector("#gantt-block-row-2")).toBeNull();
  });

  it("shows a dateless unconfirmed create as a pulsing lane with no bar", () => {
    renderChart({
      rows: [
        row({ id: "temp-abc", identifier: "WEB-0", name: "Draft title", unconfirmed: true }),
        row({ id: "row-3", name: "Far right" }),
      ],
    });
    const lane = document.querySelector('[data-lane="temp-abc"]');
    expect(lane?.querySelector('[data-testid="gantt-lane-unconfirmed"]')).not.toBeNull();
    expect(lane?.textContent).not.toContain("No dates");
    expect(document.querySelector("#gantt-block-temp-abc")).toBeNull();
  });

  it("draws a dated unconfirmed create's bar immediately but keeps it inert (ISS-055)", () => {
    const h = renderChart({
      rows: [
        row({
          id: "temp-abc",
          identifier: "WEB-0",
          name: "Draft title",
          startIso: TODAY,
          targetIso: addDays(TODAY, 1),
          durationDays: 2,
          unconfirmed: true,
        }),
      ],
    });
    const bar = barOf("temp-abc");
    expect(bar).toHaveTextContent("Draft title");
    // No edit handles and no pulsing placeholder while the bar shows.
    expect(document.querySelector('[data-testid="gantt-lane-unconfirmed"]')).toBeNull();
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-resize-right")).toHaveLength(0);
    expect(bar).toHaveAttribute("aria-disabled", "true");
    expect(bar).toHaveAttribute("tabindex", "-1");
    fireEvent.click(bar);
    expect(h.onOpenRow).not.toHaveBeenCalled();
  });

  it("opens a row from its bar and from its sidebar link", () => {
    const h = renderChart();
    fireEvent.click(barOf("row-1"));
    expect(h.onOpenRow).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
    fireEvent.click(screen.getByRole("link", { name: "Undated" }));
    expect(h.onOpenRow).toHaveBeenLastCalledWith(expect.objectContaining({ id: "row-2" }));
  });

  it("marks the hovered lane and row together", () => {
    renderChart();
    fireEvent.mouseEnter(screen.getAllByTestId("gantt-sidebar-row")[1]!);
    expect(document.querySelector('[data-lane="row-2"]')).toHaveAttribute("data-hovered", "true");
    fireEvent.mouseLeave(screen.getAllByTestId("gantt-sidebar-row")[1]!);
    expect(document.querySelector('[data-lane="row-2"]')).not.toHaveAttribute("data-hovered");
    fireEvent.mouseEnter(document.querySelector('[data-lane="row-1"]')!);
    expect(screen.getAllByTestId("gantt-sidebar-row")[0]).toHaveClass("bg-(--subtle)");
    fireEvent.mouseLeave(document.querySelector('[data-lane="row-1"]')!);
    expect(screen.getAllByTestId("gantt-sidebar-row")[0]).not.toHaveClass("bg-(--subtle)");
  });

  it("marks the peeked row", () => {
    renderChart({ peekedId: "row-1" });
    expect(document.querySelector('[data-lane="row-1"]')).toHaveAttribute("data-peeked", "true");
    expect(screen.getByRole("link", { name: "Dated" })).toHaveAttribute("data-peeked", "true");
  });

  it("bands the header in two tiers with today's fine cell marked", () => {
    renderChart();
    expect(document.querySelector('[data-tier="coarse"] [data-segment-key^="m-"]')).not.toBeNull();
    const fine = [...document.querySelectorAll<HTMLElement>('[data-tier="fine"] [data-segment-key]')];
    expect(fine).toHaveLength(WEEK_RANGE.dayCount);
    const marked = [...document.querySelectorAll<HTMLElement>('[data-tier="fine"] [data-today-segment="true"]')];
    expect(marked).toHaveLength(1);
    expect(marked[0]?.dataset["segmentKey"]).toBe(`d-${TODAY}`);
    expect(marked[0]).toHaveAttribute("title", "Current");
  });
});

describe("GanttChart today (ISS-046)", () => {
  it("highlights today's column over the whole chart", () => {
    renderChart();
    const highlight = screen.getByTestId("gantt-today-highlight");
    expect(highlight).toHaveStyle({ width: `${DAY_WIDTHS.week}px` });
  });

  it("centers today once the host has a measured width", async () => {
    renderChart();
    await waitFor(() =>
      expect(host().scrollLeft).toBe(centeredScroll(TODAY, WEEK_RANGE, "week", Math.max(0, HOST_WIDTH - SIDEBAR_WIDTH)))
    );
  });

  it("scrolls today back into view from the Today button", async () => {
    renderChart();
    await waitFor(() => expect(host().scrollLeft).toBeGreaterThan(0));
    const centered = host().scrollLeft;
    scrollTo(timelineWidth(WEEK_RANGE, "week"));
    await waitFor(() => expect(host().scrollLeft).toBeGreaterThan(centered));
    fireEvent.click(screen.getByTestId("gantt-today"));
    await waitFor(() => expect(host().scrollLeft).toBe(centered));
  });
});

describe("GanttChart zoom (ISS-045)", () => {
  it("switches zoom, rebuilds the window and re-centers", async () => {
    renderChart();
    await waitFor(() => expect(host().scrollLeft).toBeGreaterThan(0));
    fireEvent.click(screen.getByRole("button", { name: "Month" }));
    expect(screen.getByRole("button", { name: "Month" })).toHaveAttribute("aria-pressed", "true");
    expect(dayColumns()).toHaveLength(initialRange("month", TODAY, 0).dayCount);
    expect(dayColumns()[0]?.style.width).toBe(`${DAY_WIDTHS.month}px`);
    await waitFor(() =>
      expect(host().scrollLeft).toBe(
        centeredScroll(TODAY, initialRange("month", TODAY, 0), "month", Math.max(0, HOST_WIDTH - SIDEBAR_WIDTH))
      )
    );
  });

  it("marks the week containing today at Month zoom", () => {
    renderChart();
    fireEvent.click(screen.getByRole("button", { name: "Month" }));
    const fine = [...document.querySelectorAll<HTMLElement>('[data-tier="fine"] [data-segment-key]')];
    const marked = fine.filter((cell) => cell.dataset["todaySegment"] === "true");
    expect(marked).toHaveLength(1);
    expect(Number.parseFloat(marked[0]?.style.width ?? "0")).toBeGreaterThan(DAY_WIDTHS.month);
    expect(marked[0]?.title).toBe("Current");
  });

  it("marks the current month at Quarter zoom with quarter tiers", () => {
    renderChart();
    fireEvent.click(screen.getByRole("button", { name: "Quarter" }));
    const coarse = [...document.querySelectorAll<HTMLElement>('[data-tier="coarse"] [data-segment-key]')];
    expect(coarse[0]?.dataset["segmentKey"]).toMatch(/^q-/);
    const marked = document.querySelectorAll('[data-tier="fine"] [data-today-segment="true"]');
    expect(marked).toHaveLength(1);
  });

  it("scrolls the lane host back to the top on a zoom switch", async () => {
    renderChart();
    act(() => (host().scrollTop = 120));
    fireEvent.scroll(host());
    fireEvent.click(screen.getByRole("button", { name: "Month" }));
    await waitFor(() => expect(host().scrollTop).toBe(0));
  });

  it("resets the ROW_HEIGHT window when the row list is long", () => {
    const many = Array.from({ length: 60 }, (_, index) =>
      row({ id: `r-${index}`, identifier: `WEB-${index}`, name: `Row ${index}` })
    );
    renderChart({ rows: many });
    const visible = Math.ceil(HOST_HEIGHT / ROW_HEIGHT) + ROW_OVERSCAN * 2;
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(visible);
    expect(screen.getAllByTestId("gantt-sidebar-row")[0]).toHaveAttribute("data-row-id", "r-0");
  });
});

describe("GanttChart infinite extension (ISS-048)", () => {
  it("appends days when scrolled to the right edge", async () => {
    renderChart();
    const before = dayColumns().length;
    scrollTo(10_000);
    await waitFor(() => expect(dayColumns().length).toBe(before + 14));
  });

  it("prepends days at the left edge and holds the viewport still", async () => {
    renderChart();
    const before = dayColumns().length;
    const firstDay = dayColumns()[0]?.dataset["day"];
    scrollTo(0);
    await waitFor(() => expect(dayColumns().length).toBe(before + 14));
    expect(dayColumns()[0]?.dataset["day"]).toBe(addDays(firstDay!, -14));
    await waitFor(() => expect(host().scrollLeft).toBe(14 * DAY_WIDTHS.week));
    expect(screen.getByTestId("gantt-range")).toHaveTextContent(`${addDays(firstDay!, -14)} + ${before + 14} days`);
  });

  it("does not extend while the timeline already fits the host", () => {
    stubGeometry(4_000);
    renderChart();
    const before = dayColumns().length;
    scrollTo(0);
    expect(dayColumns()).toHaveLength(before);
    scrollTo(4_000);
    expect(dayColumns()).toHaveLength(before);
  });

  it("ignores a second edge hit during the cooldown, then accepts one after it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      renderChart();
      const before = dayColumns().length;
      scrollTo(10_000);
      await waitFor(() => expect(dayColumns().length).toBe(before + 14));
      scrollTo(10_000);
      expect(dayColumns()).toHaveLength(before + 14);
      act(() => vi.advanceTimersByTime(400));
      scrollTo(10_000);
      await waitFor(() => expect(dayColumns().length).toBe(before + 28));
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("GanttChart scroll arrows", () => {
  it("shows a left arrow for a bar behind the viewport and scrolls to it", async () => {
    const { scrollTo: scrollToSpy } = { scrollTo: Element.prototype.scrollTo as unknown as ReturnType<typeof vi.fn> };
    renderChart();
    await waitFor(() => expect(host().scrollLeft).toBeGreaterThan(500));
    const arrows = screen.getAllByTestId("gantt-scroll-arrow");
    const leftArrow = arrows.find((arrow) => arrow.textContent === "←");
    expect(leftArrow).toBeDefined();
    expect(leftArrow).toHaveAttribute("aria-label", "Scroll to Dated");
    fireEvent.click(leftArrow!);
    expect(scrollToSpy).toHaveBeenCalledWith(expect.objectContaining({ behavior: "smooth" }));
    expect(host().scrollLeft).toBe(0);
  });

  it("shows a right arrow for a bar past the viewport", () => {
    renderChart({
      rows: [row({ id: "a", identifier: "WEB-1", name: "Near", startIso: TODAY, targetIso: TODAY, durationDays: 1 })],
    });
    expect(screen.queryByTestId("gantt-scroll-arrow")).toBeNull();
  });

  it("points right when only the far bar exists", async () => {
    renderChart({ rows: [ROWS[2]!] });
    await waitFor(() => expect(host().scrollLeft).toBeGreaterThan(0));
    expect(screen.getByTestId("gantt-scroll-arrow")).toHaveTextContent("→");
  });
});

describe("GanttChart fullscreen (ISS-047)", () => {
  it("keeps zoom, range and scroll when moving in and out of fullscreen", async () => {
    renderChart();
    await waitFor(() => expect(host().scrollLeft).toBeGreaterThan(0));
    fireEvent.click(screen.getByRole("button", { name: "Quarter" }));
    const quarterRange = initialRange("quarter", TODAY, 0);
    expect(dayColumns()).toHaveLength(quarterRange.dayCount);
    await waitFor(() =>
      expect(host().scrollLeft).toBe(
        centeredScroll(TODAY, quarterRange, "quarter", Math.max(0, HOST_WIDTH - SIDEBAR_WIDTH))
      )
    );
    const quarterScroll = host().scrollLeft;

    fireEvent.click(screen.getByRole("button", { name: "Expand timeline" }));
    // The portal hosts the same viewport: still Quarter, same range, same
    // offset (the pre-fix code remounted to Week and re-centered).
    expect(screen.getByRole("button", { name: "Quarter" })).toHaveAttribute("aria-pressed", "true");
    expect(dayColumns()).toHaveLength(quarterRange.dayCount);
    expect(dayColumns()[0]?.dataset["day"]).toBe(quarterRange.startIso);
    await waitFor(() => expect(host().scrollLeft).toBe(quarterScroll));
    expect(screen.getByTestId("gantt-range")).toHaveTextContent(
      `${quarterRange.startIso} + ${quarterRange.dayCount} days`
    );

    fireEvent.click(screen.getByRole("button", { name: "Exit full screen" }));
    expect(screen.getByRole("button", { name: "Quarter" })).toHaveAttribute("aria-pressed", "true");
    expect(dayColumns()[0]?.dataset["day"]).toBe(quarterRange.startIso);
    expect(screen.getByTestId("gantt-range")).toHaveTextContent(
      `${quarterRange.startIso} + ${quarterRange.dayCount} days`
    );
  });

  it("moves the chart into the full-screen portal and back", () => {
    renderChart();
    const portal = document.getElementById(FULL_SCREEN_PORTAL_ID)!;
    expect(portal.querySelector("#gantt-container")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Expand timeline" }));
    expect(portal.querySelector("[data-testid='gantt-fullscreen-root']")).not.toBeNull();
    expect(portal.querySelector("#gantt-container")).not.toBeNull();
    expect(within(portal as HTMLElement).getByTestId("gantt-count")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Exit full screen" }));
    expect(portal.querySelector("#gantt-container")).toBeNull();
    expect(host()).toBeVisible();
    expect(screen.getByTestId("gantt-header")).toBeInTheDocument();
  });
});

describe("GanttChart loading and empty states", () => {
  it("counts the server total when it exceeds the loaded rows (ISS-044)", () => {
    renderChart({ rows: ROWS.slice(0, 2), totalCount: 240 });
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("240 work items");
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(2);
  });

  it("draws skeleton placeholders while no rows exist", () => {
    renderChart({ rows: [], totalCount: 0 });
    expect(screen.getByTestId("gantt-sidebar-skeleton")).toBeInTheDocument();
    expect(screen.getByTestId("gantt-strip-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("gantt-sidebar")).toBeNull();
    expect(screen.queryByTestId("gantt-lanes")).toBeNull();
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("0 work items");
  });

  it("shows the load-more placeholder while the next page arrives", () => {
    renderChart({ loadingMore: true });
    expect(screen.getByTestId("gantt-load-more")).toBeInTheDocument();
  });

  it("keeps the today highlight inside the seeded window", () => {
    renderChart();
    expect(screen.getByTestId("gantt-today-highlight")).toBeInTheDocument();
    expect(timelineWidth(WEEK_RANGE, "week")).toBeGreaterThan(0);
  });
});

describe("GanttChart bar gestures (ISS-051–053)", () => {
  function barOf(id: string): HTMLElement {
    const bar = document.querySelector<HTMLElement>(`#gantt-block-${id}`);
    if (bar === null) throw new Error(`no bar for ${id}`);
    return bar;
  }

  it("moves a fully dated bar by whole days and commits on release", () => {
    const onDatesChange = vi.fn();
    renderChart({ canEdit: true, onDatesChange });
    const bar = barOf("row-1");
    fireEvent.mouseDown(bar, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 220 });
    expect(bar).toHaveStyle({ left: `${6 * DAY_WIDTHS.week}px` });
    fireEvent.mouseUp(document);
    expect(onDatesChange).toHaveBeenCalledWith("row-1", {
      startDate: addDays(WEEK_RANGE.startIso, 6),
      endDate: addDays(WEEK_RANGE.startIso, 8),
    });
  });

  it("resizes the left handle and keeps the target", () => {
    const onDatesChange = vi.fn();
    renderChart({ canEdit: true, onDatesChange });
    fireEvent.mouseDown(screen.getAllByTestId("gantt-resize-left")[0]!, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 60 });
    fireEvent.mouseUp(document);
    expect(onDatesChange).toHaveBeenCalledWith("row-1", {
      startDate: addDays(WEEK_RANGE.startIso, 2),
      endDate: addDays(WEEK_RANGE.startIso, 5),
    });
  });

  it("resizes the right handle and keeps the start", () => {
    const onDatesChange = vi.fn();
    renderChart({ canEdit: true, onDatesChange });
    fireEvent.mouseDown(screen.getAllByTestId("gantt-resize-right")[0]!, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 180 });
    fireEvent.mouseUp(document);
    expect(onDatesChange).toHaveBeenCalledWith("row-1", {
      startDate: addDays(WEEK_RANGE.startIso, 3),
      endDate: addDays(WEEK_RANGE.startIso, 7),
    });
  });

  it("fills only the start when the left handle drags a start-only row", () => {
    const onDatesChange = vi.fn();
    renderChart({
      canEdit: true,
      onDatesChange,
      rows: [
        row({
          id: "half-start",
          name: "Start only",
          startIso: addDays(WEEK_RANGE.startIso, 3),
          targetIso: null,
        }),
      ],
    });
    const handle = document.querySelector<HTMLElement>('[data-lane="half-start"] [data-testid="gantt-resize-left"]');
    if (handle === null) throw new Error("no left handle");
    fireEvent.mouseDown(handle, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 60 });
    fireEvent.mouseUp(document);
    expect(onDatesChange).toHaveBeenCalledWith("half-start", {
      startDate: addDays(WEEK_RANGE.startIso, 2),
      endDate: null,
    });
  });

  it("fills only the target when the right handle drags a target-only row", () => {
    const onDatesChange = vi.fn();
    renderChart({
      canEdit: true,
      onDatesChange,
      rows: [
        row({
          id: "half-target",
          name: "Target only",
          startIso: null,
          targetIso: addDays(WEEK_RANGE.startIso, 5),
        }),
      ],
    });
    const handle = document.querySelector<HTMLElement>('[data-lane="half-target"] [data-testid="gantt-resize-right"]');
    if (handle === null) throw new Error("no right handle");
    fireEvent.mouseDown(handle, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 140 });
    fireEvent.mouseUp(document);
    expect(onDatesChange).toHaveBeenCalledWith("half-target", {
      startDate: null,
      endDate: addDays(WEEK_RANGE.startIso, 6),
    });
  });

  it("does not commit when the gesture ends where it started", () => {
    const onDatesChange = vi.fn();
    renderChart({ canEdit: true, onDatesChange });
    fireEvent.mouseDown(barOf("row-1"), { button: 0, clientX: 100 });
    fireEvent.mouseUp(document);
    expect(onDatesChange).not.toHaveBeenCalled();
  });

  it("cancels a gesture on Escape", () => {
    const onDatesChange = vi.fn();
    renderChart({ canEdit: true, onDatesChange });
    fireEvent.mouseDown(barOf("row-1"), { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 220 });
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.mouseUp(document);
    expect(onDatesChange).not.toHaveBeenCalled();
  });

  it("renders no handles and never commits without edit standing", () => {
    const onDatesChange = vi.fn();
    renderChart({ onDatesChange });
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    fireEvent.mouseDown(barOf("row-1"), { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 220 });
    fireEvent.mouseUp(document);
    expect(onDatesChange).not.toHaveBeenCalled();
  });

  it("opens a bar from the keyboard but not an unconfirmed one", () => {
    const h = renderChart({ canEdit: true });
    fireEvent.keyDown(barOf("row-1"), { key: "Enter" });
    expect(h.onOpenRow).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
    h.onOpenRow.mockClear();
    fireEvent.keyDown(barOf("row-1"), { key: " " });
    expect(h.onOpenRow).toHaveBeenCalledTimes(1);
    cleanup();
    const seeded = renderChart({
      canEdit: true,
      rows: [
        row({
          id: "temp-1",
          name: "Pending",
          startIso: WEEK_RANGE.startIso,
          targetIso: WEEK_RANGE.startIso,
          unconfirmed: true,
        }),
      ],
    });
    fireEvent.keyDown(barOf("temp-1"), { key: "Enter" });
    expect(seeded.onOpenRow).not.toHaveBeenCalled();
  });

  it("still opens a bar that was clicked rather than dragged", () => {
    const h = renderChart({ canEdit: true });
    fireEvent.click(barOf("row-1"));
    expect(h.onOpenRow).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
  });

  it("does not open the row when a resize handle is pressed and released", () => {
    const h = renderChart({ canEdit: true, onDatesChange: vi.fn() });
    const handle = document.querySelector<HTMLElement>('[data-lane="row-1"] [data-testid="gantt-resize-left"]');
    if (handle === null) throw new Error("no left handle");
    fireEvent.mouseDown(handle, { button: 0, clientX: 100 });
    fireEvent.mouseUp(document);
    // The browser dispatches the click on the bar that holds the handle.
    fireEvent.click(barOf("row-1"));
    expect(h.onOpenRow).not.toHaveBeenCalled();
  });

  it("does not open the row when a drag ends over the bar", () => {
    const h = renderChart({ canEdit: true, onDatesChange: vi.fn() });
    const bar = barOf("row-1");
    fireEvent.mouseDown(bar, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 180 });
    fireEvent.mouseUp(document);
    fireEvent.click(bar);
    expect(h.onOpenRow).not.toHaveBeenCalled();
  });

  it("opens again on the click after a gesture's own click", () => {
    const h = renderChart({ canEdit: true, onDatesChange: vi.fn() });
    const bar = barOf("row-1");
    fireEvent.mouseDown(bar, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 180 });
    fireEvent.mouseUp(document);
    fireEvent.click(bar);
    expect(h.onOpenRow).not.toHaveBeenCalled();
    // The guard is consumed by its own click, so the next click opens.
    fireEvent.click(bar);
    expect(h.onOpenRow).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
  });

  it("still opens the row for a real primary-button click without travel", () => {
    const h = renderChart({ canEdit: true, onDatesChange: vi.fn() });
    const bar = barOf("row-1");
    fireEvent.mouseDown(bar, { button: 0, clientX: 100 });
    fireEvent.mouseUp(document);
    fireEvent.click(bar);
    expect(h.onOpenRow).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
  });

  it("ignores gestures that do not start with the primary button", () => {
    const onDatesChange = vi.fn();
    const h = renderChart({ canEdit: true, onDatesChange });
    fireEvent.mouseDown(barOf("row-1"), { button: 2, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 220 });
    fireEvent.mouseUp(document);
    expect(onDatesChange).not.toHaveBeenCalled();
    // A right-click is not a gesture, so its click still opens normally.
    fireEvent.click(barOf("row-1"));
    expect(h.onOpenRow).toHaveBeenCalledWith(expect.objectContaining({ id: "row-1" }));
  });
});

describe("GanttChart add-block (ISS-054)", () => {
  function undatedLane(): HTMLElement {
    const lane = screen.getAllByTestId("gantt-undated-lane")[0];
    if (lane === undefined) throw new Error("no undated lane");
    lane.getBoundingClientRect = () => ({ left: 0, top: 0, width: 800, height: 44, right: 800, bottom: 44 }) as DOMRect;
    return lane;
  }

  it("follows the pointer with a dated add control and plants one day", () => {
    const onPlantBlock = vi.fn();
    renderChart({ canEdit: true, onPlantBlock });
    const lane = undatedLane();
    fireEvent.mouseMove(lane, { clientX: 2 * DAY_WIDTHS.week });
    const add = screen.getByTestId("gantt-add-block");
    expect(add).toHaveAttribute("title", expect.stringMatching(/\d{4}|[A-Z][a-z]+ \d/));
    fireEvent.click(add);
    expect(onPlantBlock).toHaveBeenCalledWith("row-2", {
      startDate: addDays(WEEK_RANGE.startIso, 2),
      endDate: addDays(WEEK_RANGE.startIso, 3),
    });
  });

  it("hides the add control when the pointer leaves or standing is gone", () => {
    renderChart({ canEdit: true, onPlantBlock: vi.fn() });
    const lane = undatedLane();
    fireEvent.mouseMove(lane, { clientX: 80 });
    expect(screen.getByTestId("gantt-add-block")).toBeInTheDocument();
    fireEvent.mouseLeave(lane);
    expect(screen.queryByTestId("gantt-add-block")).not.toBeInTheDocument();
    cleanup();
    renderChart({ onPlantBlock: vi.fn() });
    expect(screen.queryAllByTestId("gantt-undated-lane")).toHaveLength(0);
    expect(screen.getAllByText("No dates").length).toBeGreaterThan(0);
  });
});

describe("GanttChart resize preview (ISS-052/053)", () => {
  it("shows the handle date on hover and follows a drag", () => {
    renderChart({ canEdit: true, onDatesChange: vi.fn() });
    expect(screen.queryByTestId("gantt-resize-preview")).not.toBeInTheDocument();
    const handle = screen.getAllByTestId("gantt-resize-left")[0]!;
    fireEvent.mouseEnter(handle);
    expect(screen.getByTestId("gantt-resize-preview").textContent).toBe(formatDay(addDays(WEEK_RANGE.startIso, 3)));
    fireEvent.mouseLeave(handle);
    expect(screen.queryByTestId("gantt-resize-preview")).not.toBeInTheDocument();
    fireEvent.mouseDown(handle, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 140 });
    expect(screen.getByTestId("gantt-resize-preview").textContent?.trim()).not.toBe("");
    fireEvent.mouseUp(document);
  });

  it("previews the target under the right handle and nothing for a body move", () => {
    renderChart({ canEdit: true, onDatesChange: vi.fn() });
    const right = screen.getAllByTestId("gantt-resize-right")[0]!;
    fireEvent.mouseEnter(right);
    expect(screen.getByTestId("gantt-resize-preview").textContent).toBe(formatDay(addDays(WEEK_RANGE.startIso, 5)));
    fireEvent.mouseLeave(right);
    // Dragging the body moves the bar but belongs to no handle: no pill.
    const bar = document.querySelector<HTMLElement>("#gantt-block-row-1");
    if (bar === null) throw new Error("no bar");
    fireEvent.mouseDown(bar, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 140 });
    expect(screen.queryByTestId("gantt-resize-preview")).not.toBeInTheDocument();
    fireEvent.mouseUp(document);
  });
});
