// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board column header (parity/specs/issues.md §5, ISS-028/030, ISS-036):
// the name and live count, the collapse fold, and the add control the
// expanded header carries.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { COLLAPSED_COLUMN_WIDTH, KanbanColumnHeader } from "./KanbanColumnHeader.js";

describe("KanbanColumnHeader", () => {
  it("renders the name, count and add control expanded", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(
      <KanbanColumnHeader
        name="Todo"
        count={3}
        collapsed={false}
        onToggleCollapse={onToggle}
        create={<div data-testid="header-create" />}
      />
    );
    expect(screen.getByTestId("kanban-column-header")).toHaveTextContent("Todo");
    expect(screen.getByTestId("kanban-column-count")).toHaveTextContent("3");
    expect(screen.getByTestId("header-create")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Collapse Todo column" }));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("renders without the add control when none is given", () => {
    render(<KanbanColumnHeader name="Todo" count={0} collapsed={false} onToggleCollapse={vi.fn()} />);
    expect(screen.getByTestId("kanban-column-header")).toHaveTextContent("Todo");
  });

  it("folds to the narrow strip and drops the add control", () => {
    render(
      <KanbanColumnHeader
        name="Todo"
        count={3}
        collapsed={true}
        onToggleCollapse={vi.fn()}
        create={<div data-testid="header-create" />}
      />
    );
    const header = screen.getByTestId("kanban-column-header");
    expect(header).toHaveAttribute("data-collapsed", "true");
    expect(header).toHaveStyle({ width: `${COLLAPSED_COLUMN_WIDTH}px` });
    expect(screen.queryByTestId("header-create")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Expand Todo column" })).toHaveAttribute("aria-expanded", "false");
  });
});
