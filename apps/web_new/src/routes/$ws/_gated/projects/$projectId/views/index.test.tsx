// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views tab home (parity/specs/views.md §3, VIEW-001). The route
// plumbs its params into the views-owned list screen; the screen suite
// owns the states. Reads are stubbed so the route renders without a
// server.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the route plumbing.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../../../../core/api/client.js";
import { makeMember, makeView } from "../../../../../../features/views/testing.js";
import { Route } from "./index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function projectRow() {
  return {
    id: PROJECT_ID,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
  };
}

beforeEach(() => {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/`)) return jsonResponse(url, 200, projectRow());
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, { [PROJECT_ID]: 20 });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/views/`)) {
      return jsonResponse(url, 200, [makeView({ name: "Triage" })]);
    }
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, [makeMember()]);
    if (url.endsWith("/api/users/me/workspaces/")) {
      return jsonResponse(url, 200, [
        { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
      ]);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
  vi.restoreAllMocks();
});

describe("views tab route", () => {
  it("mounts the views-owned list screen for the route params", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID });
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Component />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    expect(screen.getByTestId("project-views-list")).toHaveTextContent("Triage");
    expect(screen.getByRole("button", { name: "Add view" })).toBeInTheDocument();
    expect(document.title).toBe("Website relaunch - Views");
  });
});
