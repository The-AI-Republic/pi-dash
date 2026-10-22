// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-ins for the router: navigation is TanStack's own tested behavior.
// What this suite owns is the screen composition and its reads.
const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => routerHarness.navigate,
}));

// The create dialog's filter builder reads the session; the screen never
// opens the dialog in the suites above, so these stand-ins only serve the
// create-dialog wiring tests below.
const sessionHarness = vi.hoisted(() => ({ useMe: vi.fn(), usePermissions: vi.fn() }));

vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  useMe: sessionHarness.useMe,
  usePermissions: sessionHarness.usePermissions,
}));

// The screen preloads the dialog's filter chunk on mount; every other
// barrel entry stays real so the dialog tests below load the real builder.
const issuesBarrel = vi.hoisted(() => ({ preloadRichFilterBuilder: vi.fn() }));

vi.mock("../../issues/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../issues/index.js")>()),
  preloadRichFilterBuilder: issuesBarrel.preloadRichFilterBuilder,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { jsonResponse, makeMember, makeView, VIEWS_OWNER_ID } from "../testing.js";
import { ProjectViewsListScreen } from "./ProjectViewsListScreen.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

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

function workspaceRow() {
  return { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 3, role: 20 };
}

interface Harness {
  project: Record<string, unknown>;
  projectStatus: number;
  roles: Record<string, number>;
  rolesStatus: number;
  views: ReturnType<typeof makeView>[];
  viewsStatus: number;
  viewsDelayMs: number;
  projectDelayMs: number;
  viewsCalls: number;
  members: ReturnType<typeof makeMember>[];
  workspaces: ReturnType<typeof workspaceRow>[];
}

let harness: Harness;

function resetHarness(): Harness {
  return {
    project: projectRow(),
    projectStatus: 200,
    roles: { [PROJECT_ID]: 20 },
    rolesStatus: 200,
    views: [],
    viewsStatus: 200,
    viewsDelayMs: 0,
    projectDelayMs: 0,
    viewsCalls: 0,
    members: [makeMember(VIEWS_OWNER_ID)],
    workspaces: [workspaceRow()],
  };
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  harness = resetHarness();
  routerHarness.navigate.mockReset();
  sessionHarness.useMe.mockReturnValue({ data: { id: VIEWS_OWNER_ID } });
  sessionHarness.usePermissions.mockReturnValue({ isAdmin: false, isLoading: false });
  issuesBarrel.preloadRichFilterBuilder.mockClear();
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/`)) {
      if (harness.projectDelayMs > 0) await sleep(harness.projectDelayMs);
      return jsonResponse(url, harness.projectStatus, harness.project);
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, harness.rolesStatus, harness.roles);
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/views/`)) {
      harness.viewsCalls += 1;
      if (harness.viewsDelayMs > 0) await sleep(harness.viewsDelayMs);
      return jsonResponse(url, harness.viewsStatus, harness.views);
    }
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, harness.members);
    if (url.endsWith("/api/users/me/workspaces/")) return jsonResponse(url, 200, harness.workspaces);
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
});

function renderScreen(onCreateView?: () => void): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ProjectViewsListScreen workspaceSlug="acme" projectId={PROJECT_ID} onCreateView={onCreateView} />
    </QueryClientProvider>
  );
}

