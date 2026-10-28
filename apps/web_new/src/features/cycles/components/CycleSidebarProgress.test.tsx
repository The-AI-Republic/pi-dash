// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar progress: the count pairs, the burndown chart with its
// measure picker, the breakdown tabs, and entry clicks toggling the
// cycle's rich filter through the shared model (CYC-038, CYC-039).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CycleDistribution, CycleProgress, CycleRow } from "@pidash/api-client";
import type { ReactNode } from "react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("recharts", () => ({
  ResponsiveContainer: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  AreaChart: ({ children, data }: { children?: ReactNode; data?: Array<Record<string, unknown>> }) => (
    <div data-testid="mock-area-chart" data-rows={JSON.stringify(data)}>
      {children}
    </div>
  ),
  CartesianGrid: () => <div />,
  XAxis: () => <div />,
  YAxis: () => <div />,
  Area: () => <div />,
  Legend: () => <div />,
  Tooltip: () => <div />,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  CYCLES_CYCLE_ID,
  CYCLES_PROJECT_ID,
  jsonResponse,
  makeCycleDistribution,
  makeCycleProgress,
  makeCycleRow,
  makeCycleUserProperties,
} from "../testing.js";
import type { CycleBreakdownTab } from "../lib/cycleSidebar.js";
import { CycleSidebarProgress } from "./CycleSidebarProgress.js";

const WS = "acme";
const PREFS_PATH = `/api/workspaces/${WS}/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}/user-properties/`;

const ADA_ID = "aaaaaaaa-0000-4000-8000-000000000001";
const BUG_ID = "bbbbbbbb-0000-4000-8000-000000000002";

let serverRichFilters: Record<string, unknown>;
let patchCalls: Record<string, unknown>[];

function datedCycle(): CycleRow {
  return makeCycleRow({ start_date: "2026-09-01T12:00:00Z", end_date: "2026-12-31T12:00:00Z", status: "current" });
}

function pointsAnalytics(): CycleDistribution {
  return makeCycleDistribution({
    assignees: [
      {
        display_name: "Ada Lovelace",
        assignee_id: ADA_ID,
        total_issues: 0,
        total_estimates: 2,
      },
    ],
    labels: [],
    completion_chart: { "2026-10-01": 5 },
  });
}

