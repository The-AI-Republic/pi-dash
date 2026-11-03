// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero progress card (parity/specs/cycles.md §15, CYC-041): the
// closed summary with its cancelled line, clickable nonempty groups,
// static zeros, and the empty view.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const reads = vi.hoisted(() => ({ progress: vi.fn() }));
vi.mock("../api/heroQueries.js", () => ({
  heroProgressQueryOptions: (_client: unknown, workspaceSlug: string, projectId: string, cycleId: string) => ({
    queryKey: ["hero-progress", workspaceSlug, projectId, cycleId],
    queryFn: reads.progress,
  }),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import { HERO_PROGRESS_EMPTY, HeroProgressCard } from "./HeroProgressCard.js";

function counts(): Record<string, number> {
  return {
    total_issues: 13,
    backlog_issues: 4,
    unstarted_issues: 0,
    started_issues: 8,
    completed_issues: 1,
    cancelled_issues: 0,
  };
}

function renderCard(onGroupClick: (group: string) => void = () => undefined, disabled = false): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <HeroProgressCard
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        cycleId={CYCLES_CYCLE_ID}
        onGroupClick={onGroupClick as never}
        disabled={disabled}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  // The card reads the client at render; the mocked queries never touch it.
  createAppClient("https://api.test", (() => new Promise<never>(() => undefined)) as never);
});

afterEach(() => {
  resetAppClient();
});

describe("HeroProgressCard", () => {
  it("summarises closed work and lists the five groups in order", async () => {
    reads.progress.mockResolvedValue(counts());
    const onGroupClick = vi.fn();
    renderCard(onGroupClick);
    expect(await screen.findByTestId("hero-progress-closed")).toHaveTextContent("1/13 Work items closed");
    expect(screen.getByText("Cancelled work items 0")).toBeInTheDocument();
    const groups = screen.getAllByTestId("hero-progress-group");
    expect(groups.map((group) => group.getAttribute("data-group"))).toEqual([
      "backlog",
      "unstarted",
      "started",
      "completed",
      "cancelled",
    ]);
    fireEvent.click(screen.getByRole("button", { name: /backlog/i }));
    expect(onGroupClick).toHaveBeenCalledWith("backlog");
    fireEvent.click(screen.getByRole("button", { name: /started/i }));
    expect(onGroupClick).toHaveBeenCalledWith("started");
  });

  it("reads zero groups as static rows", async () => {
    reads.progress.mockResolvedValue(counts());
    renderCard();
    await screen.findByTestId("hero-progress-closed");
    expect(screen.getByRole("button", { name: /backlog/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /unstarted/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /cancelled/i })).not.toBeInTheDocument();
  });

  it("disables the groups while a click writes", async () => {
    reads.progress.mockResolvedValue(counts());
    renderCard(() => undefined, true);
    await screen.findByTestId("hero-progress-closed");
    expect(screen.getByRole("button", { name: /backlog/i })).toBeDisabled();
  });

  it("holds the loading line while the read flies", () => {
    reads.progress.mockReturnValue(new Promise(() => undefined));
    renderCard();
    expect(screen.getByTestId("hero-progress-loading")).toBeInTheDocument();
  });

  it("shows the empty view without work, on failure, or on a missing answer", async () => {
    reads.progress.mockResolvedValue({ ...counts(), total_issues: 0 });
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <HeroProgressCard
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          cycleId={CYCLES_CYCLE_ID}
          onGroupClick={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByText(HERO_PROGRESS_EMPTY)).toBeInTheDocument();
    unmount();

    reads.progress.mockRejectedValue(new Error("boom"));
    const failed = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <HeroProgressCard
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          cycleId={CYCLES_CYCLE_ID}
          onGroupClick={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByText(HERO_PROGRESS_EMPTY)).toBeInTheDocument();
    failed.unmount();

    reads.progress.mockResolvedValue(undefined);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <HeroProgressCard
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          cycleId={CYCLES_CYCLE_ID}
          onGroupClick={() => undefined}
        />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByText(HERO_PROGRESS_EMPTY)).toBeInTheDocument());
  });
});
