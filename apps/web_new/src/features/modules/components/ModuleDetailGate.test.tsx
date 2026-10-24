// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail gate renders the gone-away view for a missing, deleted or
// archived-via-live-address module and the page otherwise (MOD-029).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { ModuleDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { MODULES_MODULE_ID, MODULES_PROJECT_ID, jsonResponse } from "../testing.js";
import { ModuleDetailGate } from "./ModuleDetailGate.js";

function detailFixture(overrides: Partial<ModuleDetail> = {}): ModuleDetail {
  return {
    id: MODULES_MODULE_ID,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: MODULES_PROJECT_ID,
    name: "Delivery",
    description: "",
    start_date: "2026-09-01",
    target_date: "2026-09-30",
    status: "in-progress",
    lead_id: null,
    member_ids: [],
    sort_order: 0,
    is_favorite: false,
    total_issues: 2,
    completed_issues: 1,
    cancelled_issues: 0,
    started_issues: 0,
    unstarted_issues: 0,
    backlog_issues: 0,
    total_estimate_points: 2,
    completed_estimate_points: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    link_module: [],
    sub_issues: null,
    distribution: { assignees: [], labels: [], completion_chart: {} },
    estimate_distribution: { assignees: [], labels: [], completion_chart: {} },
    ...overrides,
  };
}

const harness = vi.hoisted(() => ({ status: 200, body: null as unknown, calls: 0 }));

beforeEach(() => {
  harness.status = 200;
  harness.body = detailFixture();
  harness.calls = 0;
  routerHarness.navigate.mockReset();
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}/`)) {
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
      <ModuleDetailGate workspaceSlug="acme" projectId={MODULES_PROJECT_ID} moduleId={MODULES_MODULE_ID}>
        <div data-testid="module-page">page</div>
      </ModuleDetailGate>
    </QueryClientProvider>
  );
}

describe("ModuleDetailGate", () => {
  it("renders the page for a live module, including while loading", async () => {
    renderGate();
    expect(await screen.findByTestId("module-page")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-gone-away")).not.toBeInTheDocument();
  });

  it("gone-aways a missing id and routes back to the list", async () => {
    harness.status = 404;
    harness.body = { detail: "missing" };
    renderGate();
    expect(await screen.findByTestId("modules-gone-away")).toBeInTheDocument();
    expect(screen.queryByTestId("module-page")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back to modules" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/projects/${MODULES_PROJECT_ID}/modules/` });
  });

  it("gone-aways an archived record served on the live address", async () => {
    harness.body = detailFixture({ archived_at: "2026-03-01T00:00:00Z" });
    renderGate();
    expect(await screen.findByTestId("modules-gone-away")).toBeInTheDocument();
    expect(screen.queryByTestId("module-page")).not.toBeInTheDocument();
  });

  it("keeps other failures on the page", async () => {
    harness.status = 500;
    harness.body = { detail: "boom" };
    renderGate();
    expect(await screen.findByTestId("module-page")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-gone-away")).not.toBeInTheDocument();
  });
});
