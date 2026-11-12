// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics custom-insights section tests (parity/specs/analytics.md §10, ANL-010, ANL-011).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsChart: vi.fn(), getProjectAnalyticsChart: vi.fn(), listWorkspaceStates: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/charts/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/charts/index.js")>();
  return {
    ...original,
    BarChart: (props: { bars?: unknown; segments?: unknown; stacked?: boolean; fills?: unknown }) => (
      <div
        data-testid="mock-chart-bar"
        data-bars={JSON.stringify(props.bars)}
        data-segments={JSON.stringify(props.segments)}
        data-stacked={String(props.stacked)}
        data-fills={props.fills === undefined ? undefined : JSON.stringify(props.fills)}
      />
    ),
  };
});

import { CustomInsightsSection } from "./CustomInsightsSection.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

const GUIDANCE =
  "Create projects with work items and assignees to start tracking performance, progress, and team impact here.";

const PRIORITY_ROWS = [
  { key: "urgent", name: "urgent", count: 1 },
  { key: "high", name: "high", count: 2 },
];

function renderSection(projectIds: readonly string[] = []): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CustomInsightsSection workspaceSlug="acme" projectIds={projectIds} />
    </QueryClientProvider>
  );
}

function renderScoped(dialogScope: { projectId: string; cycleId?: string | null }): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CustomInsightsSection workspaceSlug="acme" projectIds={[]} dialogScope={dialogScope} />
    </QueryClientProvider>
  );
}

