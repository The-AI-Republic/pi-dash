// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { PEEK_MISSING_TITLE } from "./peekStates.js";
import { ResolvingIssuePeek } from "./ResolvingIssuePeek.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ROW_ISSUE = "323e4567-e89b-12d3-a456-426614174002";
const LITE_ISSUE = "623e4567-e89b-12d3-a456-426614174006";
const RAW_ISSUE = "723e4567-e89b-12d3-a456-426614174007";
const OTHER_PROJECT = "823e4567-e89b-12d3-a456-426614174008";

const ROWS = [{ id: ROW_ISSUE, project_id: PROJECT, sequence_id: 42 }];

function liteRow() {
  return {
    id: LITE_ISSUE,
    sequence_id: 9,
    name: "Lite child",
    description_html: "<p></p>",
    sort_order: 1,
    project_id: PROJECT,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    is_epic: false,
    is_intake: false,
    is_synced: false,
    archived_at: null,
  };
}

function detailRow(id: string, name: string) {
  return {
    id,
    name,
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: 1,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 0,
    attachment_count: null,
    link_count: 0,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p></p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

interface ResolveExtras {
  liteStatus?: number;
  holdLite?: boolean;
}

function renderResolving(peeked: string, options: { peekProjectId?: string; extras?: ResolveExtras } = {}) {
  const onClose = vi.fn();
  const reads: string[] = [];
  let releaseLite: () => void = () => {};
  const liteGate = options.extras?.holdLite === true ? new Promise<void>((resolve) => (releaseLite = resolve)) : null;
  createAppClient("https://api.test", (async (url: string, init: { method: string }) => {
    if (init.method === "GET") reads.push(url);
    const answer = (code: number, payload: string) => ({
      status: code,
      url,
      headers: { get: () => null },
      text: async () => payload,
    });
    if (url.endsWith("/auth/get-csrf-token/")) return answer(200, JSON.stringify({ csrf_token: "csrf" }));
    if (url.includes("/work-items/WEB-9/")) {
      if (liteGate !== null) await liteGate;
      const code = options.extras?.liteStatus ?? 200;
      if (code !== 200) return answer(code, "{}");
      return answer(200, JSON.stringify(liteRow()));
    }
    const detailMatch = /\/issues\/([^/]+)\/$/.exec(new URL(url).pathname);
    if (init.method === "GET" && detailMatch !== null) {
      const names: Record<string, string> = {
        [ROW_ISSUE]: "Row issue",
        [LITE_ISSUE]: "Lite issue",
        [RAW_ISSUE]: "Raw issue",
      };
      return answer(
        200,
        JSON.stringify(detailRow(detailMatch[1] as string, names[detailMatch[1] as string] ?? "Issue"))
      );
    }
    return answer(404, "{}");
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const issuesRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/issues",
    component: () => (
      <ResolvingIssuePeek
        workspaceSlug={SLUG}
        projectId={PROJECT}
        projectIdentifier="WEB"
        peeked={peeked}
        peekProjectId={options.peekProjectId}
        rows={ROWS}
        onClose={onClose}
      />
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([issuesRoute])]),
    history: createMemoryHistory({ initialEntries: [`/${SLUG}/projects/${PROJECT}/issues`] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { onClose, reads, releaseLite };
}

afterEach(() => {
  resetAppClient();
});

describe("ResolvingIssuePeek", () => {
  it("reads a held row without the lite lookup", async () => {
    const { reads } = renderResolving("WEB-42");
    expect(await screen.findByRole("heading", { name: "Row issue" })).toBeInTheDocument();
    expect(reads.some((url) => url.includes(`/issues/${ROW_ISSUE}/`))).toBe(true);
    expect(reads.some((url) => url.includes("/work-items/"))).toBe(false);
  });

  it("reads a raw id under the peeked project", async () => {
    const { reads } = renderResolving(RAW_ISSUE, { peekProjectId: OTHER_PROJECT });
    expect(await screen.findByRole("heading", { name: "Raw issue" })).toBeInTheDocument();
    expect(reads.some((url) => url.includes(`/projects/${OTHER_PROJECT}/issues/${RAW_ISSUE}/`))).toBe(true);
  });

  it("reads a raw id under the route project without an override", async () => {
    const { reads } = renderResolving(RAW_ISSUE);
    expect(await screen.findByRole("heading", { name: "Raw issue" })).toBeInTheDocument();
    expect(reads.some((url) => url.includes(`/projects/${PROJECT}/issues/${RAW_ISSUE}/`))).toBe(true);
  });

  it("resolves an unmatched key through the lite read", async () => {
    const { reads } = renderResolving("WEB-9");
    expect(await screen.findByRole("heading", { name: "Lite issue" })).toBeInTheDocument();
    expect(reads.some((url) => url.includes("/work-items/WEB-9/"))).toBe(true);
    expect(reads.some((url) => url.includes(`/issues/${LITE_ISSUE}/`))).toBe(true);
  });

  it("waits on the lite read with the shared skeleton", async () => {
    const { releaseLite } = renderResolving("WEB-9", { extras: { holdLite: true } });
    expect(await screen.findByRole("status", { name: "Loading work item" })).toBeInTheDocument();
    releaseLite();
    expect(await screen.findByRole("heading", { name: "Lite issue" })).toBeInTheDocument();
  });

  it("collapses an unknown key into the error panel", async () => {
    renderResolving("WEB-9", { extras: { liteStatus: 404 } });
    expect(await screen.findByRole("heading", { name: PEEK_MISSING_TITLE })).toBeInTheDocument();
  });

  it("collapses a failed lite read into the error panel", async () => {
    renderResolving("WEB-9", { extras: { liteStatus: 500 } });
    expect(await screen.findByRole("heading", { name: PEEK_MISSING_TITLE })).toBeInTheDocument();
  });

  it("closes from the resolving panel", async () => {
    const { onClose } = renderResolving("WEB-42");
    await screen.findByRole("heading", { name: "Row issue" });
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
