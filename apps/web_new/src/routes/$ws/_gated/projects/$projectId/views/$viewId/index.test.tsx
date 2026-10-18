// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved project view route (ISS-104). Proves the view record gates the
// load like the issues screen, its stored expression drives both the list
// query and the filter row, and the row's actions are wired to the record
// rather than the personal preference store.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, createClient, type IssueListResponse, type Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../../../core/api/client.js";
import { sessionKeys } from "../../../../../../../core/session/queries.js";
import { authKeys } from "../../../../../../../features/auth/queries.js";
import { issuesQueryOptions, prefsQueryOptions, type PrefsRecord } from "../../../../../../../features/issues/index.js";
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

function renderScreen(role: number | null = 20, view: ReturnType<typeof viewRecord> = viewRecord(), rows = 0): void {
  vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
  vi.spyOn(Route, "useSearch").mockReturnValue({});
  const apiClient = createAppClient("https://api.test", (() => {
    throw new Error("unexpected network fetch in a seeded render");
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  vi.spyOn(Route, "useRouteContext").mockReturnValue({ queryClient, apiClient } as never);
  queryClient.setQueryData(
    projectViewQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, PARAMS.viewId).queryKey,
    view
  );
  queryClient.setQueryData(projectsQueryOptions(apiClient, PARAMS.ws).queryKey, [projectSeed()]);
  queryClient.setQueryData(
    issuesQueryOptions(
      apiClient,
      PARAMS.ws,
      PARAMS.projectId,
      {},
      EXPRESSION,
      typeof view.display_filters["order_by"] === "string" ? view.display_filters["order_by"] : "sort_order"
    ).queryKey,
    issueResponse(rows)
  );
  // Display writes still target the personal record; the render itself
  // uses the view's stored settings.
  queryClient.setQueryData(prefsQueryOptions(apiClient, PARAMS.ws, ENTITY).queryKey, prefsSeed());
  queryClient.setQueryData(sessionKeys.me, { id: USER });
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: USER, name: "Acme", slug: PARAMS.ws, logo_url: null, total_members: 1, role: role ?? 20 },
  ]);
  if (role !== null) queryClient.setQueryData(authKeys.projectRoles(PARAMS.ws), { [PARAMS.projectId]: role });
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
    renderScreen();
    expect(await screen.findByRole("heading", { name: "Unstarted work" })).toBeInTheDocument();
    expect(await screen.findByTestId("rich-filter-row")).toBeInTheDocument();
    expect(screen.getByTestId("rich-condition")).toHaveTextContent("State Group");
  });

  it("offers Save as for the owned view and Update view once the row changes", async () => {
    const user = userEvent.setup();
    renderScreen();
    expect(await screen.findByTestId("rich-save-view")).toHaveTextContent("Save as");
    const pill = await screen.findByTestId("rich-condition");

    await user.click(within(pill).getByRole("button", { name: /^is$/ }));
    await user.click(await screen.findByRole("option", { name: "is not" }));
    expect(await screen.findByRole("button", { name: "Update view" })).toBeInTheDocument();
  });

  it("opens in the layout the view stored and offers the timeline in the switcher", async () => {
    renderScreen(
      20,
      {
        ...viewRecord(),
        display_filters: { layout: "gantt_chart", order_by: "sort_order" } as Record<string, unknown>,
      },
      1
    );
    const switcher = await screen.findByTestId("layout-switcher-segmented");
    expect(switcher.querySelector('[data-layout="gantt_chart"]')).not.toBeNull();
    expect(await screen.findByTestId("gantt-layout")).toBeInTheDocument();
    expect(screen.queryByLabelText("WEB-1")).toBeNull();
  });
});
