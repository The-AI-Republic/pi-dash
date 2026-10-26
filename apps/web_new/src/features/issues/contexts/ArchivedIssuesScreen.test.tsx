// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived tab body (parity/specs/issues.md §8, ISS-071). The card is
// the first-run one until this page's row adds a condition; clearing puts
// the unfiltered list back, and Set automation leaves for the project's
// automation settings.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueListResponse, Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ navigate: vi.fn() }));

// The peek's own reads and rendering belong to its suite; here the screen
// only owns whether it mounts and what selection it hands the address.
vi.mock("../peek/ArchivedIssuePeek.js", () => ({
  ArchivedIssuePeek: (props: { issueId: string; row?: { cycle_id: string | null } | null; onClose: () => void }) => (
    <div data-testid="archived-peek" data-issue={props.issueId} data-cycle={props.row?.cycle_id ?? ""}>
      <button type="button" onClick={props.onClose}>
        close archived peek
      </button>
    </div>
  ),
}));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectDetailsQueryOptions, projectsQueryOptions } from "../../projects/index.js";
import { ArchivedIssuesScreen } from "./ArchivedIssuesScreen.js";
import { archivedPageQueryOptions, ARCHIVED_FILTER_STORAGE_KEY, DEFAULT_ARCHIVED_DISPLAY } from "./archivedIssues.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const USER = "123e4567-e89b-12d3-a456-426614174000";

function project(id = PROJECT): Project {
  return {
    id,
    name: "Website",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: USER,
    cycle_view: true,
    module_view: true,
    issue_views_view: true,
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

function seedStoredExpression(expression: Record<string, unknown>): void {
  window.localStorage.setItem(
    ARCHIVED_FILTER_STORAGE_KEY,
    JSON.stringify([
      {
        key: "ARCHIVED",
        workspaceSlug: SLUG,
        viewId: PROJECT,
        filters: { richFilters: expression },
      },
    ])
  );
}

function archived(rows = 0): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: rows,
    count: rows,
    total_results: rows,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    results:
      rows === 0
        ? []
        : [
            {
              id: "423e4567-e89b-12d3-a456-426614174003",
              name: "Old login form",
              state_id: null,
              sort_order: 1,
              completed_at: "2026-01-01T00:00:00Z",
              priority: "none",
              sequence_id: 7,
              project_id: PROJECT,
              parent_id: null,
              cycle_id: null,
              module_ids: [],
              label_ids: [],
              assignee_ids: [],
              sub_issues_count: null,
              attachment_count: null,
              link_count: null,
              created_at: "2026-01-01T00:00:00Z",
              updated_at: "2026-01-02T00:00:00Z",
              created_by: null,
              updated_by: null,
              is_draft: false,
              archived_at: "2026-01-03",
            },
          ],
  };
}

