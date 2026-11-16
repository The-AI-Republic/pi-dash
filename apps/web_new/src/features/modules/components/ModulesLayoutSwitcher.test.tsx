// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The layout switcher (MOD-011): three pressed-state options on wide
// screens, choosing one reports it and re-choosing the active one does
// nothing; small screens replace it with a menu offering list and gallery
// only, where timeline maps onto list.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const viewportHarness = vi.hoisted(() => ({ narrow: false }));

vi.mock("../../../shared/shell/index.js", () => ({
  useNarrowViewport: () => viewportHarness.narrow,
}));

import { ModulesLayoutSwitcher } from "./ModulesLayoutSwitcher.js";

beforeEach(() => {
  viewportHarness.narrow = false;
});

afterEach(() => {
  vi.restoreAllMocks();
});

function labels(): string[] {
  return screen.getAllByRole("button").map((button) => button.getAttribute("aria-label") ?? "");
}

describe("ModulesLayoutSwitcher", () => {
  it("offers list, board and timeline with the active one pressed", () => {
    render(<ModulesLayoutSwitcher layout="board" onLayoutChange={vi.fn()} />);
    expect(screen.getByRole("group", { name: "Module layout" })).toBeInTheDocument();
    expect(labels()).toEqual(["List layout", "Board layout", "Timeline layout"]);
    expect(screen.getByRole("button", { name: "List layout" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Board layout" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByTestId("modules-layout-menu")).not.toBeInTheDocument();
  });

  it("reports a newly chosen layout", () => {
    const onLayoutChange = vi.fn();
    render(<ModulesLayoutSwitcher layout="list" onLayoutChange={onLayoutChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Timeline layout" }));
    expect(onLayoutChange).toHaveBeenCalledWith("timeline");
  });

  it("does nothing when the active layout is chosen again", () => {
    const onLayoutChange = vi.fn();
    render(<ModulesLayoutSwitcher layout="list" onLayoutChange={onLayoutChange} />);
    fireEvent.click(screen.getByRole("button", { name: "List layout" }));
    expect(onLayoutChange).not.toHaveBeenCalled();
  });

  it("hides the wide switcher on small screens in favour of the menu", () => {
    viewportHarness.narrow = true;
    render(<ModulesLayoutSwitcher layout="board" onLayoutChange={vi.fn()} />);
    expect(screen.queryByRole("group", { name: "Module layout" })).not.toBeInTheDocument();
    expect(screen.getByTestId("modules-layout-menu")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change module layout, current Board" })).toBeInTheDocument();
  });

  it("offers exactly list and gallery through the compact menu", () => {
    viewportHarness.narrow = true;
    render(<ModulesLayoutSwitcher layout="list" onLayoutChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Change module layout, current List" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["List layout", "Gallery layout"]);
  });

  it("switches through the compact menu and skips the current choice", () => {
    viewportHarness.narrow = true;
    const onLayoutChange = vi.fn();
    render(<ModulesLayoutSwitcher layout="list" onLayoutChange={onLayoutChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Change module layout, current List" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Gallery layout" }));
    expect(onLayoutChange).toHaveBeenCalledWith("board");
    fireEvent.click(screen.getByRole("button", { name: "Change module layout, current List" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "List layout" }));
    expect(onLayoutChange).toHaveBeenCalledTimes(1);
  });

  it("shows a remembered timeline choice as the list on small screens", () => {
    viewportHarness.narrow = true;
    render(<ModulesLayoutSwitcher layout="timeline" onLayoutChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Change module layout, current List" })).toBeInTheDocument();
  });
});
