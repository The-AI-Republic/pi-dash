// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));

import { LegacyResolver } from "./LegacyResolver.js";
import { SpaceProgressProvider, SpaceRouteProgress } from "./SpaceRouteProgress.js";

const UUID = "11111111-1111-4111-8111-111111111111";

function settings(): Record<string, unknown> {
  return {
    id: UUID,
    project_details: null,
    workspace_detail: { name: "Acme", slug: "acme", id: UUID, logo_url: null },
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    deleted_at: null,
    created_by: null,
    updated_by: null,
    workspace: UUID,
    project: UUID,
    entity_identifier: "PROJ",
    entity_name: "project",
    anchor: "anchor-1",
    is_comments_enabled: false,
    is_reactions_enabled: false,
    is_votes_enabled: false,
    view_props: {},
    is_disabled: false,
    intake: null,
  };
}

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

function renderHeldResolver(transport: Transport): void {
  vi.stubGlobal("scrollTo", vi.fn());
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const root = createRootRoute();
  const legacy = createRoute({
    getParentRoute: () => root,
    path: "/spaces/$workspaceSlug/$projectId/",
    component: () => <LegacyResolver workspaceSlug="acme" projectId={UUID} search={{ board: "kanban" }} />,
  });
  const board = createRoute({
    getParentRoute: () => root,
    path: "/spaces/issues/$anchor/",
    component: () => <p>board route</p>,
  });
  const router = createRouter({
    routeTree: root.addChildren([legacy, board]),
    history: createMemoryHistory({ initialEntries: [`/spaces/acme/${UUID}/`] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={runtime.queryClient as QueryClient}>
      <SpaceProgressProvider>
        <SpaceRouteProgress delayMs={10} />
        <RouterProvider router={router} />
      </SpaceProgressProvider>
    </QueryClientProvider>
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("LegacyResolver progress", () => {
  it("raises the progress bar while a slow resolution is in flight, then removes it on settle", async () => {
    let release!: (value: Awaited<ReturnType<Transport>>) => void;
    const held = new Promise<Awaited<ReturnType<Transport>>>((resolve) => {
      release = resolve;
    });
    renderHeldResolver(() => held);
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
    expect(await screen.findByTestId("space-route-progress")).toBeInTheDocument();
    release(reply(200, settings()));
    expect(await screen.findByText("board route")).toBeInTheDocument();
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
  });

  it("shows no bar for an instant resolution", async () => {
    renderHeldResolver(() => Promise.resolve(reply(200, settings())));
    expect(await screen.findByText("board route")).toBeInTheDocument();
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
  });
});
