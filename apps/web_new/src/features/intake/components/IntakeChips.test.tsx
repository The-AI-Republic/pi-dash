// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { INTAKE_STATUS } from "@pidash/api-client";
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { defaultSelectionForTab } from "../lib/filters.js";
import { IntakeChips } from "./IntakeChips.js";

const MEMBERS = new Map([
  ["u-1", "Rin"],
  ["u-2", "Sol"],
]);
const LABELS = new Map([["l-1", "Alpha"]]);

const BASE = {
  memberNameById: MEMBERS,
  labelNameById: LABELS,
  onSelectionChange: () => {},
};

function group(key: string): HTMLElement {
  const found = screen.getAllByTestId("intake-chip-group").find((node) => node.getAttribute("data-chip-group") === key);
  if (found === undefined) throw new Error(`missing chip group ${key}`);
  return found;
}

describe("IntakeChips (INT-009)", () => {
  it("chips the tab default status with no remove control", () => {
    render(<IntakeChips {...BASE} selection={defaultSelectionForTab("open")} />);
    const status = group("status");
    expect(within(status).getByTestId("intake-chip-value")).toHaveTextContent("Pending");
    expect(within(status).queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("intake-chip-group")).toHaveLength(1);
  });

  it("chips resolved defaults on Closed", () => {
    render(<IntakeChips {...BASE} selection={defaultSelectionForTab("closed")} />);
    const values = within(group("status"))
      .getAllByTestId("intake-chip-value")
      .map((node) => node.textContent);
    expect(values).toEqual(["Accepted", "Declined", "Duplicate"]);
  });

  it("removes one status while several remain", () => {
    const onSelectionChange = vi.fn();
    const selection = {
      ...defaultSelectionForTab("open"),
      statuses: [INTAKE_STATUS.pending, INTAKE_STATUS.snoozed],
    };
    render(<IntakeChips {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Remove Snoozed Status filter" }));
    expect(onSelectionChange).toHaveBeenCalledWith({ ...selection, statuses: [INTAKE_STATUS.pending] });
  });

  it("chips priorities, members and labels with remove controls", () => {
    const onSelectionChange = vi.fn();
    const selection = {
      ...defaultSelectionForTab("open"),
      priorities: ["urgent", "bogus"],
      assignees: ["u-1"],
      createdBy: ["u-2"],
      labels: ["l-1"],
    };
    render(<IntakeChips {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    expect(
      within(group("priority"))
        .getAllByTestId("intake-chip-value")
        .map((node) => node.textContent)
    ).toEqual(["Urgent", "bogus"]);
    expect(within(group("assignees")).getByTestId("intake-chip-value")).toHaveTextContent("Rin");
    expect(within(group("createdBy")).getByTestId("intake-chip-value")).toHaveTextContent("Sol");
    expect(within(group("labels")).getByTestId("intake-chip-value")).toHaveTextContent("Alpha");
    fireEvent.click(screen.getByRole("button", { name: "Remove Rin Assignees filter" }));
    expect(onSelectionChange).toHaveBeenCalledWith({ ...selection, assignees: [] });
  });

  it("skips values the reference reads no longer know", () => {
    const selection = {
      ...defaultSelectionForTab("open"),
      assignees: ["u-gone"],
      createdBy: ["u-gone"],
      labels: ["l-gone"],
    };
    render(<IntakeChips {...BASE} selection={selection} />);
    expect(screen.getAllByTestId("intake-chip-group")).toHaveLength(1);
  });

  it("chips date presets and explicit ranges", () => {
    const selection = {
      ...defaultSelectionForTab("open"),
      createdAt: { kind: "preset", preset: "today" },
      updatedAt: { kind: "custom-range", from: "2026-10-15", to: "2026-10-20" },
    } as const;
    render(<IntakeChips {...BASE} selection={selection} />);
    expect(within(group("createdAt")).getByTestId("intake-chip-value")).toHaveTextContent("Today");
    expect(within(group("updatedAt")).getByTestId("intake-chip-value")).toHaveTextContent("Oct 15 - 20, 2026");
  });

  it("clears a group through its clear-all control", () => {
    const onSelectionChange = vi.fn();
    const selection = {
      ...defaultSelectionForTab("open"),
      priorities: ["urgent", "high"],
      createdAt: { kind: "preset", preset: "today" },
    } as const;
    render(<IntakeChips {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Clear Priority filters" }));
    expect(onSelectionChange).toHaveBeenCalledWith({ ...selection, priorities: [] });
    fireEvent.click(screen.getByRole("button", { name: "Clear Created date filters" }));
    expect(onSelectionChange).toHaveBeenCalledWith({ ...selection, createdAt: null });
  });

  it("removes a date value like clearing its group", () => {
    const onSelectionChange = vi.fn();
    const selection = {
      ...defaultSelectionForTab("open"),
      updatedAt: { kind: "custom-after", day: "2026-10-15" },
    } as const;
    render(<IntakeChips {...BASE} selection={selection} onSelectionChange={onSelectionChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Remove After Oct 15, 2026 Updated date filter" }));
    expect(onSelectionChange).toHaveBeenCalledWith({ ...selection, updatedAt: null });
  });
});
