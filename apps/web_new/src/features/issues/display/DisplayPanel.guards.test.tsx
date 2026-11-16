// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Defensive matrix arms (parity/specs/issues.md §9, ISS-075). The real
// matrix always carries pills and switches, so the empty-option and
// unknown-label guards only run when a future surface narrows the table;
// a mocked matrix exercises them so the panel cannot regress into
// rendering bare section shells.
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DisplayPanel } from "./DisplayPanel.js";

const matrix = vi.hoisted(() => ({
  layoutDisplayMatrix: vi.fn(),
}));

vi.mock("./matrix.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./matrix.js")>();
  return { ...actual, layoutDisplayMatrix: matrix.layoutDisplayMatrix };
});

const EMPTY_MATRIX = {
  properties: [],
  groupBy: null,
  subGroupBy: null,
  orderBy: null,
  switches: [],
};

function renderWithMatrix(overrides: Record<string, unknown>) {
  matrix.layoutDisplayMatrix.mockReturnValue({ ...EMPTY_MATRIX, ...overrides });
  return render(
    <DisplayPanel
      layout="list"
      state={{
        groupBy: null,
        subGroupBy: null,
        orderBy: "sort_order",
        showEmptyGroups: false,
        showSubIssues: false,
        properties: {},
      }}
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
}

describe("DisplayPanel defensive matrix arms", () => {
  it("renders no section shells for an empty matrix", () => {
    renderWithMatrix({});
    const panel = screen.getByTestId("display-options-panel");
    expect(within(panel).queryAllByRole("heading")).toHaveLength(0);
  });

  it("renders the switches block with an empty switch list omitted", () => {
    renderWithMatrix({ properties: ["key"] });
    expect(screen.getByTestId("display-section-properties")).toBeInTheDocument();
    expect(screen.queryByTestId("display-section-options")).not.toBeInTheDocument();
  });

  it("falls back to the stored key for an unknown sort label", () => {
    renderWithMatrix({ orderBy: ["future_sort"] });
    const order = screen.getByTestId("display-section-order");
    expect(within(order).getByRole("button", { name: "future_sort" })).toHaveAttribute("aria-pressed", "false");
  });
});
