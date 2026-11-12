// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Swimlane header (parity/specs/issues.md §5, ISS-029/031): the name,
// cumulative count, chevron direction and both toggle inputs.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { KanbanLaneHeader } from "./KanbanLaneHeader.js";

describe("KanbanLaneHeader", () => {
  it("renders the lane name and cumulative count expanded", () => {
    render(<KanbanLaneHeader name="Bug" count={4} collapsed={false} onToggle={vi.fn()} />);
    const header = screen.getByTestId("kanban-lane-header");
    expect(header).toHaveTextContent("Bug");
    expect(header).toHaveAttribute("aria-expanded", "true");
    expect(header).toHaveAttribute("aria-label", "Collapse Bug swimlane");
    expect(screen.getByTestId("kanban-lane-count")).toHaveTextContent("4");
  });

  it("reads a folded lane as expandable", () => {
    render(<KanbanLaneHeader name="Bug" count={4} collapsed onToggle={vi.fn()} />);
    const header = screen.getByTestId("kanban-lane-header");
    expect(header).toHaveAttribute("aria-expanded", "false");
    expect(header).toHaveAttribute("aria-label", "Expand Bug swimlane");
  });

  it("toggles on click and on Enter or Space", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(<KanbanLaneHeader name="Bug" count={1} collapsed={false} onToggle={onToggle} />);
    const header = screen.getByTestId("kanban-lane-header");
    await user.click(header);
    fireEvent.keyDown(header, { key: "Enter" });
    fireEvent.keyDown(header, { key: " " });
    fireEvent.keyDown(header, { key: "a" });
    expect(onToggle).toHaveBeenCalledTimes(3);
  });
});
