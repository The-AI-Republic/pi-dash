// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline sidebar (parity/specs/issues.md §6, ISS-049): identifier plus
// name per row, a duration only for fully dated rows, the virtualization
// window, hover/peek marks, click behavior (open, or leave modified
// clicks to the browser), the inert pulsing row for an unconfirmed create
// and the loading placeholder.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DRAG_THRESHOLD_PX } from "./drag.js";
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
    sortOrder: 1,
    priority: "none",
    sequenceId: 1,
    archived: false,
    unconfirmed: false,
    isEpic: false,
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
    <GanttSidebar
      rows={ROWS}
      workspaceSlug="acme"
      peekedId={null}
      hoveredId={null}
      onHover={onHover}
      onOpenRow={onOpenRow}
      {...props}
    />
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

  it("carries the issue id on the link and an absolute workspace browse href", () => {
    renderSidebar();
    const link = screen.getByRole("link", { name: "Second" });
    expect(link).toHaveAttribute("id", "issue-row-2");
    // Absolute so Cmd/middle-click and copy-link land on the browse route
    // instead of resolving under the issues path (ISS-049).
    expect(link).toHaveAttribute("href", "/acme/browse/WEB-2");
    expect(link.getAttribute("href")?.startsWith("browse/")).toBe(false);
  });

  it("uses the workspace slug it is given for every row", () => {
    renderSidebar({ workspaceSlug: "other-space" });
    expect(screen.getByRole("link", { name: "First" })).toHaveAttribute("href", "/other-space/browse/WEB-1");
    expect(screen.getByRole("link", { name: "Third" })).toHaveAttribute("href", "/other-space/browse/WEB-3");
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

  it("renders an unconfirmed create as an inert pulsing row", () => {
    const h = renderSidebar({
      rows: [row({ id: "temp-abc", identifier: "WEB-0", name: "Draft title", unconfirmed: true }), row()],
    });
    // No key, no link and no peek target until the server confirms it.
    expect(screen.queryByRole("link", { name: "Draft title" })).not.toBeInTheDocument();
    expect(document.getElementById("issue-temp-abc")).toBeNull();
    const placeholder = screen.getByTestId("gantt-row-unconfirmed");
    expect(placeholder).toHaveTextContent("Draft title");
    expect(placeholder).not.toHaveTextContent("WEB-0");
    expect(placeholder).toHaveAttribute("aria-label", "Draft title, saving");
    const sidebarRow = placeholder.closest('[data-testid="gantt-sidebar-row"]');
    expect(sidebarRow?.querySelector(".animate-pulse")).not.toBeNull();
    fireEvent.click(placeholder);
    expect(h.onOpenRow).not.toHaveBeenCalled();
    // The confirmed row beside it keeps its link.
    expect(screen.getByRole("link", { name: "First" })).toHaveAttribute("href", "/acme/browse/WEB-1");
  });

  it("shows the load-more placeholder only while rows are arriving", () => {
    const { unmount } = renderSidebar();
    expect(screen.queryByTestId("gantt-load-more")).not.toBeInTheDocument();
    unmount();
    renderSidebar({ loadingMore: true });
    expect(screen.getByTestId("gantt-load-more")).toBeInTheDocument();
  });

  it("shows the load-more placeholder while the server holds more rows (ISS-059)", () => {
    renderSidebar({ hasMore: true });
    const trailing = screen.getByTestId("gantt-load-more");
    expect(trailing.querySelector(".animate-pulse")).not.toBeNull();
  });

  it("renders an empty list with zero-height spacers", () => {
    const { container } = renderSidebar({ rows: [] });
    expect(screen.queryAllByTestId("gantt-sidebar-row")).toHaveLength(0);
    const spacers = container.querySelectorAll<HTMLElement>("[aria-hidden='true']");
    expect(spacers).toHaveLength(2);
    for (const spacer of spacers) expect(spacer.style.height).toBe("0px");
  });
});

