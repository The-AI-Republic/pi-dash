// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics created-vs-resolved section tests (parity/specs/analytics.md §9, ANL-009).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsChart: vi.fn(), getProjectAnalyticsChart: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/charts/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/charts/index.js")>();
  return {
    ...original,
    AreaChart: (props: { points?: unknown; series?: unknown }) => (
      <div
        data-testid="mock-chart-area"
        data-points={JSON.stringify(props.points)}
        data-series={JSON.stringify(props.series)}
      />
    ),
  };
});

import { CreatedResolvedSection } from "./CreatedResolvedSection.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "423e4567-e89b-12d3-a456-426614174003";

const GUIDANCE =
  "Create projects with work items and assignees to start tracking performance, progress, and team impact here.";

function renderSection(projectIds: readonly string[] = []): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CreatedResolvedSection workspaceSlug="acme" projectIds={projectIds} />
    </QueryClientProvider>
  );
}

function renderScoped(dialogScope: { projectId: string; moduleId?: string | null }): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CreatedResolvedSection workspaceSlug="acme" projectIds={[]} dialogScope={dialogScope} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  document.documentElement.removeAttribute("data-theme");
});

describe("CreatedResolvedSection", () => {
  it("covers the fetch with a skeleton behind the section header", () => {
    contractMocks.getAnalyticsChart.mockReturnValue(new Promise<never>(() => {}));
    renderSection();
    expect(screen.getByRole("heading", { name: "Created vs Resolved" })).toBeInTheDocument();
    expect(screen.getByTestId("analytics-created-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("mock-chart-area")).not.toBeInTheDocument();
    expect(screen.queryByTestId("analytics-section-guidance")).not.toBeInTheDocument();
  });

  it("plots resolved then created with calendar labels", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({
      data: [
        { key: "2026-10-01", name: "2026-10-01", count: 3, created_issues: 3, completed_issues: 1 },
        { key: "2026-11-01", name: "2026-11-01", count: 0, created_issues: 0, completed_issues: 2 },
      ],
      schema: {},
    });
    renderSection(["a"]);
    const chart = await screen.findByTestId("mock-chart-area");
    expect(chart.dataset["series"]).toBe(
      JSON.stringify([
        { key: "resolved", name: "Resolved", color: "#2f9e5b" },
        { key: "created", name: "Created", color: "#2563eb" },
      ])
    );
    expect(chart.dataset["points"]).toBe(
      JSON.stringify([
        { label: "Oct 01, 2026", values: { resolved: 1, created: 3 } },
        { label: "Nov 01, 2026", values: { resolved: 2, created: 0 } },
      ])
    );
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(expect.anything(), "acme", {
      type: "work-items",
      projectIds: ["a"],
    });
    expect(screen.queryByTestId("analytics-section-guidance")).not.toBeInTheDocument();
  });

  it("resolves dark series colors under the dark document theme", async () => {
    document.documentElement.setAttribute("data-theme", "dark");
    contractMocks.getAnalyticsChart.mockResolvedValue({
      data: [{ key: "2026-10-01", name: "2026-10-01", count: 1, created_issues: 1, completed_issues: 0 }],
      schema: {},
    });
    renderSection();
    const chart = await screen.findByTestId("mock-chart-area");
    expect(chart.dataset["series"]).toBe(
      JSON.stringify([
        { key: "resolved", name: "Resolved", color: "#3db86c" },
        { key: "created", name: "Created", color: "#60a5fa" },
      ])
    );
  });

  it("plots the project-scoped series in the dialog and never the workspace read", async () => {
    contractMocks.getProjectAnalyticsChart.mockResolvedValue({
      data: [{ key: "2026-10-01", name: "2026-10-01", count: 2, created_issues: 2, completed_issues: 1 }],
      schema: {},
    });
    contractMocks.getAnalyticsChart.mockRejectedValue(new Error("workspace read must not run"));
    renderScoped({ projectId: PROJECT, moduleId: MODULE });
    const chart = await screen.findByTestId("mock-chart-area");
    expect(chart.dataset["points"]).toBe(
      JSON.stringify([{ label: "Oct 01, 2026", values: { resolved: 1, created: 2 } }])
    );
    expect(contractMocks.getProjectAnalyticsChart).toHaveBeenCalledWith(
      expect.anything(),
      "acme",
      PROJECT,
      { type: "work-items" },
      { projectIds: [PROJECT], moduleId: MODULE }
    );
    expect(contractMocks.getAnalyticsChart).not.toHaveBeenCalled();
  });

  it("keeps the dialog on its project read when the workspace read would answer", async () => {
    contractMocks.getProjectAnalyticsChart.mockRejectedValue(new Error("scoped read failed"));
    contractMocks.getAnalyticsChart.mockResolvedValue({
      data: [{ key: "2026-10-01", name: "2026-10-01", count: 5, created_issues: 5, completed_issues: 0 }],
      schema: {},
    });
    renderScoped({ projectId: PROJECT, moduleId: null });
    const guidance = await screen.findByTestId("analytics-section-guidance");
    expect(guidance).toHaveTextContent(GUIDANCE);
    expect(screen.queryByTestId("mock-chart-area")).not.toBeInTheDocument();
    expect(contractMocks.getAnalyticsChart).not.toHaveBeenCalled();
  });

  it("shows the guidance panel when the read fails", async () => {
    contractMocks.getAnalyticsChart.mockRejectedValue(new Error("series read failed"));
    renderSection();
    const guidance = await screen.findByTestId("analytics-section-guidance");
    expect(guidance).toHaveTextContent(GUIDANCE);
    expect(screen.queryByTestId("mock-chart-area")).not.toBeInTheDocument();
  });

  it("shows the guidance panel when there are no buckets", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: [], schema: {} });
    renderSection();
    const guidance = await screen.findByTestId("analytics-section-guidance");
    expect(guidance).toHaveTextContent(GUIDANCE);
    expect(screen.queryByTestId("mock-chart-area")).not.toBeInTheDocument();
  });
});
