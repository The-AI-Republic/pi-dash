// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board columns (parity/specs/issues.md §5, ISS-028/030, ISS-041–042): the
// header's glyph, name and live count, the collapse fold to the narrow
// strip, the skeleton during the first page, the quick-add slot, the
// swimlane cell's fold, the drop-disabled overlay, the scroll sentinel
// and the explicit load-more entry.
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KanbanColumn, KanbanColumnSentinel, KanbanLaneColumn, KanbanLoadMore } from "./KanbanColumn.js";

describe("KanbanColumn", () => {
  it("renders the name, count, cards and footer", () => {
    const onToggle = vi.fn();
    render(
      <KanbanColumn
        columnKey="todo"
        title="Todo"
        count={3}
        rows={[<div key="a" data-testid="card-a" />, <div key="b" data-testid="card-b" />]}
        loading={false}
        collapsed={false}
        onToggleCollapse={onToggle}
        footer={<div data-testid="footer" />}
      />
    );
    expect(screen.getByTestId("kanban-column")).toHaveAttribute("data-column-name", "Todo");
    expect(screen.getByTestId("kanban-column-header")).toHaveTextContent("Todo");
    expect(screen.getByTestId("kanban-column-count")).toHaveTextContent("3");
    expect(screen.getByTestId("card-a")).toBeInTheDocument();
    expect(screen.getByTestId("footer")).toBeInTheDocument();
  });

  it("folds the column and still shows its count", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(
      <KanbanColumn
        columnKey="todo"
        title="Todo"
        count={3}
        rows={[<div key="a" data-testid="card-a" />]}
        loading={false}
        collapsed
        onToggleCollapse={onToggle}
        footer={<div data-testid="footer" />}
      />
    );
    expect(screen.getByTestId("kanban-column")).toHaveAttribute("data-collapsed", "true");
    expect(screen.getByTestId("kanban-column")).toHaveStyle({ width: "44px" });
    expect(screen.queryByTestId("card-a")).not.toBeInTheDocument();
    expect(screen.queryByTestId("footer")).not.toBeInTheDocument();
    expect(screen.getByTestId("kanban-column-count")).toHaveTextContent("3");
    await user.click(screen.getByRole("button", { name: "Expand Todo column" }));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("offers the collapse control expanded and names it for screen readers", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(
      <KanbanColumn
        columnKey="todo"
        title="Todo"
        count={0}
        rows={[]}
        loading={false}
        collapsed={false}
        onToggleCollapse={onToggle}
      />
    );
    const toggle = screen.getByRole("button", { name: "Collapse Todo column" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("shows skeleton shells while the first page loads", () => {
    render(
      <KanbanColumn
        columnKey="todo"
        title="Todo"
        count={2}
        rows={[]}
        loading
        collapsed={false}
        onToggleCollapse={vi.fn()}
      />
    );
    const skeleton = screen.getByTestId("kanban-column-loading");
    expect(skeleton).toBeInTheDocument();
    expect(skeleton).toHaveClass("shrink-0");
  });

  it("keeps loaded cards instead of the skeleton while a refresh runs", () => {
    render(
      <KanbanColumn
        columnKey="todo"
        title="Todo"
        count={1}
        rows={[<div key="a" data-testid="card-a" />]}
        loading
        collapsed={false}
        onToggleCollapse={vi.fn()}
      />
    );
    expect(screen.queryByTestId("kanban-column-loading")).not.toBeInTheDocument();
    expect(screen.getByTestId("card-a")).toBeInTheDocument();
  });
});

describe("column header add control", () => {
  it("passes the add control through to the header", () => {
    render(
      <KanbanColumn
        columnKey="todo"
        title="Todo"
        count={1}
        rows={[]}
        loading={false}
        collapsed={false}
        onToggleCollapse={vi.fn()}
        create={<div data-testid="column-create" />}
      />
    );
    expect(screen.getByTestId("column-create")).toBeInTheDocument();
  });
});

describe("column drop overlay and pagination slot", () => {
  it("paints the drop-disabled notice over the column body", () => {
    render(
      <KanbanColumn
        columnKey="todo"
        title="Todo"
        count={1}
        rows={[<div key="a" data-testid="card-a" />]}
        loading={false}
        collapsed={false}
        onToggleCollapse={vi.fn()}
        dropOverlayText="Ordered by Priority"
        afterRows={<div data-testid="sentinel" />}
      />
    );
    expect(screen.getByTestId("kanban-drop-overlay")).toHaveTextContent("Ordered by Priority");
    expect(screen.getByTestId("sentinel")).toBeInTheDocument();
  });

  it("renders no overlay without a notice", () => {
    render(
      <KanbanColumn
        columnKey="todo"
        title="Todo"
        count={1}
        rows={[<div key="a" data-testid="card-a" />]}
        loading={false}
        collapsed={false}
        onToggleCollapse={vi.fn()}
        dropOverlayText={null}
      />
    );
    expect(screen.queryByTestId("kanban-drop-overlay")).not.toBeInTheDocument();
  });

  it("paints the notice over a swimlane cell", () => {
    render(
      <KanbanLaneColumn
        laneKey="bug"
        columnKey="todo"
        collapsed={false}
        rows={[<div key="a" data-testid="card-a" />]}
        dropOverlayText="Cannot move to a completed cycle"
        afterRows={<div data-testid="cell-more" />}
      />
    );
    expect(screen.getByTestId("kanban-drop-overlay")).toHaveTextContent("Cannot move to a completed cycle");
    expect(screen.getByTestId("cell-more")).toBeInTheDocument();
  });
});

describe("KanbanColumnSentinel", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("fires once its end nears the viewport", () => {
    const callbacks: Array<(entries: Array<{ isIntersecting: boolean }>) => void> = [];
    const disconnects: Array<() => void> = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(callback: (entries: Array<{ isIntersecting: boolean }>) => void) {
          callbacks.push(callback);
        }
        observe(): void {}
        disconnect(): void {
          disconnects.push(() => undefined);
        }
      }
    );
    const onVisible = vi.fn();
    const { unmount } = render(<KanbanColumnSentinel onVisible={onVisible} />);
    expect(screen.getByTestId("kanban-column-sentinel")).toBeInTheDocument();
    act(() => callbacks[0]?.([{ isIntersecting: false }]));
    expect(onVisible).not.toHaveBeenCalled();
    act(() => callbacks[0]?.([{ isIntersecting: true }]));
    expect(onVisible).toHaveBeenCalledTimes(1);
    unmount();
    expect(disconnects).toHaveLength(1);
  });

  it("renders inert without an observer", () => {
    const onVisible = vi.fn();
    render(<KanbanColumnSentinel onVisible={onVisible} />);
    expect(screen.getByTestId("kanban-column-sentinel")).toBeInTheDocument();
    expect(onVisible).not.toHaveBeenCalled();
  });
});

describe("KanbanLoadMore", () => {
  it("loads the next page on press", async () => {
    const user = userEvent.setup();
    const onLoad = vi.fn();
    render(<KanbanLoadMore onLoad={onLoad} />);
    await user.click(screen.getByTestId("kanban-load-more"));
    expect(onLoad).toHaveBeenCalledTimes(1);
  });
});

describe("KanbanLaneColumn", () => {
  it("renders a cell's cards and footer when the lane is open", () => {
    render(
      <KanbanLaneColumn
        laneKey="bug"
        columnKey="todo"
        collapsed={false}
        rows={[<div key="a" data-testid="card-a" />]}
        footer={<div data-testid="footer" />}
      />
    );
    const cell = screen.getByTestId("kanban-cell");
    expect(cell).toHaveAttribute("data-lane-key", "bug");
    expect(cell).toHaveAttribute("data-column-key", "todo");
    expect(screen.getByTestId("card-a")).toBeInTheDocument();
    expect(screen.getByTestId("footer")).toBeInTheDocument();
  });

  it("hides a collapsed lane's cards and footer but keeps the cell", () => {
    render(
      <KanbanLaneColumn
        laneKey="bug"
        columnKey="todo"
        collapsed
        rows={[<div key="a" data-testid="card-a" />]}
        footer={<div data-testid="footer" />}
      />
    );
    expect(screen.getByTestId("kanban-cell")).toBeInTheDocument();
    expect(screen.queryByTestId("card-a")).not.toBeInTheDocument();
    expect(screen.queryByTestId("footer")).not.toBeInTheDocument();
  });
});
