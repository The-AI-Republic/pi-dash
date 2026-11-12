// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LAYOUT_ORDER } from "../layouts/kinds.js";
import { LayoutSwitcher } from "./LayoutSwitcher.js";

describe("LayoutSwitcher", () => {
  it("offers the registered layouts in canonical order, active pressed", () => {
    render(<LayoutSwitcher offered={[...LAYOUT_ORDER].reverse()} active="kanban" onSwitch={() => {}} />);
    const group = screen.getByTestId("layout-switcher-segmented");
    expect(group).toHaveAttribute("role", "group");
    const buttons = within(group).getAllByRole("button");
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
      "List layout",
      "Board layout",
      "Calendar layout",
      "Spreadsheet layout",
      "Timeline layout",
    ]);
    expect(buttons.map((button) => button.getAttribute("aria-pressed"))).toEqual([
      "false",
      "true",
      "false",
      "false",
      "false",
    ]);
    expect(buttons.map((button) => button.getAttribute("data-layout"))).toEqual([...LAYOUT_ORDER]);
    expect(screen.getByRole("button", { name: "Board layout" })).toHaveAttribute("data-active", "true");
    expect(screen.getByRole("button", { name: "List layout" })).not.toHaveAttribute("data-active");
  });

  it("switches on inactive segments and no-ops on the active one", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    render(<LayoutSwitcher offered={LAYOUT_ORDER} active="list" onSwitch={onSwitch} />);
    await user.click(screen.getByRole("button", { name: "Timeline layout" }));
    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(onSwitch).toHaveBeenCalledWith("gantt_chart");
    await user.click(screen.getByRole("button", { name: "List layout" }));
    expect(onSwitch).toHaveBeenCalledTimes(1);
  });

  it("renders nothing when no layout is registered", () => {
    render(<LayoutSwitcher offered={[]} active="list" onSwitch={() => {}} />);
    expect(screen.queryByTestId("layout-switcher-segmented")).not.toBeInTheDocument();
    expect(screen.queryByTestId("layout-switcher-dropdown")).not.toBeInTheDocument();
  });

  it("collapses to a dropdown below the wide breakpoint", () => {
    render(<LayoutSwitcher offered={LAYOUT_ORDER} active="list" onSwitch={() => {}} />);
    expect(screen.getByTestId("layout-switcher-segmented")).toHaveClass("hidden");
    expect(screen.getByTestId("layout-switcher-segmented")).toHaveClass("lg:inline-flex");
    expect(screen.getByTestId("layout-switcher-dropdown")).toHaveClass("lg:hidden");
  });

  it("offers every layout through the dropdown with a check on the active one", () => {
    render(<LayoutSwitcher offered={LAYOUT_ORDER} active="calendar" onSwitch={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Change issue layout, current Calendar" }));
    const items = screen.getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["List", "Board", "Calendar", "Spreadsheet", "Timeline"]);
    const checks = items.map((item) => item.querySelectorAll("svg").length);
    expect(checks).toEqual([1, 1, 2, 1, 1]);
  });

  it("switches through the dropdown and no-ops on the active entry", () => {
    const onSwitch = vi.fn();
    render(<LayoutSwitcher offered={LAYOUT_ORDER} active="list" onSwitch={onSwitch} />);
    fireEvent.click(screen.getByRole("button", { name: "Change issue layout, current List" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Board" }));
    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(onSwitch).toHaveBeenCalledWith("kanban");
    fireEvent.click(screen.getByRole("button", { name: "Change issue layout, current List" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "List" }));
    expect(onSwitch).toHaveBeenCalledTimes(1);
  });
});
