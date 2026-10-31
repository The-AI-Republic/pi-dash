// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { INTAKE_STATUS } from "@pidash/api-client";
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { defaultSelectionForTab } from "../lib/filters.js";
import { IntakeFilterPanel } from "./IntakeFilterPanel.js";

const MEMBERS = [
  { userId: "u-1", displayName: "Rin", avatarUrl: null, suspended: false },
  { userId: "u-2", displayName: "Sol", avatarUrl: "https://example.com/sol.png", suspended: true },
];
const LABELS = [{ id: "l-1", name: "Alpha", color: "#ff0000" }];

const BASE = {
  tab: "open",
  selection: defaultSelectionForTab("open"),
  assigneeOptions: MEMBERS,
  createdByOptions: MEMBERS,
  membersLoading: false,
  membersFailed: false,
  labelOptions: LABELS,
  labelsLoading: false,
  labelsFailed: false,
  today: "2026-10-23",
  weekStart: 0,
  onSelectionChange: () => {},
} as const;

function section(key: string): HTMLElement {
  return screen.getByTestId(`intake-filter-${key}`);
}

function sectionOptions(key: string): string[] {
  return within(section(key))
    .queryAllByRole("checkbox")
    .map((node) => node.getAttribute("aria-label") ?? "");
}

function openPanel(): void {
  fireEvent.click(screen.getByTestId("intake-filters-trigger"));
}

describe("IntakeFilterPanel (INT-009)", () => {
  it("badges the trigger with the running applied count", () => {
    render(<IntakeFilterPanel {...BASE} />);
    expect(screen.getByTestId("intake-filters-count")).toHaveTextContent("1");
    expect(screen.getByTestId("intake-filters-trigger")).toHaveAttribute("aria-label", "Filters, 1 applied");
  });

  it("lays seven sections out in fixed order", () => {
    render(<IntakeFilterPanel {...BASE} />);
    openPanel();
    const panel = screen.getByTestId("intake-filters-panel");
    const testIds = within(panel)
      .getAllByTestId(/^intake-filter-/)
      .map((node) => node.getAttribute("data-testid"));
    expect(testIds).toEqual([
      "intake-filter-status",
      "intake-filter-priority",
      "intake-filter-assignees",
      "intake-filter-createdBy",
      "intake-filter-labels",
      "intake-filter-createdAt",
      "intake-filter-updatedAt",
    ]);
  });

  it("offers tab-dependent status options with the default marked", () => {
    const { rerender } = render(<IntakeFilterPanel {...BASE} />);
    openPanel();
    expect(sectionOptions("status")).toEqual(["Pending", "Snoozed"]);
    expect(screen.getByRole("checkbox", { name: "Pending" })).toHaveAttribute("aria-checked", "true");
    expect(within(section("status")).getByRole("heading", { name: "Status (1)" })).toBeInTheDocument();
    rerender(<IntakeFilterPanel {...BASE} tab="closed" selection={defaultSelectionForTab("closed")} />);
    expect(sectionOptions("status")).toEqual(["Accepted", "Declined", "Duplicate"]);
  });

  it("widens and narrows the status set through the host", () => {
    const onSelectionChange = vi.fn();
    const selection = defaultSelectionForTab("open");
    render(<IntakeFilterPanel {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    openPanel();
    fireEvent.click(screen.getByRole("checkbox", { name: "Snoozed" }));
    expect(onSelectionChange).toHaveBeenCalledWith({
      ...selection,
      statuses: [INTAKE_STATUS.pending, INTAKE_STATUS.snoozed],
    });
  });

  it("refuses to uncheck the last status", () => {
    const onSelectionChange = vi.fn();
    const selection = defaultSelectionForTab("open");
    render(<IntakeFilterPanel {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    openPanel();
    fireEvent.click(screen.getByRole("checkbox", { name: "Pending" }));
    expect(onSelectionChange).toHaveBeenCalledWith(selection);
    expect(onSelectionChange.mock.calls[0]?.[0]).toBe(selection);
  });

  it("toggles priorities, members and labels through the host", () => {
    const onSelectionChange = vi.fn();
    const selection = defaultSelectionForTab("open");
    render(<IntakeFilterPanel {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    openPanel();
    expect(sectionOptions("priority")).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
    fireEvent.click(screen.getByRole("checkbox", { name: "Urgent" }));
    expect(onSelectionChange).toHaveBeenCalledWith({ ...selection, priorities: ["urgent"] });
    fireEvent.click(within(section("assignees")).getByRole("checkbox", { name: "Rin" }));
    expect(onSelectionChange).toHaveBeenCalledWith({ ...selection, assignees: ["u-1"] });
    fireEvent.click(screen.getByRole("checkbox", { name: "Alpha" }));
    expect(onSelectionChange).toHaveBeenCalledWith({ ...selection, labels: ["l-1"] });
  });

  it("leaves suspended members unselectable", () => {
    render(<IntakeFilterPanel {...BASE} />);
    openPanel();
    const suspended = within(section("assignees")).getByRole("checkbox", { name: "Sol" });
    expect(suspended).toBeDisabled();
  });

  it("searches every section from the one box", () => {
    render(<IntakeFilterPanel {...BASE} />);
    openPanel();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search filters" }), { target: { value: "urg" } });
    expect(sectionOptions("priority")).toEqual(["Urgent"]);
    expect(sectionOptions("status")).toEqual([]);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search filters" }), { target: { value: "" } });
    expect(sectionOptions("priority")).toHaveLength(5);
  });

  it("tracks member and label reads through loading and failure", () => {
    const { rerender } = render(<IntakeFilterPanel {...BASE} membersLoading labelsLoading />);
    openPanel();
    expect(within(section("assignees")).getByText("Loading values")).toBeInTheDocument();
    expect(within(section("labels")).getByText("Loading values")).toBeInTheDocument();
    rerender(<IntakeFilterPanel {...BASE} membersFailed labelsFailed />);
    expect(within(section("assignees")).getByRole("alert")).toHaveTextContent("Couldn't load members.");
    expect(within(section("createdBy")).getByRole("alert")).toHaveTextContent("Couldn't load members.");
    expect(within(section("labels")).getByRole("alert")).toHaveTextContent("Couldn't load labels.");
  });

  it("toggles date presets through the host", () => {
    const onSelectionChange = vi.fn();
    const selection = defaultSelectionForTab("open");
    render(<IntakeFilterPanel {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    openPanel();
    expect(
      within(section("createdAt"))
        .getAllByRole("radio")
        .map((node) => node.getAttribute("aria-label"))
    ).toEqual(["Today", "Yesterday", "Last 7 days", "Last 30 days", "Custom"]);
    fireEvent.click(within(section("updatedAt")).getByRole("radio", { name: "Today" }));
    expect(onSelectionChange).toHaveBeenCalledWith({
      ...selection,
      updatedAt: { kind: "preset", preset: "today" },
    });
  });

  it("applies an explicit range through the host", () => {
    const onSelectionChange = vi.fn();
    const selection = defaultSelectionForTab("open");
    render(<IntakeFilterPanel {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    openPanel();
    fireEvent.click(within(section("createdAt")).getByRole("radio", { name: "Custom" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(onSelectionChange).toHaveBeenCalledWith({
      ...selection,
      createdAt: { kind: "custom-range", from: "2026-10-23", to: "2026-11-23" },
    });
  });
});
