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
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ProjectList,
  RecentVisit,
  RecentVisitList,
  StateList,
  Transport,
  TransportInit,
} from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/index.js";
import { projectStatesKeys } from "../api/projectStates.js";
import { recentsKeys } from "../api/recents.js";
import type { RecentFilter } from "../lib/recentRows.js";
import { RecentsWidget } from "./RecentsWidget.js";

const CSRF_URL = "https://api.test/auth/get-csrf-token/";

interface SeenRequest {
  url: string;
  method: string;
  body: string | undefined;
}

function stubTransport(seen: SeenRequest[], pending = false): Transport {
  return async (url: string, init: TransportInit) => {
    seen.push({ url, method: init.method, body: init.body });
    if (url === CSRF_URL) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (pending) return new Promise<never>(() => {});
    if (init.method === "PATCH") {
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }
    return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
  };
}

function testClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function buildRouter(projects: ProjectList | undefined): ReturnType<typeof createRouter> {
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const widgetRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: () => <RecentsWidget workspaceSlug="acme" projects={projects} />,
  });
  const splashRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "$",
    component: () => null,
  });
  return createRouter({
    routeTree: rootRoute.addChildren([widgetRoute, splashRoute]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
}

async function renderWidget(
  queryClient: QueryClient,
  projects: ProjectList | undefined = []
): Promise<ReturnType<typeof createRouter>> {
  const router = buildRouter(projects);
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  await screen.findByRole("region", { name: "Recents" });
  return router;
}

function makeVisit(id: string, entity_name: string, entity_data: Record<string, unknown> | null): RecentVisit {
  return {
    id,
    entity_name,
    entity_identifier: `${id}-entity`,
    entity_data,
    visited_at: new Date().toISOString(),
  };
}

function issuePayload(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: "issue-1",
    name: "Parity first issue",
    project_id: "project-1",
    sequence_id: 12,
    project_identifier: "ABC",
    state: "state-1",
    priority: "high",
    assignees: [],
    ...overrides,
  };
}

function projectEntry(overrides: Partial<ProjectList[number]> & { id: string }): ProjectList[number] {
  return {
    name: "Website",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: null,
    intake_count: 0,
    archived_at: null,
    workspace: "workspace-1",
    cycle_view: false,
    issue_views_view: true,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function stateEntry(overrides: Partial<StateList[number]> & { id: string }): StateList[number] {
  return {
    project_id: "project-1",
    workspace_id: "workspace-1",
    name: "In progress",
    color: "#ff0000",
    group: "started",
    default: false,
    description: "",
    sequence: 1,
    ...overrides,
  };
}

function seedFeed(queryClient: QueryClient, filter: RecentFilter, feed: RecentVisitList): void {
  queryClient.setQueryData(recentsKeys.list("acme", filter), feed);
}

let consoleError: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  resetAppClient();
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("RecentsWidget", () => {
  it("renders the feed in visit order with resolved context", async () => {
    const seen: SeenRequest[] = [];
    createAppClient("https://api.test", stubTransport(seen));
    const queryClient = testClient();
    seedFeed(queryClient, "all", [
      makeVisit("visit-1", "issue", issuePayload()),
      makeVisit(
        "visit-2",
        "issue",
        issuePayload({
          id: "issue-2",
          name: "Parity second issue",
          project_identifier: null,
          project_id: "project-2",
          sequence_id: 7,
          state: "state-9",
        })
      ),
      makeVisit("visit-3", "project", { id: "project-1", name: "Website", identifier: "WEB" }),
    ]);
    queryClient.setQueryData(projectStatesKeys.list("acme", "project-1"), [stateEntry({ id: "state-1" })]);
    queryClient.setQueryData(projectStatesKeys.list("acme", "project-2"), []);
    await renderWidget(queryClient, [projectEntry({ id: "project-2", identifier: "WEB" })]);

    const section = screen.getByRole("region", { name: "Recents" });
    expect(section).toHaveAttribute("id", "home-recents");
    expect(section).toHaveAttribute("data-widget-title", "Recents");
    const rows = screen.getAllByTestId("recent-row");
    expect(rows.map((row) => within(row).getByText(/Parity|Website/).textContent)).toEqual([
      "Parity first issue",
      "Parity second issue",
      "Website",
    ]);
    expect(screen.getByText("ABC-12")).toBeInTheDocument();
    expect(screen.getByText("WEB-7")).toBeInTheDocument();
    expect(screen.getByText("In progress")).toBeInTheDocument();
    expect(screen.getAllByText("just now").length).toBeGreaterThan(0);
    expect(screen.getByRole("combobox", { name: "Filter recents" })).toHaveValue("all");
    expect(screen.queryByRole("button", { name: /show all/i })).not.toBeInTheDocument();
  });

  it("shows the skeleton shape while the feed resolves", async () => {
    createAppClient("https://api.test", stubTransport([], true));
    await renderWidget(testClient());
    const skeleton = screen.getByTestId("recents-skeleton");
    expect(skeleton).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("recent-row")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Recents" })).toBeInTheDocument();
  });

  it("refetches per filter key and shows per-type empty art", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    const second = makeVisit("visit-2", "issue", issuePayload({ id: "issue-2", name: "Parity second issue" }));
    seedFeed(queryClient, "all", [makeVisit("visit-1", "issue", issuePayload())]);
    seedFeed(queryClient, "issue", [second]);
    seedFeed(queryClient, "page", []);
    seedFeed(queryClient, "project", [
      makeVisit("visit-3", "project", { id: "project-1", name: "Website", identifier: "WEB" }),
    ]);
    await renderWidget(queryClient);

    const filter = () => screen.getByRole("combobox", { name: "Filter recents" });
    const pick = (key: string) => {
      fireEvent.change(filter(), { target: { value: key } });
    };

    pick("issue");
    expect(filter()).toHaveValue("issue");
    expect(screen.getByText("Parity second issue")).toBeInTheDocument();
    expect(screen.queryByText("Parity first issue")).not.toBeInTheDocument();

    pick("page");
    expect(screen.getByTestId("recents-empty")).toHaveTextContent("No recently viewed pages");
    expect(screen.queryByTestId("recent-row")).not.toBeInTheDocument();

    pick("project");
    expect(screen.getByText("Website")).toBeInTheDocument();

    pick("all");
    expect(screen.getByText("Parity first issue")).toBeInTheDocument();
  });

  it("falls back to All on an unknown filter value", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(queryClient, "all", [makeVisit("visit-1", "issue", issuePayload())]);
    await renderWidget(queryClient);

    const filter = screen.getByRole("combobox", { name: "Filter recents" });
    fireEvent.change(filter, { target: { value: "bogus" } });
    expect(filter).toHaveValue("all");
    expect(screen.getByText("Parity first issue")).toBeInTheDocument();
  });

  it("shows the All empty art on an empty feed", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(queryClient, "all", []);
    await renderWidget(queryClient);
    expect(screen.getByTestId("recents-empty")).toHaveTextContent("No recent activity");
  });

  it("shows each filtered slice its own empty art", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(queryClient, "all", []);
    seedFeed(queryClient, "issue", []);
    seedFeed(queryClient, "project", []);
    await renderWidget(queryClient);

    const filter = () => screen.getByRole("combobox", { name: "Filter recents" });
    fireEvent.change(filter(), { target: { value: "issue" } });
    expect(screen.getByTestId("recents-empty")).toHaveTextContent("No recently viewed work items");
    fireEvent.change(filter(), { target: { value: "project" } });
    expect(screen.getByTestId("recents-empty")).toHaveTextContent("No recently viewed projects");
  });

  it("caps long feeds behind the expander", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(
      queryClient,
      "all",
      Array.from({ length: 6 }, (_, index) =>
        makeVisit(`visit-${index}`, "issue", issuePayload({ id: `issue-${index}`, name: `Issue ${index}` }))
      )
    );
    await renderWidget(queryClient);

    const list = screen.getByRole("list", { name: "Recently visited" });
    expect(list.parentElement).toHaveClass("max-h-[26rem]");
    fireEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(screen.getByRole("button", { name: "Show less" })).toBeInTheDocument();
    expect(list.parentElement).not.toHaveClass("max-h-[26rem]");
    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    expect(screen.getByRole("button", { name: "Show all" })).toBeInTheDocument();
  });

  it("resets the expander when the filter changes", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(
      queryClient,
      "all",
      Array.from({ length: 6 }, (_, index) =>
        makeVisit(`visit-${index}`, "issue", issuePayload({ id: `issue-${index}`, name: `Issue ${index}` }))
      )
    );
    seedFeed(queryClient, "issue", [makeVisit("visit-9", "issue", issuePayload({ name: "Lone issue" }))]);
    await renderWidget(queryClient);

    const filter = () => screen.getByRole("combobox", { name: "Filter recents" });
    fireEvent.click(screen.getByRole("button", { name: "Show all" }));
    fireEvent.change(filter(), { target: { value: "issue" } });
    expect(screen.getByText("Lone issue")).toBeInTheDocument();
    fireEvent.change(filter(), { target: { value: "all" } });
    expect(screen.getByRole("button", { name: "Show all" })).toBeInTheDocument();
  });

  it("keeps prior rows and offers no retry when the fetch fails", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    await renderWidget(queryClient);

    await waitFor(() => expect(consoleError).toHaveBeenCalled());
    expect(consoleError).toHaveBeenCalledWith("[web_new] home fetch failed: recents/acme/all", expect.anything());
    expect(screen.getByRole("region", { name: "Recents" })).toBeInTheDocument();
    expect(screen.queryByTestId("recent-row")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry|try again/i })).not.toBeInTheDocument();
  });

  it("keeps painted rows when a refetch fails", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(queryClient, "all", [makeVisit("visit-1", "issue", issuePayload())]);
    queryClient.setQueryData(projectStatesKeys.list("acme", "project-1"), []);
    await renderWidget(queryClient);
    expect(screen.getByText("Parity first issue")).toBeInTheDocument();

    await queryClient.invalidateQueries({ queryKey: recentsKeys.list("acme", "all") });
    await waitFor(() => expect(consoleError).toHaveBeenCalled());
    expect(screen.getByText("Parity first issue")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry|try again/i })).not.toBeInTheDocument();
  });

  it("previews work items in place without leaving home", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(queryClient, "all", [makeVisit("visit-1", "issue", issuePayload())]);
    queryClient.setQueryData(projectStatesKeys.list("acme", "project-1"), [stateEntry({ id: "state-1" })]);
    const router = await renderWidget(queryClient);

    await user.click(screen.getByTestId("recent-row"));
    const panel = await screen.findByTestId("issue-preview");
    expect(document.getElementById(FULL_SCREEN_PORTAL_ID)).toContainElement(panel);
    expect(screen.getByRole("textbox", { name: "Work item title" })).toHaveValue("Parity first issue");
    expect(within(panel).getByText("ABC-12")).toBeInTheDocument();
    expect(within(panel).getByText(/in progress/i)).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/");

    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
  });

  it("commits preview renames through the work-item update", async () => {
    const user = userEvent.setup();
    const seen: SeenRequest[] = [];
    createAppClient("https://api.test", stubTransport(seen));
    const queryClient = testClient();
    seedFeed(queryClient, "all", [makeVisit("visit-1", "issue", issuePayload())]);
    queryClient.setQueryData(projectStatesKeys.list("acme", "project-1"), []);
    await renderWidget(queryClient);

    await user.click(screen.getByTestId("recent-row"));
    const field = await screen.findByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Renamed issue{Enter}");
    await waitFor(() => expect(seen.some((request) => request.method === "PATCH")).toBe(true));
    const patch = seen.find((request) => request.method === "PATCH");
    expect(patch?.url).toBe("https://api.test/api/workspaces/acme/projects/project-1/issues/issue-1/");
    expect(patch?.body).toBe(JSON.stringify({ name: "Renamed issue" }));
  });

  it("keeps preview edits local without a project", async () => {
    const user = userEvent.setup();
    const seen: SeenRequest[] = [];
    createAppClient("https://api.test", stubTransport(seen));
    const queryClient = testClient();
    seedFeed(queryClient, "all", [
      makeVisit("visit-1", "issue", issuePayload({ project_id: null, project_identifier: null })),
    ]);
    await renderWidget(queryClient);

    await user.click(screen.getByTestId("recent-row"));
    const field = await screen.findByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Local draft{Enter}");
    expect(field).toHaveValue("Local draft");
    expect(seen.some((request) => request.method === "PATCH")).toBe(false);
  });

  it("navigates page rows to their scope", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(queryClient, "all", [
      makeVisit("visit-1", "page", { id: "page-1", name: "Launch notes", project_id: "project-1" }),
    ]);
    const router = await renderWidget(queryClient);

    await user.click(screen.getByTestId("recent-row"));
    await waitFor(() => expect(router.state.location.pathname).toBe("/acme/projects/project-1/pages/page-1"));
  });

  it("navigates project rows to their work list", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = testClient();
    seedFeed(queryClient, "all", [
      makeVisit("visit-1", "project", { id: "project-1", name: "Website", identifier: "WEB" }),
    ]);
    const router = await renderWidget(queryClient);

    await user.click(screen.getByTestId("recent-row"));
    await waitFor(() => expect(router.state.location.pathname).toBe("/acme/projects/project-1/issues"));
  });
});
