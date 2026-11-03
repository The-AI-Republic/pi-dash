// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-cycle hero (parity/specs/cycles.md §15, CYC-041–042): the
// disclosure with its count, the loading/error/empty/active panel
// states, the list-predicate narrowing, the shared measure, and the
// breakdown click wiring.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const hero = vi.hoisted(() => ({
  read: vi.fn(),
  progress: vi.fn(),
  estimates: vi.fn(),
  project: { current: { data: { estimate: 1 } as { estimate: number | null } | undefined } },
  apply: vi.fn(),
  pending: false,
}));
vi.mock("../api/heroQueries.js", () => ({
  heroQueryOptions: () => ({ queryKey: ["hero"], queryFn: hero.read }),
  heroProgressQueryOptions: () => ({ queryKey: ["hero-progress"], queryFn: hero.progress }),
}));
vi.mock("../../estimates/index.js", () => ({
  projectEstimatesQueryOptions: () => ({ queryKey: ["estimates"], queryFn: hero.estimates }),
}));
vi.mock("../../auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../auth/index.js")>()),
  useProjectRow: () => hero.project.current,
}));
vi.mock("../api/heroBreakdown.js", () => ({
  useApplyBreakdownFilter: () => ({ apply: hero.apply, pending: hero.pending }),
}));
vi.mock("./CycleRow.js", () => ({
  CycleRow: (props: { cycle: { id: string; name: string } }) => (
    <div data-testid="hero-row" data-cycle={props.cycle.id}>
      {props.cycle.name}
    </div>
  ),
}));
vi.mock("./HeroProgressCard.js", () => ({
  HeroProgressCard: (props: { onGroupClick: (group: string) => void; disabled: boolean }) => (
    <div data-testid="hero-progress-probe" data-disabled={String(props.disabled)}>
      <button type="button" onClick={() => props.onGroupClick("backlog")}>
        probe group
      </button>
    </div>
  ),
}));
vi.mock("./HeroBurndownCard.js", () => ({
  HeroBurndownCard: (props: {
    measure: string;
    estimatesEnabled: boolean;
    estimateType: string | null;
    onMeasureChange: (measure: string) => void;
  }) => (
    <div
      data-testid="hero-burndown-probe"
      data-measure={props.measure}
      data-estimates={String(props.estimatesEnabled)}
      data-estimate-type={props.estimateType}
    >
      <button type="button" onClick={() => props.onMeasureChange("points")}>
        probe measure
      </button>
    </div>
  ),
}));
vi.mock("./HeroBreakdownCard.js", () => ({
  HeroBreakdownCard: (props: {
    measure: string;
    disabled: boolean;
    onEntryClick: (dimension: string, value: string, peek?: string) => void;
  }) => (
    <div data-testid="hero-breakdown-probe" data-measure={props.measure} data-disabled={String(props.disabled)}>
      <button type="button" onClick={() => props.onEntryClick("assignee_id", "ada")}>
        probe entry
      </button>
      <button type="button" onClick={() => props.onEntryClick("priority", "urgent", "issue-9")}>
        probe priority
      </button>
    </div>
  ),
}));
vi.mock("./NoActiveCycle.js", () => ({
  NoActiveCycle: (props: { canCreate: boolean }) => (
    <div data-testid="hero-empty-probe" data-can-create={String(props.canCreate)} />
  ),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID, makeCycleRow, makeEstimate, makeEstimateList } from "../testing.js";
import { ACTIVE_CYCLE_HEADING, ActiveCycleHero } from "./ActiveCycleHero.js";

function activeRow(): Record<string, unknown> {
  return makeCycleRow({ id: CYCLES_CYCLE_ID, name: "Active sprint", status: "current" }) as never;
}

function renderHero(canMutate = true, cycleFilter?: (cycle: CycleRowData) => boolean): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ActiveCycleHero
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        projectTimeZone="UTC"
        directory={new Map()}
        canMutate={canMutate}
        onPeek={() => undefined}
        currentProject={{ id: CYCLES_PROJECT_ID, name: "Website", identifier: "WEB" }}
        cycleFilter={cycleFilter}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", (() => new Promise<never>(() => undefined)) as never);
  hero.project.current = { data: { estimate: 1 } };
  hero.pending = false;
  hero.progress.mockResolvedValue({ total_estimate_points: 0 });
  hero.estimates.mockResolvedValue(makeEstimateList([makeEstimate()]));
  hero.read.mockResolvedValue([activeRow()]);
});

