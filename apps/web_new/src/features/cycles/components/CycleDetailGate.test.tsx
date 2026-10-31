// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail gate renders the gone-away view for a missing, deleted or
// archived-via-live-address cycle and the page otherwise (CYC-013).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID, jsonResponse, makeCycleRow } from "../testing.js";
import { CycleDetailGate } from "./CycleDetailGate.js";

const harness = vi.hoisted(() => ({ status: 200, body: null as unknown, calls: 0 }));

beforeEach(() => {
  harness.status = 200;
  harness.body = makeCycleRow();
  harness.calls = 0;
  routerHarness.navigate.mockReset();
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}/`)) {
      harness.calls += 1;
      return jsonResponse(url, harness.status, harness.body);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

function renderGate(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CycleDetailGate workspaceSlug="acme" projectId={CYCLES_PROJECT_ID} cycleId={CYCLES_CYCLE_ID}>
        <div data-testid="cycle-page">page</div>
      </CycleDetailGate>
    </QueryClientProvider>
  );
}

describe("CycleDetailGate", () => {
  it("renders the page for a live cycle, including while loading", async () => {
    renderGate();
    expect(await screen.findByTestId("cycle-page")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-gone-away")).not.toBeInTheDocument();
  });

  it("gone-aways a missing id and routes back to the list", async () => {
    harness.status = 404;
    harness.body = { detail: "missing" };
    renderGate();
    expect(await screen.findByTestId("cycles-gone-away")).toBeInTheDocument();
    expect(screen.queryByTestId("cycle-page")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "View other cycles" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/projects/${CYCLES_PROJECT_ID}/cycles/` });
  });

  it("gone-aways an archived cycle read through a live address", async () => {
    harness.body = makeCycleRow({ archived_at: "2026-09-01T00:00:00.000Z" });
    renderGate();
    expect(await screen.findByTestId("cycles-gone-away")).toBeInTheDocument();
  });

  it("keeps the page on a non-404 failure", async () => {
    harness.status = 500;
    harness.body = { detail: "boom" };
    renderGate();
    expect(await screen.findByTestId("cycle-page")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-gone-away")).not.toBeInTheDocument();
  });
});
