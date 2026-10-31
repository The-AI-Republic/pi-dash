// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueListItem } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import { CalendarDay, CALENDAR_ADD_EXISTING_ITEM, CALENDAR_ADD_ITEM } from "./CalendarDay.js";
import type { CalendarCell } from "./model.js";

const CELL: CalendarCell = {
  iso: "2026-10-15",
  day: 15,
  inMonth: true,
  weekend: false,
  isToday: true,
};

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

function renderDay(overrides: Partial<React.ComponentProps<typeof CalendarDay>> = {}) {
  const handlers = {
    onSelect: vi.fn(),
    onDragOver: vi.fn(),
    onOpen: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onDragStart: vi.fn(),
    onQuickAdd: vi.fn(),
    onAddExisting: vi.fn(),
    onLoadMore: vi.fn(),
  };
  render(
    <CalendarDay
      cell={CELL}
      rows={[]}
      totalRows={0}
      workspaceSlug="acme"
      projectIdentifier="WEB"
      stateColors={new Map()}
      stateMeta={new Map()}
      highlightedId={null}
      narrow={false}
      canEdit
      canDrag
      canAddExisting
      selected={false}
      isDropTarget={false}
      extraRows={[]}
      loadingMore={false}
      {...handlers}
      {...overrides}
    />
  );
  return handlers;
}

function tileFor(iso: string): HTMLElement {
  return document.querySelector(`[data-calendar-day="${iso}"]`) as HTMLElement;
}

