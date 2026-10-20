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

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));

vi.mock("../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/hooks.js")>()),
  usePermissions: () => ({ role: 20, isMember: true, canEdit: true, isAdmin: false, isLoading: false }),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { ArchivedIssuesScreen } from "./ArchivedIssuesScreen.js";
import { archivedIssuesQueryOptions, ARCHIVED_FILTER_STORAGE_KEY } from "./archivedIssues.js";

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
              archived_at: "2026-01-03T00:00:00Z",
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
  } = {}
) {
  const projects = options.projects ?? [project()];
  // Any expression the row holds is answered from here; the base read has
  // the rows the test names and a narrowed read returns `filteredRows`.
  const apiClient = createAppClient("https://api.test", (async (url: string) => {
    const body = url.includes("/archived-issues/")
      ? archived(url.includes("filters=") ? (options.filteredRows?.(url) ?? 0) : (options.rows ?? 0))
      : projects;
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  queryClient.setQueryData(projectsQueryOptions(apiClient, SLUG).queryKey, projects);
  queryClient.setQueryData(
    archivedIssuesQueryOptions(apiClient, SLUG, PROJECT, (options.stored ?? {}) as never, "-created_at").queryKey,
    archived(options.rows ?? 0)
  );
  render(
    <QueryClientProvider client={queryClient}>
      <React.Suspense fallback={<p>loading</p>}>
        <ArchivedIssuesScreen workspaceSlug={SLUG} projectId={PROJECT} />
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

  it("keeps the first-run card for a stored expression until this visit adds one", async () => {
    renderScreen({ stored: { state_group__in: "cancelled" } });
    expect(await screen.findByText("No archived work items yet")).toBeInTheDocument();
    expect(screen.queryByText("No matching results.")).not.toBeInTheDocument();
  });

  it("seeds the row and its read from the window-local expression (ISS-071)", async () => {
    window.localStorage.setItem(
      ARCHIVED_FILTER_STORAGE_KEY,
      JSON.stringify([
        {
          key: "ARCHIVED",
          workspaceSlug: SLUG,
          viewId: PROJECT,
          filters: { richFilters: { state_group__in: "cancelled" } },
        },
      ])
    );
    renderScreen({ stored: { state_group__in: "cancelled" } });
    expect(await screen.findByTestId("rich-condition")).toHaveTextContent("State Group");
    expect(screen.getByText("No archived work items yet")).toBeInTheDocument();
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

  it("asks for another project when the record points outside the list", async () => {
    renderScreen({ projects: [project(USER)] });
    expect(await screen.findByRole("heading", { name: "Project not found" })).toBeInTheDocument();
    expect(screen.queryByTestId("archived-issues")).not.toBeInTheDocument();
  });
});
