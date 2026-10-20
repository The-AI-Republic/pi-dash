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
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ApiError, createClient } from "@pidash/api-client";
import type { IssueListItem, IssueListResponse, Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { FULL_RUN_TEST_TIMEOUT_MS } from "../../../../../../testing/testBudget.js";

import { createAppClient, resetAppClient } from "../../../../../../core/api/client.js";
import {
  issuesQueryOptions,
  prefsQueryOptions,
  type IssueSearch,
  type PrefsRecord,
} from "../../../../../../features/issues/index.js";
import { listPageQueryOptions } from "../../../../../../features/issues/list/pages.js";
import { loadListLayout } from "../../../../../../features/issues/list/lazy.js";
import { loadProjectEmptyState } from "../../../../../../features/issues/emptyStates/lazy.js";
import { projectsQueryOptions } from "../../../../../../features/projects/index.js";
import { orderByOf } from "../../../../../../features/issues/prefs/prefsQuery.js";
import { clearCommands, listCommands } from "../../../../../../shared/commands/index.js";
import * as queryModule from "../../../../../../core/query/optimistic.js";
import * as shellModule from "../../../../../../shared/shell/index.js";
import { authKeys } from "../../../../../../features/auth/queries.js";
import { sessionKeys } from "../../../../../../core/session/queries.js";
import { sheetIssuesQueryOptions } from "../../../../../../features/issues/spreadsheet/data.js";
import { pickerCyclesQueryOptions } from "../../../../../../shared/pickers/reference.js";
import { Route } from "./index.js";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: ({ children }: { children?: React.ReactNode }) => <a href="#work-item">{children}</a>,
  };
});

const PARAMS = { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" };
const ENTITY = { scope: "project", projectId: PARAMS.projectId } as const;

// Lazy layout and filter leaves resolve their chunk on first render. Under a
// full-suite run that import can outlast the 1 s query default, so every
// wait below that crosses a lazy boundary uses the measured full-run budget.
const LAZY_WAIT = { timeout: FULL_RUN_TEST_TIMEOUT_MS };

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
    queryClient.setQueryData(issuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, {}).queryKey, {
      grouped_by: null,
      sub_grouped_by: null,
      total_count: 0,
      next_cursor: "",
      prev_cursor: "",
      next_page_results: false,
      prev_page_results: false,
      count: 0,
      total_results: 0,
      results: [],
    } satisfies IssueListResponse);
    queryClient.setQueryData(
      listPageQueryOptions(apiClient, PARAMS.ws, {
        entity: { scope: "project", projectId: PARAMS.projectId },
        groupBy: null,
        orderBy: "sort_order",
        showSubIssues: true,
      }).queryKey,
      {
        grouped_by: null,
        sub_grouped_by: null,
        total_count: 0,
        next_cursor: "",
        prev_cursor: "",
        next_page_results: false,
        prev_page_results: false,
        count: 0,
        total_results: 0,
        results: [],
      }
    );
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
    queryClient.setQueryData(
      listPageQueryOptions(apiClient, PARAMS.ws, {
        entity: { scope: "project", projectId: PARAMS.projectId },
        groupBy: null,
        orderBy: "sort_order",
        showSubIssues: true,
      }).queryKey,
      {
        grouped_by: null,
        sub_grouped_by: null,
        total_count: 0,
        next_cursor: "",
        prev_cursor: "",
        next_page_results: false,
        prev_page_results: false,
        count: 0,
        total_results: 0,
        results: [],
      }
    );
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

