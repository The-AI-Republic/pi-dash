// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared chart tooltip and legend content tests (NEWFRONT-732).
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ChartLegendContent, ChartTooltipContent } from "./ChartTooltip.js";

describe("ChartTooltipContent", () => {
  it("renders the label with one row per hovered entry", () => {
    render(
      <ChartTooltipContent
        label="Work Items"
        payload={[
          { name: "Count", value: 2, color: "#5e6ad2" },
          { name: "Resolved", value: "3" },
        ]}
      />
    );
    expect(screen.getByText("Work Items")).toBeInTheDocument();
    expect(screen.getByText("Count")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("Resolved")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("tags rows for the parity driver (NEWFRONT-733)", () => {
    render(
      <ChartTooltipContent
        label="Urgent"
        payload={[
          { name: "Backlog", value: 1 },
          { name: "Done", value: 1 },
        ]}
      />
    );
    expect(screen.getByTestId("chart-tooltip-label")).toHaveTextContent("Urgent");
    const rows = screen.getAllByTestId("chart-tooltip-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]!).getByTestId("chart-tooltip-name")).toHaveTextContent("Backlog");
    expect(within(rows[0]!).getByTestId("chart-tooltip-value")).toHaveTextContent("1");
    expect(within(rows[1]!).getByTestId("chart-tooltip-name")).toHaveTextContent("Done");
  });

  it("renders nothing without hovered entries", () => {
    const { container: missing } = render(<ChartTooltipContent label="Work Items" />);
    expect(missing).toBeEmptyDOMElement();
    const { container: empty } = render(<ChartTooltipContent label="Work Items" payload={[]} />);
    expect(empty).toBeEmptyDOMElement();
  });
});

describe("ChartLegendContent", () => {
  it("renders one swatch plus label per series in order", () => {
    render(<ChartLegendContent payload={[{ value: "Resolved", color: "#2f9e5b" }, { value: "Created" }]} />);
    const legend = screen.getByTestId("chart-legend");
    const labels = within(legend)
      .getAllByText(/^(Resolved|Created)$/)
      .map((node) => node.textContent);
    expect(labels).toEqual(["Resolved", "Created"]);
  });

  it("renders nothing without legend rows", () => {
    const { container: missing } = render(<ChartLegendContent />);
    expect(missing).toBeEmptyDOMElement();
    const { container: empty } = render(<ChartLegendContent payload={[]} />);
    expect(empty).toBeEmptyDOMElement();
  });
});
