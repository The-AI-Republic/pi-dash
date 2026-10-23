// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board columns (parity/specs/issues.md §5, ISS-028/030): the header's
// glyph, name and live count, the collapse fold to the narrow strip, the
// skeleton during the first page, the quick-add slot and the swimlane
// cell's fold.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { KanbanColumn, KanbanLaneColumn } from "./KanbanColumn.js";

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
    expect(screen.getByTestId("kanban-column-loading")).toBeInTheDocument();
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