describe("issues route loader", () => {
  it("prefetches the issue list in the stored order at 100 a page", async () => {
    const seen: string[] = [];
    const apiClient = createAppClient("https://api.test", (async (url: string) => {
      seen.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify(
            url.includes("user-properties")
              ? {
                  ...prefsSeed(),
                  id: ISSUE_UUID,
                  workspace: ISSUE_UUID,
                  user: ISSUE_UUID,
                  project: PARAMS.projectId,
                  preferences: {},
                  sort_order: 1,
                  display_filters: { layout: "list", order_by: "-created_at" },
                  created_by: null,
                  updated_by: null,
                  created_at: "2024-01-02T03:04:05Z",
                  updated_at: "2024-02-03T04:05:06Z",
                }
              : {
                  grouped_by: null,
                  sub_grouped_by: null,
                  total_count: 0,
                  next_cursor: "",
                  prev_cursor: "",
                  next_page_results: false,
                  prev_page_results: false,
                  count: 0,
                  total_results: 0,
                  results: [],
                }
          ),
      };
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    await beforeLoad({ context: { queryClient, apiClient }, params: PARAMS, search: {} } as never);
    expect(seen).toContain(
      `https://api.test/api/workspaces/${PARAMS.ws}/projects/${PARAMS.projectId}/issues/?per_page=100&order_by=-created_at`
    );
  });

  it("prefetches the workspace project list", async () => {
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

function prefsSeed(layout = "list"): PrefsRecord {
  return {
    filters: {},
    display_filters: { layout, group_by: null, order_by: "sort_order" },
    display_properties: {},
    rich_filters: {},
  };
}

function spreadsheetPrefsSeed(): PrefsRecord {
  return {
    ...prefsSeed(),
    display_filters: { layout: "spreadsheet", group_by: null, order_by: "-created_at" },
  };
}

// Seeded render: the loaders own fetching, so the transport throws if a
// query misses its seed instead of hanging the suite on a timeout.
function renderScreen(
  search: IssueSearch,
  totalCount: number,
  rows: IssueListItem[],
  options: {
    richFilters?: Record<string, unknown>;
    project?: Partial<Project>;
    role?: number | null;
    prefs?: PrefsRecord;
    unfilteredSheetRows?: IssueListItem[];
    displayFilters?: Record<string, unknown>;
    projects?: Project[];
  } = {}
): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: PARAMS.ws, projectId: PARAMS.projectId });
  vi.spyOn(Route, "useSearch").mockReturnValue(search);
  const apiClient = createAppClient("https://api.test", (() => {
    throw new Error("unexpected network fetch in a seeded render");
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  vi.spyOn(Route, "useRouteContext").mockReturnValue({ queryClient, apiClient } as never);
  vi.spyOn(Route, "useNavigate").mockReturnValue((() => undefined) as never);
  queryClient.setQueryData(
    projectsQueryOptions(apiClient, PARAMS.ws).queryKey,
    options.projects ?? [{ ...projectSeed(), ...options.project }]
  );
  if (options.role !== null) {
    queryClient.setQueryData(sessionKeys.workspaces, [
      {
        id: ISSUE_UUID,
        name: "Acme",
        slug: PARAMS.ws,
        logo_url: null,
        total_members: 1,
        role: options.role ?? 20,
      },
    ]);
    queryClient.setQueryData(authKeys.projectRoles(PARAMS.ws), { [PARAMS.projectId]: options.role ?? 20 });
  }
  const prefs = options.prefs ?? {
    ...prefsSeed(),
    ...(options.displayFilters === undefined ? {} : { display_filters: options.displayFilters }),
    rich_filters: options.richFilters ?? {},
  };
  queryClient.setQueryData(
    issuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, search, prefs.rich_filters, orderByOf(prefs)).queryKey,
    {
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
    } satisfies IssueListResponse
  );
  queryClient.setQueryData(prefsQueryOptions(apiClient, PARAMS.ws, ENTITY).queryKey, prefs);
  queryClient.setQueryData(
    sheetIssuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, {
      orderBy: "-created_at",
      showSubIssues: true,
      richExpression: prefs.rich_filters,
    }).queryKey,
    {
      pages: [
        {
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
        } satisfies IssueListResponse,
      ],
      pageParams: [""],
    }
  );
  if (options.unfilteredSheetRows !== undefined) {
    queryClient.setQueryData(
      sheetIssuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, {
        orderBy: "-created_at",
        showSubIssues: true,
      }).queryKey,
      {
        pages: [
          {
            grouped_by: null,
            sub_grouped_by: null,
            total_count: options.unfilteredSheetRows.length,
            next_cursor: "",
            prev_cursor: "",
            next_page_results: false,
            prev_page_results: false,
            count: options.unfilteredSheetRows.length,
            total_results: options.unfilteredSheetRows.length,
            results: options.unfilteredSheetRows,
          } satisfies IssueListResponse,
        ],
        pageParams: [""],
      }
    );
  }
  queryClient.setQueryData(pickerCyclesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId).queryKey, []);
  queryClient.setQueryData(
    listPageQueryOptions(apiClient, PARAMS.ws, {
      entity: ENTITY,
      groupBy: null,
      orderBy: orderByOf(prefs),
      showSubIssues: true,
      richFilters: prefs.rich_filters,
    }).queryKey,
    {
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
    } satisfies IssueListResponse
  );

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

// jsdom reports zero element sizes; give the flat list's scroll region and
// rows a height so the virtualized rows mount.
const realHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");
beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("role") === "region" || this.getAttribute("data-testid") === "list-scroll") return 400;
      if (this.hasAttribute("data-index")) return 44;
      return realHeight?.get?.call(this) ?? 0;
    },
  });
});