function renderScreen(
  options: {
    rows?: number;
    filteredRows?: (url: string) => number;
    projects?: Project[];
    stored?: Record<string, unknown>;
    peekIssueId?: string | null;
    onPeekChange?: (issueId: string | null) => void;
    display?: Record<string, unknown>;
    page?: unknown;
    failList?: boolean;
    estimate?: Record<string, unknown> | null;
  } = {}
) {
  const projects = options.projects ?? [project()];
  // Any expression the row holds is answered from here; the base read has
  // the rows the test names and a narrowed read returns `filteredRows`.
  const apiClient = createAppClient("https://api.test", (async (url: string) => {
    if (options.failList === true && url.includes("/archived-issues/")) {
      return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
    }
    const body = url.includes("/archived-issues/")
      ? (options.page ?? archived(url.includes("filters=") ? (options.filteredRows?.(url) ?? 0) : (options.rows ?? 0)))
      : projects;
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  queryClient.setQueryData(projectsQueryOptions(apiClient, SLUG).queryKey, projects);
  queryClient.setQueryData(
    projectDetailsQueryOptions(apiClient, SLUG).queryKey,
    options.estimate === undefined ? [] : [{ ...project(), estimate: options.estimate } as never]
  );
  queryClient.setQueryData(
    archivedPageQueryOptions(apiClient, SLUG, PROJECT, {
      expression: (options.stored ?? {}) as never,
      groupBy: options.display?.["group_by"] === "state" ? "state" : null,
      orderBy: "sort_order",
    }).queryKey,
    (options.page ?? archived(options.rows ?? 0)) as never
  );
  if (options.display !== undefined) {
    window.localStorage.setItem(
      ARCHIVED_FILTER_STORAGE_KEY,
      JSON.stringify([
        {
          key: "ARCHIVED",
          workspaceSlug: SLUG,
          viewId: PROJECT,
          filters: { display_filters: options.display },
        },
      ])
    );
  }
  render(
    <QueryClientProvider client={queryClient}>
      <React.Suspense fallback={<p>loading</p>}>
        <ArchivedIssuesScreen
          workspaceSlug={SLUG}
          projectId={PROJECT}
          peekIssueId={options.peekIssueId}
          onPeekChange={options.onPeekChange}
        />
      </React.Suspense>
    </QueryClientProvider>
  );
  return queryClient;
}

// jsdom reports zero heights; the list's virtual window needs rows sized.
const realHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");
function sizeList(): void {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("role") === "region" || this.getAttribute("data-testid") === "list-scroll") return 400;
      return 44;
    },
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  sizeList();
});

afterEach(() => {
  resetAppClient();
  window.localStorage.clear();
  if (realHeight) Object.defineProperty(HTMLElement.prototype, "offsetHeight", realHeight);
});

