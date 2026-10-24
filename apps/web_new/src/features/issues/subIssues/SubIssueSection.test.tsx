// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The sub-work-items collapsible (parity/specs/issues.md §15, ISS-173/
// 174/175/176/177/178/238) against the real query and store path: the
// title gauge and full-set count, rows beneath, client-side filtering,
// grouping and ordering, the filtered-empty clear, load-more, and the
// create/add-existing/delete dialogs with the writes each sends.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Issue, ProjectDetails, SubIssueItem, SubIssuesResponse, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifySuccess: vi.fn(), notifyError: vi.fn() };
});

import { createAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { ISSUE_TYPE_FILTER_FLAG, TEAM_PROJECT_FILTER_FLAG } from "../edition/flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "../edition/providers.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { SubIssueSection, type SubIssueSectionHandle } from "./SubIssueSection.js";
import { createParentState, useSubIssuesUiStore } from "./store.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PARENT = "323e4567-e89b-12d3-a456-426614174002";
const CHILD = "423e4567-e89b-12d3-a456-426614174003";
const SECOND = "523e4567-e89b-12d3-a456-426614174004";
const THIRD = "623e4567-e89b-12d3-a456-426614174005";
const OTHER_PROJECT = "723e4567-e89b-12d3-a456-426614174006";
const STATE = "823e4567-e89b-12d3-a456-426614174007";
const MEMBER = "923e4567-e89b-12d3-a456-426614174008";

function issue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: PARENT,
    name: "Parent work",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: 4,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
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
    ...overrides,
  };
}

function row(overrides: Partial<SubIssueItem> = {}): SubIssueItem {
  return {
    id: CHILD,
    name: "Child one",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: PARENT,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    state_group: "started",
    ...overrides,
  };
}

function page(overrides: Partial<SubIssuesResponse> = {}): SubIssuesResponse {
  return {
    sub_issues: [row()],
    state_distribution: { started: [CHILD], completed: [] },
    total_count: 1,
    ...overrides,
  };
}

function ok(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

const me = {
  id: MEMBER,
  email: "ada@example.test",
  username: "ada",
  first_name: "Ada",
  last_name: "L",
  display_name: "Ada L",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: true,
  last_login_medium: "",
  last_login_time: null,
};

const state = {
  id: STATE,
  project_id: PROJECT,
  workspace_id: PARENT,
  name: "In progress",
  group: "started",
  color: "#d9912b",
  default: false,
  description: "",
  sequence: 1,
};

const workspaceMember = {
  id: "b23e4567-e89b-12d3-a456-426614174010",
  member: {
    id: MEMBER,
    first_name: "Ada",
    last_name: "L",
    avatar: "",
    avatar_url: null,
    display_name: "Ada L",
    is_bot: false,
  },
  role: 15,
  is_active: true,
};

const member = {
  id: "a23e4567-e89b-12d3-a456-426614174009",
  member: MEMBER,
  project: PROJECT,
  role: 15,
  original_role: 15,
  created_at: "2026-01-01T00:00:00.000Z",
};

interface StubOptions {
  pages?: SubIssuesResponse[];
  searchHits?: unknown[];
  deleteFail?: boolean;
}

function stubApi(options: StubOptions = {}) {
  const calls: { method: string; url: string; body: unknown }[] = [];
  const pages = options.pages ?? [page()];
  let pageIndex = 0;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return ok(url, 200, { csrf_token: "csrf" });
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ method, url, body });
    if ((url.includes("/sub-issues/") || url.includes("/epics/")) && method === "GET") {
      const current = pages[Math.min(pageIndex, pages.length - 1)] ?? page();
      pageIndex += 1;
      return ok(url, 200, current);
    }
    if (url.includes("/search-issues/")) return ok(url, 200, options.searchHits ?? []);
    if (url.endsWith("/api/users/me/")) return ok(url, 200, me);
    if (url.endsWith("/profile/")) return ok(url, 200, { start_of_the_week: 0 });
    if (url.endsWith("/states/")) return ok(url, 200, [state]);
    if (url.endsWith("/projects/")) {
      return ok(url, 200, [projectListRow(PROJECT, "Atlas", "ATL"), projectListRow(OTHER_PROJECT, "Betel", "BET")]);
    }
    if (url.endsWith("/api/workspaces/acme/members/")) return ok(url, 200, [workspaceMember]);
    if (url.endsWith("/members/")) return ok(url, 200, [member]);
    if (method === "DELETE" && options.deleteFail === true) return ok(url, 500, { detail: "refused" });
    return ok(url, method === "GET" ? 200 : 204, {});
  }) as never);
  return calls;
}

