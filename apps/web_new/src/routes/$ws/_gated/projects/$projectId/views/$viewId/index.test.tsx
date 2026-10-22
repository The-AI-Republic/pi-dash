// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved project view route (ISS-104). Proves the view record gates the
// load like the issues screen, its stored expression drives both the list
// query and the filter row, and the row's actions are wired to the record
// rather than the personal preference store.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, createClient, type IssueListResponse, type Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FULL_RUN_TEST_TIMEOUT_MS } from "../../../../../../../testing/testBudget.js";

import { createAppClient, resetAppClient } from "../../../../../../../core/api/client.js";
import { sessionKeys } from "../../../../../../../core/session/queries.js";
import { authKeys } from "../../../../../../../features/auth/queries.js";
import { issuesQueryOptions, prefsQueryOptions, type PrefsRecord } from "../../../../../../../features/issues/index.js";
import { loadIssueListLeaves } from "../../../../../../../features/issues/components/lazyLeaves.js";
import { projectViewQueryOptions } from "../../../../../../../features/issues/filters/rich/savedViewQuery.js";
import { projectsQueryOptions } from "../../../../../../../features/projects/index.js";
import { Route } from "./index.js";

const PARAMS = {
  ws: "acme",
  projectId: "223e4567-e89b-12d3-a456-426614174001",
  viewId: "323e4567-e89b-12d3-a456-426614174002",
};
const USER = "123e4567-e89b-12d3-a456-426614174000";
const ENTITY = { scope: "project", projectId: PARAMS.projectId } as const;

// Lazy layout and filter leaves resolve their chunk on first render. Under a
// full-suite run that import can outlast the 1 s query default, so every
// wait below that crosses a lazy boundary uses the measured full-run budget.
const LAZY_WAIT = { timeout: FULL_RUN_TEST_TIMEOUT_MS };
const EXPRESSION = { and: [{ state_group__in: "unstarted" }] };

function viewRecord() {
  return {
    id: PARAMS.viewId,
    workspace: USER,
    project: PARAMS.projectId,
    name: "Unstarted work",
    description: "",
    query: {},
    filters: {},
    display_filters: { layout: "list" } as Record<string, unknown>,
    display_properties: {},
    rich_filters: EXPRESSION,
    access: 0,
    sort_order: 1,
    logo_props: {},
    owned_by: USER,
    is_locked: false,
    archived_at: null,
    is_favorite: false,
    created_by: USER,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

function projectSeed(): Project {
  return {
    id: PARAMS.projectId,
    name: "Website",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: USER,
    cycle_view: false,
    issue_views_view: true,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

function prefsSeed(): PrefsRecord {
  return {
    filters: {},
    display_filters: { layout: "kanban", group_by: null, order_by: "-created_at" },
    display_properties: {},
    rich_filters: {},
  };
}

function issueResponse(rows = 0): IssueListResponse {
  const results: IssueListResponse["results"] =
    rows === 0
      ? []
      : [
          {
            id: "423e4567-e89b-12d3-a456-426614174003",
            name: "Draft onboarding",
            state_id: null,
            sort_order: 1,
            completed_at: null,
            priority: "none",
            sequence_id: 1,
            project_id: PARAMS.projectId,
            parent_id: null,
            cycle_id: null,
            module_ids: [],
            label_ids: [],
            assignee_ids: [],
            sub_issues_count: null,
            attachment_count: null,
            link_count: null,
            created_at: "2026-01-02T00:00:00Z",
            updated_at: "2026-01-03T00:00:00Z",
            created_by: null,
            updated_by: null,
            is_draft: false,
            archived_at: null,
          },
        ];
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: rows,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: rows,
    total_results: rows,
    results,
  };
}

function failingClient(status: number) {
  return createClient({
    baseUrl: "https://api.test",
    transport: async (url) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => "{}",
    }),
  });
}

async function load(apiClient: ReturnType<typeof failingClient>): Promise<{ ok: unknown } | { thrown: unknown }> {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return beforeLoad({ context: { queryClient, apiClient }, params: PARAMS, search: {} } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

async function renderScreen(
  role: number | null = 20,
  view: ReturnType<typeof viewRecord> = viewRecord(),
  rows = 0,
  transport?: (url: string, init: { method?: string; body?: string }) => Promise<unknown>,
  projects: Project[] = [projectSeed()],
  totalCount = rows
): Promise<void> {
  vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
  vi.spyOn(Route, "useSearch").mockReturnValue({});
  const apiClient = createAppClient(
    "https://api.test",
    (transport ??
      (() => {
        throw new Error("unexpected network fetch in a seeded render");
      })) as never
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  vi.spyOn(Route, "useRouteContext").mockReturnValue({ queryClient, apiClient } as never);
  queryClient.setQueryData(
    projectViewQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, PARAMS.viewId).queryKey,
    view
  );
  queryClient.setQueryData(projectsQueryOptions(apiClient, PARAMS.ws).queryKey, projects);
  queryClient.setQueryData(
    issuesQueryOptions(
      apiClient,
      PARAMS.ws,
      PARAMS.projectId,
      {},
      EXPRESSION,
      typeof view.display_filters["order_by"] === "string" ? view.display_filters["order_by"] : "sort_order"
    ).queryKey,
    { ...issueResponse(rows), total_count: totalCount, total_results: totalCount }
  );
  // Display writes still target the personal record; the render itself
  // uses the view's stored settings.
  queryClient.setQueryData(prefsQueryOptions(apiClient, PARAMS.ws, ENTITY).queryKey, prefsSeed());
  queryClient.setQueryData(sessionKeys.me, { id: USER });
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: USER, name: "Acme", slug: PARAMS.ws, logo_url: null, total_members: 1, role: role ?? 20 },
  ]);
  if (role !== null) queryClient.setQueryData(authKeys.projectRoles(PARAMS.ws), { [PARAMS.projectId]: role });
  await loadIssueListLeaves();
  const Component = Route.options.component;
  if (!Component) throw new Error("saved view route has no component");
  render(
    <QueryClientProvider client={queryClient}>
      <React.Suspense fallback={<p>loading</p>}>
        <Component />
      </React.Suspense>
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("saved view load", () => {
  it("loads through when the view and list reads succeed", async () => {
    const apiClient = failingClient(200);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(
      projectViewQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, PARAMS.viewId).queryKey,
      viewRecord() as never
    );
    queryClient.setQueryData(issuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, {}, EXPRESSION).queryKey, {
      grouped_by: null,
    } as never);
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    await expect(
      beforeLoad({ context: { queryClient, apiClient }, params: PARAMS, search: {} } as never)
    ).resolves.toBeUndefined();
  });

  it.each([403, 404, 409])("lets a %s through to the project gate", async (status) => {
    await expect(load(failingClient(status))).resolves.toEqual({ ok: undefined });
  });

  it("throws anything else to the load error", async () => {
    const outcome = await load(failingClient(500));
    expect("thrown" in outcome && outcome.thrown).toBeInstanceOf(ApiError);
  });

  it("asks for the list in the view's stored order, not the personal one", async () => {
    const seen: string[] = [];
    const apiClient = createAppClient("https://api.test", (async (url: string) => {
      seen.push(url);
      if (url.endsWith(`/views/${PARAMS.viewId}/`)) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () =>
            JSON.stringify({ ...viewRecord(), display_filters: { layout: "list", order_by: "-updated_at" } }),
        };
      }
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(issueResponse()),
      };
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    await beforeLoad({ context: { queryClient, apiClient }, params: PARAMS, search: {} } as never);
    expect(seen.some((url) => url.includes("issues/?") && url.includes("order_by=-updated_at"))).toBe(true);
    // The saved view never reads the personal preference record.
    expect(seen.some((url) => url.includes("user-properties"))).toBe(false);
  });
});

