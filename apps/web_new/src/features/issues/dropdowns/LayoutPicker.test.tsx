// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { LAYOUT_ORDER } from "../layouts/kinds.js";
import { LayoutPicker } from "./LayoutPicker.js";

describe("LayoutPicker", () => {
  it("names the current layout on the trigger", () => {
    render(<LayoutPicker value="list" onChange={() => {}} />);
    const trigger = screen.getByRole("button", { name: "Layout: List" });
    expect(trigger).toHaveTextContent("List");
    expect(trigger).toHaveAttribute("data-testid", "layout-picker-trigger");
  });

  it("honors a custom label and test id", () => {
    render(<LayoutPicker value="kanban" onChange={() => {}} label="View layout" testId="view-layout" />);
    expect(screen.getByRole("button", { name: "View layout: Board" })).toHaveAttribute(
      "data-testid",
      "view-layout-trigger"
    );
    expect(screen.getByTestId("view-layout")).toBeInTheDocument();
  });

  it("lists every layout with no search and a check on the selected one", () => {
    render(<LayoutPicker value="list" onChange={() => {}} />);
    fireEvent.click(screen.getByTestId("layout-picker-trigger"));
    const items = screen.getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["List", "Board", "Calendar", "Spreadsheet", "Timeline"]);
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(items.map((item) => item.querySelectorAll("svg").length)).toEqual([2, 1, 1, 1, 1]);
  });

  it("picks through the menu and no-ops on the active entry", () => {
    const onChange = vi.fn();
    render(<LayoutPicker value="list" onChange={onChange} />);
    fireEvent.click(screen.getByTestId("layout-picker-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Board" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("kanban");
    fireEvent.click(screen.getByTestId("layout-picker-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "List" }));
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("hides the excluded kinds", () => {
    render(<LayoutPicker value="list" onChange={() => {}} excluded={["kanban", "gantt_chart"]} />);
    fireEvent.click(screen.getByTestId("layout-picker-trigger"));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "List",
      "Calendar",
      "Spreadsheet",
    ]);
  });

  it("still names an excluded current value, with no check in the menu", () => {
    render(<LayoutPicker value="kanban" onChange={() => {}} excluded={["kanban"]} />);
    expect(screen.getByRole("button", { name: "Layout: Board" })).toHaveTextContent("Board");
    fireEvent.click(screen.getByTestId("layout-picker-trigger"));
    const items = screen.getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["List", "Calendar", "Spreadsheet", "Timeline"]);
    expect(items.map((item) => item.querySelectorAll("svg").length)).toEqual([1, 1, 1, 1]);
  });

  it("renders nothing when every layout is excluded", () => {
    render(<LayoutPicker value="list" onChange={() => {}} excluded={[...LAYOUT_ORDER]} />);
    expect(screen.queryByTestId("layout-picker")).not.toBeInTheDocument();
  });

  it("does not open while disabled", () => {
    render(<LayoutPicker value="list" onChange={() => {}} disabled />);
    expect(screen.getByRole("button", { name: "Layout: List" })).toBeDisabled();
    fireEvent.click(screen.getByTestId("layout-picker-trigger"));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});
