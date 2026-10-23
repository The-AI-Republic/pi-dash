// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IntakeIssueRow } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { IntakeListPane } from "./IntakeListPane.js";
import { INTAKE_LIST_PANEL_ID } from "./IntakeTabs.js";

function row(id: string, name: string): IntakeIssueRow {
  return {
    id: `intake-${id}`,
    status: -2,
    duplicate_to: null,
    snoozed_till: null,
    source: null,
    created_by: null,
    issue: {
      id,
      name,
      priority: "none",
      sequence_id: 3,
      project_id: "223e4567-e89b-12d3-a456-426614174001",
      created_at: "2026-10-01T00:00:00Z",
      label_ids: [],
      created_by: null,
    },
  };
}

const BASE = {
  tab: "open",
  openTotal: 2,
  loading: false,
  failed: false,
  rows: [] as IntakeIssueRow[],
  emptyKind: "open-guidance",
  selectedId: undefined,
  onSelectTab: () => {},
  onSelectRow: () => {},
  onRetry: () => {},
} as const;

describe("IntakeListPane (INT-001/004/008)", () => {
  it("renders the tab-controlled panel with the tabs above it", () => {
    render(<IntakeListPane {...BASE} />);
    const panel = screen.getByRole("tabpanel", { name: "Open intake requests" });
    expect(panel).toHaveAttribute("id", INTAKE_LIST_PANEL_ID);
    expect(screen.getAllByRole("tab")).toHaveLength(2);
  });

  it("shows only the skeleton while the first page loads", () => {
    render(<IntakeListPane {...BASE} loading rows={[row("i-1", "A")]} />);
    expect(screen.getByTestId("intake-list-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-list")).not.toBeInTheDocument();
    expect(screen.queryByTestId("intake-empty-open")).not.toBeInTheDocument();
  });

  it("offers a retry when the read failed", () => {
    const onRetry = vi.fn();
    render(<IntakeListPane {...BASE} failed onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("intake-empty-open")).not.toBeInTheDocument();
  });

  it("renders the empty state that fits the tab when no rows arrive", () => {
    render(<IntakeListPane {...BASE} />);
    expect(screen.getByTestId("intake-empty-open")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-list")).not.toBeInTheDocument();
  });

  it("renders rows as selectable titles and reports selection", () => {
    const onSelectRow = vi.fn();
    const first = row("i-1", "Alpha");
    const second = row("i-2", "Beta");
    render(<IntakeListPane {...BASE} rows={[first, second]} selectedId="i-2" onSelectRow={onSelectRow} />);
    const rows = screen.getAllByTestId("intake-row");
    expect(rows).toHaveLength(2);
    expect(screen.getAllByTestId("intake-row-title").map((node) => node.textContent)).toEqual(["Alpha", "Beta"]);
    expect(rows[1]).toHaveAttribute("aria-current", "true");
    expect(rows[0]).not.toHaveAttribute("aria-current");
    fireEvent.click(rows[0] as HTMLElement);
    expect(onSelectRow).toHaveBeenCalledWith("i-1");
    expect(screen.queryByTestId("intake-empty-open")).not.toBeInTheDocument();
  });
});
