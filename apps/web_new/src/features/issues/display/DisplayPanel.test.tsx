// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { LayoutKind } from "../layouts/kinds.js";
import { DisplayPanel, type DisplayPanelProps, type DisplayPanelState } from "./DisplayPanel.js";

function state(overrides: Partial<DisplayPanelState> = {}): DisplayPanelState {
  return {
    groupBy: null,
    subGroupBy: null,
    orderBy: "sort_order",
    showEmptyGroups: false,
    showSubIssues: false,
    properties: {},
    ...overrides,
  };
}

function handlers() {
  return {
    onGroupBy: vi.fn(),
    onSubGroupBy: vi.fn(),
    onOrderBy: vi.fn(),
    onToggleSwitch: vi.fn(),
    onToggleProperty: vi.fn(),
  };
}

function renderPanel(
  layout: LayoutKind,
  props: Omit<Partial<DisplayPanelProps>, "state"> & { state?: Partial<DisplayPanelState> } = {}
) {
  const calls = handlers();
  const { state: partial, ...rest } = props;
  const rendered = render(
    <DisplayPanel
      layout={layout}
      state={state(partial)}
      hideCycle={false}
      hideModule={false}
      isEpic={false}
      {...calls}
      {...rest}
    />
  );
  return { calls, ...rendered };
}

function section(name: string): HTMLElement {
  return screen.getByTestId(`display-section-${name}`);
}

