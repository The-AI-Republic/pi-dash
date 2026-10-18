// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import type { IssueListItem } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import { CalendarGrid } from "./CalendarGrid.js";
import { scrollContainerTowardPointer } from "./autoScroll.js";
import type { CalendarCell, CalendarWeek } from "./model.js";

vi.mock("./autoScroll.js", () => ({ scrollContainerTowardPointer: vi.fn() }));

function cell(day: number, overrides: Partial<CalendarCell> = {}): CalendarCell {
  return {
    iso: `2026-10-${String(day).padStart(2, "0")}`,
    day,
    inMonth: true,
    weekend: day === 17 || day === 18,
    isToday: day === 15,
    ...overrides,
  };
}

function week(days: number[]): CalendarWeek {
  return days.map((day) => cell(day));
}

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "323e4567-e89b-12d3-a456-426614174002",
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

const noop = vi.fn();

function renderGrid(overrides: Partial<React.ComponentProps<typeof CalendarGrid>> = {}) {
  render(
    <CalendarGrid
      weeks={[week([11, 12, 13, 14, 15, 16, 17])]}
      weekdayLabels={["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]}
      rowsByDay={new Map([["2026-10-15", [row()]]])}
      totalsByDay={new Map([["2026-10-15", 1]])}
      extraRowsByDay={new Map()}
      loadingDay={null}
      projectIdentifier="WEB"
      stateColors={new Map()}
      highlightedId={null}
      narrow={false}
      canEdit
      canAddExisting
      selectedDay={null}
      dropTarget={null}
      dragging={false}
      onSelect={noop}
      onDrop={noop}
      onDragOver={noop}
      onOpen={noop}
      onEdit={noop}
      onDelete={noop}
      onDragStart={noop}
      onQuickAdd={noop}
      onAddExisting={noop}
      onLoadMore={noop}
      {...overrides}
    />
  );
}

describe("CalendarGrid", () => {
  it("renders the sticky weekday header and every tile", () => {
    renderGrid();
    expect(screen.getByText("Sat").closest("div.sticky")).not.toBeNull();
    expect(document.querySelectorAll("[data-calendar-day]")).toHaveLength(7);
    expect(screen.getByRole("link", { name: "WEB-42" })).toBeInTheDocument();
  });

  it("trims the header to five columns when weekends are hidden", () => {
    renderGrid({
      weeks: [week([12, 13, 14, 15, 16])],
      weekdayLabels: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    });
    expect(screen.queryByText("Sat")).not.toBeInTheDocument();
    expect(document.querySelectorAll("[data-calendar-day]")).toHaveLength(5);
  });

  it("merges extra rows and reports the loading day", () => {
    renderGrid({
      extraRowsByDay: new Map([["2026-10-15", [row({ id: "extra", name: "Extra", sequence_id: 43 })]]]),
      totalsByDay: new Map([["2026-10-15", 2]]),
      loadingDay: "2026-10-15",
    });
    expect(screen.getByText("Extra")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  });

  it("passes selected and drop-target state to the tile", () => {
    renderGrid({ selectedDay: "2026-10-15", dropTarget: "2026-10-15" });
    const tile = document.querySelector('[data-calendar-day="2026-10-15"]') as HTMLElement;
    expect(tile).toHaveClass("ring-2");
  });

  it("does not auto-scroll without a drag", () => {
    renderGrid({ dragging: false });
    const scroll = screen.getByTestId("calendar-scroll");
    scroll.dispatchEvent(new MouseEvent("pointermove", { clientY: 499, bubbles: true }));
    expect(scrollContainerTowardPointer).not.toHaveBeenCalled();
  });

  it("auto-scrolls the grid near an edge while dragging", () => {
    renderGrid({ dragging: true });
    const scroll = screen.getByTestId("calendar-scroll");
    scroll.dispatchEvent(new MouseEvent("pointermove", { clientY: 499, bubbles: true }));
    expect(scrollContainerTowardPointer).toHaveBeenCalledWith(scroll, 499);
  });
});