afterEach(() => {
  resetAppClient();
});

describe("ActiveCycleHero", () => {
  it("leads with the headed disclosure and collapses it", async () => {
    renderHero();
    const heading = await screen.findByRole("button", { name: /Active cycle/ });
    expect(heading).toHaveTextContent(ACTIVE_CYCLE_HEADING);
    expect(heading).toHaveAttribute("aria-expanded", "true");
    expect(await screen.findByTestId("hero-row")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-hero-count")).toHaveTextContent("1");
    fireEvent.click(heading);
    expect(screen.getByRole("button", { name: /Active cycle/ })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("hero-row")).not.toBeInTheDocument();
    expect(screen.getByTestId("cycle-hero-count")).toHaveTextContent("1");
  });

  it("holds the loading line while the hero read flies and hides on failure", async () => {
    hero.read.mockReturnValue(new Promise(() => undefined));
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ActiveCycleHero
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          projectTimeZone="UTC"
          directory={new Map()}
          canMutate={true}
          onPeek={() => undefined}
          currentProject={{ id: CYCLES_PROJECT_ID, name: "Website", identifier: "WEB" }}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("cycle-hero-loading")).toBeInTheDocument();
    unmount();

    hero.read.mockRejectedValue(new Error("boom"));
    renderHero();
    await waitFor(() => expect(screen.queryByTestId("cycle-hero-loading")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: /Active cycle/ })).toBeInTheDocument();
    expect(screen.getByTestId("cycle-hero-count")).toHaveTextContent("0");
    expect(screen.queryByTestId("hero-row")).not.toBeInTheDocument();
  });

  it("becomes the empty view with no cycle covering today", async () => {
    hero.read.mockResolvedValue([]);
    renderHero(false);
    expect(await screen.findByTestId("hero-empty-probe")).toHaveAttribute("data-can-create", "false");
    expect(screen.getByTestId("cycle-hero-count")).toHaveTextContent("0");
    expect(screen.queryByTestId("cycle-hero-cards")).not.toBeInTheDocument();
  });

  it("becomes the empty view when the list predicate rejects the active cycle", async () => {
    renderHero(true, () => false);
    expect(await screen.findByTestId("hero-empty-probe")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-hero-count")).toHaveTextContent("0");
    expect(screen.queryByTestId("hero-row")).not.toBeInTheDocument();
    expect(screen.queryByTestId("cycle-hero-cards")).not.toBeInTheDocument();
  });

  it("keeps the row when the list predicate accepts the active cycle", async () => {
    const seen: string[] = [];
    renderHero(true, (cycle) => {
      seen.push(cycle.id);
      return true;
    });
    expect(await screen.findByTestId("hero-row")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-hero-count")).toHaveTextContent("1");
    expect(seen).toEqual([CYCLES_CYCLE_ID]);
  });

  it("shares one measure between the burn-down and the breakdown", async () => {
    renderHero();
    expect(await screen.findByTestId("hero-burndown-probe")).toHaveAttribute("data-measure", "issues");
    expect(screen.getByTestId("hero-breakdown-probe")).toHaveAttribute("data-measure", "issues");
    expect(screen.getByTestId("hero-burndown-probe")).toHaveAttribute("data-estimates", "true");
    expect(screen.getByTestId("hero-burndown-probe")).toHaveAttribute("data-estimate-type", "points");
    fireEvent.click(screen.getByRole("button", { name: "probe measure" }));
    expect(screen.getByTestId("hero-burndown-probe")).toHaveAttribute("data-measure", "points");
    expect(screen.getByTestId("hero-breakdown-probe")).toHaveAttribute("data-measure", "points");
  });

  it("passes the active family and counts items for categories", async () => {
    hero.estimates.mockResolvedValue(makeEstimateList([makeEstimate({ type: "categories" })]));
    renderHero();
    expect(await screen.findByTestId("hero-burndown-probe")).toHaveAttribute("data-estimate-type", "categories");
    expect(screen.getByTestId("hero-burndown-probe")).toHaveAttribute("data-measure", "issues");
    expect(screen.getByTestId("hero-breakdown-probe")).toHaveAttribute("data-measure", "issues");
    fireEvent.click(screen.getByRole("button", { name: "probe measure" }));
    expect(screen.getByTestId("hero-burndown-probe")).toHaveAttribute("data-measure", "issues");
    expect(screen.getByTestId("hero-breakdown-probe")).toHaveAttribute("data-measure", "issues");
  });

  it("keeps point-data cycles on items when estimates are disabled", async () => {
    hero.project.current = { data: { estimate: null } };
    hero.progress.mockResolvedValue({ total_estimate_points: 6 });
    renderHero();
    expect(await screen.findByTestId("hero-burndown-probe")).toHaveAttribute("data-measure", "issues");
    expect(screen.getByTestId("hero-breakdown-probe")).toHaveAttribute("data-measure", "issues");
  });

  it("still reads the family for categories when estimates are disabled with point data", async () => {
    hero.project.current = { data: { estimate: null } };
    hero.progress.mockResolvedValue({ total_estimate_points: 6 });
    hero.estimates.mockResolvedValue(makeEstimateList([makeEstimate({ type: "categories" })]));
    renderHero();
    await waitFor(() =>
      expect(screen.getByTestId("hero-burndown-probe")).toHaveAttribute("data-estimate-type", "categories")
    );
    expect(screen.getByTestId("hero-burndown-probe")).toHaveAttribute("data-measure", "issues");
    expect(screen.getByTestId("hero-breakdown-probe")).toHaveAttribute("data-measure", "issues");
  });

  it("drops estimates where the project disables them or still loads", async () => {
    hero.project.current = { data: { estimate: null } };
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ActiveCycleHero
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          projectTimeZone="UTC"
          directory={new Map()}
          canMutate={true}
          onPeek={() => undefined}
          currentProject={{ id: CYCLES_PROJECT_ID, name: "Website", identifier: "WEB" }}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("hero-burndown-probe")).toHaveAttribute("data-estimates", "false");
    unmount();

    hero.project.current = { data: undefined };
    renderHero();
    expect(await screen.findByTestId("hero-burndown-probe")).toHaveAttribute("data-estimates", "false");
  });

  it("filters the cycle view from group and entry clicks", async () => {
    renderHero();
    await screen.findByTestId("hero-row");
    fireEvent.click(screen.getByRole("button", { name: "probe group" }));
    expect(hero.apply).toHaveBeenCalledWith("state_group", "backlog");
    fireEvent.click(screen.getByRole("button", { name: "probe entry" }));
    expect(hero.apply).toHaveBeenCalledWith("assignee_id", "ada", undefined);
    fireEvent.click(screen.getByRole("button", { name: "probe priority" }));
    expect(hero.apply).toHaveBeenCalledWith("priority", "urgent", "issue-9");
  });

  it("pins the entries while a click writes", async () => {
    hero.pending = true;
    renderHero();
    expect(await screen.findByTestId("hero-progress-probe")).toHaveAttribute("data-disabled", "true");
    expect(screen.getByTestId("hero-breakdown-probe")).toHaveAttribute("data-disabled", "true");
  });
});