describe("DisplayPanel", () => {
  it("renders the list sections in fixed order without sub-group by", () => {
    renderPanel("list");
    const panel = screen.getByTestId("display-options-panel");
    const headings = within(panel)
      .getAllByRole("heading")
      .map((heading) => heading.textContent);
    expect(headings).toEqual(["Display Properties", "Group by", "Order by", "Options"]);
    expect(screen.queryByTestId("display-section-subGroup")).not.toBeInTheDocument();
    for (const pill of ["Assignee", "Priority", "State", "Labels"]) {
      expect(within(section("properties")).getByRole("button", { name: pill })).toBeInTheDocument();
    }
  });

  it("shows sub-group by only for kanban with a grouping", () => {
    const { rerender } = render(
      <DisplayPanel
        layout="list"
        state={state({ groupBy: "state" })}
        hideCycle={false}
        hideModule={false}
        isEpic={false}
        onGroupBy={vi.fn()}
        onSubGroupBy={vi.fn()}
        onOrderBy={vi.fn()}
        onToggleSwitch={vi.fn()}
        onToggleProperty={vi.fn()}
      />
    );
    expect(screen.queryByTestId("display-section-subGroup")).not.toBeInTheDocument();

    rerender(
      <DisplayPanel
        layout="kanban"
        state={state({ groupBy: null })}
        hideCycle={false}
        hideModule={false}
        isEpic={false}
        onGroupBy={vi.fn()}
        onSubGroupBy={vi.fn()}
        onOrderBy={vi.fn()}
        onToggleSwitch={vi.fn()}
        onToggleProperty={vi.fn()}
      />
    );
    expect(screen.queryByTestId("display-section-subGroup")).not.toBeInTheDocument();

    rerender(
      <DisplayPanel
        layout="kanban"
        state={state({ groupBy: "state" })}
        hideCycle={false}
        hideModule={false}
        isEpic={false}
        onGroupBy={vi.fn()}
        onSubGroupBy={vi.fn()}
        onOrderBy={vi.fn()}
        onToggleSwitch={vi.fn()}
        onToggleProperty={vi.fn()}
      />
    );
    expect(screen.getByTestId("display-section-subGroup")).toBeInTheDocument();
  });

  it("renders the minimal calendar and gantt matrices", () => {
    const { unmount } = render(
      <DisplayPanel
        layout="calendar"
        state={state()}
        hideCycle={false}
        hideModule={false}
        isEpic={false}
        onGroupBy={vi.fn()}
        onSubGroupBy={vi.fn()}
        onOrderBy={vi.fn()}
        onToggleSwitch={vi.fn()}
        onToggleProperty={vi.fn()}
      />
    );
    expect(screen.getByTestId("display-section-properties")).toBeInTheDocument();
    expect(within(section("properties")).getByRole("button", { name: "ID" })).toBeInTheDocument();
    expect(screen.queryByTestId("display-section-group")).not.toBeInTheDocument();
    expect(screen.queryByTestId("display-section-order")).not.toBeInTheDocument();
    expect(within(section("options")).getByRole("button", { name: "Show sub-work items" })).toBeInTheDocument();
    expect(within(section("options")).queryByRole("button", { name: "Show empty groups" })).not.toBeInTheDocument();
    unmount();

    renderPanel("gantt_chart");
    expect(screen.getByTestId("display-section-order")).toBeInTheDocument();
    expect(screen.queryByTestId("display-section-group")).not.toBeInTheDocument();
  });

  it("keeps spreadsheet pills with ordering and only the sub-issue switch", () => {
    renderPanel("spreadsheet");
    expect(within(section("properties")).getByRole("button", { name: "State" })).toBeInTheDocument();
    expect(screen.queryByTestId("display-section-group")).not.toBeInTheDocument();
    expect(within(section("order")).getByRole("button", { name: "Manual" })).toBeInTheDocument();
    expect(within(section("order")).queryByRole("button", { name: "Due date" })).not.toBeInTheDocument();
    expect(within(section("options")).getByRole("button", { name: "Show sub-work items" })).toBeInTheDocument();
    expect(within(section("options")).queryByRole("button", { name: "Show empty groups" })).not.toBeInTheDocument();
  });

  it("collapses and reopens each section", async () => {
    const user = userEvent.setup();
    renderPanel("list");
    const toggle = screen.getByRole("button", { name: "Collapse Group by section" });
    expect(within(section("group")).getByRole("button", { name: "States" })).toBeInTheDocument();
    await user.click(toggle);
    expect(screen.getByRole("button", { name: "Expand Group by section" })).toBeInTheDocument();
    expect(within(section("group")).queryByRole("button", { name: "States" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand Group by section" }));
    expect(within(section("group")).getByRole("button", { name: "States" })).toBeInTheDocument();
  });

  it("marks the active group, sub-group and order choices", () => {
    renderPanel("kanban", { state: { groupBy: "state", subGroupBy: "priority", orderBy: "-created_at" } });
    expect(within(section("group")).getByRole("button", { name: "States" })).toHaveAttribute("aria-pressed", "true");
    expect(within(section("group")).getByRole("button", { name: "Labels" })).toHaveAttribute("aria-pressed", "false");
    expect(within(section("subGroup")).getByRole("button", { name: "Priority" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(within(section("subGroup")).getByRole("button", { name: "None" })).toHaveAttribute("aria-pressed", "false");
    expect(within(section("order")).getByRole("button", { name: "Last created" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(within(section("order")).getByRole("button", { name: "Manual" })).toHaveAttribute("aria-pressed", "false");
  });

  it("hides the other grouping axis on kanban", () => {
    const first = renderPanel("kanban", { state: { groupBy: "state", subGroupBy: "priority" } });
    expect(within(section("group")).queryByRole("button", { name: "Priority" })).not.toBeInTheDocument();
    expect(within(section("subGroup")).queryByRole("button", { name: "States" })).not.toBeInTheDocument();
    // Off kanban there is no cross-hide: the sub-group choice never hides a group option.
    first.unmount();
    renderPanel("list", { state: { groupBy: null, subGroupBy: "priority" } });
    expect(within(section("group")).getByRole("button", { name: "Priority" })).toBeInTheDocument();
  });

  it("suppresses cycle and module while their features are off", () => {
    renderPanel("list", { hideCycle: true, hideModule: true });
    expect(within(section("group")).queryByRole("button", { name: "Cycle" })).not.toBeInTheDocument();
    expect(within(section("group")).queryByRole("button", { name: "Module" })).not.toBeInTheDocument();
    expect(within(section("properties")).queryByRole("button", { name: "Cycle" })).not.toBeInTheDocument();
    expect(within(section("properties")).queryByRole("button", { name: "Module" })).not.toBeInTheDocument();
  });

  it("reads pills as active unless explicitly disabled", () => {
    renderPanel("list", { state: { properties: { assignee: false } } });
    expect(within(section("properties")).getByRole("button", { name: "State" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(within(section("properties")).getByRole("button", { name: "Assignee" })).toHaveAttribute(
      "aria-pressed",
      "false"
    );
  });

  it("relabels the sub-issue count on epic surfaces", () => {
    renderPanel("list", { isEpic: true });
    expect(within(section("properties")).getByRole("button", { name: "Work item count" })).toBeInTheDocument();
    expect(
      within(section("properties")).queryByRole("button", { name: "Sub-work item count" })
    ).not.toBeInTheDocument();
  });

  it("routes every control to its callback", async () => {
    const user = userEvent.setup();
    const { calls } = renderPanel("kanban", {
      state: { groupBy: "priority", showEmptyGroups: true, properties: { state: false } },
    });
    await user.click(within(section("group")).getByRole("button", { name: "States" }));
    expect(calls.onGroupBy).toHaveBeenCalledWith("state");
    await user.click(within(section("subGroup")).getByRole("button", { name: "None" }));
    expect(calls.onSubGroupBy).toHaveBeenCalledWith(null);
    await user.click(within(section("order")).getByRole("button", { name: "Last updated" }));
    expect(calls.onOrderBy).toHaveBeenCalledWith("-updated_at");
    await user.click(within(section("options")).getByRole("button", { name: "Show empty groups" }));
    expect(calls.onToggleSwitch).toHaveBeenCalledWith("show_empty_groups");
    await user.click(within(section("options")).getByRole("button", { name: "Show sub-work items" }));
    expect(calls.onToggleSwitch).toHaveBeenCalledWith("sub_issue");
    await user.click(within(section("properties")).getByRole("button", { name: "Assignee" }));
    expect(calls.onToggleProperty).toHaveBeenCalledWith("assignee", false);
    await user.click(within(section("properties")).getByRole("button", { name: "State" }));
    expect(calls.onToggleProperty).toHaveBeenCalledWith("state", true);
  });

  it("leaves every order unchecked for an unknown stored sort", () => {
    renderPanel("list", { state: { orderBy: "future_sort" } });
    const options = within(section("order"))
      .getAllByRole("button")
      .filter((option) => option.getAttribute("aria-pressed") !== null);
    expect(options).toHaveLength(6);
    for (const option of options) {
      expect(option).toHaveAttribute("aria-pressed", "false");
    }
  });
});