describe("ArchivedIssuesScreen", () => {
  it("states the empty archive and points at the automation rule (ISS-071)", async () => {
    const user = userEvent.setup();
    renderScreen();
    expect(await screen.findByText("No archived work items yet")).toBeInTheDocument();
    expect(screen.getByTestId("issues-empty-state")).toHaveAttribute("data-context", "archived");
    await user.click(screen.getByRole("button", { name: "Set automation" }));
    expect(mocks.navigate).toHaveBeenCalledWith({ to: `/${SLUG}/settings/projects/${PROJECT}/automations` });
  });

  it("lists the archived rows when the archive holds some", async () => {
    renderScreen({ rows: 1 });
    expect(await screen.findByText("Old login form")).toBeInTheDocument();
    expect(screen.queryByText("No archived work items yet")).not.toBeInTheDocument();
  });

  it("shows the no-match card for a stored expression that hides every row", async () => {
    seedStoredExpression({ state_group__in: "cancelled" });
    renderScreen({ stored: { state_group__in: "cancelled" } });
    expect(await screen.findByText("No matching results.")).toBeInTheDocument();
    expect(screen.getByTestId("issues-empty-state")).toHaveAttribute("data-context", "archived-filtered");
    expect(
      within(screen.getByTestId("issues-empty-state")).getByRole("button", { name: "Clear filters" })
    ).toBeEnabled();
    expect(screen.queryByText("No archived work items yet")).not.toBeInTheDocument();
  });

  it("seeds the row and its read from the window-local expression (ISS-071)", async () => {
    seedStoredExpression({ state_group__in: "cancelled" });
    renderScreen({ stored: { state_group__in: "cancelled" } });
    expect(await screen.findByTestId("rich-condition")).toHaveTextContent("State Group");
    expect(screen.getByText("No matching results.")).toBeInTheDocument();
  });

  it("switches to the filtered card once a condition is added here, and clears back", async () => {
    const user = userEvent.setup();
    renderScreen();
    await screen.findByText("No archived work items yet");
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    await user.click(await screen.findByRole("option", { name: "Priority" }));
    await user.click(await screen.findByRole("checkbox", { name: "Urgent" }));
    expect(await screen.findByText("No matching results.")).toBeInTheDocument();
    const card = screen.getByTestId("issues-empty-state");
    expect(card).toHaveAttribute("data-context", "archived-filtered");
    expect(within(card).getByRole("button", { name: "Clear filters" })).toBeEnabled();
    await user.click(within(card).getByRole("button", { name: "Clear filters" }));
    expect(await screen.findByText("No archived work items yet")).toBeInTheDocument();
  });

  it("writes the expression locally so the next visit restores it", async () => {
    const user = userEvent.setup();
    renderScreen();
    await screen.findByText("No archived work items yet");
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    await user.click(await screen.findByRole("option", { name: "Priority" }));
    await user.click(await screen.findByRole("checkbox", { name: "Urgent" }));
    await waitFor(() => {
      const raw = window.localStorage.getItem(ARCHIVED_FILTER_STORAGE_KEY);
      expect(raw).toContain("ARCHIVED");
      expect(raw).toContain("priority");
    });
  });

  it("leaves no stored entry on a pristine visit", async () => {
    renderScreen();
    await screen.findByText("No archived work items yet");
    expect(window.localStorage.getItem(ARCHIVED_FILTER_STORAGE_KEY)).toBeNull();
  });

  it("strips this project's owned members on a pristine visit and keeps the rest", async () => {
    window.localStorage.setItem(
      ARCHIVED_FILTER_STORAGE_KEY,
      JSON.stringify([
        {
          key: "ARCHIVED",
          workspaceSlug: SLUG,
          viewId: PROJECT,
          filters: {
            display_filters: { layout: "kanban" },
            kanban_filters: { group_by: ["done"] },
          },
        },
      ])
    );
    renderScreen();
    await screen.findByText("No archived work items yet");
    await waitFor(() => {
      const raw = window.localStorage.getItem(ARCHIVED_FILTER_STORAGE_KEY);
      expect(raw).not.toBeNull();
      const [entry] = JSON.parse(raw ?? "[]") as Array<{ viewId: string; filters: Record<string, unknown> }>;
      expect(entry?.filters).toEqual({
        display_filters: { layout: "kanban" },
        kanban_filters: { group_by: ["done"] },
      });
    });
  });

  it("keeps unknown display members while persisting a changed choice", async () => {
    window.localStorage.setItem(
      ARCHIVED_FILTER_STORAGE_KEY,
      JSON.stringify([
        {
          key: "ARCHIVED",
          workspaceSlug: SLUG,
          viewId: PROJECT,
          filters: {
            display_filters: { layout: "kanban", calendar: { layout: "week" } },
            kanban_filters: { group_by: ["done"] },
          },
        },
      ])
    );
    const user = userEvent.setup();
    renderScreen();
    await screen.findByText("No archived work items yet");
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    await user.click(await screen.findByRole("option", { name: "Priority" }));
    await user.click(await screen.findByRole("checkbox", { name: "Urgent" }));
    await waitFor(() => {
      const raw = window.localStorage.getItem(ARCHIVED_FILTER_STORAGE_KEY) ?? "[]";
      const [entry] = JSON.parse(raw) as Array<{ filters: Record<string, unknown> }>;
      expect(entry?.filters).toMatchObject({
        display_filters: { layout: "kanban", calendar: { layout: "week" }, order_by: DEFAULT_ARCHIVED_DISPLAY.orderBy },
        kanban_filters: { group_by: ["done"] },
      });
      expect(entry?.filters["richFilters"]).toBeTruthy();
    });
  });

  it("asks for another project when the record points outside the list", async () => {
    renderScreen({ projects: [project(USER)] });
    expect(await screen.findByRole("heading", { name: "Project not found" })).toBeInTheDocument();
    expect(screen.queryByTestId("archived-issues")).not.toBeInTheDocument();
  });

  it("disables the entries for a project guest (ISS-071)", async () => {
    renderScreen({ projects: [{ ...project(), member_role: 5 }] });
    expect(await screen.findByText("No archived work items yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set automation" })).toBeDisabled();
  });

  it("hands the peek the list row so its memberships survive the detail read", async () => {
    const cycle = "323e4567-e89b-12d3-a456-426614174002";
    const page = archived(1);
    const [first] = page.results;
    if (first === undefined) throw new Error("fixture row missing");
    page.results[0] = { ...first, cycle_id: cycle, module_ids: [], label_ids: [], assignee_ids: [] };
    renderScreen({ rows: 1, page, peekIssueId: "423e4567-e89b-12d3-a456-426614174003", onPeekChange: () => undefined });
    const peek = await screen.findByTestId("archived-peek");
    expect(peek).toHaveAttribute("data-issue", "423e4567-e89b-12d3-a456-426614174003");
    expect(peek).toHaveAttribute("data-cycle", cycle);
  });

  it("opens the peek for the clicked row and closes it back through the address", async () => {
    const user = userEvent.setup();
    const changed: Array<string | null> = [];
    renderScreen({ rows: 1, onPeekChange: (id) => changed.push(id) });
    await user.click(await screen.findByText("Old login form"));
    expect(changed).toEqual(["423e4567-e89b-12d3-a456-426614174003"]);
  });

  it("clears the selection when the open row is clicked again", async () => {
    const user = userEvent.setup();
    const changed: Array<string | null> = [];
    renderScreen({
      rows: 1,
      peekIssueId: "423e4567-e89b-12d3-a456-426614174003",
      onPeekChange: (id) => changed.push(id),
    });
    await user.click(await screen.findByText("Old login form"));
    expect(changed).toEqual([null]);
  });

  it("mounts the archived peek for a selected id and clears the address on close", async () => {
    const user = userEvent.setup();
    const changed: Array<string | null> = [];
    renderScreen({
      rows: 1,
      peekIssueId: "423e4567-e89b-12d3-a456-426614174003",
      onPeekChange: (id) => changed.push(id),
    });
    expect(await screen.findByTestId("archived-peek")).toHaveAttribute(
      "data-issue",
      "423e4567-e89b-12d3-a456-426614174003"
    );
    await user.click(screen.getByRole("button", { name: "close archived peek" }));
    expect(changed).toEqual([null]);
  });

  it("mounts the peek with no row when the selection is not on the first page", async () => {
    renderScreen({ rows: 0, peekIssueId: "423e4567-e89b-12d3-a456-426614174003", onPeekChange: () => undefined });
    expect(await screen.findByTestId("archived-peek")).toHaveAttribute("data-cycle", "");
  });

  it("reads the estimate system for a project that has one", async () => {
    renderScreen({ rows: 1, estimate: { id: "623e4567-e89b-12d3-a456-426614174005" } });
    expect(await screen.findByText("Old login form")).toBeInTheDocument();
  });

  it("mounts no peek when the address carries no selection", async () => {
    renderScreen({ rows: 1 });
    await screen.findByText("Old login form");
    expect(screen.queryByTestId("archived-peek")).not.toBeInTheDocument();
  });

  it("renders grouped sections with a per-section Load more when the bucket has more", async () => {
    const user = userEvent.setup();
    const sectionA = "323e4567-e89b-12d3-a456-426614174002";
    const sectionB = "423e4567-e89b-12d3-a456-426614174003";
    const groupedPage = {
      grouped_by: "state_id",
      sub_grouped_by: null,
      total_count: 3,
      next_cursor: "",
      prev_cursor: "",
      next_page_results: false,
      prev_page_results: false,
      count: 2,
      total_results: 3,
      results: {
        [sectionA]: {
          results: [
            {
              id: "523e4567-e89b-12d3-a456-426614174004",
              name: "Grouped one",
              state_id: sectionA,
              sort_order: 1,
              completed_at: "2026-01-01T00:00:00Z",
              priority: "none",
              sequence_id: 1,
              project_id: PROJECT,
              parent_id: null,
              module_ids: [],
              label_ids: [],
              assignee_ids: [],
              sub_issues_count: null,
              attachment_count: null,
              link_count: null,
              created_at: "2026-01-01T00:00:00Z",
              updated_at: "2026-01-02T00:00:00Z",
              created_by: null,
              updated_by: null,
              is_draft: false,
              archived_at: "2026-01-03",
            },
          ],
          total_results: 2,
        },
        [sectionB]: { results: [], total_results: 1 },
      },
    };
    renderScreen({
      display: { group_by: "state", order_by: "sort_order", show_empty_groups: true },
      page: groupedPage,
    });
    expect(await screen.findByText("Grouped one")).toBeInTheDocument();
    // Each section with unseen rows carries its own control: A has 1 of 2
    // and the empty-but-shown B has 0 of 1.
    const more = screen.getAllByRole("button", { name: "Load more" });
    expect(more).toHaveLength(2);
    await user.click(more[0]!);
  });

  it("renders the flat Load more when the first page has more rows", async () => {
    renderScreen({
      page: {
        grouped_by: null,
        sub_grouped_by: null,
        total_count: 3,
        next_cursor: "100:1:0",
        prev_cursor: "",
        next_page_results: true,
        prev_page_results: false,
        count: 1,
        total_results: 3,
        results: [
          {
            id: "523e4567-e89b-12d3-a456-426614174004",
            name: "Old login form",
            state_id: null,
            sort_order: 1,
            completed_at: "2026-01-01T00:00:00Z",
            priority: "none",
            sequence_id: 1,
            project_id: PROJECT,
            parent_id: null,
            module_ids: [],
            label_ids: [],
            assignee_ids: [],
            sub_issues_count: null,
            attachment_count: null,
            link_count: null,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-02T00:00:00Z",
            created_by: null,
            updated_by: null,
            is_draft: false,
            archived_at: "2026-01-03",
          },
        ],
      },
    });
    await new Promise((r) => setTimeout(r, 3000));

    expect(await screen.findByText("Old login form")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Load more" })).toBeInTheDocument();
  });

  it("renders a grouped section with no Load more once its rows are all shown", async () => {
    const sectionA = "323e4567-e89b-12d3-a456-426614174002";
    const page = {
      grouped_by: "state_id",
      sub_grouped_by: null,
      total_count: 1,
      next_cursor: "",
      prev_cursor: "",
      next_page_results: false,
      prev_page_results: false,
      count: 1,
      total_results: 1,
      results: {
        [sectionA]: {
          results: [
            {
              id: "523e4567-e89b-12d3-a456-426614174004",
              name: "Grouped done",
              state_id: sectionA,
              sort_order: 1,
              completed_at: "2026-01-01T00:00:00Z",
              priority: "none",
              sequence_id: 1,
              project_id: PROJECT,
              parent_id: null,
              module_ids: [],
              label_ids: [],
              assignee_ids: [],
              sub_issues_count: null,
              attachment_count: null,
              link_count: null,
              created_at: "2026-01-01T00:00:00Z",
              updated_at: "2026-01-02T00:00:00Z",
              created_by: null,
              updated_by: null,
              is_draft: false,
              archived_at: "2026-01-03",
            },
          ],
          total_results: 1,
        },
      },
    };
    renderScreen({
      display: { group_by: "state", order_by: "sort_order", show_empty_groups: false },
      page,
    });
    expect(await screen.findByText("Grouped done")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  });

  it("offers a retry when the archived read fails", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const apiClient = createAppClient("https://api.test", (async (url: string) => ({
      status: 500,
      url,
      headers: { get: () => null },
      text: async () => "{}",
    })) as never);
    queryClient.setQueryData(projectsQueryOptions(apiClient, SLUG).queryKey, [project()]);
    queryClient.setQueryData(projectDetailsQueryOptions(apiClient, SLUG).queryKey, []);
    render(
      <QueryClientProvider client={queryClient}>
        <React.Suspense fallback={<p>loading</p>}>
          <ArchivedIssuesScreen workspaceSlug={SLUG} projectId={PROJECT} />
        </React.Suspense>
      </QueryClientProvider>
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load the archived work items.");
    await user.click(screen.getByRole("button", { name: "Try again" }));
  });
});
