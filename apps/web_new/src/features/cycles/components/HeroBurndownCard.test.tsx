// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero burn-down card (parity/specs/cycles.md §15, CYC-041): the
// pending line, the chart per measure with its placeholder, the
// estimates switch, and the empty view.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const reads = vi.hoisted(() => ({ progress: vi.fn(), analytics: vi.fn() }));
vi.mock("../api/heroQueries.js", () => ({
  heroProgressQueryOptions: (_client: unknown, workspaceSlug: string, projectId: string, cycleId: string) => ({
    queryKey: ["hero-progress", workspaceSlug, projectId, cycleId],
    queryFn: reads.progress,
  }),
  heroAnalyticsQueryOptions: (
    _client: unknown,
    workspaceSlug: string,
    projectId: string,
    cycleId: string,
    type: string
  ) => ({
    queryKey: ["hero-analytics", workspaceSlug, projectId, cycleId, type],
    queryFn: reads.analytics,
  }),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { HeroMeasure } from "../lib/cycleHero.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import { HERO_BURNDOWN_EMPTY, HERO_BURNDOWN_NO_CHART, HeroBurndownCard } from "./HeroBurndownCard.js";

function progress(): Record<string, number> {
  return {
    total_issues: 13,
    backlog_issues: 4,
    unstarted_issues: 0,
    started_issues: 8,
    completed_issues: 1,
    cancelled_issues: 0,
  };
}

function analytics(): Record<string, unknown> {
  return {
    completion_chart: { "2026-10-01": 12, "2026-10-02": 10 },
    assignees: [],
    labels: [],
  };
}

function renderCard(
  props: {
    measure?: HeroMeasure;
    estimatesEnabled?: boolean;
    startDate?: string | null;
    endDate?: string | null;
    onMeasureChange?: (measure: HeroMeasure) => void;
  } = {}
): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <HeroBurndownCard
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        cycleId={CYCLES_CYCLE_ID}
        startDate={props.startDate ?? "2026-10-01T00:00:00Z"}
        endDate={props.endDate ?? "2026-10-31T00:00:00Z"}
        measure={props.measure ?? "issues"}
        estimatesEnabled={props.estimatesEnabled ?? true}
        onMeasureChange={props.onMeasureChange ?? (() => undefined)}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", (() => new Promise<never>(() => undefined)) as never);
  reads.progress.mockResolvedValue(progress());
  reads.analytics.mockResolvedValue(analytics());
});

afterEach(() => {
  resetAppClient();
});

describe("HeroBurndownCard", () => {
  it("plots the pending line and the chart with the measure switch", async () => {
    const onMeasureChange = vi.fn();
    renderCard({ onMeasureChange });
    expect(await screen.findByTestId("hero-burndown-pending")).toHaveTextContent("Pending work items - 12");
    expect(await screen.findByTestId("chart-area")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Items" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Estimates" })).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(screen.getByRole("button", { name: "Estimates" }));
    expect(onMeasureChange).toHaveBeenCalledWith("points");
  });

  it("hides the switch where estimates are disabled", async () => {
    renderCard({ estimatesEnabled: false });
    expect(await screen.findByTestId("hero-burndown-pending")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Items" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Estimates" })).not.toBeInTheDocument();
  });

  it("holds the chart placeholder for undated and future-start cycles", async () => {
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <HeroBurndownCard
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          cycleId={CYCLES_CYCLE_ID}
          startDate={null}
          endDate={null}
          measure="issues"
          estimatesEnabled={true}
          onMeasureChange={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("hero-burndown-placeholder")).toHaveTextContent(HERO_BURNDOWN_NO_CHART);
    expect(screen.queryByTestId("chart-area")).not.toBeInTheDocument();
    unmount();

    renderCard({ startDate: "2996-10-01T00:00:00Z", endDate: "2996-10-31T00:00:00Z" });
    expect(await screen.findByTestId("hero-burndown-placeholder")).toBeInTheDocument();
  });

  it("holds the loading line while either read flies", () => {
    reads.analytics.mockReturnValue(new Promise(() => undefined));
    renderCard();
    expect(screen.getByTestId("hero-burndown-loading")).toBeInTheDocument();
  });

  it("shows the empty view without work, on failure, or on a missing answer", async () => {
    reads.progress.mockResolvedValue({ ...progress(), total_issues: 0 });
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <HeroBurndownCard
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          cycleId={CYCLES_CYCLE_ID}
          startDate="2026-10-01T00:00:00Z"
          endDate="2026-10-31T00:00:00Z"
          measure="issues"
          estimatesEnabled={true}
          onMeasureChange={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByText(HERO_BURNDOWN_EMPTY)).toBeInTheDocument();
    unmount();

    reads.progress.mockRejectedValue(new Error("boom"));
    const failed = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <HeroBurndownCard
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          cycleId={CYCLES_CYCLE_ID}
          startDate="2026-10-01T00:00:00Z"
          endDate="2026-10-31T00:00:00Z"
          measure="issues"
          estimatesEnabled={true}
          onMeasureChange={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByText(HERO_BURNDOWN_EMPTY)).toBeInTheDocument();
    failed.unmount();

    reads.progress.mockResolvedValue(undefined);
    reads.analytics.mockResolvedValue({ completion_chart: undefined, assignees: [], labels: [] });
    renderCard();
    expect(await screen.findByText(HERO_BURNDOWN_EMPTY)).toBeInTheDocument();
  });

  it("shows the empty view when the analytics read fails", async () => {
    reads.analytics.mockRejectedValue(new Error("boom"));
    renderCard();
    expect(await screen.findByText(HERO_BURNDOWN_EMPTY)).toBeInTheDocument();
  });
});
