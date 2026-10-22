// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Headline totals grid tests (parity/specs/analytics.md §§5, 8, ANL-005, ANL-008).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsTotals: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import type { AnalyticsTabId } from "../lib/tabs.js";
import { HeadlineCards } from "./HeadlineCards.js";

function renderGrid(tab: AnalyticsTabId = "overview"): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <HeadlineCards workspaceSlug="acme" tab={tab} projectIds={[]} />
    </QueryClientProvider>
  );
}

function cards(): HTMLElement[] {
  return screen.getAllByTestId("analytics-headline-card");
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("HeadlineCards", () => {
  it("shows one placeholder per card with labels visible while loading", () => {
    contractMocks.getAnalyticsTotals.mockReturnValue(new Promise<never>(() => {}));
    renderGrid();
    expect(screen.getAllByTestId("analytics-headline-placeholder")).toHaveLength(11);
    expect(screen.queryByTestId("analytics-headline-value")).not.toBeInTheDocument();
    expect(screen.getByText("Total Users")).toBeInTheDocument();
  });

  it("renders eleven overview values in order once settled", async () => {
    contractMocks.getAnalyticsTotals.mockResolvedValue({
      total_users: { count: 3 },
      total_admins: { count: 1 },
    });
    renderGrid("overview");
    await waitFor(() => expect(screen.getAllByTestId("analytics-headline-value")).toHaveLength(11));
    const rendered = cards();
    expect(within(rendered[0]!).getByTestId("analytics-headline-label")).toHaveTextContent("Total Users");
    expect(within(rendered[0]!).getByTestId("analytics-headline-value")).toHaveTextContent("3");
    expect(within(rendered[1]!).getByTestId("analytics-headline-value")).toHaveTextContent("1");
    expect(within(rendered[10]!).getByTestId("analytics-headline-label")).toHaveTextContent("Agent total tokens");
  });

  it("renders the five work-items cards", async () => {
    contractMocks.getAnalyticsTotals.mockResolvedValue({ total_work_items: { count: 4 } });
    renderGrid("work-items");
    await waitFor(() => expect(screen.getAllByTestId("analytics-headline-value")).toHaveLength(5));
    expect(cards().map((card) => within(card).getByTestId("analytics-headline-label").textContent)).toEqual([
      "Total Work items",
      "Started Work items",
      "Backlog Work items",
      "Unstarted Work items",
      "Completed Work items",
    ]);
    expect(contractMocks.getAnalyticsTotals).toHaveBeenCalledWith(expect.anything(), "acme", {
      tab: "work-items",
      projectIds: [],
    });
  });

  it("reads missing metrics as zero", async () => {
    contractMocks.getAnalyticsTotals.mockResolvedValue({});
    renderGrid("work-items");
    await waitFor(() => expect(screen.getAllByTestId("analytics-headline-value")).toHaveLength(5));
    for (const card of cards()) {
      expect(within(card).getByTestId("analytics-headline-value")).toHaveTextContent("0");
    }
  });

  it("settles a failed read to zeros with no error banner", async () => {
    contractMocks.getAnalyticsTotals.mockRejectedValue(new Error("forbidden"));
    renderGrid("overview");
    await waitFor(() => expect(screen.getAllByTestId("analytics-headline-value")).toHaveLength(11));
    for (const card of cards()) {
      expect(within(card).getByTestId("analytics-headline-value")).toHaveTextContent("0");
    }
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
