// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero breakdown card (parity/specs/cycles.md §15, CYC-041): the
// three tabs with their entries, per-tab persistence and empties,
// and the click wiring into the filtered detail.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const reads = vi.hoisted(() => ({ analytics: vi.fn(), issues: vi.fn() }));
vi.mock("../api/heroQueries.js", () => ({
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
  heroIssuesQueryOptions: (_client: unknown, workspaceSlug: string, projectId: string, cycleId: string) => ({
    queryKey: ["hero-issues", workspaceSlug, projectId, cycleId],
    queryFn: reads.issues,
  }),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { BreakdownDimension } from "../lib/cycleBreakdownFilter.js";
import type { HeroMeasure } from "../lib/cycleHero.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import {
  HERO_ASSIGNEES_EMPTY,
  HERO_LABELS_EMPTY,
  HERO_PRIORITY_EMPTY,
  HeroBreakdownCard,
  readStoredHeroTab,
  storeHeroTab,
} from "./HeroBreakdownCard.js";

const TAB_KEY = `cycle-hero-tab-${CYCLES_CYCLE_ID}`;
const MEMBER = "123e4567-e89b-12d3-a456-426614174010";

function analytics(): Record<string, unknown> {
  return {
    completion_chart: {},
    assignees: [
      {
        assignee_id: MEMBER,
        display_name: "Ada Lovelace",
        avatar_url: null,
        total_issues: 3,
        total_estimates: 8,
      },
      {
        assignee_id: "grace",
        display_name: "Grace Hopper",
        avatar_url: "https://cdn.test/grace.png",
        total_issues: 1,
        total_estimates: 2,
      },
    ],
    labels: [
      { label_id: "bug", label_name: "Bug", color: "#ff0000", total_issues: 2, total_estimates: 5 },
      { label_id: "docs", label_name: "Docs", color: null, total_issues: 1, total_estimates: 1 },
    ],
  };
}

function issues(): Record<string, unknown> {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 3,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 3,
    total_results: 3,
    results: [
      { id: "low-1", name: "Low chore", priority: "low" },
      { id: "urgent-1", name: "Urgent fix", priority: "urgent" },
      { id: "high-1", name: "High value", priority: "high" },
    ],
  };
}

function renderCard(
  onEntryClick: (dimension: BreakdownDimension, value: string, peekIssueId?: string) => void = () => undefined,
  measure: HeroMeasure = "issues",
  disabled = false
): { unmount: () => void } {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <HeroBreakdownCard
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        cycleId={CYCLES_CYCLE_ID}
        measure={measure}
        onEntryClick={onEntryClick}
        disabled={disabled}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  createAppClient("https://api.test", (() => new Promise<never>(() => undefined)) as never);
  reads.analytics.mockResolvedValue(analytics());
  reads.issues.mockResolvedValue(issues());
});

afterEach(() => {
  resetAppClient();
  window.localStorage.clear();
});

