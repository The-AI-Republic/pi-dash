// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issues beforeLoad denial tolerance (AUTH-024 seam) plus the thin screen
// composition. The project gate owns 403/404/409 from the issues and
// preferences reads; anything else still throws to the load error. The
// screen suite proves the route mounts the layout root with the project
// title, its rows and the interim empty states; layouts themselves belong
// to the issues area.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { ApiError, createClient } from "@pidash/api-client";
import type { IssueListItem, IssueListResponse, Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../../core/api/client.js";
import {
  issuesQueryOptions,
  prefsQueryOptions,
  type IssueSearch,
  type PrefsRecord,
} from "../../../../../../features/issues/index.js";
import { projectsQueryOptions } from "../../../../../../features/projects/index.js";
import { clearCommands } from "../../../../../../shared/commands/index.js";
import { Route } from "./index.js";

const PARAMS = { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" };
const ENTITY = { scope: "project", projectId: PARAMS.projectId } as const;

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

describe("issues denial tolerance", () => {
  it("loads through when the issues and preferences reads succeed", async () => {
    const apiClient = failingClient(200);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(issuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, {} as never).queryKey, {
      grouped_by: null,
    } as never);
    queryClient.setQueryData(prefsQueryOptions(apiClient, PARAMS.ws, ENTITY).queryKey, {
      display_filters: { layout: "list" },
      display_properties: {},
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

  it("lets a preferences denial through while the issues read succeeds", async () => {
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: async (url) => ({
        status: url.includes("user-properties") ? 403 : 200,
        url,
        headers: { get: () => null },
        text: async () => "{}",
      }),
    });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(issuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, {} as never).queryKey, {
      grouped_by: null,
    } as never);
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    await expect(
      beforeLoad({ context: { queryClient, apiClient }, params: PARAMS, search: {} } as never)
    ).resolves.toBeUndefined();
  });

  it("throws anything else to the load error", async () => {
    const outcome = await load(failingClient(500));
    expect("thrown" in outcome && outcome.thrown).toBeInstanceOf(ApiError);
  });

  it("throws transport failures to the load error", async () => {
    const offline = createClient({
      baseUrl: "https://api.test",
      transport: () => {
        throw new Error("offline");
      },
    });
    const outcome = await load(offline);
    expect("thrown" in outcome && outcome.thrown).toBeInstanceOf(Error);
  });
});

describe("issues pending placeholder", () => {
  it("shows the list skeleton while the prefetch runs", () => {
    const pending = Route.options.pendingComponent;
    if (!pending) throw new Error("missing pendingComponent");
    render(React.createElement(pending));
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
  });
});

const ISSUE_UUID = "123e4567-e89b-12d3-a456-426614174000";

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
    workspace: ISSUE_UUID,
    cycle_view: false,
    issue_views_view: false,
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

function issueRow(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE_UUID,
    name: "Fix login",
    state_id: PARAMS.projectId,
    sort_order: 1,
    completed_at: null,
    priority: "high",
    sequence_id: 42,
    project_id: PARAMS.projectId,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function prefsSeed(): PrefsRecord {
  return {
    filters: {},
    display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
    display_properties: {},
    rich_filters: {},
  };
}

// Seeded render: the loaders own fetching, so the transport throws if a
// query misses its seed instead of hanging the suite on a timeout.
function renderScreen(search: IssueSearch, totalCount: number, rows: IssueListItem[]): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: PARAMS.ws, projectId: PARAMS.projectId });
  vi.spyOn(Route, "useSearch").mockReturnValue(search);
  const apiClient = createAppClient("https://api.test", (() => {
    throw new Error("unexpected network fetch in a seeded render");
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  vi.spyOn(Route, "useRouteContext").mockReturnValue({ queryClient, apiClient } as never);
  queryClient.setQueryData(projectsQueryOptions(apiClient, PARAMS.ws).queryKey, [projectSeed()]);
  queryClient.setQueryData(issuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, search).queryKey, {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: totalCount,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: rows.length,
    total_results: totalCount,
    results: rows,
  } satisfies IssueListResponse);
  queryClient.setQueryData(prefsQueryOptions(apiClient, PARAMS.ws, ENTITY).queryKey, prefsSeed());
  const Component = Route.options.component;
  if (!Component) throw new Error("issues route has no component");
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
  clearCommands();
  vi.restoreAllMocks();
});

describe("project issues screen", () => {
  it("composes the layout root with the project title and its rows", () => {
    const now = new Date().toISOString();
    renderScreen({}, 1, [issueRow({ created_at: now, updated_at: now })]);

    expect(screen.getByRole("heading", { name: "Website" })).toBeInTheDocument();
    expect(screen.getByTestId("layout-switcher-segmented")).toBeInTheDocument();
    const row = screen.getByLabelText("WEB-42");
    expect(row).toHaveTextContent("Fix login");
    expect(row).toHaveClass("h-11");
    expect(row).not.toHaveClass("h-8");
  });

  it("mounts the Display control in the desktop and mobile headers", () => {
    renderScreen({}, 1, [issueRow()]);

    const triggers = screen.getAllByTestId("display-options-trigger");
    expect(triggers).toHaveLength(2);
    for (const trigger of triggers) {
      expect(trigger).toHaveAccessibleName("Display");
    }
  });

  it("passes the compact density through from the search param", () => {
    const now = new Date().toISOString();
    renderScreen({ layout: "compact" }, 1, [issueRow({ created_at: now, updated_at: now })]);

    const row = screen.getByLabelText("WEB-42");
    expect(row).toHaveClass("h-8");
    expect(row).not.toHaveClass("h-11");
  });

  it("renders the interim empty state when the project has no issues", () => {
    renderScreen({}, 0, []);

    expect(screen.getByRole("heading", { name: "No issues yet" })).toBeInTheDocument();
    expect(screen.getByText("New issues in this project will appear here.")).toBeInTheDocument();
    expect(screen.queryByTestId("issues-fresh-strip")).not.toBeInTheDocument();
  });

  it("renders the interim no-match state when filters exclude every row", () => {
    renderScreen({}, 2, []);

    expect(screen.getByRole("heading", { name: "No matching issues" })).toBeInTheDocument();
    expect(screen.getByText("No issues match the current filters.")).toBeInTheDocument();
  });
});