function projectListRow(id: string, name: string, identifier: string) {
  return {
    id,
    name,
    identifier,
    sort_order: 1,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: PARENT,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
  };
}

const PROJECT_ROW = {
  id: PROJECT,
  name: "Atlas",
  identifier: "ATL",
  memberRole: 15,
  archived: false,
};

function renderSection(props: Partial<React.ComponentProps<typeof SubIssueSection>> = {}) {
  const onOpen = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <SubIssueSection
        workspaceSlug={WS}
        issue={props.issue ?? issue()}
        project={props.project}
        editable={props.editable ?? true}
        projectIdentifier={props.projectIdentifier ?? "ATL"}
        canWrite={props.canWrite ?? true}
        onOpen={props.onOpen ?? onOpen}
        createProjects={props.createProjects ?? [PROJECT_ROW]}
        {...(props.heading === undefined ? {} : { heading: props.heading })}
      />
    </QueryClientProvider>
  );
  return { onOpen, ...view };
}

function resetStore(): void {
  useSubIssuesUiStore.setState({ parents: {} });
}

beforeEach(() => {
  vi.clearAllMocks();
  resetStore();
});

afterEach(() => {
  useSubIssuesUiStore.setState({ parents: {} });
  resetEditionIssuesProviders();
  delete edition.flags[TEAM_PROJECT_FILTER_FLAG];
  delete edition.flags[ISSUE_TYPE_FILTER_FLAG];
});

describe("SubIssueSection title", () => {
  it("draws the gauge, the gauge label and the full-set done count", async () => {
    stubApi({
      pages: [
        page({
          sub_issues: [row(), row({ id: SECOND, name: "Child two", sequence_id: 8 })],
          state_distribution: { started: [CHILD], completed: [SECOND] },
          total_count: 4,
        }),
      ],
    });
    renderSection();
    await screen.findByText("Child one");
    const header = screen.getByTestId("sub-issues-collapsible");
    expect(within(header).getByText("Sub-work items")).toBeInTheDocument();
    expect(within(header).getByText("1/4 Done")).toBeInTheDocument();
    expect(within(header).getByRole("img", { name: "25% done" })).toHaveAttribute("data-percent", "25");
  });

  it("reads an epic as work items and keeps the gauge", async () => {
    stubApi({
      pages: [page({ state_distribution: { started: [CHILD] }, total_count: 1 })],
    });
    renderSection({ issue: issue({ is_epic: true }) });
    await screen.findByText("Child one");
    const header = screen.getByTestId("sub-issues-collapsible");
    expect(within(header).getByText("Work items")).toBeInTheDocument();
    expect(within(header).getByText("0/1 Done")).toBeInTheDocument();
    expect(within(header).getByRole("img", { name: "0% done" })).toHaveAttribute("data-percent", "0");
  });

  it("draws a childless parent's header so its first child can be added", async () => {
    stubApi({ pages: [page({ sub_issues: [], state_distribution: {}, total_count: 0 })] });
    renderSection();
    const header = await screen.findByTestId("sub-issues-collapsible");
    await waitFor(() => expect(within(header).getByText("0/0 Done")).toBeInTheDocument());
    expect(screen.queryByText("Child one")).not.toBeInTheDocument();
    expect(screen.getByTestId("sub-issues-add")).toBeInTheDocument();
  });

  it("reopens a collapsed section after a remount", async () => {
    stubApi();
    const user = userEvent.setup();
    const first = renderSection();
    const header = await screen.findByTestId("sub-issues-collapsible");
    await user.click(header);
    expect(useSubIssuesUiStore.getState().stateFor(PARENT).open).toBe(false);
    first.unmount();
    renderSection();
    const remounted = await screen.findByTestId("sub-issues-collapsible");
    expect(remounted).toHaveAttribute("aria-expanded", "false");
    await user.click(remounted);
    expect(await screen.findByText("Child one")).toBeInTheDocument();
  });

  it("keeps the header up while the first read is in flight", async () => {
    const waiting: (() => void)[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, 200, { csrf_token: "csrf" });
      await new Promise<void>((resolve) => waiting.push(resolve));
      return ok(url, 200, page());
    }) as never);
    renderSection();
    const header = await screen.findByTestId("sub-issues-collapsible");
    expect(header).toHaveTextContent("Sub-work items");
    expect(await screen.findByText("Loading sub-work items")).toBeInTheDocument();
    for (const release of waiting) release();
    await waitFor(() => expect(screen.queryByText("Loading sub-work items")).not.toBeInTheDocument());
  });

  it("collapses and reopens, keeping the setting per parent", async () => {
    stubApi();
    const user = userEvent.setup();
    renderSection();
    const header = await screen.findByTestId("sub-issues-collapsible");
    expect(header).toHaveAttribute("aria-expanded", "true");
    await user.click(header);
    expect(header).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Child one")).not.toBeInTheDocument();
    expect(useSubIssuesUiStore.getState().stateFor(PARENT).open).toBe(false);
    await user.click(header);
    expect(await screen.findByText("Child one")).toBeInTheDocument();
  });
});

