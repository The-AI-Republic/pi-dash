// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { INTAKE_LIST_PANEL_ID, IntakeTabs, openTabAccessibleName } from "./IntakeTabs.js";

describe("IntakeTabs (INT-004)", () => {
  it("renders a tablist with Open then Closed", () => {
    render(<IntakeTabs activeTab="open" openTotal={3} onSelect={() => {}} />);
    const tablist = screen.getByRole("tablist");
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Open3", "Closed"]);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
    expect(tabs[1]).toHaveAttribute("aria-selected", "false");
    for (const tab of tabs) expect(tab).toHaveAttribute("aria-controls", INTAKE_LIST_PANEL_ID);
    expect(tablist).toBeInTheDocument();
  });

  it("badges Open with the active read's total and names it accessibly", () => {
    render(<IntakeTabs activeTab="open" openTotal={4} onSelect={() => {}} />);
    expect(screen.getByTestId("intake-open-count")).toHaveTextContent("4");
    expect(screen.getByRole("tab", { name: "Open, 4 pending" })).toBeInTheDocument();
  });

  it("omits the badge and the count wording while the total is unknown", () => {
    render(<IntakeTabs activeTab="open" openTotal={undefined} onSelect={() => {}} />);
    expect(screen.queryByTestId("intake-open-count")).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Open" })).toBeInTheDocument();
    expect(openTabAccessibleName(undefined)).toBe("Open");
  });

  it("hides the badge on Closed even when a total is passed", () => {
    render(<IntakeTabs activeTab="closed" openTotal={9} onSelect={() => {}} />);
    expect(screen.queryByTestId("intake-open-count")).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Closed" })).toHaveAttribute("aria-selected", "true");
  });

  it("selects on click and moves focus with the arrow keys", () => {
    const onSelect = vi.fn();
    render(<IntakeTabs activeTab="open" openTotal={1} onSelect={onSelect} />);
    fireEvent.click(screen.getByRole("tab", { name: /Closed/ }));
    expect(onSelect).toHaveBeenCalledWith("closed");
    const open = screen.getByTestId("intake-tab-open");
    const closed = screen.getByTestId("intake-tab-closed");
    open.focus();
    fireEvent.keyDown(open, { key: "ArrowRight" });
    expect(closed).toHaveFocus();
    fireEvent.keyDown(closed, { key: "ArrowLeft" });
    expect(open).toHaveFocus();
    fireEvent.keyDown(open, { key: "Tab" });
    expect(open).toHaveFocus();
  });

  it("moves backward from Closed", () => {
    render(<IntakeTabs activeTab="closed" openTotal={1} onSelect={() => {}} />);
    const open = screen.getByTestId("intake-tab-open");
    const closed = screen.getByTestId("intake-tab-closed");
    closed.focus();
    fireEvent.keyDown(closed, { key: "ArrowRight" });
    expect(open).toHaveFocus();
  });

  it("keeps only the active tab in the tab order", () => {
    render(<IntakeTabs activeTab="closed" openTotal={1} onSelect={() => {}} />);
    expect(screen.getByTestId("intake-tab-open")).toHaveAttribute("tabindex", "-1");
    expect(screen.getByTestId("intake-tab-closed")).toHaveAttribute("tabindex", "0");
  });
});
