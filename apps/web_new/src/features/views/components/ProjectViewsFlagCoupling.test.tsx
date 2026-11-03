// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views-side coupling to the project features switch
// (parity/specs/views.md §12, VIEW-041). The switch itself belongs to
// project settings (PSET-043); this suite proves the list reacts to its
// effect in both directions: flipping the cached flag off gates the
// list and its creation entries, flipping it back on restores them.
// The flip writes the same cache key the switch's fold writes, so the
// screen cannot tell this probe from the real settings write.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router: navigation is TanStack's own tested behavior.
// What this suite owns is the gate/list swap on flag flips.
vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { authKeys } from "../../auth/index.js";
import { jsonResponse, makeMember, makeView, VIEWS_OWNER_ID } from "../testing.js";
import { ProjectViewsListScreen } from "./ProjectViewsListScreen.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function projectRow(overrides: Record<string, unknown> = {}) {
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
    ...overrides,
  };
}

const harness = vi.hoisted(() => ({ views: [] as ReturnType<typeof makeView>[], viewsCalls: 0 }));

let queryClient: QueryClient;

function renderScreen(): void {
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectViewsListScreen workspaceSlug="acme" projectId={PROJECT_ID} />
    </QueryClientProvider>
  );
}

/** Flip the cached switch the way the settings fold does. */
function flipSwitch(on: boolean): void {
  const key = authKeys.project("acme", PROJECT_ID);
  const row = queryClient.getQueryData<Record<string, unknown>>(key);
  queryClient.setQueryData(key, { ...row, issue_views_view: on });
}

beforeEach(() => {
  harness.views = [];
  harness.viewsCalls = 0;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/`)) {
      return jsonResponse(url, 200, projectRow());
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, { [PROJECT_ID]: 20 });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/views/`)) {
      harness.viewsCalls += 1;
      return jsonResponse(url, 200, harness.views);
    }
    if (url.endsWith("/api/workspaces/acme/members/")) {
      return jsonResponse(url, 200, [makeMember(VIEWS_OWNER_ID)]);
    }
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
});

describe("project views flag coupling", () => {
  it("gates the rows and the Add entry when the switch flips off, restores them when it flips on", async () => {
    harness.views = [makeView({ name: "Triage" })];
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    expect(screen.getByTestId("project-views-list")).toHaveTextContent("Triage");
    expect(screen.getByRole("button", { name: "Add view" })).toBeInTheDocument();

    flipSwitch(false);
    await waitFor(() => expect(screen.getByTestId("project-views-gate")).toBeInTheDocument());
    expect(screen.getByRole("heading", { name: /Views are not enabled for the project\./ })).toBeInTheDocument();
    expect(screen.queryByTestId("project-views-list")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add view" })).not.toBeInTheDocument();

    flipSwitch(true);
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    expect(screen.getByTestId("project-views-list")).toHaveTextContent("Triage");
    expect(screen.getByRole("button", { name: "Add view" })).toBeInTheDocument();
    expect(screen.queryByTestId("project-views-gate")).not.toBeInTheDocument();
    // The restored list reads the warm cache: no second fetch.
    expect(harness.viewsCalls).toBe(1);
  });

  it("restores the zero-state creation entry when the switch flips back on", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Create view" })).toBeInTheDocument();

    flipSwitch(false);
    await waitFor(() => expect(screen.getByTestId("project-views-gate")).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Create view" })).not.toBeInTheDocument();

    flipSwitch(true);
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Create view" })).toBeInTheDocument();
  });
});
