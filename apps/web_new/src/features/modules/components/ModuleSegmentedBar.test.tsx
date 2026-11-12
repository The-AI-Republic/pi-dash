// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The card's segmented work-item bar (parity/specs/modules.md §3.6,
// MOD-020): one slice per populated state group, proportional widths, its
// own hue each, and an empty track for a module without work items.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { MODULE_SEGMENT_COLORS } from "../lib/moduleIdentity.js";
import { makeModuleRow } from "../testing.js";
import { ModuleSegmentedBar, moduleSegmentSummary } from "./ModuleSegmentedBar.js";

describe("ModuleSegmentedBar", () => {
  it("draws a slice per populated group with its own width and hue", () => {
    render(
      <ModuleSegmentedBar
        module={makeModuleRow({ total_issues: 4, completed_issues: 2, started_issues: 1, backlog_issues: 1 })}
      />
    );
    const slices = screen.getAllByTestId("modules-card-progress-segment");
    expect(slices.map((slice) => slice.getAttribute("data-segment-group"))).toEqual([
      "completed",
      "started",
      "backlog",
    ]);
    expect(slices.map((slice) => slice.getAttribute("data-segment-count"))).toEqual(["2", "1", "1"]);
    expect(slices[0]?.style.width).toBe("50%");
    expect(slices[1]?.style.width).toBe("25%");
    expect(slices[0]?.style.backgroundColor).toBe("rgb(22, 163, 74)");
    expect(MODULE_SEGMENT_COLORS.completed).toBe("#16a34a");
  });

  it("names the mix in words for a reader", () => {
    render(<ModuleSegmentedBar module={makeModuleRow({ total_issues: 3, completed_issues: 2, started_issues: 1 })} />);
    expect(screen.getByRole("img")).toHaveAccessibleName("Work items by state: 2 completed, 1 started");
  });

  it("keeps an empty track for a module without work items", () => {
    const { container } = render(<ModuleSegmentedBar module={makeModuleRow()} />);
    expect(screen.queryAllByTestId("modules-card-progress-segment")).toEqual([]);
    expect(screen.getByRole("img")).toHaveAccessibleName("Work items by state: No work items yet");
    expect(container.querySelector('[data-testid="modules-card-progress"]')).toBeInTheDocument();
  });
});

describe("moduleSegmentSummary", () => {
  it("says no work items for an empty mix", () => {
    expect(moduleSegmentSummary([])).toBe("No work items yet");
  });

  it("joins the slices in reading order", () => {
    expect(
      moduleSegmentSummary([
        { group: "completed", label: "Completed", count: 2, width: 50, color: "#000" },
        { group: "cancelled", label: "Cancelled", count: 1, width: 25, color: "#111" },
      ])
    ).toBe("2 completed, 1 cancelled");
  });
});