describe("SubIssueSection rows and pagination", () => {
  it("reads an epic parent's children through the epics collection", async () => {
    const calls = stubApi();
    renderSection({ issue: issue({ is_epic: true }) });
    await screen.findByText("Child one");
    const reads = calls.filter((call) => call.method === "GET");
    expect(reads.some((call) => call.url.includes(`/epics/${PARENT}/issues/`))).toBe(true);
    expect(reads.some((call) => call.url.includes(`/issues/${PARENT}/sub-issues/`))).toBe(false);
  });

  it("renders the loaded rows and requests fifty at a time", async () => {
    const calls = stubApi();
    renderSection();
    expect(await screen.findByText("Child one")).toBeInTheDocument();
    const read = calls.find((call) => call.method === "GET" && call.url.includes("/sub-issues/"));
    expect(read?.url).toContain("per_page=50");
  });

  it("appends the next page from the load-more row and hides it at the end", async () => {
    const calls = stubApi({
      pages: [
        page({
          sub_issues: [row()],
          state_distribution: { started: [CHILD, SECOND], completed: [] },
          total_count: 2,
          next_cursor: "c2",
        }),
        page({
          sub_issues: [row({ id: SECOND, name: "Child two", sequence_id: 8 })],
          state_distribution: { started: [CHILD, SECOND], completed: [] },
          total_count: 2,
        }),
      ],
    });
    const user = userEvent.setup();
    renderSection();
    expect(await screen.findByText("Child one")).toBeInTheDocument();
    await user.click(screen.getByTestId("sub-issues-load-more"));
    expect(await screen.findByText("Child two")).toBeInTheDocument();
    expect(screen.queryByTestId("sub-issues-load-more")).not.toBeInTheDocument();
    const reads = calls.filter((call) => call.method === "GET" && call.url.includes("/sub-issues/"));
    expect(reads[1]?.url).toContain("cursor=c2");
  });

  it("shows a small parent in one request with no load-more", async () => {
    stubApi();
    renderSection();
    expect(await screen.findByText("Child one")).toBeInTheDocument();
    expect(screen.queryByTestId("sub-issues-load-more")).not.toBeInTheDocument();
  });
});

