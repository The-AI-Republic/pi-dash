// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { LayoutSwitcher, SPACE_LAYOUT_LABELS } from "./LayoutSwitcher.js";

describe("LayoutSwitcher (SPC-020/021)", () => {
  it("offers exactly the layouts it is given, in order", () => {
    render(<LayoutSwitcher offered={["list", "kanban"]} active="list" onSelect={() => undefined} />);
    const options = screen.getAllByTestId("space-layout-option");
    expect(options.map((option) => option.getAttribute("data-layout"))).toEqual(["list", "kanban"]);
  });

  it("still renders a single-option switcher", () => {
    render(<LayoutSwitcher offered={["kanban"]} active="kanban" onSelect={() => undefined} />);
    const options = screen.getAllByTestId("space-layout-option");
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveAttribute("aria-pressed", "true");
  });

  it("announces the pressed state and names each layout", () => {
    const onSelect = vi.fn();
    render(<LayoutSwitcher offered={["list", "kanban"]} active="kanban" onSelect={onSelect} />);
    const [list, kanban] = screen.getAllByTestId("space-layout-option");
    expect(list).toHaveAttribute("aria-pressed", "false");
    expect(list).toHaveAccessibleName("List layout");
    expect(list).toHaveAttribute("title", SPACE_LAYOUT_LABELS.list.short);
    expect(kanban).toHaveAttribute("aria-pressed", "true");
    expect(kanban).toHaveAccessibleName("Board layout");
    expect(kanban).toHaveAttribute("data-active", "true");
    expect(list).not.toHaveAttribute("data-active");
  });

  it("keeps the switcher's tooltip labels the parity driver reads", () => {
    expect(SPACE_LAYOUT_LABELS.list.short).toBe("List");
    expect(SPACE_LAYOUT_LABELS.kanban.short).toBe("Board");
  });

  it("reports the layout the visitor picks, including the active one", async () => {
    const onSelect = vi.fn();
    render(<LayoutSwitcher offered={["list", "kanban"]} active="list" onSelect={onSelect} />);
    await userEvent.click(screen.getByRole("button", { name: "Board layout" }));
    expect(onSelect).toHaveBeenCalledWith("kanban");
    await userEvent.click(screen.getByRole("button", { name: "List layout" }));
    expect(onSelect).toHaveBeenLastCalledWith("list");
  });

  it("is keyboard operable", async () => {
    const onSelect = vi.fn();
    render(<LayoutSwitcher offered={["list", "kanban"]} active="list" onSelect={onSelect} />);
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "List layout" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith("list");
    await userEvent.tab();
    await userEvent.keyboard(" ");
    expect(onSelect).toHaveBeenLastCalledWith("kanban");
  });

  it("renders nothing when no layout is offered", () => {
    render(<LayoutSwitcher offered={[]} active="list" onSelect={() => undefined} />);
    expect(screen.queryByTestId("space-layout-switcher")).not.toBeInTheDocument();
  });
});