async function selectOption(user: ReturnType<typeof userEvent.setup>, trigger: number, name: string): Promise<void> {
  await user.click(screen.getByTestId(`analytics-custom-trigger-${trigger}`));
  await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
  await user.click(within(screen.getByRole("listbox")).getByRole("option", { name }));
  await waitFor(() => expect(screen.queryByRole("listbox")).not.toBeInTheDocument());
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("CustomInsightsSection", () => {
  it("covers the fetch with a skeleton behind the pickers", () => {
    contractMocks.getAnalyticsChart.mockReturnValue(new Promise<never>(() => {}));
    renderSection();
    expect(screen.getByRole("heading", { name: "Custom Insights" })).toBeInTheDocument();
    expect(screen.getByTestId("analytics-custom-trigger-1")).toHaveTextContent("Priority");
    expect(screen.getByTestId("analytics-custom-skeleton")).toBeInTheDocument();
    const table = screen.getByTestId("analytics-custom-table");
    expect(table.querySelectorAll("tbody tr")).toHaveLength(5);
    expect(table.querySelectorAll("tbody td > span[style*='height'][style*='20px']")).toHaveLength(10);
    expect(within(table).queryByRole("button", { name: "Export as csv" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("mock-chart-bar")).not.toBeInTheDocument();
    expect(screen.queryByTestId("analytics-section-guidance")).not.toBeInTheDocument();
  });

  it("plots single-series priority bars without reading workspace states", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: PRIORITY_ROWS, schema: {} });
    renderSection(["a"]);
    const chart = await screen.findByTestId("mock-chart-bar");
    expect(chart.dataset["stacked"]).toBe("false");
    expect(chart.dataset["segments"]).toBe(JSON.stringify([{ name: "Count", color: "#6172e8" }]));
    expect(chart.dataset["bars"]).toBe(
      JSON.stringify([
        { label: "Urgent", values: [1] },
        { label: "High", values: [2] },
      ])
    );
    expect(chart.dataset["fills"]).toBe(JSON.stringify(["#ef4444", "#f97316"]));
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(expect.anything(), "acme", {
      type: "custom-work-items",
      xAxis: "PRIORITY",
      yAxis: "WORK_ITEM_COUNT",
      projectIds: ["a"],
    });
    expect(contractMocks.listWorkspaceStates).not.toHaveBeenCalled();
  });

  it("plots the project-scoped bars in the dialog and never the workspace read", async () => {
    contractMocks.getProjectAnalyticsChart.mockResolvedValue({ data: PRIORITY_ROWS, schema: {} });
    contractMocks.getAnalyticsChart.mockRejectedValue(new Error("workspace read must not run"));
    renderScoped({ projectId: PROJECT, cycleId: CYCLE });
    const chart = await screen.findByTestId("mock-chart-bar");
    expect(chart.dataset["bars"]).toBe(
      JSON.stringify([
        { label: "Urgent", values: [1] },
        { label: "High", values: [2] },
      ])
    );
    expect(contractMocks.getProjectAnalyticsChart).toHaveBeenCalledWith(
      expect.anything(),
      "acme",
      PROJECT,
      { type: "custom-work-items", xAxis: "PRIORITY", yAxis: "WORK_ITEM_COUNT" },
      { projectIds: [PROJECT], cycleId: CYCLE }
    );
    expect(contractMocks.getAnalyticsChart).not.toHaveBeenCalled();
    expect(contractMocks.listWorkspaceStates).not.toHaveBeenCalled();
    const table = screen.getByTestId("analytics-custom-table");
    expect(within(table).getByRole("button", { name: "Export as csv" })).toBeInTheDocument();
  });

  it("shows the dialog guidance when its own read fails, without falling back to the workspace", async () => {
    contractMocks.getProjectAnalyticsChart.mockRejectedValue(new Error("scoped read failed"));
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: PRIORITY_ROWS, schema: {} });
    renderScoped({ projectId: PROJECT, cycleId: null });
    const guidance = await screen.findByTestId("analytics-section-guidance");
    expect(guidance).toHaveTextContent(GUIDANCE);
    expect(screen.queryByTestId("mock-chart-bar")).not.toBeInTheDocument();
    expect(contractMocks.getAnalyticsChart).not.toHaveBeenCalled();
  });

  it("re-queries stacked once a split is chosen, then ungrouped once cleared", async () => {
    const user = userEvent.setup();
    contractMocks.getAnalyticsChart.mockImplementation(
      async (_client: unknown, _slug: string, query: Record<string, unknown>) => {
        if (query["groupBy"] === "STATES") {
          return {
            data: [{ key: "urgent", name: "urgent", count: 2, s1: 1, s2: 1 }],
            schema: { s1: "Backlog", s2: "Done" },
          };
        }
        return { data: PRIORITY_ROWS, schema: {} };
      }
    );
    renderSection();
    await screen.findByTestId("mock-chart-bar");

    await selectOption(user, 2, "State name");
    await waitFor(() =>
      expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(expect.anything(), "acme", {
        type: "custom-work-items",
        xAxis: "PRIORITY",
        yAxis: "WORK_ITEM_COUNT",
        groupBy: "STATES",
        projectIds: [],
      })
    );
    await waitFor(() => expect(screen.getByTestId("mock-chart-bar").dataset["stacked"]).toBe("true"));
    const stacked = screen.getByTestId("mock-chart-bar");
    expect(stacked.dataset["segments"]).toBe(
      JSON.stringify([
        { name: "Backlog", color: "#6172e8" },
        { name: "Done", color: "#8b6edb" },
      ])
    );
    expect(stacked.dataset["bars"]).toBe(JSON.stringify([{ label: "Urgent", values: [1, 1] }]));
    expect(stacked.getAttribute("data-fills")).toBeNull();
    expect(screen.getByTestId("analytics-custom-trigger-2")).toHaveTextContent("State name");

    await selectOption(user, 2, "No value");
    await waitFor(() =>
      expect(contractMocks.getAnalyticsChart).toHaveBeenLastCalledWith(expect.anything(), "acme", {
        type: "custom-work-items",
        xAxis: "PRIORITY",
        yAxis: "WORK_ITEM_COUNT",
        projectIds: [],
      })
    );
    expect(screen.getByTestId("analytics-custom-trigger-2")).toHaveTextContent("Add Property");
    await waitFor(() => expect(screen.getByTestId("mock-chart-bar").dataset["stacked"]).toBe("false"));
  });

  it("re-queries when x moves", async () => {
    const user = userEvent.setup();
    contractMocks.getAnalyticsChart.mockImplementation(
      async (_client: unknown, _slug: string, query: Record<string, unknown>) =>
        query["xAxis"] === "LABELS"
          ? { data: [{ key: "None", name: "None", count: 2 }], schema: {} }
          : { data: PRIORITY_ROWS, schema: {} }
    );
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 1, "Label");
    await waitFor(() =>
      expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(expect.anything(), "acme", {
        type: "custom-work-items",
        xAxis: "LABELS",
        yAxis: "WORK_ITEM_COUNT",
        projectIds: [],
      })
    );
    const expectedLabelBars = JSON.stringify([{ label: "None", values: [2] }]);
    await waitFor(() => expect(screen.getByTestId("mock-chart-bar").dataset["bars"]).toBe(expectedLabelBars));
  });

  it("groups by the estimate axis with one raw-label bar per stored value", async () => {
    const user = userEvent.setup();
    contractMocks.getAnalyticsChart.mockImplementation(
      async (_client: unknown, _slug: string, query: Record<string, unknown>) =>
        query["xAxis"] === "ESTIMATE_POINTS"
          ? {
              data: [
                { key: 3, name: "3", count: 1 },
                { key: 1, name: "1", count: 1 },
              ],
              schema: {},
            }
          : { data: PRIORITY_ROWS, schema: {} }
    );
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 1, "Estimate point");
    await waitFor(() =>
      expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(expect.anything(), "acme", {
        type: "custom-work-items",
        xAxis: "ESTIMATE_POINTS",
        yAxis: "WORK_ITEM_COUNT",
        projectIds: [],
      })
    );
    const expectedEstimateBars = JSON.stringify([
      { label: "3", values: [1] },
      { label: "1", values: [1] },
    ]);
    await waitFor(() => expect(screen.getByTestId("mock-chart-bar").dataset["bars"]).toBe(expectedEstimateBars));
    expect(screen.getByTestId("analytics-custom-trigger-0")).toHaveTextContent("Work item");
  });

  it("re-picking the metric fires no new read", async () => {
    const user = userEvent.setup();
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: PRIORITY_ROWS, schema: {} });
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 0, "Work item");
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledTimes(1);
  });

  it("paints state bars from the workspace state colors", async () => {
    const user = userEvent.setup();
    contractMocks.getAnalyticsChart.mockImplementation(
      async (_client: unknown, _slug: string, query: Record<string, unknown>) =>
        query["xAxis"] === "STATES"
          ? {
              data: [
                { key: "s1", name: "Backlog", count: 5 },
                { key: "gone", name: "Missing", count: 1 },
              ],
              schema: {},
            }
          : { data: PRIORITY_ROWS, schema: {} }
    );
    contractMocks.listWorkspaceStates.mockResolvedValue([
      {
        id: "s1",
        project_id: "p1",
        workspace_id: "w1",
        name: "Backlog",
        color: "#123456",
        group: "backlog",
        default: true,
        description: "",
        sequence: 0,
      },
    ]);
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 1, "State name");
    await waitFor(() => expect(contractMocks.listWorkspaceStates).toHaveBeenCalled());
    const expectedStateBars = JSON.stringify([
      { label: "Backlog", values: [5] },
      { label: "Missing", values: [1] },
    ]);
    await waitFor(() => expect(screen.getByTestId("mock-chart-bar").dataset["bars"]).toBe(expectedStateBars));
    expect(screen.getByTestId("mock-chart-bar").dataset["fills"]).toBe(JSON.stringify(["#123456", "#8b6edb"]));
  });

  it("holds the skeleton while the state reference read flies", async () => {
    const user = userEvent.setup();
    contractMocks.getAnalyticsChart.mockImplementation(
      async (_client: unknown, _slug: string, query: Record<string, unknown>) =>
        query["xAxis"] === "STATES"
          ? { data: [{ key: "s1", name: "Backlog", count: 1 }], schema: {} }
          : { data: PRIORITY_ROWS, schema: {} }
    );
    contractMocks.listWorkspaceStates.mockReturnValue(new Promise<never>(() => {}));
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 1, "State name");
    await waitFor(() => expect(screen.getByTestId("analytics-custom-skeleton")).toBeInTheDocument());
    expect(screen.queryByTestId("mock-chart-bar")).not.toBeInTheDocument();
  });

  it("falls back to the palette when the state reference read fails", async () => {
    const user = userEvent.setup();
    contractMocks.getAnalyticsChart.mockImplementation(
      async (_client: unknown, _slug: string, query: Record<string, unknown>) =>
        query["xAxis"] === "STATES"
          ? { data: [{ key: "s1", name: "Backlog", count: 1 }], schema: {} }
          : { data: PRIORITY_ROWS, schema: {} }
    );
    contractMocks.listWorkspaceStates.mockRejectedValue(new Error("states read failed"));
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 1, "State name");
    const expectedFallbackBars = JSON.stringify([{ label: "Backlog", values: [1] }]);
    await waitFor(() => expect(screen.getByTestId("mock-chart-bar").dataset["bars"]).toBe(expectedFallbackBars));
    expect(screen.getByTestId("mock-chart-bar").dataset["fills"]).toBe(JSON.stringify(["#6172e8"]));
  });

  it("labels date axes by day with year-aware labels", async () => {
    const year = new Date().getFullYear();
    contractMocks.getAnalyticsChart.mockResolvedValue({
      data: [
        { key: "2024-03-15", name: "2024-03-15", count: 1 },
        { key: `${year}-10-23`, name: `${year}-10-23`, count: 1 },
        { key: "None", name: "None", count: 1 },
      ],
      schema: {},
    });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <CustomInsightsSection workspaceSlug="acme" projectIds={[]} />
      </QueryClientProvider>
    );
    const user = userEvent.setup();
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 1, "Due date");
    await waitFor(() =>
      expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(expect.anything(), "acme", {
        type: "custom-work-items",
        xAxis: "TARGET_DATE",
        yAxis: "WORK_ITEM_COUNT",
        projectIds: [],
      })
    );
    // The stub answers the same date rows for every axis; the labels
    // prove the day grouping, not the request.
    const expectedDateBars = JSON.stringify([
      { label: "Mar 15, 2024", values: [1] },
      { label: "Oct 23", values: [1] },
      { label: "None", values: [1] },
    ]);
    await waitFor(() => expect(screen.getByTestId("mock-chart-bar").dataset["bars"]).toBe(expectedDateBars));
  });

  it("shows the guidance panel when the custom read fails", async () => {
    contractMocks.getAnalyticsChart.mockRejectedValue(new Error("custom read failed"));
    renderSection();
    const guidance = await screen.findByTestId("analytics-section-guidance");
    expect(guidance).toHaveTextContent(GUIDANCE);
    expect(screen.getByTestId("analytics-custom-trigger-1")).toBeInTheDocument();
    expect(screen.queryByTestId("mock-chart-bar")).not.toBeInTheDocument();
  });

  it("shows the guidance panel when the custom read has no rows", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: [], schema: {} });
    renderSection();
    const guidance = await screen.findByTestId("analytics-section-guidance");
    expect(guidance).toHaveTextContent(GUIDANCE);
    expect(screen.queryByTestId("mock-chart-bar")).not.toBeInTheDocument();
  });
});

