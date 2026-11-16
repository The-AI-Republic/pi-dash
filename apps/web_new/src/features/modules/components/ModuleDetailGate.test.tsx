// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail gate renders the gone-away view for a missing, deleted or
// archived-via-live-address module and the page otherwise (MOD-029).

import { QueryClient, QueryClientProvider, useSuspenseQuery } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ModuleDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
}));

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { MODULES_MODULE_ID, MODULES_PROJECT_ID, jsonResponse } from "../testing.js";
import { ModuleDetailGate } from "./ModuleDetailGate.js";
import { moduleDetailQueryOptions } from "./sidebar/moduleSidebarQueries.js";

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

const harness = vi.hoisted(() => ({
  status: 200,
  body: null as unknown,
  calls: 0,
  membershipRole: 15 as number | null,
  joinStatus: 201,
  joinPosts: [] as Array<{ url: string; body: string }>,
}));

beforeEach(() => {
  harness.status = 200;
  harness.body = detailFixture();
  harness.calls = 0;
  harness.membershipRole = 15;
  harness.joinStatus = 201;
  harness.joinPosts = [];
  routerHarness.navigate.mockReset();
  createAppClient("https://api.test", (async (url: string, init?: RequestInit) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}/`)) {
      harness.calls += 1;
      return jsonResponse(url, harness.status, harness.body);
    }
    if (url.endsWith("/workspace-members/me/")) {
      const role = harness.membershipRole;
      const row =
        role === null
          ? null
          : {
              id: "123e4567-e89b-12d3-a456-426614174000",
              workspace: "123e4567-e89b-12d3-a456-426614174000",
              member: "123e4567-e89b-12d3-a456-426614174000",
              role,
              company_role: null,
              view_props: {},
              default_props: {},
              issue_props: {},
              is_active: true,
              getting_started_checklist: {},
              tips: {},
              explored_features: {},
              draft_issue_count: 0,
              created_by: null,
              updated_by: null,
              created_at: "2024-01-02T03:04:05Z",
              updated_at: "2024-02-03T04:05:06Z",
            };
      return jsonResponse(url, 200, row);
    }
    if (url.endsWith("/projects/invitations/")) {
      harness.joinPosts.push({ url, body: String(init?.body ?? "") });
      return jsonResponse(url, harness.joinStatus, { message: "joined" });
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

function makeClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: true } } });
}

function renderGate(client: QueryClient = makeClient()): void {
  render(
    <QueryClientProvider client={client}>
      <ModuleDetailGate workspaceSlug="acme" projectId={MODULES_PROJECT_ID} moduleId={MODULES_MODULE_ID}>
        <div data-testid="module-page">page</div>
      </ModuleDetailGate>
    </QueryClientProvider>
  );
}

/**
 * The screen's own read: the detail page suspends on the same key the
 * gate decides on. Rendering it unbounded is what threw the 404 past the
 * gate into the route error boundary live (NEWFRONT-1026).
 */
function SuspendingScreen({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement {
  const { data } = useSuspenseQuery(
    moduleDetailQueryOptions(getAppClient(), workspaceSlug, MODULES_PROJECT_ID, MODULES_MODULE_ID)
  );
  return <div data-testid="module-page">{data.name}</div>;
}

function renderGateWithScreen(client: QueryClient): void {
  render(
    <QueryClientProvider client={client}>
      <ModuleDetailGate workspaceSlug="acme" projectId={MODULES_PROJECT_ID} moduleId={MODULES_MODULE_ID}>
        <React.Suspense fallback={<div data-testid="screen-loading" />}>
          <SuspendingScreen workspaceSlug="acme" />
        </React.Suspense>
      </ModuleDetailGate>
    </QueryClientProvider>
  );
}

/** Minimal stand-in for the route's error boundary around the screen. */
class ScreenErrorBoundary extends React.Component<
  { children: React.ReactNode; fallback: (error: unknown) => React.ReactNode },
  { error: unknown }
> {
  override state: { error: unknown } = { error: null };

  static getDerivedStateFromError(error: unknown): { error: unknown } {
    return { error };
  }

  override render(): React.ReactNode {
    return this.state.error === null ? this.props.children : this.props.fallback(this.state.error);
  }
}

function renderGateWithRouteBoundary(client: QueryClient): void {
  render(
    <QueryClientProvider client={client}>
      <ModuleDetailGate workspaceSlug="acme" projectId={MODULES_PROJECT_ID} moduleId={MODULES_MODULE_ID}>
        <ScreenErrorBoundary fallback={(error) => <div data-testid="route-error">{String(error)}</div>}>
          <React.Suspense fallback={<div data-testid="screen-loading" />}>
            <SuspendingScreen workspaceSlug="acme" />
          </React.Suspense>
        </ScreenErrorBoundary>
      </ModuleDetailGate>
    </QueryClientProvider>
  );
}

describe("ModuleDetailGate", () => {
  it("holds a skeleton, never the page, while the read is pending", () => {
    renderGate();
    expect(screen.getByTestId("module-detail-gate-pending")).toBeInTheDocument();
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
    expect(screen.queryByTestId("module-page")).not.toBeInTheDocument();
  });

  it("renders the page for a live module once the read settles", async () => {
    renderGate();
    expect(await screen.findByTestId("module-page")).toBeInTheDocument();
    expect(screen.queryByTestId("module-detail-gate-pending")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-gone-away")).not.toBeInTheDocument();
  });

  it("shows the gone-away view with a suspended screen on the same key when a cached 404 refetches (NEWFRONT-1026)", async () => {
    harness.status = 404;
    harness.body = { error: "Module not found" };
    const client = makeClient();
    // The route loader reads the detail before the first render, so the
    // 404 is already in the cache when the gate and the screen mount.
    await client
      .ensureQueryData(moduleDetailQueryOptions(getAppClient(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID))
      .catch(() => undefined);
    renderGateWithScreen(client);
    expect(await screen.findByTestId("modules-gone-away")).toBeInTheDocument();
    expect(screen.queryByTestId("module-page")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back to modules" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/projects/${MODULES_PROJECT_ID}/modules/` });
  });

  it("renders the suspended screen for a cached live module after the gate settles", async () => {
    const client = makeClient();
    await client.ensureQueryData(
      moduleDetailQueryOptions(getAppClient(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID)
    );
    renderGateWithScreen(client);
    expect(await screen.findByTestId("module-page")).toHaveTextContent("Delivery");
    expect(screen.queryByTestId("modules-gone-away")).not.toBeInTheDocument();
  });

  it("shows the gone-away view for an archived record behind the gate, without mounting the screen", async () => {
    harness.body = detailFixture({ archived_at: "2026-03-01T00:00:00Z" });
    const client = makeClient();
    await client.ensureQueryData(
      moduleDetailQueryOptions(getAppClient(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID)
    );
    renderGateWithScreen(client);
    expect(await screen.findByTestId("modules-gone-away")).toBeInTheDocument();
    expect(screen.queryByTestId("module-page")).not.toBeInTheDocument();
  });

  it("keeps a 500 on the page so the route error view still owns it", async () => {
    harness.status = 500;
    harness.body = { error: "boom" };
    const client = makeClient();
    await client
      .ensureQueryData(moduleDetailQueryOptions(getAppClient(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID))
      .catch(() => undefined);
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    try {
      renderGateWithRouteBoundary(client);
      expect(await screen.findByTestId("route-error")).toHaveTextContent("boom");
    } finally {
      consoleError.mockRestore();
    }
    expect(screen.queryByTestId("modules-gone-away")).not.toBeInTheDocument();
  });

  it("shows the access wall for a cached refusal, without mounting the screen", async () => {
    harness.status = 403;
    harness.body = { error: "You do not have permission" };
    const client = makeClient();
    await client
      .ensureQueryData(moduleDetailQueryOptions(getAppClient(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID))
      .catch(() => undefined);
    renderGateWithScreen(client);
    expect(await screen.findByTestId("project-access-wall")).toHaveAttribute("data-wall-kind", "no-access");
    expect(screen.queryByTestId("module-page")).not.toBeInTheDocument();
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

  it("renders the shared join wall for a project the reader has not joined (409)", async () => {
    harness.status = 409;
    harness.body = { error: "You are not a member of this project" };
    renderGate();
    const wall = await screen.findByTestId("project-access-wall");
    expect(wall).toHaveAttribute("data-wall-kind", "join");
    expect(screen.getByRole("heading", { name: "You haven't joined this project yet" })).toBeInTheDocument();
    expect(screen.queryByTestId("modules-gone-away")).not.toBeInTheDocument();
    expect(screen.queryByTestId("module-page")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Join project" }));
    await waitFor(() => expect(harness.joinPosts).toHaveLength(1));
    expect(harness.joinPosts[0]?.url).toBe(`https://api.test/api/users/me/workspaces/acme/projects/invitations/`);
    expect(harness.joinPosts[0]?.body).toContain(MODULES_PROJECT_ID);
  });

  it("offers the join wall to a workspace admin facing a private project (403)", async () => {
    harness.status = 403;
    harness.body = { error: "You do not have permission" };
    harness.membershipRole = 20;
    renderGate();
    // The verdict settles once the membership read lands: a wall drawn
    // before it never upgrades a refusal into an offer on its own.
    await waitFor(() => expect(screen.getByTestId("project-access-wall")).toHaveAttribute("data-wall-kind", "join"));
    expect(await screen.findByRole("button", { name: "Join project" })).toBeEnabled();
  });

  it("shows the no-access wall to a refused non-admin (403), never the missing state", async () => {
    harness.status = 403;
    harness.body = { error: "You do not have permission" };
    harness.membershipRole = 15;
    renderGate();
    const wall = await screen.findByTestId("project-access-wall");
    expect(wall).toHaveAttribute("data-wall-kind", "no-access");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-gone-away")).not.toBeInTheDocument();
    expect(screen.queryByTestId("module-page")).not.toBeInTheDocument();
  });

  it("treats a missing membership as non-admin for the wall split", async () => {
    harness.status = 403;
    harness.body = { error: "You do not have permission" };
    harness.membershipRole = null;
    renderGate();
    expect(await screen.findByTestId("project-access-wall")).toHaveAttribute("data-wall-kind", "no-access");
  });

  it("keeps other failures on the page", async () => {
    harness.status = 500;
    harness.body = { detail: "boom" };
    renderGate();
    expect(await screen.findByTestId("module-page")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-gone-away")).not.toBeInTheDocument();
  });
});
