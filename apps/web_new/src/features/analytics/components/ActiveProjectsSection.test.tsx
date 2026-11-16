// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics active-projects section tests (parity/specs/analytics.md §7, ANL-007).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getProjectStats: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { ActiveProjectsSection } from "./ActiveProjectsSection.js";

const LONG_NAME = "Anl Active With An Extremely Long Project Name That Must Shorten";
const PROJECTS = [
  { id: "full", name: LONG_NAME, identifier: "AF", logo_props: {}, member_role: 15, archived_at: null },
  { id: "half", name: "Half", identifier: "AH", logo_props: {}, member_role: 15, archived_at: null },
  { id: "none", name: "None", identifier: "AN", logo_props: {}, member_role: 15, archived_at: null },
];

function renderSection(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ActiveProjectsSection workspaceSlug="acme" projectIds={[]} projects={PROJECTS} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("ActiveProjectsSection", () => {
  it("covers the fetch with five placeholder rows behind the section header", () => {
    contractMocks.getProjectStats.mockReturnValue(new Promise<never>(() => {}));
    renderSection();
    expect(screen.getByRole("heading", { name: "Active Projects" })).toBeInTheDocument();
    expect(screen.getAllByTestId("analytics-active-placeholder")).toHaveLength(5);
    expect(screen.queryByTestId("analytics-active-row")).not.toBeInTheDocument();
  });

  it("badges each resolved project with the halfway tone flip", async () => {
    contractMocks.getProjectStats.mockResolvedValue([
      { id: "full", total_issues: 2, completed_issues: 2 },
      { id: "half", total_issues: 2, completed_issues: 1 },
      { id: "none", total_issues: 1, completed_issues: 0 },
    ]);
    renderSection();
    const rows = await screen.findAllByTestId("analytics-active-row");
    expect(rows).toHaveLength(3);
    const badges = rows.map((row) => within(row).getByTestId("analytics-active-badge"));
    expect(badges.map((badge) => badge.textContent)).toEqual(["100%", "50%", "0%"]);
    expect(badges.map((badge) => badge.dataset["tone"])).toEqual(["success", "danger", "danger"]);
    expect(screen.getByTestId("analytics-active-list")).toBeInTheDocument();
  });

  it("shortens long names with the full form on hover", async () => {
    contractMocks.getProjectStats.mockResolvedValue([{ id: "full", total_issues: 1, completed_issues: 1 }]);
    renderSection();
    const name = await screen.findByTestId("analytics-active-name");
    expect(name).toHaveClass("truncate");
    expect(name.dataset["projectName"]).toBe(LONG_NAME);
    fireEvent.focus(name);
    fireEvent.mouseEnter(name);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(LONG_NAME);
  });

  it("skips entries that no longer resolve", async () => {
    contractMocks.getProjectStats.mockResolvedValue([
      { id: "half", total_issues: 2, completed_issues: 1 },
      { id: "stale", total_issues: 2, completed_issues: 2 },
    ]);
    renderSection();
    const rows = await screen.findAllByTestId("analytics-active-row");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("Half");
  });

  it("shows an empty list with no guidance when nothing resolves", async () => {
    contractMocks.getProjectStats.mockResolvedValue([]);
    renderSection();
    const list = await screen.findByTestId("analytics-active-list");
    expect(within(list).queryByTestId("analytics-active-row")).not.toBeInTheDocument();
    expect(screen.queryByTestId("analytics-section-guidance")).not.toBeInTheDocument();
  });

  it("shows an empty list when the read fails", async () => {
    contractMocks.getProjectStats.mockRejectedValue(new Error("stats read failed"));
    renderSection();
    const list = await screen.findByTestId("analytics-active-list");
    expect(within(list).queryByTestId("analytics-active-row")).not.toBeInTheDocument();
  });
});
