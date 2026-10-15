// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { RouterContext } from "../../../__root.js";
import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import {
  clearCommands,
  clearPalettePages,
  findCommand,
  findPalettePage,
  listCommands,
} from "../../../../shared/commands/index.js";
import { Route } from "./$issueKey.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function projectRow() {
  return {
    id: PROJECT_ID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: UUID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

function issueRow() {
  return {
    id: ISSUE_ID,
    name: "Fix login",
    state_id: UUID,
    sort_order: 1.5,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: null,
    target_date: null,
    sequence_id: 1,
    project_id: PROJECT_ID,
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

function detailsRow(overrides: Record<string, unknown> = {}) {
  return {
    ...projectRow(),
    description: "",
    description_text: null,
    description_html: null,
    default_assignee: null,
    emoji: null,
    icon_prop: null,
    intake_view: true,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    members_can_edit_states: true,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: UUID,
    archive_in: 0,
    close_in: 0,
    default_state: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: {},
    deleted_at: null,
    ...overrides,
  };
}

function meRow() {
  return {
    id: UUID,
    email: "ada@example.com",
    username: "ada",
    first_name: "Ada",
    last_name: "L",
    display_name: "Ada L",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2024-01-02T03:04:05Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "",
    last_login_time: null,
  };
}

const STATE_ROW = {
  id: "423e4567-e89b-12d3-a456-426614174003",
  project_id: PROJECT_ID,
  workspace_id: UUID,
  name: "Done",
  color: "#00ff00",
  group: "completed",
  default: false,
  description: "",
  sequence: 2,
};

function userPropertiesPayload() {
  return {
    id: UUID,
    workspace: UUID,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: 10,
    navigation_control_preference: "TABBED",
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

function renderBrowse(
  known: boolean,
  isEpic?: boolean,
  extras: { details?: "member" | "archived"; me?: boolean; archivedIssue?: boolean; estimate?: string | null } = {}
) {
  const calls: string[] = [];
  const apiClient = createAppClient("https://api.test", async (url: string, init: { method: string }) => {
    calls.push(`${init.method} ${url}`);
    let status = 404;
    let body = "{}";
    if (url.endsWith("/auth/get-csrf-token/")) {
      status = 200;
      body = JSON.stringify({ csrf_token: "csrf" });
    } else if (init.method !== "GET" && url.includes(`/issues/${ISSUE_ID}/`)) {
      status = 204;
      body = "";
    } else if (url.includes("/search/")) {
      status = 200;
      body = JSON.stringify({
        results: {
          issue: known
            ? [
                {
                  name: "Fix login",
                  id: ISSUE_ID,
                  sequence_id: 1,
                  project__identifier: "WEB",
                  project_id: PROJECT_ID,
                  workspace__slug: "acme",
                },
              ]
            : [],
        },
      });
    } else if (url.includes("/project-roles/")) {
      status = 200;
      body = JSON.stringify({ [PROJECT_ID]: 20 });
    } else if (url.includes(`/projects/${PROJECT_ID}/user-properties/`)) {
      status = 200;
      body = JSON.stringify({
        ...userPropertiesPayload(),
        project: PROJECT_ID,
        preferences: { navigation: { default_tab: "work_items", hide_in_more_menu: [] } },
        sort_order: 1,
      });
    } else if (url.endsWith("/user-properties/")) {
      status = 200;
      body = JSON.stringify(userPropertiesPayload());
    } else if (url.endsWith("/projects/")) {
      status = 200;
      body = JSON.stringify([projectRow()]);
    } else if (url.includes(`/issues/${ISSUE_ID}/`)) {
      status = 200;
      const row = isEpic === undefined ? issueRow() : { ...issueRow(), is_epic: isEpic };
      body = JSON.stringify(extras.archivedIssue === true ? { ...row, archived_at: "2024-03-04T05:06:07Z" } : row);
    } else if (url.endsWith("/projects/details/")) {
      if (extras.details !== undefined) {
        status = 200;
        const overrides: Record<string, unknown> =
          extras.details === "archived" ? { archived_at: "2024-03-04T05:06:07Z" } : {};
        if (extras.estimate !== undefined) overrides["estimate"] = extras.estimate;
        body = JSON.stringify([detailsRow(overrides)]);
      }
    } else if (url.endsWith("/api/users/me/")) {
      if (extras.me === true) {
        status = 200;
        body = JSON.stringify(meRow());
      }
    } else if (url.includes("/states/")) {
      status = 200;
      body = JSON.stringify([STATE_ROW]);
    }
    return { status, url, headers: { get: () => null }, text: async () => body };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const component = Route.options.component;
  if (!component) throw new Error("missing component");
  const rootRoute = createRootRouteWithContext<RouterContext>()();
  const wsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => <Outlet />,
  });
  const gatedRoute = createRoute({
    getParentRoute: () => wsRoute,
    id: "_gated",
    component: () => <Outlet />,
  });
  const browseRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "browse/$issueKey",
    component,
  });
  const awayRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "$",
    component: () => <p>navigated away</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([gatedRoute.addChildren([browseRoute]), awayRoute])]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({ initialEntries: ["/acme/browse/WEB-1"] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { router, calls };
}

afterEach(() => {
  resetAppClient();
});

describe("browse route", () => {
  it("renders the detail under the project strip with tracking highlighted", async () => {
    renderBrowse(true);
    await screen.findByTestId("project-tab-strip");
    expect(screen.getByTestId("project-switcher-trigger")).toHaveTextContent("Web");
    expect(screen.getByRole("link", { name: "Work Items" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
  });

  it("renders unknown keys without the strip", async () => {
    renderBrowse(false);
    await screen.findByRole("heading", { name: "No work item matches this address" });
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("highlights nothing for an epic while no epic tab renders", async () => {
    renderBrowse(true, true);
    await screen.findByTestId("project-tab-strip");
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Work Items" })).not.toHaveAttribute("aria-current");
    expect(document.querySelectorAll('[aria-current="page"]')).toHaveLength(0);
  });

  it("registers the work-item contextual group for the resolved issue", async () => {
    renderBrowse(true);
    await screen.findByTestId("project-tab-strip");
    // The stub answers no project details, so only the copy entries show.
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.copy-id`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Copy ID");
    expect(titles).toContain("Copy title");
    expect(titles).toContain("Copy URL");
    expect(titles).not.toContain("Change state");
    clearCommands();
    clearPalettePages();
  });

  it("offers the editing entries once the project details resolve", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    await screen.findByTestId("project-tab-strip");
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.state`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Change state");
    expect(titles).toContain("Change priority");
    expect(titles).toContain("Assign to");
    expect(titles).toContain("Assign to me");
    expect(titles).toContain("Change estimate");
    expect(titles).toContain("Add to cycle");
    expect(titles).toContain("Add to modules");
    expect(titles).toContain("Delete");
    clearCommands();
    clearPalettePages();
  });

  it("hides only the estimate picker when the project sets no estimate system", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, estimate: null });
    await screen.findByTestId("project-tab-strip");
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.state`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Change state");
    expect(titles).not.toContain("Change estimate");
    clearCommands();
    clearPalettePages();
  });

  it("hides editing entries for archived issues", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, archivedIssue: true });
    await screen.findByTestId("project-tab-strip");
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.copy-id`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Copy ID");
    expect(titles).toContain("Copy title");
    expect(titles).toContain("Copy URL");
    expect(titles).not.toContain("Change state");
    expect(titles).not.toContain("Delete");
    clearCommands();
    clearPalettePages();
  });

  it("hides editing entries for archived projects", async () => {
    renderBrowse(true, undefined, { details: "archived", me: true });
    await screen.findByTestId("project-tab-strip");
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.copy-id`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Copy ID");
    expect(titles).not.toContain("Change state");
    expect(titles).not.toContain("Delete");
    clearCommands();
    clearPalettePages();
  });

  it("refreshes the detail after a state pick", async () => {
    const { calls } = renderBrowse(true, undefined, { details: "member", me: true });
    await screen.findByTestId("project-tab-strip");
    await waitFor(() => expect(findPalettePage(`context.work-item.${ISSUE_ID}.state`)).toBeDefined());
    await waitFor(() =>
      expect(findPalettePage(`context.work-item.${ISSUE_ID}.state`)!.usePageState("").options).toHaveLength(1)
    );
    await act(async () => {
      findPalettePage(`context.work-item.${ISSUE_ID}.state`)!.usePageState("").options[0]!.run();
    });
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("PATCH") && call.includes(`/issues/${ISSUE_ID}/`))).toBe(true)
    );
    clearCommands();
    clearPalettePages();
  });

  it("leaves for the issue list after a confirmed delete", async () => {
    const user = userEvent.setup();
    const { router } = renderBrowse(true, undefined, { details: "member", me: true });
    await screen.findByTestId("project-tab-strip");
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.delete`)).toBeDefined());
    await act(async () => {
      await findCommand(`context.work-item.${ISSUE_ID}.delete`)!.run();
    });
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByText("navigated away")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe(`/acme/projects/${PROJECT_ID}/issues`);
    clearCommands();
    clearPalettePages();
  });
});