beforeEach(() => {
  serverRichFilters = {};
  patchCalls = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith(PREFS_PATH) && (init.method ?? "GET") === "PATCH") {
      const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      patchCalls.push(body);
      serverRichFilters = (body["rich_filters"] ?? {}) as Record<string, unknown>;
      return jsonResponse(url, 201, makeCycleUserProperties({ rich_filters: serverRichFilters }));
    }
    if (url.endsWith(PREFS_PATH)) {
      return jsonResponse(url, 200, makeCycleUserProperties({ rich_filters: serverRichFilters }));
    }
    throw new Error(`unexpected request: ${init.method ?? "GET"} ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

interface ProgressHarness {
  cycle?: CycleRow;
  progress?: CycleProgress;
  issuesAnalytics?: CycleDistribution | null;
  pointsAnalytics?: CycleDistribution | null;
  estimatesEnabled?: boolean;
  estimateType?: string | null;
  tab?: CycleBreakdownTab;
}

function renderProgress(harness: ProgressHarness = {}): void {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  function Host(): React.ReactElement {
    const [tab, setTab] = React.useState<CycleBreakdownTab>(harness.tab ?? "States");
    return (
      <CycleSidebarProgress
        workspaceSlug={WS}
        projectId={CYCLES_PROJECT_ID}
        cycle={harness.cycle ?? datedCycle()}
        projectTimeZone="UTC"
        estimatesEnabled={harness.estimatesEnabled ?? false}
        estimateType={harness.estimateType ?? null}
        progress={harness.progress ?? makeCycleProgress()}
        issuesAnalytics={harness.issuesAnalytics === undefined ? makeCycleDistribution() : harness.issuesAnalytics}
        pointsAnalytics={harness.pointsAnalytics === undefined ? pointsAnalytics() : harness.pointsAnalytics}
        tab={tab}
        onTabChange={setTab}
      />
    );
  }
  render(
    <QueryClientProvider client={client}>
      <Host />
    </QueryClientProvider>
  );
}

function entryTestId(key: string): string {
  return `cycle-progress-entry-${key}`;
}

describe("CycleSidebarProgress", () => {
  it("renders counts, chart and state entries without estimates", async () => {
    renderProgress({ progress: makeCycleProgress({ total_estimate_points: 0 }) });
    expect(screen.getByTestId("cycle-progress-work-items").textContent).toBe("1/3");
    expect(screen.queryByTestId("cycle-progress-points")).toBeNull();
    expect(screen.queryByTestId("cycle-progress-measure")).toBeNull();
    const chart = screen.getByTestId("cycle-progress-chart");
    expect((await within(chart).findByTestId("mock-area-chart")).dataset["rows"]).toBe(
      JSON.stringify([
        { label: "Oct 1, 2026", remaining: 3 },
        { label: "Oct 2, 2026", remaining: 2 },
        { label: "Oct 3, 2026", remaining: 1 },
      ])
    );
    const backlog = await screen.findByTestId(entryTestId("backlog"));
    expect(backlog.tagName).toBe("BUTTON");
    expect(backlog.textContent).toContain("Backlog");
    expect(backlog.textContent).toContain("33%");
    expect(backlog.textContent).toContain("of 3");
    expect(screen.getByTestId(entryTestId("completed")).textContent).toContain("33%");
  });

  it("renders the zero-total string when the cycle holds nothing", () => {
    renderProgress({ progress: makeCycleProgress({ total_issues: 0, completed_issues: 0 }) });
    expect(screen.getByTestId("cycle-progress-work-items").textContent).toBe("0 Work item");
  });

  it("renders empty groups inert with an empty bar", async () => {
    renderProgress();
    const started = await screen.findByTestId(entryTestId("started"));
    expect(started.tagName).toBe("DIV");
    expect(started.textContent).toContain("Started");
    expect(started.textContent).toContain("0%");
  });

  it("renders the Points pair for points systems on open cycles", () => {
    renderProgress({ estimatesEnabled: true, estimateType: "points" });
    expect(screen.getByTestId("cycle-progress-points").textContent).toBe("0/2");
  });

  it("hides Points without a points system", () => {
    renderProgress({ estimatesEnabled: true, estimateType: "time" });
    expect(screen.queryByTestId("cycle-progress-points")).toBeNull();
  });

  it("hides Points on completed cycles unless the snapshot carries estimates", () => {
    const completed = { ...datedCycle(), status: "completed" };
    renderProgress({ cycle: completed, estimatesEnabled: true, estimateType: "points" });
    expect(screen.queryByTestId("cycle-progress-points")).toBeNull();
    // Covered with a snapshot below; here the plain completed cycle hides it.
  });

  it("shows Points on completed cycles with a snapshot distribution", () => {
    const completed = { ...datedCycle(), status: "completed", progress_snapshot: { estimate_distribution: {} } };
    renderProgress({ cycle: completed, estimatesEnabled: true, estimateType: "points" });
    expect(screen.getByTestId("cycle-progress-points").textContent).toBe("0/2");
  });

  it("switches the measure and recounts chart and breakdown", async () => {
    renderProgress({ estimatesEnabled: true, estimateType: "points" });
    expect(screen.getByTestId("cycle-progress-measure")).toHaveTextContent("Work items");
    await userEvent.click(screen.getByTestId("cycle-progress-measure"));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(["Work items", "Estimates"]);
    await userEvent.click(screen.getByRole("option", { name: "Estimates" }));
    expect(screen.getByTestId("cycle-progress-measure")).toHaveTextContent("Estimates");
    const backlog = await screen.findByTestId(entryTestId("backlog"));
    expect(backlog.textContent).toContain("100%");
    expect(backlog.textContent).toContain("of 2");
    expect(
      (await within(screen.getByTestId("cycle-progress-chart")).findByTestId("mock-area-chart")).dataset["rows"]
    ).toBe(JSON.stringify([{ label: "Oct 1, 2026", remaining: 5 }]));
    await userEvent.click(screen.getByTestId("cycle-progress-measure"));
    await userEvent.click(screen.getByRole("option", { name: "Work items" }));
    expect(screen.getByTestId("cycle-progress-measure")).toHaveTextContent("Work items");
    expect((await screen.findByTestId(entryTestId("backlog"))).textContent).toContain("33%");
  });

  it("hides the measure entirely for categories systems", () => {
    renderProgress({ estimatesEnabled: true, estimateType: "categories" });
    expect(screen.queryByTestId("cycle-progress-measure")).toBeNull();
    expect(screen.queryByTestId("cycle-progress-points")).toBeNull();
    expect(screen.getByTestId("cycle-progress-work-items").textContent).toBe("1/3");
  });

  it("offers the measure for an enabled time system", () => {
    renderProgress({ estimatesEnabled: true, estimateType: "time" });
    expect(screen.getByTestId("cycle-progress-measure")).toHaveTextContent("Work items");
  });

  it("snaps back to counts when estimates are turned off with point data present", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    function Host({ estimatesEnabled }: { estimatesEnabled: boolean }): React.ReactElement {
      return (
        <CycleSidebarProgress
          workspaceSlug={WS}
          projectId={CYCLES_PROJECT_ID}
          cycle={datedCycle()}
          projectTimeZone="UTC"
          estimatesEnabled={estimatesEnabled}
          estimateType="points"
          progress={makeCycleProgress()}
          issuesAnalytics={makeCycleDistribution()}
          pointsAnalytics={pointsAnalytics()}
          tab="States"
          onTabChange={() => {}}
        />
      );
    }
    const view = render(
      <QueryClientProvider client={client}>
        <Host estimatesEnabled />
      </QueryClientProvider>
    );
    await userEvent.click(screen.getByTestId("cycle-progress-measure"));
    await userEvent.click(screen.getByRole("option", { name: "Estimates" }));
    expect(screen.getByTestId("cycle-progress-measure")).toHaveTextContent("Estimates");
    view.rerender(
      <QueryClientProvider client={client}>
        <Host estimatesEnabled={false} />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("cycle-progress-measure")).toHaveTextContent("Work items");
    expect((await screen.findByTestId(entryTestId("backlog"))).textContent).toContain("33%");
  });

  it("keeps counts when the measure is unavailable after a prior points choice", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    function Host({ estimateType }: { estimateType: string }): React.ReactElement {
      return (
        <CycleSidebarProgress
          workspaceSlug={WS}
          projectId={CYCLES_PROJECT_ID}
          cycle={datedCycle()}
          projectTimeZone="UTC"
          estimatesEnabled
          estimateType={estimateType}
          progress={makeCycleProgress()}
          issuesAnalytics={makeCycleDistribution()}
          pointsAnalytics={pointsAnalytics()}
          tab="States"
          onTabChange={() => {}}
        />
      );
    }
    const view = render(
      <QueryClientProvider client={client}>
        <Host estimateType="points" />
      </QueryClientProvider>
    );
    await userEvent.click(screen.getByTestId("cycle-progress-measure"));
    await userEvent.click(screen.getByRole("option", { name: "Estimates" }));
    expect(screen.getByTestId("cycle-progress-measure")).toHaveTextContent("Estimates");
    view.rerender(
      <QueryClientProvider client={client}>
        <Host estimateType="categories" />
      </QueryClientProvider>
    );
    expect(screen.queryByTestId("cycle-progress-measure")).toBeNull();
    expect((await screen.findByTestId(entryTestId("backlog"))).textContent).toContain("33%");
  });

  it("shows the empty notice for undated cycles with breakdowns intact", async () => {
    renderProgress({ cycle: makeCycleRow({ start_date: null, end_date: null }) });
    expect(screen.queryByTestId("cycle-progress-chart")).toBeNull();
    expect(screen.getByTestId("cycle-progress-empty").textContent).toBe("No Data yet");
    expect((await screen.findByTestId(entryTestId("backlog"))).textContent).toContain("Backlog");
  });

  it("keeps the empty notice when the analytics read fails on an undated cycle", async () => {
    renderProgress({ cycle: makeCycleRow({ start_date: null, end_date: null }), issuesAnalytics: null });
    expect(screen.queryByTestId("cycle-progress-chart")).toBeNull();
    expect(screen.getByTestId("cycle-progress-empty").textContent).toBe("No Data yet");
    expect((await screen.findByTestId(entryTestId("backlog"))).textContent).toContain("Backlog");
  });

  it("leaves distribution tabs empty rather than spinning when the read fails", () => {
    renderProgress({
      cycle: makeCycleRow({ start_date: null, end_date: null }),
      issuesAnalytics: null,
      tab: "Assignees",
    });
    expect(screen.queryByTestId("cycle-progress-entries-pending")).toBeNull();
    expect(screen.getByTestId("cycle-progress-entries").childElementCount).toBe(0);
  });

  it("keeps the pending marker for a pending points read", async () => {
    renderProgress({
      tab: "Assignees",
      estimatesEnabled: true,
      estimateType: "points",
      pointsAnalytics: null,
    });
    await userEvent.click(screen.getByTestId("cycle-progress-measure"));
    await userEvent.click(screen.getByRole("option", { name: "Estimates" }));
    expect(screen.getByTestId("cycle-progress-entries-pending")).not.toBeNull();
  });

  it("hides the chart silently for future-start cycles", async () => {
    const today = new Date();
    const shift = (delta: number): string => {
      const at = new Date(today);
      at.setUTCDate(at.getUTCDate() + delta);
      return `${at.toISOString().slice(0, 10)}T12:00:00Z`;
    };
    renderProgress({ cycle: makeCycleRow({ start_date: shift(10), end_date: shift(40) }) });
    expect(screen.queryByTestId("cycle-progress-chart")).toBeNull();
    expect(screen.queryByTestId("cycle-progress-empty")).toBeNull();
    expect((await screen.findByTestId(entryTestId("backlog"))).textContent).toContain("Backlog");
  });

  it("hides the chart silently for one-sided cycles", () => {
    renderProgress({ cycle: makeCycleRow({ start_date: "2026-09-01T12:00:00Z", end_date: null }) });
    expect(screen.queryByTestId("cycle-progress-chart")).toBeNull();
    expect(screen.queryByTestId("cycle-progress-empty")).toBeNull();
  });

  it("renders an empty series as an empty chart", async () => {
    renderProgress({ issuesAnalytics: makeCycleDistribution({ completion_chart: {} }) });
    expect(
      (await within(screen.getByTestId("cycle-progress-chart")).findByTestId("mock-area-chart")).dataset["rows"]
    ).toBe(JSON.stringify([]));
  });

  it("narrows, combines and lifts the state filter", async () => {
    renderProgress();
    const backlog = await screen.findByTestId(entryTestId("backlog"));
    await userEvent.click(backlog);
    await waitFor(() => expect(patchCalls).toHaveLength(1));
    expect(patchCalls[0]).toEqual({ rich_filters: { state_group__in: "backlog" } });
    await waitFor(() => expect(screen.getByTestId(entryTestId("backlog")).getAttribute("aria-pressed")).toBe("true"));
    await userEvent.click(screen.getByTestId(entryTestId("completed")));
    await waitFor(() => expect(patchCalls).toHaveLength(2));
    expect(patchCalls[1]).toEqual({ rich_filters: { state_group__in: "backlog,completed" } });
    await userEvent.click(screen.getByTestId(entryTestId("backlog")));
    await waitFor(() => expect(patchCalls).toHaveLength(3));
    expect(patchCalls[2]).toEqual({ rich_filters: { state_group__in: "completed" } });
    await waitFor(() => expect(screen.getByTestId(entryTestId("backlog")).getAttribute("aria-pressed")).toBe("false"));
  });

  it("marks seeded entries pressed on mount", async () => {
    serverRichFilters = { state_group__in: "backlog,completed" };
    renderProgress();
    await waitFor(() => expect(screen.getByTestId(entryTestId("backlog")).getAttribute("aria-pressed")).toBe("true"));
    expect(screen.getByTestId(entryTestId("completed")).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByTestId(entryTestId("unstarted")).getAttribute("aria-pressed")).toBe("false");
  });

  it("filters by assignee and label entries", async () => {
    renderProgress({ tab: "Assignees" });
    const ada = await screen.findByTestId(entryTestId(ADA_ID));
    expect(ada.tagName).toBe("BUTTON");
    expect(ada.textContent).toContain("Ada Lovelace");
    expect(ada.textContent).toContain("67%");
    await userEvent.click(ada);
    await waitFor(() => expect(patchCalls).toHaveLength(1));
    expect(patchCalls[0]).toEqual({ rich_filters: { assignee_id__in: ADA_ID } });
    await userEvent.click(screen.getByTestId("cycle-progress-tab-Labels"));
    const bug = await screen.findByTestId(entryTestId(BUG_ID));
    expect(bug.textContent).toContain("Bug");
    await userEvent.click(bug);
    await waitFor(() => expect(patchCalls).toHaveLength(2));
  });

  it("renders entries without an id inert", async () => {
    const issues = makeCycleDistribution({
      assignees: [{ display_name: null, assignee_id: null, total_issues: 1 }],
      labels: [],
    });
    renderProgress({ tab: "Assignees", issuesAnalytics: issues });
    const entry = await screen.findByTestId(entryTestId("entry-0"));
    expect(entry.tagName).toBe("DIV");
    expect(entry.textContent).toContain("Unassigned");
  });

  it("holds entries and chart while the points read is pending", async () => {
    renderProgress({ tab: "Assignees", estimatesEnabled: true, estimateType: "points", pointsAnalytics: null });
    expect(screen.getByTestId("cycle-progress-chart")).not.toBeNull();
    await userEvent.click(screen.getByTestId("cycle-progress-measure"));
    await userEvent.click(screen.getByRole("option", { name: "Estimates" }));
    expect(await screen.findByTestId("cycle-progress-entries-pending")).not.toBeNull();
    expect(screen.queryByTestId("cycle-progress-chart")).toBeNull();
  });

  it("switches tabs with selected state", async () => {
    renderProgress({ tab: "Assignees" });
    expect(screen.getByTestId("cycle-progress-tab-Assignees").getAttribute("aria-selected")).toBe("true");
    expect(screen.getByTestId("cycle-progress-tab-States").getAttribute("aria-selected")).toBe("false");
    await screen.findByTestId(entryTestId(ADA_ID));
    await userEvent.click(screen.getByTestId("cycle-progress-tab-States"));
    expect(screen.getByTestId("cycle-progress-tab-States").getAttribute("aria-selected")).toBe("true");
    expect((await screen.findByTestId(entryTestId("backlog"))).textContent).toContain("Backlog");
  });
});
