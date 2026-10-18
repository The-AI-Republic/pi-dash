// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline sidebar (parity/specs/issues.md §6, ISS-049): identifier plus
// name per row, a duration only for fully dated rows, the virtualization
// window, hover/peek marks, click behavior (open, or leave modified
// clicks to the browser) and the loading placeholder.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { GanttSidebar } from "./GanttSidebar.js";
import { ROW_HEIGHT, type GanttRow } from "./rows.js";

function row(overrides: Partial<GanttRow> = {}): GanttRow {
  return {
    id: "row-1",
    identifier: "WEB-1",
    name: "First",
    startIso: null,
    targetIso: null,
    durationDays: null,
    stateId: null,
    archived: false,
    ...overrides,
  };
}

const ROWS = [
  row(),
  row({
    id: "row-2",
    identifier: "WEB-2",
    name: "Second",
    startIso: "2026-10-01",
    targetIso: "2026-10-03",
    durationDays: 3,
  }),
  row({ id: "row-3", identifier: "WEB-3", name: "Third", archived: true }),
];

function renderSidebar(props: Partial<React.ComponentProps<typeof GanttSidebar>> = {}) {
  const onOpenRow = vi.fn();
  const onHover = vi.fn();
  const view = render(
    <GanttSidebar rows={ROWS} peekedId={null} hoveredId={null} onHover={onHover} onOpenRow={onOpenRow} {...props} />
  );
  return { onOpenRow, onHover, ...view };
}

describe("GanttSidebar", () => {
  it("renders identifier and name for every row", () => {
    renderSidebar();
    expect(screen.getByTestId("gantt-sidebar")).toBeInTheDocument();
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(3);
    expect(screen.getByRole("link", { name: "First" })).toHaveTextContent("WEB-1");
    expect(screen.getByRole("link", { name: "First" })).toHaveTextContent("First");
  });

  it("shows a duration only for fully dated rows", () => {
    renderSidebar();
    const durations = screen.getAllByTestId("gantt-row-duration");
    expect(durations).toHaveLength(1);
    expect(durations[0]).toHaveTextContent("3 days");
  });

  it("carries the issue id on the link and a browse href", () => {
    renderSidebar();
    const link = screen.getByRole("link", { name: "Second" });
    expect(link).toHaveAttribute("id", "issue-row-2");
    expect(link).toHaveAttribute("href", "browse/WEB-2");
  });

  it("marks archived rows", () => {
    renderSidebar();
    expect(screen.getByRole("link", { name: "Third" })).toHaveAttribute("data-archived", "true");
    expect(screen.getByRole("link", { name: "First" })).not.toHaveAttribute("data-archived");
  });

  it("opens a row on a plain click and suppresses navigation", () => {
    const h = renderSidebar();
    const notCancelled = fireEvent.click(screen.getByRole("link", { name: "Second" }));
    expect(notCancelled).toBe(false);
    expect(h.onOpenRow).toHaveBeenCalledWith(expect.objectContaining({ id: "row-2" }));
  });

  it.each([
    ["metaKey", { metaKey: true }],
    ["ctrlKey", { ctrlKey: true }],
    ["shiftKey", { shiftKey: true }],
    ["altKey", { altKey: true }],
  ])("leaves a %s click to the browser", (_label, init) => {
    const h = renderSidebar();
    const notCancelled = fireEvent.click(screen.getByRole("link", { name: "Second" }), init);
    expect(notCancelled).toBe(true);
    expect(h.onOpenRow).not.toHaveBeenCalled();
  });

  it("reports hover on enter and clears it on leave", () => {
    const h = renderSidebar();
    const target = screen.getAllByTestId("gantt-sidebar-row")[1];
    fireEvent.mouseEnter(target!);
    expect(h.onHover).toHaveBeenCalledWith("row-2");
    fireEvent.mouseLeave(target!);
    expect(h.onHover).toHaveBeenCalledWith(null);
  });

  it("marks the peeked and hovered rows", () => {
    renderSidebar({ peekedId: "row-2", hoveredId: "row-1" });
    expect(screen.getByRole("link", { name: "Second" })).toHaveAttribute("data-peeked", "true");
    expect(screen.getByRole("link", { name: "First" })).not.toHaveAttribute("data-peeked");
    const hovered = screen.getAllByTestId("gantt-sidebar-row")[0];
    expect(hovered).toHaveClass("bg-(--subtle)");
  });

  it("windows the rows and keeps the full scroll height", () => {
    const { container } = renderSidebar({ windowStart: 1, windowEnd: 2 });
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(1);
    expect(screen.getByRole("link", { name: "Second" })).toBeInTheDocument();
    const spacers = container.querySelectorAll<HTMLElement>("[aria-hidden='true']");
    expect(spacers[0]?.style.height).toBe(`${ROW_HEIGHT}px`);
    expect(spacers[1]?.style.height).toBe(`${ROW_HEIGHT}px`);
  });

  it("shows the load-more placeholder only while rows are arriving", () => {
    const { unmount } = renderSidebar();
    expect(screen.queryByTestId("gantt-load-more")).not.toBeInTheDocument();
    unmount();
    renderSidebar({ loadingMore: true });
    expect(screen.getByTestId("gantt-load-more")).toBeInTheDocument();
  });

  it("renders an empty list with zero-height spacers", () => {
    const { container } = renderSidebar({ rows: [] });
    expect(screen.queryAllByTestId("gantt-sidebar-row")).toHaveLength(0);
    const spacers = container.querySelectorAll<HTMLElement>("[aria-hidden='true']");
    expect(spacers).toHaveLength(2);
    for (const spacer of spacers) expect(spacer.style.height).toBe("0px");
  });
});
