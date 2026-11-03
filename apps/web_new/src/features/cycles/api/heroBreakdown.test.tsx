// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero breakdown clicks (parity/specs/cycles.md §15, CYC-041): the
// click merges its value into the stored rich filters — reading first
// so the row exists — refreshes the cached copy, and opens the
// filtered detail, peeking priority picks. A refused write reports and
// stays on the list.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({ notifyError: notify.notifyError }));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import { HERO_FILTER_FAILED_TITLE, useApplyBreakdownFilter } from "./heroBreakdown.js";

const WS = "acme";
const MEMBER = "123e4567-e89b-12d3-a456-426614174010";

let queryClient: QueryClient;
let router: ReturnType<typeof createRouter>;
let storedFilters: Record<string, unknown>;
let gets: number;
let patches: unknown[];
let patchStatus: number;
let rowExists: boolean;

const USER_ID = "123e4567-e89b-12d3-a456-426614174020";
const RECORD_ID = "123e4567-e89b-12d3-a456-426614174021";

function record(): Record<string, unknown> {
  return {
    id: RECORD_ID,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: CYCLES_PROJECT_ID,
    cycle: CYCLES_CYCLE_ID,
    user: USER_ID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: storedFilters,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

function transport(url: string, init: { method?: string; body?: string }): Promise<unknown> {
  const method = init.method ?? "GET";
  const respond = (status: number, body: unknown): unknown => ({
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
  if (url.includes("/user-properties/") && method === "GET") {
    gets += 1;
    rowExists = true;
    return Promise.resolve(respond(200, record()));
  }
  if (url.includes("/user-properties/") && method === "PATCH") {
    patches.push(init.body === undefined ? null : JSON.parse(init.body));
    // No row exists until the first GET creates it (NEWFRONT-266): a
    // PATCH-first write 404s like the old handler's.
    if (!rowExists) return Promise.resolve(respond(404, { detail: "The required object does not exist." }));
    if (patchStatus !== 200) return Promise.resolve(respond(patchStatus, { detail: "boom" }));
    const body = JSON.parse(init.body ?? "{}") as { rich_filters?: Record<string, unknown> };
    storedFilters = body.rich_filters ?? {};
    return Promise.resolve(respond(200, record()));
  }
  return Promise.resolve(respond(200, []));
}

function Host({ peek }: { peek: string | undefined }): React.ReactElement {
  const { apply, pending } = useApplyBreakdownFilter({
    workspaceSlug: WS,
    projectId: CYCLES_PROJECT_ID,
    cycleId: CYCLES_CYCLE_ID,
  });
  return (
    <button
      type="button"
      data-testid="entry"
      data-pending={pending}
      onClick={() => {
        void apply("assignee_id", MEMBER, peek);
      }}
    >
      entry
    </button>
  );
}

async function renderHost(peek?: string): Promise<void> {
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const listRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws/projects/$projectId/cycles",
    component: () => <Host peek={peek} />,
  });
  const detailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws/projects/$projectId/cycles/$cycleId",
    component: () => <p>detail</p>,
  });
  router = createRouter({
    routeTree: rootRoute.addChildren([listRoute, detailRoute]),
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${CYCLES_PROJECT_ID}/cycles`] }),
    defaultPreload: false,
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  await screen.findByTestId("entry");
}

beforeEach(() => {
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  createAppClient("https://api.test", transport as never);
  storedFilters = {};
  gets = 0;
  patches = [];
  patchStatus = 200;
  rowExists = false;
  notify.notifyError.mockClear();
});

afterEach(() => {
  resetAppClient();
});

describe("useApplyBreakdownFilter", () => {
  it("reads first, merges the value, and opens the filtered detail", async () => {
    storedFilters = { state_group__in: ["started"] };
    await renderHost();
    fireEvent.click(screen.getByTestId("entry"));
    await waitFor(() => expect(patches).toHaveLength(1));
    // The GET lands before the PATCH so a missing row is created first.
    expect(gets).toBeGreaterThanOrEqual(1);
    expect(patches[0]).toEqual({
      rich_filters: { and: [{ state_group__in: "started" }, { assignee_id__in: MEMBER }] },
    });
    await waitFor(() => expect(router.state.location.pathname).toContain(CYCLES_CYCLE_ID));
    expect(router.state.location.search).toEqual({});
  });

  it("peeks the picked work item on priority clicks", async () => {
    await renderHost("issue-9");
    fireEvent.click(screen.getByTestId("entry"));
    await waitFor(() => expect(patches).toHaveLength(1));
    await waitFor(() => expect(router.state.location.pathname).toContain(CYCLES_CYCLE_ID));
    expect(router.state.location.search).toEqual({ peekIssueId: "issue-9" });
  });

  it("reports a refused write and stays on the list", async () => {
    patchStatus = 403;
    await renderHost();
    fireEvent.click(screen.getByTestId("entry"));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(HERO_FILTER_FAILED_TITLE));
    expect(router.state.location.pathname).not.toContain(CYCLES_CYCLE_ID);
    expect(patches).toHaveLength(1);
  });

  it("collapses synchronous double-clicks into one write", async () => {
    await renderHost();
    fireEvent.click(screen.getByTestId("entry"));
    fireEvent.click(screen.getByTestId("entry"));
    await waitFor(() => expect(patches).toHaveLength(1));
    await waitFor(() => expect(router.state.location.pathname).toContain(CYCLES_CYCLE_ID));
  });

  it("applies the filter on a first click before any properties row exists", async () => {
    await renderHost();
    fireEvent.click(screen.getByTestId("entry"));
    await waitFor(() => expect(patches).toHaveLength(1));
    // The read lands first and creates the row, so the PATCH that
    // 404s for PATCH-first writers succeeds here and the click
    // navigates with the filter applied.
    expect(gets).toBeGreaterThanOrEqual(1);
    expect(patches[0]).toEqual({
      rich_filters: { assignee_id__in: MEMBER },
    });
    await waitFor(() => expect(router.state.location.pathname).toContain(CYCLES_CYCLE_ID));
    expect(notify.notifyError).not.toHaveBeenCalled();
  });
});
