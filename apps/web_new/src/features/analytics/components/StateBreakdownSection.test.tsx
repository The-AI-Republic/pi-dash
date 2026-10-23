// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// State-breakdown section tests (parity/specs/analytics.md §12, ANL-013).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AnalyticsStats, Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsStats: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { StateBreakdownSection } from "./StateBreakdownSection.js";

function project(id: string, name: string): Project {
  return {
    id,
    name,
    identifier: name.slice(0, 2).toUpperCase(),
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: "ws",
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
  };
}

function statsRow(
  projectId: string,
  name: string,
  extra: Partial<AnalyticsStats[number]> = {}
): AnalyticsStats[number] {
  return {
    project_id: projectId,
    project__name: name,
    cancelled_work_items: 0,
    completed_work_items: 1,
    backlog_work_items: 2,
    un_started_work_items: 3,
    started_work_items: 4,
    ...extra,
  };
}

const PROJECTS = [project("a", "Alpha"), project("b", "Beta")];

/** jsdom's Blob has no text(); FileReader is present in the test DOM. */
function readBlob(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(blob);
  });
}

function renderSection(projectIds: readonly string[] = []): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <StateBreakdownSection workspaceSlug="acme" projectIds={projectIds} projects={PROJECTS} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("StateBreakdownSection", () => {
  it("renders an uploaded project image inside the resolved row", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([statsRow("a", "Alpha")]);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <StateBreakdownSection
          workspaceSlug="acme"
          projectIds={[]}
          projects={[{ ...project("a", "Alpha"), logo_props: { in_use: "upload", url: "https://cdn.test/a.png" } }]}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    expect(document.querySelector("img")).toHaveAttribute("src", "https://cdn.test/a.png");
  });

  it("renders an icon-backed project mark inside the resolved row", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([statsRow("a", "Alpha")]);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <StateBreakdownSection
          workspaceSlug="acme"
          projectIds={[]}
          projects={[{ ...project("a", "Alpha"), logo_props: { in_use: "icon", icon: { name: "star", color: null } } }]}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByText("A")).toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
  });

  it("shows five skeleton rows under the six real headers while loading", () => {
    contractMocks.getAnalyticsStats.mockReturnValue(new Promise<never>(() => {}));
    renderSection(["a"]);
    expect(screen.getByRole("heading", { name: "State breakdown" })).toBeInTheDocument();
    expect(screen.getAllByRole("columnheader")).toHaveLength(6);
    expect(document.querySelectorAll("tbody tr")).toHaveLength(5);
    expect(document.querySelectorAll("tbody td > span[style*='height'][style*='20px']")).toHaveLength(30);
  });

  it("requests the work-items stats read under the picker scope", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([statsRow("a", "Alpha")]);
    renderSection(["a", "b"]);
    await screen.findByText("Alpha");
    expect(contractMocks.getAnalyticsStats).toHaveBeenCalledWith(expect.anything(), "acme", {
      type: "work-items",
      projectIds: ["a", "b"],
    });
  });

  it("renders resolved project names and stage counts in server order", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([statsRow("b", "Beta"), statsRow("a", "Alpha")]);
    renderSection();
    await screen.findByText("Beta");
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows.map((row) => row.textContent)).toEqual(["Beta24310", "Alpha24310"]);
  });

  it("shows guidance and a zero count with real headers on an empty read", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([]);
    renderSection();
    expect(await screen.findByTestId("analytics-section-guidance")).toHaveTextContent(
      "Create projects with work items and assignees to start tracking performance, progress, and team impact here."
    );
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("0 Projects");
    expect(screen.getAllByRole("columnheader")).toHaveLength(6);
  });

  it("shows guidance on a failed read without inventing rows", async () => {
    contractMocks.getAnalyticsStats.mockRejectedValue(new Error("forbidden"));
    renderSection();
    expect(await screen.findByTestId("analytics-section-guidance")).toBeInTheDocument();
    expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("0 Projects");
  });

  it("filters by project name and exports only the matching row", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([statsRow("a", "Alpha"), statsRow("b", "Beta")]);
    const user = userEvent.setup();
    const createObjectUrl = vi.fn((_blob: Blob) => "blob:csv");
    Object.assign(URL, { createObjectURL: createObjectUrl, revokeObjectURL: vi.fn() });
    renderSection();
    await screen.findByText("Alpha");
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(screen.getByPlaceholderText("Search"), "beta");
    expect(screen.queryByText("Alpha")).not.toBeInTheDocument();
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    await user.click(screen.getByRole("button", { name: "Export as csv" }));
    expect(createObjectUrl).toHaveBeenCalledOnce();
    const blob = createObjectUrl.mock.calls[0]?.[0] as Blob;
    expect(await readBlob(blob)).toBe("Project,Backlog,Started,Unstarted,Completed,Cancelled\r\nBeta,2,4,3,1,0\r\n");
  });

  it("exports the server project name when the live id no longer resolves", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([statsRow("gone", "Server name")]);
    const user = userEvent.setup();
    const createObjectUrl = vi.fn((_blob: Blob) => "blob:csv");
    Object.assign(URL, { createObjectURL: createObjectUrl, revokeObjectURL: vi.fn() });
    renderSection();
    await waitFor(() => expect(screen.getByTestId("analytics-table-count")).toHaveTextContent("1 Projects"));
    expect(screen.queryByText("Server name")).not.toBeInTheDocument();
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    await user.click(screen.getByRole("button", { name: "Export as csv" }));
    const blob = createObjectUrl.mock.calls[0]?.[0] as Blob;
    expect((await readBlob(blob)).split("\r\n")[1]).toBe("Server name,2,4,3,1,0");
  });
});
