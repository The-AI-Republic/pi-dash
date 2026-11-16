// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Transport } from "@pidash/api-client";
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

import { createClient } from "@pidash/api-client";

import { LegacyResolver } from "./LegacyResolver.js";

const UUID = "11111111-1111-4111-8111-111111111111";

function settings(overrides: Record<string, unknown> = {}): Record<string, unknown> {
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
    ...overrides,
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

async function renderResolver(transport: Transport) {
  vi.stubGlobal("scrollTo", vi.fn());
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const root = createRootRoute();
  const legacy = createRoute({
    getParentRoute: () => root,
    path: "/spaces/$workspaceSlug/$projectId/",
    component: () => (
      <LegacyResolver workspaceSlug="acme" projectId={UUID} search={{ board: "kanban", peekId: "p1" }} />
    ),
  });
  const board = createRoute({
    getParentRoute: () => root,
    path: "/spaces/issues/$anchor/",
    component: () => <p>board route</p>,
  });
  const notFound = createRoute({
    getParentRoute: () => root,
    path: "/404",
    component: () => <p>generic not found</p>,
  });
  const router = createRouter({
    routeTree: root.addChildren([legacy, board, notFound]),
    history: createMemoryHistory({ initialEntries: [`/spaces/acme/${UUID}/?board=kanban&peekId=p1`] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={runtime.queryClient as QueryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return router;
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("LegacyResolver", () => {
  it("holds a full-viewport spinner while resolving", async () => {
    await renderResolver(() => new Promise(() => undefined));
    expect(await screen.findByTestId("space-legacy-loading")).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Resolving board" })).toBeInTheDocument();
  });

  it("forwards a published project with only board and peekId", async () => {
    const router = await renderResolver(() => Promise.resolve(reply(200, settings())));
    expect(await screen.findByText("board route")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1/");
    expect(router.state.location.search).toMatchObject({ board: "kanban", peekId: "p1" });
  });

  it("sends an unpublished project to the fixed generic not-found address", async () => {
    const router = await renderResolver(() => Promise.resolve(reply(404, { detail: "missing" })));
    expect(await screen.findByText("generic not found")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/404");
  });

  it("sends a non-project payload to the generic address", async () => {
    const router = await renderResolver(() => Promise.resolve(reply(200, settings({ entity_name: "page" }))));
    expect(await screen.findByText("generic not found")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/404");
  });
});