describe("SubIssueSection filters and display", () => {
  const many = () =>
    page({
      sub_issues: [
        row({ id: CHILD, name: "Alpha", priority: "high", sequence_id: 7 }),
        row({ id: SECOND, name: "Beta", priority: "low", sequence_id: 8, state_group: "completed" }),
        row({ id: THIRD, name: "Gamma", priority: "high", sequence_id: 9, project_id: OTHER_PROJECT }),
      ],
      state_distribution: { started: [CHILD], completed: [SECOND, THIRD] },
      total_count: 3,
    });

  it("filters the loaded page and offers clearing when nothing matches", { timeout: 15_000 }, async () => {
    stubApi({ pages: [many()] });
    const user = userEvent.setup();
    renderSection();
    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    await user.click(screen.getByTestId("sub-issue-filters-trigger"));
    const panel = screen.getByTestId("sub-issue-filters-panel");
    await user.click(within(panel).getByRole("checkbox", { name: "High" }));
    await waitFor(() => expect(screen.queryByText("Beta")).not.toBeInTheDocument());
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Gamma")).toBeInTheDocument();
    // Gestures past the loaded page: a state group no row carries.
    await user.click(within(panel).getByRole("checkbox", { name: "Review" }));
    expect(await screen.findByText("No sub-work items match the filters you've applied.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(await screen.findByText("Beta")).toBeInTheDocument();
    expect(useSubIssuesUiStore.getState().stateFor(PARENT).filters.priority).toEqual([]);
  });

  it("names the epic's filtered-empty copy", { timeout: 15_000 }, async () => {
    stubApi({ pages: [many()] });
    const user = userEvent.setup();
    renderSection({ issue: issue({ is_epic: true }) });
    await screen.findByText("Alpha");
    await user.click(screen.getByTestId("sub-issue-filters-trigger"));
    await user.click(screen.getByRole("checkbox", { name: "Urgent" }));
    await user.click(screen.getByRole("checkbox", { name: "Medium" }));
    expect(await screen.findByText("No work items match the filters you've applied.")).toBeInTheDocument();
  });

  it("offers the project filter only for children in another project", { timeout: 15_000 }, async () => {
    stubApi({ pages: [many()] });
    renderSection();
    await screen.findByText("Alpha");
    await userEvent.setup().click(screen.getByTestId("sub-issue-filters-trigger"));
    expect(screen.getByTestId("sub-issue-filter-project")).toBeInTheDocument();
  });

  it("hides the project filter when every child shares the parent's project", { timeout: 15_000 }, async () => {
    stubApi();
    renderSection();
    await screen.findByText("Child one");
    await userEvent.setup().click(screen.getByTestId("sub-issue-filters-trigger"));
    expect(screen.queryByTestId("sub-issue-filter-project")).not.toBeInTheDocument();
  });

  it("groups by priority with per-group counts and orders the rows", async () => {
    stubApi({ pages: [many()] });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Alpha");
    await user.click(screen.getByTestId("sub-issue-display-trigger"));
    const grouping = screen.getByTestId("sub-issue-display-group");
    await user.click(within(grouping).getByRole("radio", { name: "Priority" }));
    const groupHeaders = await screen.findAllByTestId("sub-issue-group-header");
    expect(groupHeaders.map((header) => header.textContent)).toEqual(["High2", "Low1"]);
    expect(useSubIssuesUiStore.getState().stateFor(PARENT).display.groupBy).toBe("priority");
  });

  it("hides a column through the display properties", async () => {
    stubApi({ pages: [many()] });
    const user = userEvent.setup();
    renderSection();
    expect(await screen.findByText("ATL-7")).toBeInTheDocument();
    await user.click(screen.getByTestId("sub-issue-display-trigger"));
    await user.click(within(screen.getByTestId("sub-issue-display-properties")).getByRole("button", { name: "ID" }));
    await waitFor(() => expect(screen.queryByText("ATL-7")).not.toBeInTheDocument());
  });

  it("offers no edition dimension in the community build (ISS-234)", { timeout: 15_000 }, async () => {
    const teamSource = vi.fn(async () => [{ id: OTHER_PROJECT, name: "Betel", identifier: "BET" }]);
    registerEditionIssuesProviders({ teamProjects: teamSource });
    stubApi({ pages: [many()] });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Alpha");
    await user.click(screen.getByTestId("sub-issue-filters-trigger"));
    expect(screen.queryByTestId("sub-issue-filter-team-project")).toBeNull();
    expect(screen.queryByTestId("edition-filter-chips")).toBeNull();
    expect(teamSource).not.toHaveBeenCalled();
  });

  it("filters children by team project and clears the chip (ISS-234)", { timeout: 15_000 }, async () => {
    edition.flags[TEAM_PROJECT_FILTER_FLAG] = true;
    registerEditionIssuesProviders({
      teamProjects: async () => [
        { id: PROJECT, name: "Atlas", identifier: "ATL" },
        { id: OTHER_PROJECT, name: "Betel", identifier: "BET" },
      ],
    });
    stubApi({ pages: [many()] });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Alpha");
    await user.click(screen.getByTestId("sub-issue-filters-trigger"));
    const section = await screen.findByTestId("sub-issue-filter-team-project");
    await user.click(await within(section).findByRole("checkbox", { name: "Betel" }));
    await waitFor(() => expect(screen.queryByText("Alpha")).not.toBeInTheDocument());
    expect(screen.getByText("Gamma")).toBeInTheDocument();
    const chips = await screen.findByTestId("edition-filter-chips");
    expect(chips).toHaveTextContent("Betel");
    await user.click(within(chips).getByRole("button", { name: "Remove Betel filter" }));
    expect(await screen.findByText("Alpha")).toBeInTheDocument();
  });

  it("filters children by work-item type, never matching an untyped row (ISS-234)", { timeout: 15_000 }, async () => {
    edition.flags[ISSUE_TYPE_FILTER_FLAG] = true;
    const typeId = "c23e4567-e89b-12d3-a456-42661417400b";
    registerEditionIssuesProviders({ workItemTypes: async () => [{ id: typeId, name: "Bug" }] });
    stubApi({
      pages: [
        page({
          sub_issues: [
            row({ id: CHILD, name: "Typed", type_id: typeId }),
            row({ id: SECOND, name: "Untyped", sequence_id: 8, type_id: null }),
          ],
          state_distribution: { started: [CHILD, SECOND], completed: [] },
          total_count: 2,
        }),
      ],
    });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Typed");
    await user.click(screen.getByTestId("sub-issue-filters-trigger"));
    const section = await screen.findByTestId("sub-issue-filter-type");
    await user.click(await within(section).findByRole("checkbox", { name: "Bug" }));
    await waitFor(() => expect(screen.queryByText("Untyped")).not.toBeInTheDocument());
    expect(screen.getByText("Typed")).toBeInTheDocument();
  });

  it("keeps filters apart between parents", async () => {
    stubApi({ pages: [many()] });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Alpha");
    await user.click(screen.getByTestId("sub-issue-filters-trigger"));
    await user.click(screen.getByRole("checkbox", { name: "High" }));
    const other = createParentState();
    expect(other.filters.priority).toEqual([]);
    expect(useSubIssuesUiStore.getState().stateFor(PARENT).filters.priority).toEqual(["high"]);
  });
});

describe("SubIssueSection writes", () => {
  it("removes a child from the row menu and toasts", async () => {
    const calls = stubApi({
      pages: [page(), page({ sub_issues: [], state_distribution: { started: [], completed: [] }, total_count: 0 })],
    });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByRole("button", { name: "Actions for ATL-7" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove sub-work item" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.body).toEqual({ parent_id: null });
    await waitFor(() => expect(screen.queryByText("Child one")).not.toBeInTheDocument());
  });

  it("asks before deleting, then destroys the child", async () => {
    stubApi({
      pages: [page(), page({ sub_issues: [], state_distribution: { started: [], completed: [] }, total_count: 0 })],
    });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByRole("button", { name: "Actions for ATL-7" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(
      await screen.findByText('"Child one" will be deleted for everyone. This cannot be undone.')
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByText(/will be deleted/)).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Actions for ATL-7" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    await user.click(
      within(await screen.findByRole("dialog", { name: /Delete this work item/ })).getByRole("button", {
        name: "Delete",
      })
    );
    await waitFor(() => expect(screen.queryByText("Child one")).not.toBeInTheDocument());
  });

  it("patches one field from an inline edit", async () => {
    const calls = stubApi({ pages: [page({ sub_issues: [row({ target_date: "2026-10-20" })] })] });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByTestId("sub-issue-due-date"));
    await user.click(await screen.findByTestId("sub-issue-due-date-clear"));
    await waitFor(() => expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ target_date: null }));
  });

  it("copies the child's link and reports the copy", async () => {
    stubApi();
    const user = userEvent.setup();
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(globalThis.navigator, "clipboard", { value: { writeText }, configurable: true });
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByRole("button", { name: "Actions for ATL-7" }));
    await user.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(writeText).toHaveBeenCalledWith(`${globalThis.location.origin}/acme/browse/ATL-7`);
  });

  it("reports a refused copy instead of failing silently", async () => {
    stubApi();
    const user = userEvent.setup();
    Object.defineProperty(globalThis.navigator, "clipboard", {
      value: { writeText: vi.fn(async () => Promise.reject(new Error("no"))) },
      configurable: true,
    });
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByRole("button", { name: "Actions for ATL-7" }));
    await user.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
  });

  it("draws each row's assignees by member name", async () => {
    stubApi({ pages: [page({ sub_issues: [row({ assignee_ids: [MEMBER] })] })] });
    renderSection();
    expect(await screen.findByText("Child one")).toBeInTheDocument();
    expect(await screen.findByRole("img", { name: "Ada L" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByText(MEMBER)).not.toBeInTheDocument();
  });

  it("labels a child from another project with that project's code", async () => {
    stubApi({
      pages: [
        page({
          sub_issues: [row({ id: THIRD, name: "Gamma", sequence_id: 9, project_id: OTHER_PROJECT })],
          state_distribution: { started: [THIRD] },
          total_count: 1,
        }),
      ],
    });
    renderSection();
    expect(await screen.findByText("Gamma")).toBeInTheDocument();
    expect(await screen.findByText("BET-9")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Gamma/ })).toHaveAttribute("href", "/acme/browse/BET-9");
  });

  it("opens the child on a row click", async () => {
    stubApi();
    const user = userEvent.setup();
    const { onOpen } = renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByRole("link", { name: /Child one/ }));
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: CHILD }), "ATL-7");
  });

  it("opens a foreign child under its own project code", async () => {
    stubApi({
      pages: [
        page({
          sub_issues: [row({ id: THIRD, name: "Gamma", sequence_id: 9, project_id: OTHER_PROJECT })],
          state_distribution: { started: [THIRD] },
          total_count: 1,
        }),
      ],
    });
    const user = userEvent.setup();
    const { onOpen } = renderSection();
    await screen.findByText("Gamma");
    await user.click(screen.getByRole("link", { name: /Gamma/ }));
    await waitFor(() => expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: THIRD }), "BET-9"));
  });

  it("gives guests the rows but no add control or write entries", async () => {
    stubApi();
    renderSection({ editable: false, canWrite: false });
    await screen.findByText("Child one");
    expect(screen.queryByRole("button", { name: "Add a sub-work item" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Actions for ATL-7" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-row-menu")).not.toBeInTheDocument();
  });
});