describe("saved view screen", () => {
  it("renders the view's name as the title and its stored expression in the row", async () => {
    await renderScreen();
    expect(await screen.findByRole("heading", { name: "Unstarted work" }, LAZY_WAIT)).toBeInTheDocument();
    expect(await screen.findByTestId("rich-filter-row", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.getByTestId("rich-condition")).toHaveTextContent("State Group");
  });

  it("offers Save as for the owned view and Update view once the row changes", async () => {
    const user = userEvent.setup();
    await renderScreen();
    expect(await screen.findByTestId("rich-save-view", undefined, LAZY_WAIT)).toHaveTextContent("Save as");
    const pill = await screen.findByTestId("rich-condition", undefined, LAZY_WAIT);

    await user.click(within(pill).getByRole("button", { name: /^is$/ }));
    await user.click(await screen.findByRole("option", { name: "is not" }, LAZY_WAIT));
    expect(await screen.findByRole("button", { name: "Update view" }, LAZY_WAIT)).toBeInTheDocument();
  });

  it("reads a default view record as untouched: no Update view on mount", async () => {
    await renderScreen(20, {
      ...viewRecord(),
      // The server's IssueView defaults, as a view nobody edited stores them.
      display_filters: {
        group_by: null,
        order_by: "-created_at",
        type: null,
        sub_issue: true,
        show_empty_groups: true,
        layout: "list",
        calendar_date_range: "",
      },
      display_properties: { priority: true, state: true, key: true },
    });
    expect(await screen.findByTestId("rich-save-view", undefined, LAZY_WAIT)).toHaveTextContent("Save as");
    expect(screen.queryByRole("button", { name: "Update view" })).not.toBeInTheDocument();
  });

  it("opens in the layout the view stored and offers the timeline in the switcher", async () => {
    await renderScreen(
      20,
      {
        ...viewRecord(),
        display_filters: { layout: "gantt_chart", order_by: "sort_order" } as Record<string, unknown>,
      },
      1
    );
    const switcher = await screen.findByTestId("layout-switcher-segmented", undefined, LAZY_WAIT);
    expect(switcher.querySelector('[data-layout="gantt_chart"]')).not.toBeNull();
    expect(await screen.findByTestId("gantt-layout", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.queryByLabelText("WEB-1")).toBeNull();
  });

  it("renders the view's timeline read-only for an archived project (ISS-222)", async () => {
    renderScreen(
      20,
      {
        ...viewRecord(),
        display_filters: { layout: "gantt_chart", order_by: "sort_order" } as Record<string, unknown>,
      },
      1,
      undefined,
      [{ ...projectSeed(), archived_at: "2026-01-01T00:00:00Z" }]
    );
    expect(await screen.findByTestId("gantt-layout", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("turns a layout switch in the header into an Update view carrying the view's settings", async () => {
    const user = userEvent.setup();
    const patches: Array<Record<string, unknown>> = [];
    await renderScreen(20, viewRecord(), 1, async (url, init) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      if (init.method !== "PATCH") {
        return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(issueResponse(1)) };
      }
      const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      patches.push(body);
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ ...viewRecord(), ...body }),
      };
    });
    const switcher = await screen.findByTestId("layout-switcher-segmented", undefined, LAZY_WAIT);
    const timeline = within(switcher).getByRole("button", { name: /timeline|gantt/i });
    await user.click(timeline);
    expect(await screen.findByTestId("gantt-layout", undefined, LAZY_WAIT)).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Update view" }, LAZY_WAIT));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]?.["display_filters"]).toMatchObject({ layout: "gantt_chart" });
    expect(Object.keys(patches[0] ?? {}).sort()).toEqual(["display_filters", "display_properties", "rich_filters"]);
  });
});

describe("saved view loader and route states", () => {
  it("prefetches the workspace project list the header reads", async () => {
    const apiClient = createAppClient("https://api.test", (async (url: string) => ({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify([projectSeed()]),
    })) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const loader = Route.options.loader as unknown as (input: unknown) => Promise<unknown>;
    if (!loader) throw new Error("missing loader");
    await expect(loader({ context: { queryClient, apiClient }, params: PARAMS } as never)).resolves.toBeUndefined();
    expect(queryClient.getQueryData(projectsQueryOptions(apiClient, PARAMS.ws).queryKey)).toHaveLength(1);
  });

  it("validates the search the same way the issues screen does", () => {
    const validateSearch = Route.options.validateSearch as unknown as (input: Record<string, unknown>) => unknown;
    expect(validateSearch({ layout: "compact" })).toMatchObject({ layout: "compact" });
    expect(validateSearch({ layout: "grid" })).toEqual({});
  });

  it("shows the list skeleton while the view loads", () => {
    const pending = Route.options.pendingComponent;
    if (!pending) throw new Error("missing pendingComponent");
    render(React.createElement(pending));
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
  });

  it("reports the load failure and retries from its error state", () => {
    const errorComponent = Route.options.errorComponent;
    if (!errorComponent) throw new Error("missing errorComponent");
    const reset = vi.fn();
    render(React.createElement(errorComponent, { error: new Error("Boom"), reset }));
    expect(screen.getByText("Boom")).toBeInTheDocument();
    screen.getByRole("button", { name: "Try again" }).click();
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it("falls back to fixed copy when the error is not an Error", () => {
    const errorComponent = Route.options.errorComponent;
    if (!errorComponent) throw new Error("missing errorComponent");
    render(React.createElement(errorComponent, { error: "nope", reset: () => {} }));
    expect(screen.getByText("Something went wrong.")).toBeInTheDocument();
  });

  it("asks for another project when the record points at one outside the list", async () => {
    await renderScreen(20, viewRecord(), 0, undefined, [{ ...projectSeed(), id: USER }]);
    expect(await screen.findByRole("heading", { name: "Project not found" }, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.getByText("Pick another project from the sidebar.")).toBeInTheDocument();
    expect(screen.queryByTestId("layout-switcher-segmented")).not.toBeInTheDocument();
  });

  it("offers the new-work-item entry when the view matches nothing (ISS-072)", async () => {
    await renderScreen(20, viewRecord(), 0, undefined, undefined, 0);
    expect(await screen.findByTestId("rich-filter-row", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /^View work items will appear here/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New work item" })).toBeEnabled();
  });

  it("opens the create dialog from the view's empty card (ISS-072)", async () => {
    // The dialog is a lazy chunk; it resolves to its own module under test.
    await renderScreen(20, viewRecord(), 0, undefined, undefined, 0);
    await userEvent.click(await screen.findByRole("button", { name: "New work item" }, LAZY_WAIT));
    expect(await screen.findByRole("dialog", undefined, LAZY_WAIT)).toBeInTheDocument();
  });

  it("keeps the fixed card when the view's expression excludes every row", async () => {
    await renderScreen(20, viewRecord(), 0, undefined, undefined, 3);
    expect(await screen.findByTestId("rich-filter-row", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /^View work items will appear here/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New work item" })).toBeEnabled();
  });

  it("disables the entry for a guest on this project even when another project takes work items (ISS-072)", async () => {
    // The entry follows the view's project standing, not the
    // workspace-wide creatable set: a guest here sees it dead.
    await renderScreen(20, viewRecord(), 0, undefined, [
      { ...projectSeed(), member_role: 5 },
      { ...projectSeed(), id: USER, identifier: "API", member_role: 20 },
    ]);
    expect(await screen.findByRole("button", { name: "New work item" }, LAZY_WAIT)).toBeDisabled();
  });
});