afterEach(() => {
  if (realHeight) Object.defineProperty(HTMLElement.prototype, "offsetHeight", realHeight);
  resetAppClient();
  clearCommands();
  vi.restoreAllMocks();
});

describe("project issues screen", () => {
  // The route awaits the list layout chunk in beforeLoad; these tests render
  // the component directly, so resolve the same chunk first.
  beforeAll(async () => {
    await Promise.all([loadListLayout(), loadProjectEmptyState()]);
  });

  it("composes the layout root with the project title and its rows", async () => {
    const now = new Date().toISOString();
    renderScreen({}, 1, [issueRow({ created_at: now, updated_at: now })]);

    expect(screen.getByRole("heading", { name: "Website" })).toBeInTheDocument();
    expect(screen.getByTestId("layout-switcher-segmented")).toBeInTheDocument();
    // The list is a lazy leaf; the route chunk shows the list skeleton until
    // its module resolves (NEWFRONT-669, size gate).
    const row = await screen.findByLabelText("WEB-42", undefined, LAZY_WAIT);
    expect(row).toHaveTextContent("Fix login");
    const block = screen.getByTestId("list-row");
    expect(block).toHaveClass("h-11");
    expect(block).not.toHaveClass("h-8");
  });

  it("mounts the rich-filter entry in the desktop and mobile headers", async () => {
    renderScreen({}, 1, [issueRow()]);

    const triggers = await screen.findAllByTestId("rich-filter-add", undefined, LAZY_WAIT);
    expect(triggers).toHaveLength(2);
    for (const trigger of triggers) {
      expect(trigger).toHaveAccessibleName(/Add filter/);
    }
    expect(screen.queryByTestId("rich-filter-row")).not.toBeInTheDocument();
  });

  it("renders stored rich filters and passes them to the list query", async () => {
    const rich = { priority__in: "urgent" };
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: PARAMS.ws, projectId: PARAMS.projectId });
    vi.spyOn(Route, "useSearch").mockReturnValue({});
    const apiClient = createAppClient("https://api.test", (() => {
      throw new Error("unexpected network fetch in a seeded render");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    vi.spyOn(Route, "useRouteContext").mockReturnValue({ queryClient, apiClient } as never);
    queryClient.setQueryData(projectsQueryOptions(apiClient, PARAMS.ws).queryKey, [projectSeed()]);
    queryClient.setQueryData(
      listPageQueryOptions(apiClient, PARAMS.ws, {
        entity: { scope: "project", projectId: PARAMS.projectId },
        groupBy: null,
        orderBy: "sort_order",
        showSubIssues: true,
        richFilters: rich,
      }).queryKey,
      {
        grouped_by: null,
        sub_grouped_by: null,
        total_count: 1,
        next_cursor: "",
        prev_cursor: "",
        next_page_results: false,
        prev_page_results: false,
        count: 0,
        total_results: 1,
        results: [],
      } satisfies IssueListResponse
    );
    queryClient.setQueryData(prefsQueryOptions(apiClient, PARAMS.ws, ENTITY).queryKey, {
      ...prefsSeed(),
      rich_filters: rich,
    });
    queryClient.setQueryData(issuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, {}, rich).queryKey, {
      grouped_by: null,
      sub_grouped_by: null,
      total_count: 1,
      next_cursor: "",
      prev_cursor: "",
      next_page_results: false,
      prev_page_results: false,
      count: 0,
      total_results: 1,
      results: [],
    } satisfies IssueListResponse);
    const Component = Route.options.component;
    if (!Component) throw new Error("issues route has no component");
    render(
      <QueryClientProvider client={queryClient}>
        <React.Suspense fallback={<p>loading</p>}>
          <Component />
        </React.Suspense>
      </QueryClientProvider>
    );

    expect(await screen.findByTestId("rich-filter-row", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.getAllByTestId("rich-condition")).toHaveLength(1);
    expect(screen.getByTestId("rich-condition")).toHaveTextContent("Urgent");
  });

  it("mounts the analytics entry in the desktop and mobile headers", async () => {
    renderScreen({}, 1, [issueRow()], { role: 20 });

    const entries = await screen.findAllByTestId("issues-analytics-entry", undefined, LAZY_WAIT);
    expect(entries).toHaveLength(2);
    for (const entry of entries) expect(entry).toHaveAccessibleName(/Analytics/);
  });

  it("hides the analytics entry from guests", async () => {
    const rich = { priority__in: "urgent" };
    renderScreen({}, 1, [issueRow()], { role: 5, richFilters: rich });

    // The row still renders, so the absence is the role gate, not a
    // missing render.
    expect(await screen.findByTestId("rich-filter-row", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.queryByTestId("issues-analytics-entry")).not.toBeInTheDocument();
  });

  it("offers Save view only when the project enables views", async () => {
    const rich = { priority__in: "urgent" };
    renderScreen({}, 1, [issueRow()], { role: 20, richFilters: rich, project: { issue_views_view: true } });
    expect(await screen.findByTestId("rich-save-view", undefined, LAZY_WAIT)).toBeInTheDocument();
  });

  it("hides Save view when the project disables views", async () => {
    const rich = { priority__in: "urgent" };
    renderScreen({}, 1, [issueRow()], { role: 20, richFilters: rich, project: { issue_views_view: false } });
    expect(await screen.findByTestId("rich-filter-row", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.queryByTestId("rich-save-view")).not.toBeInTheDocument();
  });

  it("mounts the Display control in the desktop and mobile headers", () => {
    renderScreen({}, 1, [issueRow()]);

    const triggers = screen.getAllByTestId("display-options-trigger");
    expect(triggers).toHaveLength(2);
    for (const trigger of triggers) {
      expect(trigger).toHaveAccessibleName("Display");
    }
  });

  it("passes the compact density through from the search param", async () => {
    const now = new Date().toISOString();
    renderScreen({ layout: "compact" }, 1, [issueRow({ created_at: now, updated_at: now })]);

    const block = await screen.findByTestId("list-row");
    expect(block).toHaveClass("h-8");
    expect(block).not.toHaveClass("h-11");
  });

  it("offers the timeline in the layout switcher", () => {
    renderScreen({}, 1, [issueRow()]);

    const switcher = screen.getByTestId("layout-switcher-segmented");
    expect(switcher.querySelector('[data-layout="gantt_chart"]')).not.toBeNull();
  });

  it("renders the timeline when it is the stored layout", async () => {
    renderScreen({}, 1, [issueRow()], {
      displayFilters: { layout: "gantt_chart", group_by: null, order_by: "sort_order" },
    });

    // The timeline is a lazy leaf; the route chunk shows the list skeleton
    // until its module resolves.
    expect(await screen.findByTestId("gantt-layout", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(1);
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("1 work item");
    expect(screen.queryByLabelText("WEB-42")).toBeNull();
  });

  it("renders the timeline read-only for an archived project (ISS-222)", async () => {
    renderScreen({}, 1, [issueRow({ start_date: "2026-10-01", target_date: "2026-10-05" })], {
      project: { archived_at: "2026-01-01T00:00:00Z" },
      displayFilters: { layout: "gantt_chart", group_by: null, order_by: "sort_order" },
    });

    expect(await screen.findByTestId("gantt-layout", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-undated-lane")).toHaveLength(0);
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("explains a project that is not in the visible list", () => {
    renderScreen({}, 1, [issueRow()], { projects: [{ ...projectSeed(), id: ISSUE_UUID }] });

    expect(screen.getByRole("heading", { name: "Project not found" })).toBeInTheDocument();
    expect(screen.getByText("Pick another project from the sidebar.")).toBeInTheDocument();
    expect(screen.queryByTestId("layout-switcher-segmented")).not.toBeInTheDocument();
  });

  it("leaves the New work item command to the shell", () => {
    // The `ni` sequence belongs to the workspace shell so it works from
    // cycle, module, view and drafts pages too (ISS-117, SHELL-086);
    // the list screen only owns its refresh command.
    renderScreen({}, 1, [issueRow()]);
    expect(listCommands().find((entry) => entry.id === "issues.create-work-item")).toBeUndefined();
    expect(listCommands().find((entry) => entry.id === "issues.refresh-list")).toBeDefined();
  });

  it("offers Add work item in the header and opens the dialog", async () => {
    renderScreen({}, 1, [issueRow()]);
    const add = screen.getByRole("button", { name: "Add work item" });
    fireEvent.click(add);
    expect(await screen.findByPlaceholderText("Title", undefined, LAZY_WAIT)).toBeInTheDocument();
  });

  it("keeps the header entry out of reach for a guest on every project", () => {
    renderScreen({}, 1, [issueRow()], { role: 20, project: { member_role: 5 } });
    expect(screen.queryByRole("button", { name: "Add work item" })).not.toBeInTheDocument();
  });

  it("offers the header entry when one project grants create rights", () => {
    const other = projectSeed();
    renderScreen({}, 1, [issueRow()], {
      role: 20,
      project: { member_role: 5 },
      projects: [
        { ...projectSeed(), member_role: 5 },
        { ...other, id: ISSUE_UUID, name: "Blog", identifier: "BLOG", member_role: 15 },
      ],
    });
    expect(screen.getByRole("button", { name: "Add work item" })).toBeInTheDocument();
  });

  it("registers a refresh command that invalidates the workspace", async () => {
    const invalidate = vi.spyOn(queryModule, "invalidateWorkspace").mockResolvedValue({} as never);
    renderScreen({}, 1, [issueRow()]);
    const command = listCommands().find((entry) => entry.id === "issues.refresh-list");
    expect(command?.title).toBe("Refresh issue list");
    expect(command?.hint).toBe("Website");
    command?.run();
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith(expect.anything(), PARAMS.ws));
  });

  it("toasts when the refresh fails", async () => {
    vi.spyOn(queryModule, "invalidateWorkspace").mockRejectedValue(new Error("offline"));
    const notify = vi.spyOn(shellModule, "notifyError").mockImplementation(() => undefined);
    renderScreen({}, 1, [issueRow()]);
    await waitFor(() => expect(listCommands().length).toBeGreaterThan(0));
    listCommands()
      .find((entry) => entry.id === "issues.refresh-list")
      ?.run();
    await waitFor(() => expect(notify).toHaveBeenCalledWith("Could not refresh issues", expect.any(String)));
  });

  it("renders the load error with its message and a retry", () => {
    const errorComponent = Route.options.errorComponent;
    if (!errorComponent) throw new Error("missing errorComponent");
    const reset = vi.fn();
    render(React.createElement(errorComponent, { error: new Error("Boom"), reset }));
    expect(screen.getByRole("heading", { name: "Could not load issues" })).toBeInTheDocument();
    expect(screen.getByText("Boom")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it("renders a generic load error message for non-errors", () => {
    const errorComponent = Route.options.errorComponent;
    if (!errorComponent) throw new Error("missing errorComponent");
    render(React.createElement(errorComponent, { error: "nope", reset: () => {} }));
    expect(screen.getByText("Something went wrong.")).toBeInTheDocument();
  });

  it("offers the spreadsheet layout and renders it from the stored preference", async () => {
    renderScreen({}, 1, [issueRow()], { prefs: spreadsheetPrefsSeed() });

    expect(await screen.findByRole("table", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.getByText("Work items")).toBeInTheDocument();
    // The sheet carries its own quick-add row; the header entry sits beside it.
    expect(
      screen.getAllByRole("button", { name: "Add work item" }).some((entry) => !entry.hasAttribute("disabled"))
    ).toBe(true);
  });

  it("hides the spreadsheet estimate column when the project has no estimate system", async () => {
    renderScreen({}, 1, [issueRow()], { project: { estimate: null }, prefs: spreadsheetPrefsSeed() });

    await screen.findByRole("table", undefined, LAZY_WAIT);
    expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).not.toContain("Estimate");
  });

  it("renders the stored rich filter's sheet rows rather than the unfiltered sheet", async () => {
    const matching = issueRow({ id: ISSUE_UUID, name: "Urgent row", sequence_id: 42 });
    const excluded = issueRow({
      id: "323e4567-e89b-12d3-a456-426614174002",
      name: "Low row",
      sequence_id: 43,
      priority: "low",
    });
    renderScreen({}, 1, [matching], {
      prefs: { ...spreadsheetPrefsSeed(), rich_filters: { priority__in: "urgent" } },
      unfilteredSheetRows: [matching, excluded],
    });

    await screen.findByRole("table", undefined, LAZY_WAIT);
    expect(screen.getByText("Urgent row")).toBeInTheDocument();
    expect(screen.queryByText("Low row")).not.toBeInTheDocument();
  });

  it("renders the spreadsheet read-only for guests and archived projects", async () => {
    const now = new Date().toISOString();
    renderScreen({}, 1, [issueRow({ created_at: now, updated_at: now })], {
      project: { member_role: 5 },
      prefs: spreadsheetPrefsSeed(),
    });
    expect(await screen.findByRole("button", { name: "Add work item" }, LAZY_WAIT)).toBeDisabled();
  });

  it("renders the spreadsheet read-only for an archived project", async () => {
    renderScreen({}, 1, [issueRow()], {
      project: { archived_at: "2026-01-01T00:00:00Z" },
      prefs: spreadsheetPrefsSeed(),
    });
    expect(await screen.findByRole("button", { name: "Add work item" }, LAZY_WAIT)).toBeDisabled();
  });

  it("renders the list read-only for an archived project", async () => {
    renderScreen({}, 1, [issueRow()], {
      project: { archived_at: "2026-01-01T00:00:00Z", member_role: 20 },
    });
    expect(await screen.findByTestId("quick-add-disabled")).toHaveTextContent("New work items are read-only for you");
    expect(screen.queryByRole("button", { name: "New work item" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add to All work items" })).not.toBeInTheDocument();
  });

  it("offers the first work item when the project has none (ISS-068)", async () => {
    renderScreen({}, 0, []);

    const card = await screen.findByTestId("issues-empty-state", undefined, LAZY_WAIT);
    expect(card).toHaveAttribute("data-context", "project");
    expect(screen.getByRole("heading", { name: /^Start with your first work item\./ })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Create your first work item" }, LAZY_WAIT)).toBeEnabled();
    expect(screen.queryByTestId("issues-fresh-strip")).not.toBeInTheDocument();
  });

  it("opens the create dialog from the first-work-item card (ISS-068)", async () => {
    renderScreen({}, 0, []);

    fireEvent.click(await screen.findByRole("button", { name: "Create your first work item" }, LAZY_WAIT));
    expect(await screen.findByRole("dialog", undefined, LAZY_WAIT)).toBeInTheDocument();
  });

  it("disables the first-work-item entry for a project guest", async () => {
    renderScreen({}, 0, [], { project: { member_role: 5 } });

    expect(await screen.findByRole("button", { name: "Create your first work item" }, LAZY_WAIT)).toBeDisabled();
  });

  it("keeps the first-work-item card for a stored expression that matches nothing", async () => {
    // The seed empties the rows but no condition was added through the
    // row in this visit, so the project list shows its first-run card.
    renderScreen({}, 0, [], { richFilters: { and: [{ priority__in: "urgent" }] } });

    const card = await screen.findByTestId("issues-empty-state", undefined, LAZY_WAIT);
    expect(card).toHaveAttribute("data-context", "project");
    expect(screen.getByRole("heading", { name: /^Start with your first work item\./ })).toBeInTheDocument();
  });

  it("mounts the calendar renderer for a stored calendar layout", async () => {
    renderScreen({}, 1, [issueRow({ target_date: "2026-10-15" })], {
      displayFilters: { layout: "calendar", group_by: null, order_by: "sort_order" },
    });

    expect(await screen.findByTestId("issues-calendar", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.queryByLabelText("WEB-42")).not.toBeInTheDocument();
  });

  it("renders an archived project's calendar without edit controls", async () => {
    renderScreen({}, 1, [issueRow({ target_date: "2026-10-15" })], {
      displayFilters: { layout: "calendar", group_by: null, order_by: "sort_order" },
      projects: [{ ...projectSeed(), archived_at: "2026-10-01T00:00:00Z" }],
    });

    expect(await screen.findByTestId("issues-calendar", undefined, LAZY_WAIT)).toBeInTheDocument();
    expect(screen.queryByText("Add work item")).not.toBeInTheDocument();
  });
});