describe("SubIssueSection dialogs", () => {
  it("opens the create dialog from the section add menu with the parent preset", async () => {
    stubApi();
    const user = userEvent.setup();
    renderSection();
    expect(await screen.findByText("Child one")).toBeInTheDocument();
    await user.click(screen.getByTestId("sub-issues-add"));
    await user.click(await screen.findByRole("menuitem", { name: "Create new" }));
    expect(await screen.findByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
    expect(screen.getByTestId("parent-tag")).toHaveTextContent("Parent work");
    expect(screen.getByRole("button", { name: "Atlas" })).toBeDisabled();
  });

  it("opens the add-existing dialog from the section add menu", async () => {
    stubApi();
    const user = userEvent.setup();
    renderSection();
    expect(await screen.findByText("Child one")).toBeInTheDocument();
    await user.click(screen.getByTestId("sub-issues-add"));
    await user.click(await screen.findByRole("menuitem", { name: "Add existing" }));
    expect(await screen.findByRole("dialog", { name: "Add existing work items" })).toBeInTheDocument();
  });

  it("opens the create dialog for a parent with no children yet", async () => {
    stubApi({ pages: [page({ sub_issues: [], state_distribution: {}, total_count: 0 })] });
    const ref = React.createRef<SubIssueSectionHandle>();
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SubIssueSection
          ref={ref}
          workspaceSlug={WS}
          issue={issue()}
          project={undefined}
          editable
          projectIdentifier="ATL"
          canWrite
          onOpen={vi.fn()}
          createProjects={[PROJECT_ROW]}
        />
      </QueryClientProvider>
    );
    ref.current?.openCreate();
    expect(await screen.findByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
    ref.current?.openExisting();
    expect(await screen.findByRole("dialog", { name: "Add existing work items" })).toBeInTheDocument();
  });

  it("exposes both entries through its handle", async () => {
    stubApi();
    const ref = React.createRef<SubIssueSectionHandle>();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <SubIssueSection
          ref={ref}
          workspaceSlug={WS}
          issue={issue()}
          project={undefined}
          editable
          projectIdentifier="ATL"
          canWrite
          onOpen={vi.fn()}
          createProjects={[PROJECT_ROW]}
        />
      </QueryClientProvider>
    );
    await screen.findByTestId("sub-issues-collapsible");
    ref.current?.openCreate();
    expect(await screen.findByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
    ref.current?.openExisting();
    expect(await screen.findByRole("dialog", { name: "Add existing work items" })).toBeInTheDocument();
  });

  it("passes the parent's project name into the add-existing dialog", async () => {
    stubApi();
    const user = userEvent.setup();
    const project = {
      id: PROJECT,
      name: "Atlas",
      identifier: "ATL",
    } as ProjectDetails;
    renderSection({ project });
    expect(await screen.findByText("Child one")).toBeInTheDocument();
    await user.click(screen.getByTestId("sub-issues-add"));
    await user.click(await screen.findByRole("menuitem", { name: "Add existing" }));
    expect(await screen.findByText("Pick work items from Atlas to place under this one.")).toBeInTheDocument();
  });

  it("describes the row a delete would destroy", async () => {
    stubApi();
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByRole("button", { name: "Actions for ATL-7" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(await screen.findByRole("dialog", { name: /Delete this work item/ })).toBeInTheDocument();
  });
});

describe("SubIssueSection branches", () => {
  it("offers a guest no row menu and sends no write", async () => {
    const calls = stubApi();
    renderSection({ editable: false });
    await screen.findByText("Child one");
    expect(screen.queryByRole("button", { name: "Actions for ATL-7" })).not.toBeInTheDocument();
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("draws the loading row until the first page arrives", async () => {
    const waiting: (() => void)[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, 200, { csrf_token: "csrf" });
      if (url.includes("/sub-issues/")) {
        await new Promise<void>((resolve) => waiting.push(resolve));
        return ok(url, 200, page());
      }
      if (url.endsWith("/states/")) return ok(url, 200, [state]);
      return ok(url, 200, {});
    }) as never);
    renderSection();
    expect(await screen.findByText("Loading sub-work items")).toBeInTheDocument();
    for (const release of waiting) release();
    expect(await screen.findByText("Child one")).toBeInTheDocument();
  });

  it("names the read failure and retries it", async () => {
    let fail = true;
    const calls: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, 200, { csrf_token: "csrf" });
      calls.push(url);
      if (url.includes("/sub-issues/")) {
        if (fail) return ok(url, 500, { detail: "down" });
        return ok(url, 200, page());
      }
      if (url.endsWith("/states/")) return ok(url, 200, [state]);
      return ok(url, 200, {});
    }) as never);
    const user = userEvent.setup();
    renderSection();
    expect(await screen.findByText("Couldn't load the sub-work items.")).toBeInTheDocument();
    fail = false;
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Child one")).toBeInTheDocument();
    expect(calls.filter((url) => url.includes("/sub-issues/")).length).toBeGreaterThan(1);
  });

  it("shows the load-more spinner while the next page is in flight", async () => {
    let release: () => void = () => {};
    let call = 0;
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return ok(url, 200, { csrf_token: "csrf" });
      if (url.includes("/sub-issues/")) {
        call += 1;
        if (call === 1) return ok(url, 200, page({ next_cursor: "c2", total_count: 2 }));
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        return ok(url, 200, page({ sub_issues: [row({ id: SECOND, name: "Child two", sequence_id: 8 })] }));
      }
      if (url.endsWith("/states/")) return ok(url, 200, [state]);
      return ok(url, 200, {});
    }) as never);
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByTestId("sub-issues-load-more"));
    const busy = await screen.findByText("Loading more…");
    expect(busy.closest("button")).toBeDisabled();
    release();
    expect(await screen.findByText("Child two")).toBeInTheDocument();
  });

  it("groups an unassigned child under None", async () => {
    stubApi({ pages: [page({ sub_issues: [row({ assignee_ids: [] })] })] });
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByTestId("sub-issue-display-trigger"));
    await user.click(within(screen.getByTestId("sub-issue-display-group")).getByRole("radio", { name: "Assignees" }));
    const headers = await screen.findAllByTestId("sub-issue-group-header");
    expect(headers.map((header) => header.textContent)).toEqual(["None1"]);
  });

  it("dismisses the delete confirmation without destroying anything", async () => {
    const calls = stubApi();
    const user = userEvent.setup();
    renderSection();
    await screen.findByText("Child one");
    await user.click(screen.getByRole("button", { name: "Actions for ATL-7" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", {
      name: /Delete this work item/,
    });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
    expect(screen.getByText("Child one")).toBeInTheDocument();
  });
});
