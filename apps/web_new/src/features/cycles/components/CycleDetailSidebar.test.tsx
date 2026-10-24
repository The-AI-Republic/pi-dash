// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail sidebar shell: the skeleton until the sidebar reads land, the
// name with its lifecycle marker and close control, the read-only
// description, the lead and member rows, honest read failures, and the
// persisted breakdown tab (CYC-035, CYC-037, CYC-040).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CycleRow } from "@pidash/api-client";
import type { ReactNode } from "react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("recharts", () => ({
  ResponsiveContainer: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  AreaChart: ({ children }: { children?: ReactNode }) => <div data-testid="mock-area-chart">{children}</div>,
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
  makeEstimate,
  makeEstimateList,
  makeWorkspaceMembers,
} from "../testing.js";
import { CYCLE_SIDEBAR_CLOSE_LABEL, CycleDetailSidebar } from "./CycleDetailSidebar.js";

const WS = "acme";
const BASE = `/api/workspaces/${WS}/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}`;
const PROGRESS_PATH = `${BASE}/progress/`;
const ANALYTICS_PATH = `${BASE}/analytics/`;
const ESTIMATES_PATH = `/api/workspaces/${WS}/projects/${CYCLES_PROJECT_ID}/estimates/`;
const MEMBERS_PATH = `/api/workspaces/${WS}/members/`;
const PREFS_PATH = `${BASE}/user-properties/`;

const ADA_ID = "aaaaaaaa-0000-4000-8000-000000000001";
const GRACE_ID = "aaaaaaaa-0000-4000-8000-000000000003";
const UNKNOWN_ID = "ffffffff-0000-4000-8000-000000000009";

interface Harness {
  progressStatus: number;
  progressDelayMs: number;
  issuesStatus: number;
  pointsStatus: number;
  estimates: unknown[];
  estimatesStatus: number;
}

let harness: Harness;

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

function fullCycle(): CycleRow {
  return makeCycleRow({
    name: "Cycle 7",
    description: "First paragraph.\n\nSecond paragraph.",
    owned_by_id: ADA_ID,
    assignee_ids: [ADA_ID, GRACE_ID],
    start_date: "2026-09-01T12:00:00Z",
    end_date: "2026-12-31T12:00:00Z",
    status: "current",
  });
}