describe("hero tab storage", () => {
  it("keeps the tab per cycle and falls back without storage", () => {
    expect(readStoredHeroTab(CYCLES_CYCLE_ID)).toBe("priority");
    storeHeroTab(CYCLES_CYCLE_ID, "labels");
    expect(window.localStorage.getItem(TAB_KEY)).toBe("labels");
    expect(readStoredHeroTab(CYCLES_CYCLE_ID)).toBe("labels");
  });

  it("survives denied storage", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    });
    try {
      expect(readStoredHeroTab(CYCLES_CYCLE_ID)).toBe("priority");
      expect(() => storeHeroTab(CYCLES_CYCLE_ID, "labels")).not.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("HeroBreakdownCard", () => {
  it("pins the tab order and opens the kept tab", async () => {
    window.localStorage.setItem(TAB_KEY, "assignees");
    renderCard();
    const tabs = await screen.findAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Priority work items", "Assignees", "Labels"]);
    expect(screen.getByRole("tab", { name: "Assignees" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveAttribute("data-tab", "assignees");
  });

  it("lists priority entries and peeks the picked work item", async () => {
    const onEntryClick = vi.fn();
    renderCard(onEntryClick);
    expect(await screen.findByRole("tab", { name: "Priority work items" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("button", { name: /Urgent fix/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /High value/ })).toBeInTheDocument();
    expect(screen.queryByText("Low chore")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Urgent fix/ }));
    expect(onEntryClick).toHaveBeenCalledWith("priority", "urgent", "urgent-1");
  });

  it("lists assignee and label entries with counts and dots", async () => {
    const onEntryClick = vi.fn();
    renderCard(onEntryClick);
    fireEvent.click(await screen.findByRole("tab", { name: "Assignees" }));
    expect(await screen.findByRole("button", { name: /Ada Lovelace/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Grace Hopper/ })).toBeInTheDocument();
    expect(window.localStorage.getItem(TAB_KEY)).toBe("assignees");
    fireEvent.click(screen.getByRole("button", { name: /Ada Lovelace/ }));
    expect(onEntryClick).toHaveBeenCalledWith("assignee_id", MEMBER);

    fireEvent.click(screen.getByRole("tab", { name: "Labels" }));
    expect(await screen.findByRole("button", { name: /Bug/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Docs/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Docs/ }));
    expect(onEntryClick).toHaveBeenCalledWith("label_id", "docs");
  });

  it("reads estimate counts under the points measure", async () => {
    renderCard(() => undefined, "points");
    fireEvent.click(await screen.findByRole("tab", { name: "Assignees" }));
    expect(await screen.findByRole("button", { name: /Ada Lovelace/ })).toHaveTextContent("8");
  });

  it("disables the entries while a click writes", async () => {
    renderCard(() => undefined, "issues", true);
    expect(await screen.findByRole("button", { name: /Urgent fix/ })).toBeDisabled();
  });

  it("shows per-tab empty views", async () => {
    reads.analytics.mockResolvedValue({ completion_chart: {}, assignees: [], labels: [] });
    reads.issues.mockResolvedValue({ ...issues(), results: [{ id: "low-1", name: "Low chore", priority: "low" }] });
    renderCard();
    expect(await screen.findByText(HERO_PRIORITY_EMPTY)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Assignees" }));
    expect(await screen.findByText(HERO_ASSIGNEES_EMPTY)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Labels" }));
    expect(await screen.findByText(HERO_LABELS_EMPTY)).toBeInTheDocument();
  });

  it("holds the loading line while the reads fly, except settled tabs", async () => {
    reads.analytics.mockReturnValue(new Promise(() => undefined));
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <HeroBreakdownCard
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          cycleId={CYCLES_CYCLE_ID}
          measure="issues"
          onEntryClick={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("hero-breakdown-loading")).toBeInTheDocument();
    unmount();

    reads.analytics.mockResolvedValue(analytics());
    reads.issues.mockReturnValue(new Promise(() => undefined));
    renderCard();
    expect(await screen.findByTestId("hero-breakdown-loading")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Labels" }));
    expect(await screen.findByRole("button", { name: /Bug/ })).toBeInTheDocument();
  });

  it("empties the tabs whose reads fail", async () => {
    reads.analytics.mockRejectedValue(new Error("boom"));
    const failing = renderCard();
    fireEvent.click(await screen.findByRole("tab", { name: "Assignees" }));
    expect(await screen.findByText(HERO_ASSIGNEES_EMPTY)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Labels" }));
    expect(await screen.findByText(HERO_LABELS_EMPTY)).toBeInTheDocument();
    failing.unmount();

    reads.analytics.mockResolvedValue(analytics());
    reads.issues.mockRejectedValue(new Error("boom"));
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <HeroBreakdownCard
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          cycleId={CYCLES_CYCLE_ID}
          measure="issues"
          onEntryClick={() => undefined}
        />
      </QueryClientProvider>
    );
    // The labels tab persisted from the previous render; switch back.
    fireEvent.click(await screen.findByRole("tab", { name: "Priority work items" }));
    expect(await screen.findByText(HERO_PRIORITY_EMPTY)).toBeInTheDocument();
    unmount();

    reads.issues.mockResolvedValue(undefined);
    const reopened = renderCard();
    expect(await screen.findByText(HERO_PRIORITY_EMPTY)).toBeInTheDocument();
    reopened.unmount();
  });
});