describe("ProjectViewsListScreen", () => {
  it("lists every saved view under the breadcrumb trail with the tab title", async () => {
    const older = makeView({
      id: "223e4567-e89b-12d3-a456-426614174011",
      name: "V1 Alpha",
      updated_at: "2026-01-01T00:00:00Z",
    });
    const newer = makeView({
      id: "223e4567-e89b-12d3-a456-426614174012",
      name: "V1 Beta",
      updated_at: "2026-03-01T00:00:00Z",
    });
    harness.views = [older, newer];
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(trail).toHaveTextContent("Acme");
    expect(trail).toHaveTextContent("Website relaunch");
    const crumbs = trail.querySelectorAll("li");
    expect(crumbs[crumbs.length - 1]).toHaveTextContent("Views");
    expect(document.title).toBe("Website relaunch - Views");
    expect(screen.getByRole("button", { name: "Add view" })).toBeEnabled();
    const rows = screen.getAllByTestId("project-views-row");
    expect(rows).toHaveLength(2);
    // Default order is updated-at descending, not the fetched order.
    expect(rows[0]).toHaveTextContent("V1 Beta");
    expect(rows[1]).toHaveTextContent("V1 Alpha");
    expect(harness.viewsCalls).toBe(1);
  });

  it("threads the signed-in owner's standing into the row actions", async () => {
    harness.views = [makeView({ name: "V1 Star", owned_by: VIEWS_OWNER_ID })];
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    // Role 20 with ownership: the star shows and the menu offers edit.
    expect(screen.getByTestId("project-views-row-star")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Actions for V1 Star" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem").length).toBeGreaterThan(0));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit",
      "Open in new tab",
      "Copy link",
      "Delete",
    ]);
  });

  it("degrades row actions while the viewer and standing stay unresolved", async () => {
    harness.views = [makeView({ name: "V1 Pending", owned_by: VIEWS_OWNER_ID })];
    harness.rolesStatus = 500;
    harness.roles = { detail: "boom" } as unknown as Record<string, number>;
    sessionHarness.useMe.mockReturnValue({ data: undefined });
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    // No viewer id and no role: the row renders with no star and the
    // menu offers only the entries open to every role.
    expect(screen.queryByTestId("project-views-row-star")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Actions for V1 Pending" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem").length).toBeGreaterThan(0));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link"]);
  });

  it("falls back to the workspace slug when the workspace row is missing", async () => {
    harness.workspaces = [];
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("acme");
  });

  it("gates the list without fetching it while the feature is off", async () => {
    harness.project = projectRow({ issue_views_view: false });
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-gate")).toBeInTheDocument());
    expect(screen.getByRole("heading", { name: /Views are not enabled for the project\./ })).toBeInTheDocument();
    expect(harness.viewsCalls).toBe(0);
    expect(screen.queryByRole("button", { name: "Add view" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-header")).not.toBeInTheDocument();
    expect(document.title).toBe("Website relaunch - Views");
  });

  it("navigates an admin to the views feature leaf from the gate", async () => {
    harness.project = projectRow({ issue_views_view: false });
    renderScreen();
    await waitFor(() => expect(screen.getByRole("button", { name: "Manage features" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Manage features" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/settings/projects/${PROJECT_ID}/features/views` });
  });

  it("disables the shortcut for members and guests", async () => {
    harness.project = projectRow({ issue_views_view: false });
    for (const role of [15, 5]) {
      harness.roles = { [PROJECT_ID]: role };
      const { unmount } = render(
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <ProjectViewsListScreen workspaceSlug="acme" projectId={PROJECT_ID} />
        </QueryClientProvider>
      );
      await waitFor(() => expect(screen.getByRole("button", { name: "Manage features" })).toBeDisabled());
      unmount();
    }
  });

  it("fails the shortcut closed when the standing read errors", async () => {
    harness.project = projectRow({ issue_views_view: false });
    harness.rolesStatus = 500;
    harness.roles = { detail: "boom" } as unknown as Record<string, number>;
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-gate")).toBeInTheDocument());
    await waitFor(() => expect(screen.getByRole("button", { name: "Manage features" })).toBeDisabled());
  });

  it("shows only the skeleton while the list loads, then swaps in the rows", async () => {
    harness.views = [makeView({ name: "V8 Row" })];
    harness.viewsDelayMs = 50;
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-skeleton")).toBeInTheDocument());
    expect(screen.queryByTestId("project-views-list")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-zero")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-no-match")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    expect(screen.queryByTestId("project-views-skeleton")).not.toBeInTheDocument();
    expect(screen.getByTestId("project-views-list")).toHaveTextContent("V8 Row");
  });

  it("shows only the skeleton while the project row loads", async () => {
    harness.projectDelayMs = 50;
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-skeleton")).toBeInTheDocument());
    expect(screen.queryByTestId("project-views-header")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
  });

  it("shows the onboarding state with an enabled shortcut on an empty list", async () => {
    const onCreateView = vi.fn();
    renderScreen(onCreateView);
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
    expect(screen.getByRole("heading", { name: /Save custom views for your project/ })).toBeInTheDocument();
    const create = screen.getByRole("button", { name: "Create view" });
    expect(create).toBeEnabled();
    fireEvent.click(create);
    expect(onCreateView).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Add view" }));
    expect(onCreateView).toHaveBeenCalledTimes(2);
  });

  it("retries a failed list read from the error state", async () => {
    harness.viewsStatus = 500;
    harness.views = { detail: "boom" } as unknown as ReturnType<typeof makeView>[];
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-error")).toBeInTheDocument());
    harness.viewsStatus = 200;
    harness.views = [makeView({ name: "Recovered" })];
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    expect(screen.getByTestId("project-views-list")).toHaveTextContent("Recovered");
  });

  it("retries a failed project read from the error state", async () => {
    harness.projectStatus = 500;
    harness.project = { detail: "boom" };
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-error")).toBeInTheDocument());
    expect(screen.queryByTestId("project-views-header")).not.toBeInTheDocument();
    harness.projectStatus = 200;
    harness.project = projectRow();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
  });

  it("opens the create dialog from the header Add action by default (VIEW-003)", async () => {
    const user = userEvent.setup();
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
    expect(screen.queryByRole("heading", { name: "Create View" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add view" }));
    expect(await screen.findByRole("heading", { name: "Create View" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Create View" })).not.toBeInTheDocument());
  });

  it("preloads the dialog's filter chunk while the list loads", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
    expect(issuesBarrel.preloadRichFilterBuilder).toHaveBeenCalledTimes(1);
  });

  it("opens the same dialog from the zero state with the project's feature flags", async () => {
    const user = userEvent.setup();
    harness.project = projectRow({ cycle_view: false, module_view: false });
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-zero")).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Create view" }));
    expect(await screen.findByRole("heading", { name: "Create View" })).toBeInTheDocument();
    await user.click(screen.getByTestId("rich-filter-add"));
    expect(await screen.findByRole("option", { name: "Target date" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Cycle" })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Module" })).not.toBeInTheDocument();
  });
});
