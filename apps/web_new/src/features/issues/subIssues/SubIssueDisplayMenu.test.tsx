// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Widget display dropdown (parity/specs/issues.md §15 + §22 ISS-094,
// ISS-178): grouping and ordering are single choices on the shared
// section chrome, the six columns are pressed pills, and the trigger
// reports how many settings differ from the defaults.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  defaultSubIssueDisplay,
  SUB_ISSUE_GROUP_OPTIONS,
  SUB_ISSUE_ORDER_OPTIONS,
  SUB_ISSUE_PROPERTY_KEYS,
  SUB_ISSUE_PROPERTY_LABELS,
} from "./rules.js";
import { SubIssueDisplayMenu } from "./SubIssueDisplayMenu.js";

function renderMenu(display = defaultSubIssueDisplay()) {
  const onGroupBy = vi.fn();
  const onOrderBy = vi.fn();
  const onToggleProperty = vi.fn();
  const view = render(
    <SubIssueDisplayMenu
      display={display}
      onGroupBy={onGroupBy}
      onOrderBy={onOrderBy}
      onToggleProperty={onToggleProperty}
    />
  );
  return { onGroupBy, onOrderBy, onToggleProperty, ...view };
}

function openMenu(): HTMLElement {
  fireEvent.click(screen.getByTestId("sub-issue-display-trigger"));
  return screen.getByTestId("sub-issue-display-panel");
}

describe("SubIssueDisplayMenu", () => {
  it("labels a default trigger and counts changed settings", () => {
    renderMenu();
    expect(screen.getByTestId("sub-issue-display-trigger")).toHaveAccessibleName("Display");
    expect(screen.getByTestId("sub-issue-display-trigger")).toHaveAttribute("aria-pressed", "false");
    render(
      <SubIssueDisplayMenu
        display={{
          groupBy: "state",
          orderBy: "-priority",
          properties: { ...defaultSubIssueDisplay().properties, key: false },
        }}
        onGroupBy={() => undefined}
        onOrderBy={() => undefined}
        onToggleProperty={() => undefined}
      />
    );
    const triggers = screen.getAllByTestId("sub-issue-display-trigger");
    expect(triggers[1]).toHaveAccessibleName("Display, 3 changed");
    expect(triggers[1]).toHaveAttribute("aria-pressed", "true");
  });

  it("marks the group and order choices and reports one pick each", () => {
    const h = renderMenu({ ...defaultSubIssueDisplay(), groupBy: "priority", orderBy: "start_date" });
    const panel = openMenu();
    const group = within(panel).getByTestId("sub-issue-display-group");
    const order = within(panel).getByTestId("sub-issue-display-order");
    expect(within(group).getByRole("radio", { name: "Priority" })).toHaveAttribute("aria-checked", "true");
    expect(within(order).getByRole("radio", { name: "Start date" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(within(panel).getByRole("radio", { name: "States" }));
    expect(h.onGroupBy).toHaveBeenCalledWith("state");
    fireEvent.click(within(panel).getByRole("radio", { name: "Last updated" }));
    expect(h.onOrderBy).toHaveBeenCalledWith("-updated_at");
  });

  it("clears grouping through the ungrouped row", () => {
    const h = renderMenu({ ...defaultSubIssueDisplay(), groupBy: "assignees" });
    const panel = openMenu();
    fireEvent.click(within(panel).getByRole("radio", { name: "None" }));
    expect(h.onGroupBy).toHaveBeenCalledWith(null);
  });

  it("draws every column as a pressed pill and toggles one", () => {
    const h = renderMenu();
    const panel = openMenu();
    const group = within(panel).getByTestId("sub-issue-display-properties");
    expect(
      within(group)
        .getAllByRole("button")
        .filter((button) =>
          SUB_ISSUE_PROPERTY_KEYS.some((key) => SUB_ISSUE_PROPERTY_LABELS[key] === button.textContent)
        )
        .map((button) => button.textContent)
    ).toEqual(SUB_ISSUE_PROPERTY_KEYS.map((key) => SUB_ISSUE_PROPERTY_LABELS[key]));
    for (const button of within(group)
      .getAllByRole("button")
      .filter((entry) => SUB_ISSUE_PROPERTY_KEYS.some((key) => SUB_ISSUE_PROPERTY_LABELS[key] === entry.textContent))) {
      expect(button).toHaveAttribute("aria-pressed", "true");
    }
    fireEvent.click(within(group).getByRole("button", { name: "Due date" }));
    expect(h.onToggleProperty).toHaveBeenCalledWith("due_date");
  });

  it("shows a hidden column unpressed", () => {
    renderMenu({
      ...defaultSubIssueDisplay(),
      properties: { ...defaultSubIssueDisplay().properties, assignee: false },
    });
    const panel = openMenu();
    expect(within(panel).getByRole("button", { name: "Assignee" })).toHaveAttribute("aria-pressed", "false");
  });

  it("keeps a short fixed list for every section", () => {
    expect(SUB_ISSUE_GROUP_OPTIONS.map((option) => option.label)).toEqual(["States", "Priority", "Assignees", "None"]);
    expect(SUB_ISSUE_ORDER_OPTIONS.map((option) => option.label)).toEqual([
      "Last created",
      "Last updated",
      "Start date",
      "Priority",
    ]);
    const h = renderMenu();
    h.unmount();
  });
});
