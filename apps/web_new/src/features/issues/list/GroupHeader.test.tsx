// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { GroupHeader } from "./GroupHeader.js";

const SECTION = { key: "todo", title: "Todo", rows: [], total: 3, value: "todo" };

function setup(overrides: Partial<React.ComponentProps<typeof GroupHeader>> = {}) {
  const props = {
    section: SECTION,
    dimension: "state",
    collapsed: false,
    onToggle: vi.fn(),
    canAdd: true,
    menu: false,
    onCreate: vi.fn(),
    onAddExisting: vi.fn(),
    ...overrides,
  };
  render(<GroupHeader {...props} />);
  return props;
}

describe("GroupHeader", () => {
  it("renders the title, count and expander state", () => {
    const props = setup();
    const header = screen.getByTestId("list-group-header");
    expect(header).toHaveAttribute("aria-expanded", "true");
    expect(header).toHaveTextContent("Todo");
    expect(header).toHaveTextContent("3");
    fireEvent.click(header);
    expect(props.onToggle).toHaveBeenCalledTimes(1);
  });

  it("toggles from the keyboard", async () => {
    const user = userEvent.setup();
    const props = setup();
    screen.getByTestId("list-group-header").focus();
    await user.keyboard("{Enter}");
    expect(props.onToggle).toHaveBeenCalledTimes(1);
  });

  it("opens the create modal directly outside cycle/module context", () => {
    const props = setup();
    fireEvent.click(screen.getByRole("button", { name: "Add to Todo" }));
    expect(props.onCreate).toHaveBeenCalledTimes(1);
    expect(props.onToggle).not.toHaveBeenCalled();
    expect(props.onAddExisting).not.toHaveBeenCalled();
  });

  it("opens the keyboard create control without toggling the section", () => {
    const props = setup();
    fireEvent.keyDown(screen.getByRole("button", { name: "Add to Todo" }), { key: "Enter" });
    expect(props.onCreate).toHaveBeenCalledTimes(1);
    expect(props.onToggle).not.toHaveBeenCalled();
  });

  it("offers create and add-existing in cycle/module context", () => {
    const props = setup({ menu: true });
    fireEvent.click(screen.getByRole("button", { name: "Add to Todo" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Create work item" }));
    expect(props.onCreate).toHaveBeenCalledTimes(1);
    expect(props.onToggle).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Add to Todo" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Add an existing work item" }));
    expect(props.onAddExisting).toHaveBeenCalledTimes(1);
  });

  it("hides the add control when creation is disabled", () => {
    setup({ canAdd: false });
    expect(screen.queryByRole("button", { name: "Add to Todo" })).not.toBeInTheDocument();
  });
});

describe("GroupHeader selection", () => {
  it("renders no selection control when selection is off", () => {
    setup();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("renders a tri-state group control and toggles the group", () => {
    const props = setup({ selection: "partial", onSelectionToggle: vi.fn() });
    const box = screen.getByRole("checkbox", { name: "Select all work items in Todo" }) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(box.indeterminate).toBe(true);
    fireEvent.click(box);
    expect(props.onSelectionToggle).toHaveBeenCalledTimes(1);
    expect(props.onToggle).not.toHaveBeenCalled();
  });

  it("checks the group control when every row is selected", () => {
    setup({ selection: "complete", onSelectionToggle: vi.fn() });
    expect(screen.getByRole("checkbox", { name: "Select all work items in Todo" })).toBeChecked();
  });
});