beforeEach(() => {
  window.localStorage.clear();
  harness = {
    progressStatus: 200,
    progressDelayMs: 0,
    issuesStatus: 200,
    pointsStatus: 200,
    estimates: makeEstimateList([makeEstimate()]),
    estimatesStatus: 200,
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith(PROGRESS_PATH)) {
      if (harness.progressDelayMs > 0) await sleep(harness.progressDelayMs);
      if (harness.progressStatus !== 200) return jsonResponse(url, harness.progressStatus, { detail: "boom" });
      return jsonResponse(url, 200, makeCycleProgress());
    }
    if (url.includes(ANALYTICS_PATH)) {
      const status = url.includes("type=points") ? harness.pointsStatus : harness.issuesStatus;
      if (status !== 200) return jsonResponse(url, status, { detail: "boom" });
      return jsonResponse(url, 200, makeCycleDistribution());
    }
    if (url.endsWith(ESTIMATES_PATH)) {
      if (harness.estimatesStatus !== 200) return jsonResponse(url, harness.estimatesStatus, { detail: "boom" });
      return jsonResponse(url, 200, harness.estimates);
    }
    if (url.endsWith(MEMBERS_PATH)) {
      return jsonResponse(url, 200, makeWorkspaceMembers());
    }
    if (url.endsWith(PREFS_PATH)) {
      if ((init.method ?? "GET") === "PATCH") {
        const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
        return jsonResponse(
          url,
          201,
          makeCycleUserProperties({ rich_filters: (body["rich_filters"] ?? {}) as Record<string, unknown> })
        );
      }
      return jsonResponse(url, 200, makeCycleUserProperties());
    }
    throw new Error(`unexpected request: ${init.method ?? "GET"} ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

function renderSidebar(
  cycle: CycleRow = fullCycle(),
  overrides: { estimatesEnabled?: boolean; editable?: boolean; onClose?: () => void } = {}
): void {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <CycleDetailSidebar
        workspaceSlug={WS}
        projectId={CYCLES_PROJECT_ID}
        cycle={cycle}
        projectTimeZone="UTC"
        estimatesEnabled={overrides.estimatesEnabled ?? false}
        editable={overrides.editable ?? true}
        onClose={overrides.onClose ?? (() => undefined)}
      />
    </QueryClientProvider>
  );
}

describe("CycleDetailSidebar", () => {
  it("skeletons until the sidebar reads arrive", async () => {
    harness.progressDelayMs = 60;
    renderSidebar();
    expect(screen.getByTestId("cycle-sidebar-skeleton")).not.toBeNull();
    expect(screen.queryByTestId("cycle-sidebar-name")).toBeNull();
    expect(await screen.findByTestId("cycle-sidebar-name")).toHaveTextContent("Cycle 7");
    expect(screen.queryByTestId("cycle-sidebar-skeleton")).toBeNull();
  });

  it("renders the name with its lifecycle marker and close control", async () => {
    const onClose = vi.fn();
    renderSidebar(fullCycle(), { onClose });
    expect((await screen.findByTestId("cycle-sidebar-name")).tagName).toBe("H2");
    expect(screen.getByTestId("cycle-sidebar").textContent).toContain("In progress");
    await userEvent.click(screen.getByRole("button", { name: CYCLE_SIDEBAR_CLOSE_LABEL }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("renders the description with paragraph breaks preserved", async () => {
    renderSidebar();
    const description = await screen.findByTestId("cycle-sidebar-description");
    expect(description.textContent).toBe("First paragraph.\n\nSecond paragraph.");
    expect(description.className).toContain("whitespace-pre-wrap");
  });

  it("renders nothing for an empty description", async () => {
    renderSidebar({ ...fullCycle(), description: "  " });
    expect(await screen.findByTestId("cycle-sidebar-name")).not.toBeNull();
    expect(screen.queryByTestId("cycle-sidebar-description")).toBeNull();
  });

  it("renders nothing for a missing description", async () => {
    const cycle = { ...fullCycle() };
    delete (cycle as Partial<CycleRow>).description;
    renderSidebar(cycle);
    expect(await screen.findByTestId("cycle-sidebar-name")).not.toBeNull();
    expect(screen.queryByTestId("cycle-sidebar-description")).toBeNull();
  });

  it("renders the lead with a fallback initial", async () => {
    renderSidebar();
    const lead = await screen.findByTestId("cycle-sidebar-lead");
    expect(within(lead).getByTestId("cycle-sidebar-lead-name")).toHaveTextContent("Ada Lovelace");
    expect(within(lead).getByRole("img").textContent).toBe("A");
  });

  it("renders the lead image when the directory carries one", async () => {
    renderSidebar({ ...fullCycle(), owned_by_id: GRACE_ID });
    const lead = await screen.findByTestId("cycle-sidebar-lead");
    expect(within(lead).getByRole("img").querySelector("img")?.getAttribute("src")).toBe("https://cdn.test/grace.png");
  });

  it("renders an empty avatar with no name without a lead", async () => {
    renderSidebar({ ...fullCycle(), owned_by_id: null });
    const lead = await screen.findByTestId("cycle-sidebar-lead");
    expect(within(lead).getByRole("img").textContent).toBe("");
    expect(within(lead).queryByTestId("cycle-sidebar-lead-name")).toBeNull();
  });

  it("renders an empty avatar with no name when the lead is unset", async () => {
    const cycle = { ...fullCycle() };
    delete (cycle as Partial<CycleRow>).owned_by_id;
    renderSidebar(cycle);
    const lead = await screen.findByTestId("cycle-sidebar-lead");
    expect(screen.queryByTestId("cycle-sidebar-lead-name")).toBeNull();
    expect(lead).not.toBeNull();
  });

  it("falls back for a lead the directory does not know", async () => {
    renderSidebar({ ...fullCycle(), owned_by_id: UNKNOWN_ID });
    expect(await screen.findByTestId("cycle-sidebar-lead-name")).toHaveTextContent("Unknown member");
  });

  it("renders the member avatars and the no-assignee text", async () => {
    renderSidebar();
    const members = await screen.findByTestId("cycle-sidebar-members");
    expect(within(members).getAllByRole("img")).toHaveLength(2);
  });

  it("renders the no-assignee text without assignees", async () => {
    renderSidebar({ ...fullCycle(), assignee_ids: [] });
    expect(await screen.findByTestId("cycle-sidebar-members")).toHaveTextContent("No assignee");
  });

  it("falls back for a member the directory does not know", async () => {
    renderSidebar({ ...fullCycle(), assignee_ids: [ADA_ID, UNKNOWN_ID] });
    const members = await screen.findByTestId("cycle-sidebar-members");
    expect(within(members).getAllByRole("img")).toHaveLength(2);
    expect(within(members).getByRole("img", { name: "Unknown member" })).not.toBeNull();
  });

  it("renders nothing for members when the ids are missing", async () => {
    const cycle = { ...fullCycle() };
    delete (cycle as Partial<CycleRow>).assignee_ids;
    renderSidebar(cycle);
    expect(await screen.findByTestId("cycle-sidebar-members")).toHaveTextContent("No assignee");
  });

  it("renders the progress section with counts and chart", async () => {
    renderSidebar();
    expect(await screen.findByTestId("cycle-progress-work-items")).toHaveTextContent("1/3");
    expect(screen.getByTestId("cycle-progress-chart")).not.toBeNull();
    expect(screen.queryByTestId("cycle-progress-measure")).toBeNull();
  });

  it("renders the measure and Points where estimates are enabled", async () => {
    renderSidebar(fullCycle(), { estimatesEnabled: true });
    expect(await screen.findByTestId("cycle-progress-measure")).toHaveTextContent("Work items");
    expect(screen.getByTestId("cycle-progress-points")).toHaveTextContent("0/2");
  });

  it("hides Points when the estimates read fails but keeps the measure", async () => {
    harness.estimatesStatus = 500;
    renderSidebar(fullCycle(), { estimatesEnabled: true });
    expect(await screen.findByTestId("cycle-progress-measure")).not.toBeNull();
    expect(screen.queryByTestId("cycle-progress-points")).toBeNull();
  });

  it("omits the progress section when the snapshot read fails", async () => {
    harness.progressStatus = 500;
    renderSidebar();
    expect(await screen.findByTestId("cycle-sidebar-name")).not.toBeNull();
    await waitFor(() => expect(screen.queryByTestId("cycle-sidebar-skeleton")).toBeNull());
    expect(screen.queryByTestId("cycle-progress-work-items")).toBeNull();
    expect(screen.getByTestId("cycle-sidebar-dates-trigger")).not.toBeNull();
  });

  it("omits the progress section when the breakdown read fails", async () => {
    harness.issuesStatus = 500;
    renderSidebar();
    expect(await screen.findByTestId("cycle-sidebar-name")).not.toBeNull();
    await waitFor(() => expect(screen.queryByTestId("cycle-sidebar-skeleton")).toBeNull());
    expect(screen.queryByTestId("cycle-progress-work-items")).toBeNull();
  });

  it("remembers the breakdown tab per cycle", async () => {
    renderSidebar();
    expect(await screen.findByTestId("cycle-progress-tab-Assignees")).toHaveAttribute("aria-selected", "true");
    await userEvent.click(screen.getByTestId("cycle-progress-tab-States"));
    expect(screen.getByTestId("cycle-progress-tab-States")).toHaveAttribute("aria-selected", "true");
    renderSidebar();
    const tabs = await screen.findAllByTestId("cycle-progress-tab-States");
    expect(tabs.at(-1)?.getAttribute("aria-selected")).toBe("true");
  });

  it("disables dates for read-only viewers", async () => {
    renderSidebar(fullCycle(), { editable: false });
    expect(await screen.findByTestId("cycle-sidebar-dates-trigger")).toHaveAttribute("disabled");
  });
});
