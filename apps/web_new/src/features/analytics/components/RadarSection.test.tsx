// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics project-insights section tests (parity/specs/analytics.md §6, ANL-006).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsChart: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/charts/index.js", () => ({
  RadarChart: (props: { points?: Array<{ label: string; value: number }>; seriesName?: string }) => (
    <div data-testid="mock-chart-radar" data-points={JSON.stringify(props.points)} data-series={props.seriesName} />
  ),
}));

import { RadarSection } from "./RadarSection.js";

const PROJECTS = [
  { id: "a", name: "Alpha" },
  { id: "b", name: "Beta" },
  { id: "c", name: "Gamma" },
  { id: "d", name: "Delta" },
];

function renderSection(projectIds: readonly string[] = []): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <RadarSection workspaceSlug="acme" projectIds={projectIds} projects={PROJECTS} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("RadarSection", () => {
  it("covers the fetch with a skeleton behind the section header", () => {
    contractMocks.getAnalyticsChart.mockReturnValue(new Promise<never>(() => {}));
    renderSection();
    expect(screen.getByRole("heading", { name: "Project Insights" })).toBeInTheDocument();
    expect(screen.getByTestId("analytics-radar-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("mock-chart-radar")).not.toBeInTheDocument();
    expect(screen.queryByTestId("analytics-radar-row")).not.toBeInTheDocument();
  });

  it("plots the server rows with a matching written summary", async () => {
    const rows = [
      { key: "work_items", name: "Work Items", count: 2 },
      { key: "cycles", name: "Cycles", count: 0 },
    ];
    contractMocks.getAnalyticsChart.mockResolvedValue(rows);
    renderSection(["a"]);
    const chart = await screen.findByTestId("mock-chart-radar");
    expect(chart.dataset["series"]).toBe("Count");
    expect(chart.dataset["points"]).toBe(
      JSON.stringify([
        { label: "Work Items", value: 2 },
        { label: "Cycles", value: 0 },
      ])
    );
    const summary = screen.getAllByTestId("analytics-radar-row");
    expect(summary.map((row) => within(row).getByTestId("analytics-radar-dimension").textContent)).toEqual([
      "Work Items",
      "Cycles",
    ]);
    expect(summary.map((row) => within(row).getByTestId("analytics-radar-count").textContent)).toEqual(["2", "0"]);
    expect(screen.getByTestId("analytics-radar-scope")).toHaveTextContent("Alpha");
    expect(screen.queryByTestId("analytics-section-guidance")).not.toBeInTheDocument();
  });

  it("summarizes the scope with the picker wording and drops unknown ids", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue([{ key: "work_items", name: "Work Items", count: 1 }]);
    renderSection(["nope", "a", "b", "c", "d"]);
    await screen.findByTestId("mock-chart-radar");
    expect(screen.getByTestId("analytics-radar-scope")).toHaveTextContent("3+ projects");
  });

  it("reads an empty scope as all projects", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue([{ key: "work_items", name: "Work Items", count: 1 }]);
    renderSection([]);
    await screen.findByTestId("mock-chart-radar");
    expect(screen.getByTestId("analytics-radar-scope")).toHaveTextContent("All projects");
  });

  it("shows the guidance panel when there is nothing to plot", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue([]);
    renderSection();
    const guidance = await screen.findByTestId("analytics-section-guidance");
    expect(guidance).toHaveTextContent("Nothing to plot yet");
    expect(screen.queryByTestId("mock-chart-radar")).not.toBeInTheDocument();
    expect(screen.queryByTestId("analytics-radar-row")).not.toBeInTheDocument();
  });

  it("keeps the headers with no plot, rows, or guidance when the read fails", async () => {
    contractMocks.getAnalyticsChart.mockRejectedValue(new Error("graph read failed"));
    renderSection(["a"]);
    await screen.findByTestId("analytics-radar-scope");
    expect(screen.getByRole("heading", { name: "Project Insights" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Project summary" })).toBeInTheDocument();
    expect(screen.queryByTestId("mock-chart-radar")).not.toBeInTheDocument();
    expect(screen.queryByTestId("analytics-radar-row")).not.toBeInTheDocument();
    expect(screen.queryByTestId("analytics-section-guidance")).not.toBeInTheDocument();
  });
});
