// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cycles screen composes exactly one body state: skeleton, error
// with retry, the admin-gated feature gate, the member-gated zero
// state, or the list slot (CYC-010–012).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CycleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router: navigation is TanStack's own tested behavior.
// What this suite owns is the screen composition and its reads.
const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CYCLES_PROJECT_ID, jsonResponse, makeCycleRow, makeProjectDetails, makeProjectRow } from "../testing.js";
import { CyclesListScreen } from "./CyclesListScreen.js";

interface Harness {
  project: Record<string, unknown>;
  projectStatus: number;
  projectDelayMs: number;
  projectCalls: number;
  roles: Record<string, number>;
  cycles: CycleRow[];
  cyclesStatus: number;
  cyclesDelayMs: number;
  cyclesCalls: number;
}

let harness: Harness;

function resetHarness(): Harness {
  return {
    project: makeProjectRow(),
    projectStatus: 200,
    projectDelayMs: 0,
    projectCalls: 0,
    roles: { [CYCLES_PROJECT_ID]: 20 },
    cycles: [],
    cyclesStatus: 200,
    cyclesDelayMs: 0,
    cyclesCalls: 0,
  };
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  harness = resetHarness();
  routerHarness.navigate.mockReset();
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/`)) {
      harness.projectCalls += 1;
      if (harness.projectDelayMs > 0) await sleep(harness.projectDelayMs);
      return jsonResponse(url, harness.projectStatus, harness.project);
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, harness.roles);
    if (url.endsWith("/projects/details/")) return jsonResponse(url, 200, [makeProjectDetails()]);
    if (url.endsWith(`/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/cycles/`)) {
      harness.cyclesCalls += 1;
      if (harness.cyclesDelayMs > 0) await sleep(harness.cyclesDelayMs);
      return jsonResponse(url, harness.cyclesStatus, harness.cycles);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
});

function renderScreen(options: { onCreateCycle?: () => void; children?: React.ReactNode } = {}): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CyclesListScreen workspaceSlug="acme" projectId={CYCLES_PROJECT_ID} onCreateCycle={options.onCreateCycle}>
        {options.children ?? <div data-testid="cycles-list-body">rows</div>}
      </CyclesListScreen>
    </QueryClientProvider>
  );
}

describe("CyclesListScreen", () => {
  it("renders the zero-state with a working creation slot for members", async () => {
    const onCreateCycle = vi.fn();
    renderScreen({ onCreateCycle });
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    expect(document.title).toBe("Website relaunch - Cycles");
    const action = screen.getByRole("button", { name: "Set your first cycle" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(onCreateCycle).toHaveBeenCalledTimes(1);
  });

  it("disables zero-state creation for guests", async () => {
    harness.roles = { [CYCLES_PROJECT_ID]: 5 };
    renderScreen();
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set your first cycle" })).toBeDisabled();
  });

  it("gates the list with an admin shortcut to the feature leaf", async () => {
    harness.project = makeProjectRow({ cycle_view: false });
    renderScreen();
    expect(await screen.findByTestId("cycles-feature-gate")).toBeInTheDocument();
    const action = screen.getByRole("button", { name: "Manage features" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: `/acme/settings/projects/${CYCLES_PROJECT_ID}/features/cycles`,
    });
    expect(harness.cyclesCalls).toBe(0);
  });

  it("disables the gate shortcut for guests", async () => {
    harness.project = makeProjectRow({ cycle_view: false });
    harness.roles = { [CYCLES_PROJECT_ID]: 5 };
    renderScreen();
    expect(await screen.findByTestId("cycles-feature-gate")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Manage features" })).toBeDisabled();
  });

  it("renders the list slot once cycles arrive", async () => {
    harness.cycles = [makeCycleRow({ name: "Cycle 7" })];
    renderScreen();
    expect(await screen.findByTestId("cycles-list-body")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-list-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("cycles-zero-state")).not.toBeInTheDocument();
  });

  it("skeletons while the project read is pending", async () => {
    harness.projectDelayMs = 50;
    renderScreen();
    expect(await screen.findByTestId("cycles-list-skeleton")).toBeInTheDocument();
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-list-skeleton")).not.toBeInTheDocument();
  });

  it("skeletons while the list read is pending", async () => {
    harness.cyclesDelayMs = 50;
    renderScreen();
    expect(await screen.findByTestId("cycles-list-skeleton")).toBeInTheDocument();
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-list-skeleton")).not.toBeInTheDocument();
  });

  it("retries a failed project read", async () => {
    harness.projectStatus = 500;
    harness.project = { detail: "boom" };
    renderScreen();
    expect(await screen.findByTestId("cycles-load-error")).toBeInTheDocument();
    expect(harness.cyclesCalls).toBe(0);
    harness.projectStatus = 200;
    harness.project = makeProjectRow();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    expect(harness.projectCalls).toBeGreaterThan(1);
  });

  it("retries a failed list read", async () => {
    harness.cyclesStatus = 500;
    harness.cycles = [];
    renderScreen();
    expect(await screen.findByTestId("cycles-load-error")).toBeInTheDocument();
    harness.cyclesStatus = 200;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    expect(harness.cyclesCalls).toBeGreaterThan(1);
  });

  it("opens the screen dialog from the header action over the zero-state", async () => {
    renderScreen();
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-list-header")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add cycle" }));
    expect(await screen.findByRole("heading", { name: "Create cycle" })).toBeInTheDocument();
  });

  it("opens the screen dialog from the zero-state shortcut", async () => {
    renderScreen();
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Set your first cycle" }));
    expect(await screen.findByRole("heading", { name: "Create cycle" })).toBeInTheDocument();
  });

  it("shows the header over the list", async () => {
    harness.cycles = [makeCycleRow({ name: "Cycle 7" })];
    renderScreen();
    expect(await screen.findByTestId("cycles-list-body")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-list-header")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add cycle" })).toBeInTheDocument();
  });

  it("hides the header while gated", async () => {
    harness.project = makeProjectRow({ cycle_view: false });
    renderScreen();
    expect(await screen.findByTestId("cycles-feature-gate")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-list-header")).not.toBeInTheDocument();
  });

  it("renders no header creation action for guests", async () => {
    harness.roles = { [CYCLES_PROJECT_ID]: 5 };
    renderScreen();
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-list-header")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add cycle" })).not.toBeInTheDocument();
  });

  it("keeps the guest list body — search and filters — while gating only creation", async () => {
    // CYC-047/NEWFRONT-268: the create gate must not swallow the body,
    // which owns the read-only search box and Filters menu.
    harness.roles = { [CYCLES_PROJECT_ID]: 5 };
    harness.cycles = [makeCycleRow({ name: "Cycle 7" })];
    renderScreen();
    expect(await screen.findByTestId("cycles-list-body")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-list-header")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add cycle" })).not.toBeInTheDocument();
  });

  it("prefers an explicit opener over the screen dialog", async () => {
    const onCreateCycle = vi.fn();
    renderScreen({ onCreateCycle });
    expect(await screen.findByTestId("cycles-zero-state")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add cycle" }));
    expect(onCreateCycle).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("heading", { name: "Create cycle" })).not.toBeInTheDocument();
  });

  it("keeps the rows while a background refetch runs", async () => {
    harness.cycles = [makeCycleRow({ name: "Cycle 7" })];
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <CyclesListScreen workspaceSlug="acme" projectId={CYCLES_PROJECT_ID}>
          <div data-testid="cycles-list-body">rows</div>
        </CyclesListScreen>
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("cycles-list-body")).toBeInTheDocument();
    harness.cyclesDelayMs = 50;
    harness.cycles = [makeCycleRow({ name: "Cycle 7" }), makeCycleRow({ name: "Cycle 8" })];
    const settled = queryClient.invalidateQueries();
    // The refetch is in flight: previous rows stay, no skeleton, no zero.
    await waitFor(() => expect(harness.cyclesCalls).toBe(2));
    expect(screen.getByTestId("cycles-list-body")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-list-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("cycles-zero-state")).not.toBeInTheDocument();
    await settled;
    expect(screen.getByTestId("cycles-list-body")).toBeInTheDocument();
  });
});