describe("GanttSidebar reorder (ISS-050)", () => {
  it("derives a drop on the target row's upper half as before", () => {
    const onReorder = vi.fn();
    renderSidebar({ reorder: { onReorder } });
    const rows = screen.getAllByTestId("gantt-sidebar-row");
    const [first, , third] = rows as [HTMLElement, HTMLElement, HTMLElement];
    first.getBoundingClientRect = () => ({ top: 0, height: 44, bottom: 44 }) as DOMRect;
    third.getBoundingClientRect = () => ({ top: 88, height: 44, bottom: 132 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(third, { clientY: 92 });
    expect(screen.getByTestId("gantt-drop-indicator")).toHaveAttribute("data-placement", "before");
    fireEvent.mouseUp(third);
    expect(onReorder).toHaveBeenCalledWith("row-1", "row-3", "before");
  });

  it("uses the lower half for an after drop", () => {
    const onReorder = vi.fn();
    renderSidebar({ reorder: { onReorder } });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    first.getBoundingClientRect = () => ({ top: 0, height: 44, bottom: 44 }) as DOMRect;
    second.getBoundingClientRect = () => ({ top: 44, height: 44, bottom: 88 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(second, { clientY: 80 });
    fireEvent.mouseUp(second);
    expect(onReorder).toHaveBeenCalledWith("row-1", "row-2", "after");
  });

  it("marks a window-level travel so the click it ends does not open the row", () => {
    const onReorder = vi.fn();
    const { onOpenRow } = renderSidebar({ reorder: { onReorder } });
    const [first] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement];
    // Travel past the threshold without ever hovering another row: no
    // drop target, so no reorder, but the gesture still owns its click.
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(document, { clientY: 22 + DRAG_THRESHOLD_PX });
    fireEvent.mouseUp(first);
    fireEvent.click(within(first).getByRole("link"));
    expect(onReorder).not.toHaveBeenCalled();
    expect(onOpenRow).not.toHaveBeenCalled();
  });

  it("shows no drop indicator until the pointer has travelled", () => {
    const onReorder = vi.fn();
    renderSidebar({ reorder: { onReorder } });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    first.getBoundingClientRect = () => ({ top: 0, height: 44, bottom: 44 }) as DOMRect;
    second.getBoundingClientRect = () => ({ top: 44, height: 44, bottom: 88 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    // A hover inside the threshold is not travel: no indicator, and the
    // release stays a click.
    fireEvent.mouseMove(second, { clientY: 22 + DRAG_THRESHOLD_PX - 1 });
    expect(screen.queryByTestId("gantt-drop-indicator")).not.toBeInTheDocument();
    fireEvent.mouseUp(second);
    expect(onReorder).not.toHaveBeenCalled();
  });

  it("cancels a travelled drag on Escape without reordering", () => {
    const onReorder = vi.fn();
    renderSidebar({ reorder: { onReorder } });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    first.getBoundingClientRect = () => ({ top: 0, height: 44, bottom: 44 }) as DOMRect;
    second.getBoundingClientRect = () => ({ top: 44, height: 44, bottom: 88 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(second, { clientY: 80 });
    expect(screen.getByTestId("gantt-drop-indicator")).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.mouseUp(second);
    expect(onReorder).not.toHaveBeenCalled();
    expect(screen.queryByTestId("gantt-drop-indicator")).not.toBeInTheDocument();
  });

  it("ignores a drop back onto the dragged row", () => {
    const onReorder = vi.fn();
    renderSidebar({ reorder: { onReorder } });
    const [first] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement];
    first.getBoundingClientRect = () => ({ top: 0, height: 44, bottom: 44 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(first, { clientY: 60 });
    fireEvent.mouseUp(first);
    expect(onReorder).not.toHaveBeenCalled();
    expect(screen.queryByTestId("gantt-drop-indicator")).not.toBeInTheDocument();
  });

  it("shows the indicator on the target's half and hides it on re-hover", () => {
    const onReorder = vi.fn();
    renderSidebar({ reorder: { onReorder } });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    first.getBoundingClientRect = () => ({ top: 0, height: 44, bottom: 44 }) as DOMRect;
    second.getBoundingClientRect = () => ({ top: 44, height: 44, bottom: 88 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(second, { clientY: 60 });
    expect(screen.getByTestId("gantt-drop-indicator")).toHaveAttribute("data-placement", "before");
    // Same target, now below its midpoint: the line moves to the bottom,
    // and a repeated hover at the same spot changes nothing.
    fireEvent.mouseMove(second, { clientY: 80 });
    expect(screen.getByTestId("gantt-drop-indicator")).toHaveAttribute("data-placement", "after");
    fireEvent.mouseMove(second, { clientY: 80 });
    fireEvent.mouseUp(second);
    expect(onReorder).toHaveBeenCalledWith("row-1", "row-2", "after");
  });

  it("does not reorder without a drag or without the binding", () => {
    const onReorder = vi.fn();
    const { unmount } = renderSidebar({ reorder: { onReorder } });
    const [first] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement];
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseUp(first);
    expect(onReorder).not.toHaveBeenCalled();
    unmount();
    renderSidebar();
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("drops a drag that ends without a target and cancels on Escape", () => {
    const onReorder = vi.fn();
    renderSidebar({ reorder: { onReorder } });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    second.getBoundingClientRect = () => ({ top: 44, height: 44, bottom: 88 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.mouseUp(second);
    expect(onReorder).not.toHaveBeenCalled();
    expect(screen.queryByTestId("gantt-drop-indicator")).not.toBeInTheDocument();
  });

  it("keeps the row link inert for the click that ends a drag", () => {
    const onReorder = vi.fn();
    const { onOpenRow } = renderSidebar({ reorder: { onReorder } });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    first.getBoundingClientRect = () => ({ top: 0, height: 44, bottom: 44 }) as DOMRect;
    second.getBoundingClientRect = () => ({ top: 44, height: 44, bottom: 88 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(second, { clientY: 80 });
    fireEvent.mouseUp(second);
    fireEvent.click(within(first).getByRole("link"));
    expect(onOpenRow).not.toHaveBeenCalled();
  });

  it("refuses to drag an unconfirmed row", () => {
    const onReorder = vi.fn();
    renderSidebar({
      rows: [row({ id: "temp-abc", name: "Draft", unconfirmed: true }), row()],
      reorder: { onReorder },
    });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    second.getBoundingClientRect = () => ({ top: 44, height: 44, bottom: 88 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(second, { clientY: 80 });
    fireEvent.mouseUp(second);
    expect(onReorder).not.toHaveBeenCalled();
  });
});

describe("GanttSidebar bulk selection (ISS-116)", () => {
  function selectionStub(selected: readonly string[] = []) {
    const onRowToggle = vi.fn();
    return {
      selection: {
        isSelected: (id: string) => selected.includes(id),
        onRowToggle,
      },
      onRowToggle,
    };
  }

  it("toggles rows through the binding and carries the reveal attribute", () => {
    const { selection, onRowToggle } = selectionStub(["row-2"]);
    renderSidebar({ selection });
    expect(screen.getByLabelText("Select WEB-1")).not.toBeChecked();
    expect(screen.getByLabelText("Select WEB-2")).toBeChecked();
    fireEvent.click(screen.getByLabelText("Select WEB-1"));
    expect(onRowToggle).toHaveBeenCalledWith("row-1", false);
    fireEvent.click(screen.getByLabelText("Select WEB-3"), { shiftKey: true });
    expect(onRowToggle).toHaveBeenCalledWith("row-3", true);
    const rows = screen.getAllByTestId("gantt-sidebar-row");
    expect(rows[0]?.getAttribute("data-selection-id")).toBe("row-1");
  });

  it("hides checkboxes without a binding, for epics and for unconfirmed rows", () => {
    const { unmount } = renderSidebar();
    expect(screen.queryByLabelText("Select WEB-1")).toBeNull();
    unmount();
    const { selection } = selectionStub();
    renderSidebar({
      selection,
      rows: [
        row({ id: "epic-1", identifier: "WEB-9", name: "Epic", isEpic: true }),
        row({ id: "temp-abc", identifier: "WEB-0", name: "Draft", unconfirmed: true }),
      ],
    });
    expect(screen.queryByRole("checkbox")).toBeNull();
  });
});

describe("GanttSidebar entity vocabulary (NEWFRONT-801)", () => {
  it("renders another entity's rows through the vocabulary", () => {
    renderSidebar({
      entity: {
        rowHref: (entry) => `/acme/projects/p/modules/${entry.id}`,
        rowPrefix: () => (
          <svg data-testid="entity-glyph" aria-hidden="true">
            <circle cx="5" cy="5" r="4" />
          </svg>
        ),
        rowDuration: () => null,
      },
    });
    const first = screen.getAllByTestId("gantt-sidebar-row")[0]!;
    const link = within(first).getByRole("link");
    expect(link).toHaveAttribute("href", "/acme/projects/p/modules/row-1");
    expect(within(first).getByTestId("entity-glyph")).toBeInTheDocument();
    expect(within(first).queryByText("WEB-1")).toBeNull();
    expect(within(first).queryByTestId("gantt-row-duration")).toBeNull();
  });

  it("falls back to the issue rendering per field", () => {
    renderSidebar({ entity: { rowDuration: () => "9 days" } });
    const rows = screen.getAllByTestId("gantt-sidebar-row");
    expect(within(rows[0]!).getByRole("link")).toHaveAttribute("href", "/acme/browse/WEB-1");
    expect(within(rows[0]!).getByText("WEB-1")).toBeInTheDocument();
    expect(screen.getAllByTestId("gantt-row-duration")[0]).toHaveTextContent("9 days");
  });
});

describe("GanttSidebar reorder attempts (NEWFRONT-801)", () => {
  it("reports a travelled press without a binding and swallows its click", async () => {
    vi.useFakeTimers();
    try {
      const onReorderAttempt = vi.fn();
      const { onOpenRow } = renderSidebar({ onReorderAttempt });
      const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
      fireEvent.mouseDown(first, { button: 0, clientY: 22 });
      fireEvent.mouseMove(second, { clientY: 22 + DRAG_THRESHOLD_PX });
      expect(onReorderAttempt).toHaveBeenCalledTimes(1);
      // A second travelled move reports nothing more.
      fireEvent.mouseMove(second, { clientY: 22 + DRAG_THRESHOLD_PX + 10 });
      expect(onReorderAttempt).toHaveBeenCalledTimes(1);
      fireEvent.mouseUp(second);
      const link = within(first).getByRole("link");
      fireEvent.click(link);
      expect(onOpenRow).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(0);
      fireEvent.click(link);
      expect(onOpenRow).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("leaves a click that never travels alone", () => {
    const onReorderAttempt = vi.fn();
    const { onOpenRow } = renderSidebar({ onReorderAttempt });
    const [first] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement];
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseUp(first);
    // A later hover can never report the forgotten press.
    fireEvent.mouseMove(first, { clientY: 22 + DRAG_THRESHOLD_PX + 40 });
    expect(onReorderAttempt).not.toHaveBeenCalled();
    fireEvent.click(within(first).getByRole("link"));
    expect(onOpenRow).toHaveBeenCalledTimes(1);
  });

  it("never attempts while a binding is mounted", () => {
    const onReorder = vi.fn();
    const onReorderAttempt = vi.fn();
    renderSidebar({ reorder: { onReorder }, onReorderAttempt });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    first.getBoundingClientRect = () => ({ top: 0, height: 44, bottom: 44 }) as DOMRect;
    second.getBoundingClientRect = () => ({ top: 44, height: 44, bottom: 88 }) as DOMRect;
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(second, { clientY: 50 });
    fireEvent.mouseUp(second);
    expect(onReorder).toHaveBeenCalledWith("row-1", "row-2", "before");
    expect(onReorderAttempt).not.toHaveBeenCalled();
  });

  it("ignores presses without the callback", () => {
    const { onOpenRow } = renderSidebar();
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(second, { clientY: 80 });
    fireEvent.mouseUp(second);
    fireEvent.click(within(first).getByRole("link"));
    expect(onOpenRow).toHaveBeenCalledTimes(1);
  });

  it("ignores non-left presses and unconfirmed rows", () => {
    const onReorderAttempt = vi.fn();
    renderSidebar({
      rows: [row({ id: "temp-abc", name: "Draft", unconfirmed: true }), row()],
      onReorderAttempt,
    });
    const [first, second] = screen.getAllByTestId("gantt-sidebar-row") as [HTMLElement, HTMLElement];
    fireEvent.mouseDown(first, { button: 0, clientY: 22 });
    fireEvent.mouseMove(second, { clientY: 80 });
    fireEvent.mouseUp(second);
    fireEvent.mouseDown(second, { button: 2, clientY: 60 });
    fireEvent.mouseMove(first, { clientY: 22 });
    fireEvent.mouseUp(first);
    expect(onReorderAttempt).not.toHaveBeenCalled();
  });
});
