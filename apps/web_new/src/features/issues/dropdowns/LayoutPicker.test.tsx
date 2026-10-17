// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { LAYOUT_ORDER } from "../layouts/kinds.js";
import { LayoutPicker } from "./LayoutPicker.js";

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

describe("LayoutPicker", () => {
  it("names the current layout and lists every layout with no search", () => {
    render(<LayoutPicker value="list" onChange={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "List" }));
    expect(optionNames()).toEqual(["List", "Board", "Calendar", "Spreadsheet", "Timeline"]);
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
    expect(screen.getByRole("option", { selected: true })).toHaveTextContent("List");
  });

  it("picks through the menu", () => {
    const onChange = vi.fn();
    render(<LayoutPicker value="list" onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "List" }));
    fireEvent.click(screen.getByRole("option", { name: "Board" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("kanban");
  });

  it("re-fires the change when picking the active entry", () => {
    const onChange = vi.fn();
    render(<LayoutPicker value="list" onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "List" }));
    fireEvent.click(screen.getByRole("option", { name: "List" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("list");
  });

  it("hides the excluded kinds", () => {
    render(<LayoutPicker value="list" onChange={() => {}} excluded={["kanban", "gantt_chart"]} />);
    fireEvent.click(screen.getByRole("button", { name: "List" }));
    expect(optionNames()).toEqual(["List", "Calendar", "Spreadsheet"]);
  });

  it("still names an excluded current value, with nothing selected", () => {
    render(<LayoutPicker value="kanban" onChange={() => {}} excluded={["kanban"]} />);
    fireEvent.click(screen.getByRole("button", { name: "Board" }));
    expect(optionNames()).toEqual(["List", "Calendar", "Spreadsheet", "Timeline"]);
    expect(screen.queryByRole("option", { selected: true })).not.toBeInTheDocument();
  });

  it("renders nothing when every layout is excluded", () => {
    render(<LayoutPicker value="list" onChange={() => {}} excluded={[...LAYOUT_ORDER]} />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("supports icon-only and custom triggers", () => {
    const { rerender } = render(<LayoutPicker value="list" onChange={() => {}} iconOnly />);
    expect(screen.getByRole("button", { name: "List" }).querySelector("svg")).not.toBeNull();
    rerender(<LayoutPicker value="list" onChange={() => {}} trigger={<button type="button">Custom layout</button>} />);
    expect(screen.getByRole("button", { name: "Custom layout" })).toBeInTheDocument();
  });

  it("follows a controlled open state", () => {
    const onOpenChange = vi.fn();
    render(<LayoutPicker value="list" onChange={() => {}} open onOpenChange={onOpenChange} />);
    expect(screen.getByRole("option", { name: "Board" })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("does not open while disabled", () => {
    render(<LayoutPicker value="list" onChange={() => {}} disabled />);
    expect(screen.getByRole("button", { name: "List" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "List" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });
});