describe("CalendarDay", () => {
  it("badges today, shades weekends, and renders blocks", () => {
    renderDay({ rows: [row()], totalRows: 1, cell: { ...CELL, weekend: true } });
    const tile = tileFor(CELL.iso);
    expect(tile).toHaveClass("bg-(--subtle)");
    expect(tile.querySelector("span.rounded-full")).not.toBeNull();
    expect(screen.getByRole("link", { name: "WEB-42" })).toBeInTheDocument();
  });

  it("shows the desktop day number and today badge on wide layouts", () => {
    renderDay({ cell: { ...CELL, isToday: true } });
    const header = tileFor(CELL.iso).firstElementChild as HTMLElement;
    expect(header).toHaveClass("md:flex");
    expect(header.querySelector("span.rounded-full")?.textContent).toBe("15");
  });

  it("badges today on the compact day face", () => {
    renderDay({ cell: { ...CELL, isToday: true } });
    const face = tileFor(CELL.iso).querySelector("div[role='button']") as HTMLElement;
    expect(face).toHaveAttribute("data-today", "true");
    expect(face.querySelector("span.rounded-full")?.textContent).toBe("15");
  });

  it("marks adjacent-month filler and drop targets", () => {
    renderDay({ cell: { ...CELL, inMonth: false, isToday: false }, isDropTarget: true });
    const tile = tileFor(CELL.iso);
    expect(tile).toHaveClass("opacity-60");
    expect(tile).toHaveClass("ring-2");
    expect(tile.querySelector("span.rounded-full")).toBeNull();
  });

  it("pages a crowded day", async () => {
    const handlers = renderDay({ rows: [row()], totalRows: 5 });
    await userEvent.setup().click(screen.getByRole("button", { name: "Load more" }));
    expect(handlers.onLoadMore).toHaveBeenCalledWith(CELL.iso);
  });

  it("creates inline from the add menu and ignores blank titles", async () => {
    const handlers = renderDay();
    fireEvent.click(screen.getByText(CALENDAR_ADD_ITEM));
    fireEvent.click(screen.getByRole("menuitem", { name: CALENDAR_ADD_ITEM }));
    const field = screen.getByPlaceholderText("Work item Title");
    fireEvent.keyDown(field, { key: "Enter" });
    expect(handlers.onQuickAdd).not.toHaveBeenCalled();
    fireEvent.change(field, { target: { value: "New thing" } });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(handlers.onQuickAdd).toHaveBeenCalledWith(CELL.iso, "New thing");
  });

  it("closes inline add on Escape and empty blur", async () => {
    const handlers = renderDay();
    fireEvent.click(screen.getByText(CALENDAR_ADD_ITEM));
    fireEvent.click(screen.getByRole("menuitem", { name: CALENDAR_ADD_ITEM }));
    fireEvent.keyDown(screen.getByPlaceholderText("Work item Title"), { key: "Escape" });
    expect(screen.queryByPlaceholderText("Work item Title")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText(CALENDAR_ADD_ITEM));
    fireEvent.click(screen.getByRole("menuitem", { name: CALENDAR_ADD_ITEM }));
    fireEvent.blur(screen.getByPlaceholderText("Work item Title"));
    expect(screen.queryByPlaceholderText("Work item Title")).not.toBeInTheDocument();
    expect(handlers.onQuickAdd).not.toHaveBeenCalled();
  });

  it("offers add-existing only when allowed", async () => {
    const handlers = renderDay();
    fireEvent.click(screen.getByText(CALENDAR_ADD_ITEM));
    expect(screen.getByRole("menuitem", { name: CALENDAR_ADD_EXISTING_ITEM })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: CALENDAR_ADD_EXISTING_ITEM }));
    expect(handlers.onAddExisting).toHaveBeenCalledWith(CELL.iso);
  });

  it("hides add controls when the caller cannot edit", () => {
    renderDay({ canEdit: false });
    expect(screen.queryByText(CALENDAR_ADD_ITEM)).not.toBeInTheDocument();
  });

  it("hides block quick actions for read-only viewers", () => {
    renderDay({ rows: [row()], totalRows: 1, canEdit: false, canDrag: false });
    expect(screen.getByRole("link", { name: "WEB-42" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Quick actions" })).not.toBeInTheDocument();
  });

  it("keeps blocks visible but not draggable on touch layouts", () => {
    const handlers = renderDay({ rows: [row()], totalRows: 1, canDrag: false });
    const block = screen.getByRole("link", { name: "WEB-42" });
    fireEvent(block, new MouseEvent("pointerdown", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    fireEvent(block, new MouseEvent("pointermove", { bubbles: true, cancelable: true, clientX: 40, clientY: 40 }));
    expect(handlers.onDragStart).not.toHaveBeenCalled();
  });

  it("selects the day from the compact face and renders no blocks", async () => {
    const handlers = renderDay({ narrow: true, rows: [row()], totalRows: 1 });
    expect(screen.queryByRole("link", { name: "WEB-42" })).not.toBeInTheDocument();
    expect(document.querySelector("span.size-1\\.5")).not.toBeNull();
    const face = document.querySelector("div.mx-auto.cursor-pointer") as HTMLElement;
    await userEvent.setup().click(face);
    expect(handlers.onSelect).toHaveBeenCalledWith(CELL.iso);
    fireEvent.keyDown(face, { key: " " });
    expect(handlers.onSelect).toHaveBeenCalledTimes(2);
  });

  it("reports hover for drag targeting", () => {
    const handlers = renderDay();
    const tile = tileFor(CELL.iso);
    fireEvent.pointerEnter(tile);
    expect(handlers.onDragOver).toHaveBeenCalledWith(CELL.iso);
  });

  it("omits add-existing when the caller does not offer it", async () => {
    renderDay({ canAddExisting: false });
    fireEvent.click(screen.getByText(CALENDAR_ADD_ITEM));
    expect(screen.queryByRole("menuitem", { name: CALENDAR_ADD_EXISTING_ITEM })).not.toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: CALENDAR_ADD_ITEM })).toBeInTheDocument();
  });

  it("passes state colour and metadata into the block preview", async () => {
    const stateId = "923e4567-e89b-12d3-a456-426614174009";
    renderDay({
      rows: [row({ state_id: stateId })],
      totalRows: 1,
      stateColors: new Map([[stateId, "#123456"]]),
      stateMeta: new Map([[stateId, { name: "In progress", group: "started" }]]),
    });
    const link = screen.getByRole("link", { name: "WEB-42" });
    fireEvent.focus(link);
    const card = screen.getByRole("tooltip");
    expect(card).toHaveTextContent("In progress");
  });

  it("keeps the day-number strip out of the pointer path", () => {
    // ISS-021: the strip is decoration; on a crowded month it must never
    // be the element that answers a click meant for a control below it.
    renderDay({ cell: { ...CELL, isToday: true } });
    const header = tileFor(CELL.iso).firstElementChild as HTMLElement;
    expect(header).toHaveClass("pointer-events-none");
    expect(header.querySelector("span.rounded-full")?.textContent).toBe("15");
  });

  it("renders Load more inside the tile's content region, below its blocks", () => {
    renderDay({ rows: [row()], totalRows: 5 });
    const content = tileFor(CELL.iso).querySelector("div.flex.min-h-0.flex-1.flex-col") as HTMLElement;
    const more = screen.getByRole("button", { name: "Load more" });
    expect(content).toContainElement(more);
    const block = screen.getByRole("link", { name: "WEB-42" });
    expect(block.compareDocumentPosition(more) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
