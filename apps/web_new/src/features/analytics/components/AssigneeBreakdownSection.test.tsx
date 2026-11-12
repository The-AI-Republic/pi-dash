// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dialog assignee breakdown tests (parity/specs/analytics.md §14, ANL-015).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AnalyticsProjectStats } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getProjectAnalyticsStats: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { AssigneeBreakdownSection } from "./AssigneeBreakdownSection.js";

const SCOPE = { projectId: "223e4567-e89b-12d3-a456-426614174001", cycleId: null, moduleId: null };

function statsRow(extra: Partial<AnalyticsProjectStats[number]> = {}): AnalyticsProjectStats[number] {
  return {
    display_name: "Ada Lovelace",
    assignee_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    avatar_url: "https://cdn.test/ada.png",
    cancelled_work_items: 0,
    completed_work_items: 1,
    backlog_work_items: 2,
    un_started_work_items: 3,
    started_work_items: 4,
    ...extra,
  };
}

function renderSection(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AssigneeBreakdownSection workspaceSlug="acme" scope={SCOPE} />
    </QueryClientProvider>
  );
}

function readBlob(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(blob);
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("AssigneeBreakdownSection", () => {
  it("shows five skeleton rows under the six assignee headers while loading", () => {
    contractMocks.getProjectAnalyticsStats.mockReturnValue(new Promise<never>(() => {}));
    renderSection();
    expect(screen.getByRole("heading", { name: "State breakdown" })).toBeInTheDocument();
    expect(screen.getAllByRole("columnheader").map((head) => head.textContent)).toEqual([
      "Assignee",
      "Backlog",
      "Started",
      "Unstarted",
      "Completed",
      "Cancelled",
    ]);
    expect(document.querySelectorAll("tbody tr")).toHaveLength(5);
    expect(screen.queryByRole("button", { name: "Export as csv" })).not.toBeInTheDocument();
  });

  it("reads the project stats endpoint with the dialog scope", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([statsRow()]);
    renderSection();
    await screen.findByText("Ada Lovelace");
    expect(contractMocks.getProjectAnalyticsStats).toHaveBeenCalledWith(expect.anything(), "acme", SCOPE.projectId, {
      projectIds: [SCOPE.projectId],
      cycleId: undefined,
      moduleId: undefined,
    });
  });

  it("renders an avatar for a pictured assignee", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([statsRow()]);
    renderSection();
    const cell = (await screen.findByText("Ada Lovelace")).closest("td");
    expect(cell?.querySelector("img")).toHaveAttribute("src", "https://cdn.test/ada.png");
  });

  it("falls back to initials when there is no avatar image", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([statsRow({ avatar_url: null })]);
    renderSection();
    const cell = (await screen.findByText("Ada Lovelace")).closest("td");
    expect(cell?.querySelector("img")).toBeNull();
    expect(within(cell as HTMLElement).getByRole("img", { name: "Ada Lovelace" })).toHaveTextContent("AL");
  });

  it("marks the unassigned row with a neutral user mark and its label", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([
      statsRow({ display_name: null, assignee_id: null, avatar_url: null }),
    ]);
    renderSection();
    const cell = (await screen.findByText("Unassigned")).closest("td");
    expect(cell?.querySelector("img")).toBeNull();
    expect(cell?.querySelector('[data-testid="assignee-unassigned-mark"]')).not.toBeNull();
  });

  it("keeps server row order and shows stage counts", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([
      statsRow({ display_name: "Zoe", avatar_url: null }),
      statsRow({ display_name: null, assignee_id: null, avatar_url: null }),
      statsRow({ display_name: "Ada" }),
    ]);
    renderSection();
    await screen.findByText("Zoe");
    const rows = screen.getAllByRole("row").slice(1);
    // Rows keep the server's order. The first cell's text also carries
    // the initials glyph when there is no image (Zoe), and nothing extra
    // when the avatar is a pictured member (Ada).
    expect(rows.map((row) => row.textContent)).toEqual(["ZOZoe24310", "Unassigned24310", "Ada24310"]);
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("3 Assignee");
  });

  it("shows guidance and a zero count on an empty read", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([]);
    renderSection();
    expect(await screen.findByTestId("analytics-section-guidance")).toHaveTextContent(
      "Create projects with work items and assignees to start tracking performance, progress, and team impact here."
    );
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("0 Assignee");
  });

  it("shows guidance on a refused read without inventing rows", async () => {
    contractMocks.getProjectAnalyticsStats.mockRejectedValue(new Error("forbidden"));
    renderSection();
    expect(await screen.findByTestId("analytics-section-guidance")).toBeInTheDocument();
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("0 Assignee");
  });

  it("searches by assignee and exports the filtered rows with display headers", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([
      statsRow({ display_name: "Ada Lovelace" }),
      statsRow({ display_name: "Bob Stone", avatar_url: null }),
    ]);
    const user = userEvent.setup();
    const createObjectUrl = vi.fn((_blob: Blob) => "blob:csv");
    Object.assign(URL, { createObjectURL: createObjectUrl, revokeObjectURL: vi.fn() });
    renderSection();
    await screen.findByText("Ada Lovelace");
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(screen.getByPlaceholderText("Search"), "bob");
    expect(screen.queryByText("Ada Lovelace")).not.toBeInTheDocument();
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("2 Assignee");
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    await user.click(screen.getByRole("button", { name: "Export as csv" }));
    expect(createObjectUrl).toHaveBeenCalledOnce();
    const blob = createObjectUrl.mock.calls[0]?.[0] as Blob;
    expect(await readBlob(blob)).toBe(
      "Assignee,Backlog,Started,Unstarted,Completed,Cancelled\r\nBob Stone,2,4,3,1,0\r\n"
    );
  });

  it("collapses the search and restores every row on close", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([statsRow(), statsRow({ display_name: "Bob Stone" })]);
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Ada Lovelace");
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(screen.getByPlaceholderText("Search"), "bob");
    await waitFor(() => expect(screen.queryByText("Ada Lovelace")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Close search" }));
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
    expect(screen.queryByPlaceholderText("Search")).not.toBeInTheDocument();
  });
});
