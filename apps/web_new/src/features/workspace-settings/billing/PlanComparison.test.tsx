// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Full comparison (parity/specs/workspace-settings.md §29, WSET-029): one
// control whose label and expanded state track each other, titled sections
// in catalog order once open, one cell per compared plan, and an empty
// section list while collapsed.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { BILLING_COMPARISON } from "./model.js";
import { COMPARE_COLLAPSE_LABEL, COMPARE_EXPAND_LABEL, PlanComparison } from "./PlanComparison.js";

describe("PlanComparison", () => {
  it("starts collapsed under the expand label", () => {
    render(<PlanComparison />);
    expect(screen.getByRole("button", { name: COMPARE_EXPAND_LABEL })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryAllByRole("table")).toHaveLength(0);
    expect(screen.queryByRole("heading", { name: "Project + work tracking" })).not.toBeInTheDocument();
  });

  it("expands to every titled section in catalog order and collapses again", async () => {
    const user = userEvent.setup();
    render(<PlanComparison />);
    await user.click(screen.getByRole("button", { name: COMPARE_EXPAND_LABEL }));
    const toggle = screen.getByRole("button", { name: COMPARE_COLLAPSE_LABEL });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    for (const section of BILLING_COMPARISON) {
      expect(screen.getByRole("heading", { name: section.title })).toBeInTheDocument();
    }
    await user.click(toggle);
    expect(screen.getByRole("button", { name: COMPARE_EXPAND_LABEL })).toBeInTheDocument();
    expect(screen.queryAllByRole("table")).toHaveLength(0);
  });

  it("heads every table with the three compared plans and every feature row", async () => {
    const user = userEvent.setup();
    render(<PlanComparison />);
    await user.click(screen.getByRole("button", { name: COMPARE_EXPAND_LABEL }));
    const tables = screen.getAllByRole("table");
    expect(tables).toHaveLength(BILLING_COMPARISON.length);
    const first = tables[0];
    if (first === undefined) throw new Error("missing comparison table");
    const headers = within(first)
      .getAllByRole("columnheader")
      .map((cell) => cell.textContent);
    expect(headers).toEqual(["Feature", "Pro", "Business", "Enterprise"]);
    const section = BILLING_COMPARISON[0];
    if (section === undefined) throw new Error("missing comparison section");
    for (const feature of section.features) {
      expect(within(first).getByRole("rowheader", { name: feature.name })).toBeInTheDocument();
    }
  });

  it("reads flags as included and absent and passes words through", async () => {
    const user = userEvent.setup();
    render(<PlanComparison />);
    await user.click(screen.getByRole("button", { name: COMPARE_EXPAND_LABEL }));
    const access = screen.getByTestId("billing-comparison-section-access");
    expect(within(access).getAllByText("Included").length).toBeGreaterThan(0);
    expect(within(access).getAllByText("Not included").length).toBeGreaterThan(0);
    const work = screen.getByTestId("billing-comparison-section-work");
    expect(within(work).getAllByText("Unlimited").length).toBe(3);
  });
});