describe("CustomInsightsSection table (ANL-012, ANL-016, ANL-017)", () => {
  function readBlob(blob: Blob): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result ?? ""));
      reader.onerror = () => reject(reader.error);
      reader.readAsText(blob);
    });
  }

  function readBlobBytes(blob: Blob): Promise<Uint8Array> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
      reader.onerror = () => reject(reader.error);
      reader.readAsArrayBuffer(blob);
    });
  }

  it("repeats the plotted rows under the x-axis and Count headers", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: PRIORITY_ROWS, schema: {} });
    renderSection(["a"]);
    await screen.findByTestId("mock-chart-bar");
    const table = screen.getByTestId("analytics-custom-table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent)
    ).toEqual(["Priority", "Count"]);
    expect(within(table).getByTestId("analytics-table-count")).toHaveTextContent("2 Priority");
    expect(
      within(table)
        .getAllByRole("row")
        .slice(1)
        .map((row) => row.textContent)
    ).toEqual(["Urgent1", "High2"]);
  });

  it("adds one exported column per split schema key", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({
      data: [{ key: "urgent", name: "urgent", count: 2, s1: 1, s2: 1 }],
      schema: { s2: "Done", s1: "Backlog" },
    });
    const user = userEvent.setup();
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 2, "State name");
    await waitFor(() =>
      expect(
        within(screen.getByTestId("analytics-custom-table"))
          .getAllByRole("columnheader")
          .map((cell) => cell.textContent)
      ).toEqual(["Priority", "Count", "Done", "Backlog"])
    );
  });

  it("expands search, filters the display first column and clears on close", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: PRIORITY_ROWS, schema: {} });
    const user = userEvent.setup();
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    const table = screen.getByTestId("analytics-custom-table");
    await user.click(within(table).getByRole("button", { name: "Search" }));
    await user.type(within(table).getByPlaceholderText("Search"), "urgent");
    const visible = within(table).getAllByRole("row").slice(1);
    expect(visible).toHaveLength(1);
    expect(visible[0]?.textContent).toBe("Urgent1");
    await user.click(within(table).getByRole("button", { name: "Close search" }));
    expect(within(table).queryByPlaceholderText("Search")).not.toBeInTheDocument();
    expect(within(table).getAllByRole("row").slice(1)).toHaveLength(2);
  });

  it("exports only the filtered rows with display headers and values", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: PRIORITY_ROWS, schema: {} });
    const user = userEvent.setup();
    const createObjectUrl = vi.fn((_blob: Blob) => "blob:csv");
    Object.assign(URL, { createObjectURL: createObjectUrl, revokeObjectURL: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    renderSection(["a"]);
    await screen.findByTestId("mock-chart-bar");
    const table = screen.getByTestId("analytics-custom-table");
    await user.click(within(table).getByRole("button", { name: "Search" }));
    await user.type(within(table).getByPlaceholderText("Search"), "urgent");
    await user.click(within(table).getByRole("button", { name: "Export as csv" }));
    const blob = createObjectUrl.mock.calls[0]?.[0] as Blob;
    expect(await readBlob(blob)).toBe("Priority,Count\r\nUrgent,1\r\n");
  });

  it("exports the drill-down with the workspace file name, split columns and shared byte rules", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({
      data: [{ key: "urgent", name: 'say "hi", now', count: 2, s1: 1, s2: 1 }],
      schema: { s1: "Backlog", s2: "Done" },
    });
    const user = userEvent.setup();
    const createObjectUrl = vi.fn((_blob: Blob) => "blob:csv");
    Object.assign(URL, { createObjectURL: createObjectUrl, revokeObjectURL: vi.fn() });
    const downloads: { name: string; href: string }[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      downloads.push({ name: this.download, href: this.href });
    });
    renderSection(["a"]);
    await screen.findByTestId("mock-chart-bar");
    await selectOption(user, 2, "State name");
    const table = screen.getByTestId("analytics-custom-table");
    await waitFor(() =>
      expect(
        within(table)
          .getAllByRole("columnheader")
          .map((cell) => cell.textContent)
      ).toEqual(["Priority", "Count", "Backlog", "Done"])
    );
    const readsBefore = contractMocks.getAnalyticsChart.mock.calls.length;

    await user.click(within(table).getByRole("button", { name: "Export as csv" }));

    expect(downloads[0]).toEqual({ name: "acme-analytics.csv", href: "blob:csv" });
    const bytes = await readBlobBytes(createObjectUrl.mock.calls[0]?.[0] as Blob);
    expect([...bytes.slice(0, 3)]).toEqual([0xef, 0xbb, 0xbf]);
    expect(new TextDecoder().decode(bytes.slice(3))).toBe('Priority,Count,Backlog,Done\r\n"Say ""hi"", now",2,1,1\r\n');
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledTimes(readsBefore);
  });

  it("does not download when the filter leaves no rows", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: PRIORITY_ROWS, schema: {} });
    const user = userEvent.setup();
    const createObjectUrl = vi.fn((_blob: Blob) => "blob:csv");
    Object.assign(URL, { createObjectURL: createObjectUrl, revokeObjectURL: vi.fn() });
    renderSection();
    await screen.findByTestId("mock-chart-bar");
    const table = screen.getByTestId("analytics-custom-table");
    await user.click(within(table).getByRole("button", { name: "Search" }));
    await user.type(within(table).getByPlaceholderText("Search"), "no match");
    expect(within(table).getAllByRole("row")).toHaveLength(2);
    expect(within(table).getByTestId("analytics-section-guidance")).toHaveTextContent(GUIDANCE);
    await user.click(within(table).getByRole("button", { name: "Export as csv" }));
    expect(createObjectUrl).not.toHaveBeenCalled();
  });
});
