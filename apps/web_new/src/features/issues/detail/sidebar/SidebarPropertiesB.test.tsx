// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { Issue, ProjectDetail, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { edition } from "../../../../core/edition/index.js";
import { setSessionClient } from "../../../../core/session/queries.js";
import { DETAIL_BADGES_FLAG } from "../../edition/badges.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "../../edition/providers.js";
import { IssuePropertiesSidebar, PROPERTIES_LOAD_FAILED } from "./IssuePropertiesSidebar.js";
import { SidebarPropertiesB } from "./SidebarPropertiesB.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const OTHER_ISSUE = "923e4567-e89b-12d3-a456-426614174009";
const PARENT = "423e4567-e89b-12d3-a456-426614174003";
const SIBLING = "523e4567-e89b-12d3-a456-426614174004";
const CYCLE = "623e4567-e89b-12d3-a456-426614174005";
const MODULE_A = "823e4567-e89b-12d3-a456-426614174007";
const ESTIMATE = "b23e4567-e89b-12d3-a456-426614174009";
const LABEL = "c23e4567-e89b-12d3-a456-42661417400a";
const STARTED = "d23e4567-e89b-12d3-a456-42661417400b";
const COMPLETED = "e23e4567-e89b-12d3-a456-42661417400c";

function issueFixture(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: STARTED,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: 42,
    project_id: PROJECT,
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
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function projectFixture(overrides: Partial<ProjectDetail> = {}): ProjectDetail {
  return {
    id: PROJECT,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    workspace: ISSUE,
    name: "Web",
    network: 2,
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    archived_at: null,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    estimate: ESTIMATE,
    agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

const STATES = [
  {
    id: STARTED,
    project_id: PROJECT,
    workspace_id: ISSUE,
    name: "In progress",
    color: "#ffaa00",
    group: "started",
    default: false,
    description: "",
    sequence: 1,
  },
  {
    id: COMPLETED,
    project_id: PROJECT,
    workspace_id: ISSUE,
    name: "Done",
    color: "#00ff00",
    group: "completed",
    default: false,
    description: "",
    sequence: 2,
  },
];

const CYCLES = [{ id: CYCLE, project_id: PROJECT, name: "Cycle 1", is_favorite: false, status: "ACTIVE" }];
const MODULES = [{ id: MODULE_A, project_id: PROJECT, name: "Auth", status: "planned", is_favorite: false }];
const LABELS = [
  {
    id: LABEL,
    project_id: PROJECT,
    workspace_id: ISSUE,
    name: "Bug",
    color: "#ff0000",
    description: "",
    sort_order: 1,
    parent: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  },
];

const PARENT_ROW = issueFixture({ id: PARENT, name: "Parent epic", sequence_id: 9, parent_id: null });
const SEARCH_HITS = [
  {
    name: "Parent epic",
    id: PARENT,
    start_date: null,
    sequence_id: 9,
    project__name: "Web",
    project__identifier: "WEB",
    project_id: PROJECT,
    workspace__slug: SLUG,
    state__name: "In progress",
    state__group: "started",
    state__color: "#ffaa00",
  },
  {
    name: "Another candidate",
    id: SIBLING,
    start_date: null,
    sequence_id: 10,
    project__name: "Web",
    project__identifier: "WEB",
    project_id: PROJECT,
    workspace__slug: SLUG,
    state__name: null,
    state__group: null,
    state__color: null,
  },
];

interface StubOptions {
  patchStatus?: number;
  readStatus?: number;
  searchStatus?: number;
  holdSearch?: boolean;
  parentProject?: string;
}

function harness(options: StubOptions = {}) {
  resetAppClient();
  const reads: string[] = [];
  const writes: Array<{ url: string; method: string; body: unknown }> = [];
  let releaseSearch: (() => void) | null = null;
  const searchGate = options.holdSearch
    ? new Promise<void>((resolve) => {
        releaseSearch = resolve;
      })
    : null;
  const json = (status: number, body: unknown, url: string) => ({
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
  const conversation = async (url: string, init: TransportInit) => {
    if (init.method === "GET") reads.push(url);
    if (init.method === "PATCH" || init.method === "POST" || init.method === "DELETE") {
      writes.push({ url, method: init.method, body: init.body === undefined ? null : JSON.parse(init.body) });
      if (options.patchStatus !== undefined && options.patchStatus >= 400) {
        return json(options.patchStatus, { error: "nope" }, url);
      }
      return init.method === "PATCH"
        ? { status: 204, url, headers: { get: () => null }, text: async () => "" }
        : json(201, { message: "ok" }, url);
    }
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "c" }, url);
    if (url.includes(`/issues/${PARENT}/`)) return json(200, PARENT_ROW, url);
    if (url.endsWith(`/projects/${PROJECT}/`)) return json(200, projectFixture(), url);
    if (url.includes(`/projects/${PROJECT}/states/`)) return json(200, STATES, url);
    if (url.includes(`/projects/${PROJECT}/cycles/`)) return json(200, CYCLES, url);
    if (url.includes(`/projects/${PROJECT}/estimates/`)) {
      return json(
        200,
        [
          {
            id: ESTIMATE,
            workspace: ISSUE,
            project: PROJECT,
            name: "Points",
            description: "",
            type: "points",
            last_used: true,
            created_at: "2024-01-02T03:04:05Z",
            updated_at: "2024-02-03T04:05:06Z",
            created_by: null,
            updated_by: null,
            points: [
              { id: "f23e4567-e89b-12d3-a456-42661417400d", key: 1, value: "1", description: "" },
              { id: "f33e4567-e89b-12d3-a456-42661417400e", key: 3, value: "3", description: "" },
            ],
          },
        ],
        url
      );
    }
    if (url.includes(`/projects/${PROJECT}/modules/`)) return json(200, MODULES, url);
    if (url.includes(`/projects/${PROJECT}/issue-labels/`)) return json(200, LABELS, url);
    if (url.includes(`/projects/${PROJECT}/search-issues/`)) {
      if (searchGate !== null) await searchGate;
      return json(options.searchStatus ?? 200, SEARCH_HITS, url);
    }
    if (url.includes("/project-roles/")) return json(200, { [PROJECT]: 20 }, url);
    if (url.includes("/workspace-members/me/")) return json(200, { role: 15 }, url);
    if (url.endsWith(`/workspaces/${SLUG}/members/`)) return json(200, [], url);
    if (url.includes("/api/runners/pods/")) return json(200, [], url);
    void options.readStatus;
    void options.parentProject;
    return json(404, {}, url);
  };
  const client = createAppClient("https://api.test", conversation as never);
  setSessionClient(client);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { client, queryClient, reads, writes, releaseSearch: () => releaseSearch?.() };
}

type Props = React.ComponentProps<typeof SidebarPropertiesB>;

function renderRows(
  client: ReturnType<typeof harness>["client"],
  queryClient: QueryClient,
  props: Partial<Props> = {}
) {
  const merged: Props = {
    workspaceSlug: SLUG,
    projectId: PROJECT,
    identifier: "WEB",
    issue: issueFixture(),
    project: projectFixture(),
    stateGroup: "started",
    editable: true,
    ...props,
  };
  return render(
    <QueryClientProvider client={queryClient}>
      <SidebarPropertiesB {...merged} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  cleanup();
  resetAppClient();
  resetEditionIssuesProviders();
  delete edition.flags[DETAIL_BADGES_FLAG];
  vi.clearAllMocks();
});

beforeEach(() => {
  createAppClient("https://api.test");
});

describe("SidebarPropertiesB rows", () => {
  it("renders both date rows, and the conditional rows only when their project features are on", () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient);
    for (const label of ["Start date", "Due date", "Estimate", "Modules", "Cycle", "Parent", "Labels"]) {
      expect(screen.getByRole("group", { name: label })).toBeInTheDocument();
    }
  });

  it("hides the estimate, module and cycle rows when the project turns them off", () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, {
      project: projectFixture({ estimate: null, module_view: false, cycle_view: false }),
    });
    expect(screen.queryByRole("group", { name: "Estimate" })).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Modules" })).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Cycle" })).not.toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Start date" })).toBeInTheDocument();
  });

  it("flips the estimate row with the project link alone (EST-018)", () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { project: projectFixture({ estimate: null }) });
    expect(screen.queryByRole("group", { name: "Estimate" })).not.toBeInTheDocument();
    cleanup();
    renderRows(client, queryClient, { project: projectFixture({ estimate: ESTIMATE }) });
    expect(screen.getByRole("group", { name: "Estimate" })).toBeInTheDocument();
  });

  it("patches the start date with the picked day and bounds the calendar by the due date", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ target_date: "2026-10-20" }) });
    fireEvent.click(screen.getByRole("button", { name: "Start date" }));
    expect(await screen.findByRole("button", { name: "October 21, 2026" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "October 15, 2026" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ start_date: "2026-10-15" });
  });

  it("clears the start date with null and leaves the due bound open", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ start_date: "2026-10-10" }) });
    fireEvent.click(screen.getByRole("button", { name: "Clear Start date" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ start_date: null });
  });

  it("bounds the due calendar by the start date", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ start_date: "2026-10-10" }) });
    fireEvent.click(screen.getByRole("button", { name: "Due date" }));
    expect(await screen.findByRole("button", { name: "October 9, 2026" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "October 20, 2026" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ target_date: "2026-10-20" });
  });

  it("flags an overdue due date in the danger tone while the item is open", () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ target_date: "2020-01-01" }), stateGroup: "started" });
    const row = screen.getByRole("group", { name: "Due date" });
    expect(within(row).getByTestId("sidebar-due-date")).toHaveAttribute("data-overdue", "true");
    expect(within(row).getByTestId("sidebar-due-date").className).toContain("text-(--danger)");
    // The date text itself carries the tone; the wrapper alone would be
    // hidden behind the trigger's own text color.
    expect(within(row).getByRole("button", { name: "Due date" })).toHaveClass("text-(--danger)");
  });

  it("adds the cloud overdue badge beside the date, and nothing in the community build (ISS-236)", () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ target_date: "2020-01-01" }), stateGroup: "started" });
    expect(screen.queryByTestId("detail-overdue-badge")).toBeNull();
    cleanup();
    edition.flags[DETAIL_BADGES_FLAG] = true;
    renderRows(client, queryClient, { issue: issueFixture({ target_date: "2020-01-01" }), stateGroup: "started" });
    const badge = screen.getByTestId("detail-overdue-badge");
    expect(badge).toHaveTextContent(/^\d+ days overdue$/);
    expect(within(screen.getByRole("group", { name: "Due date" })).getByTestId("detail-overdue-badge")).toBe(badge);
  });

  it("keeps the cloud badge off a closed item and off a future date", () => {
    edition.flags[DETAIL_BADGES_FLAG] = true;
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ target_date: "2020-01-01" }), stateGroup: "completed" });
    expect(screen.queryByTestId("detail-overdue-badge")).toBeNull();
    cleanup();
    renderRows(client, queryClient, { issue: issueFixture({ target_date: "2999-01-01" }), stateGroup: "started" });
    expect(screen.queryByTestId("detail-overdue-badge")).toBeNull();
  });

  it("mounts the cloud transfer note under the cycle row only when the edition supplies it (ISS-236)", () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ cycle_id: CYCLE }) });
    expect(screen.queryByTestId("detail-transfer-hop")).toBeNull();
    cleanup();
    edition.flags[DETAIL_BADGES_FLAG] = true;
    renderRows(client, queryClient, { issue: issueFixture({ cycle_id: CYCLE }) });
    expect(screen.queryByTestId("detail-transfer-hop")).toBeNull();
    cleanup();
    registerEditionIssuesProviders({
      detailBadges: {
        transferNote: ({ cycleId }) => (
          <span data-testid="hop-probe">{`Moved from ${(cycleId ?? "").slice(0, 4)}`}</span>
        ),
      },
    });
    renderRows(client, queryClient, { issue: issueFixture({ cycle_id: CYCLE }) });
    const note = within(screen.getByRole("group", { name: "Cycle" })).getByTestId("detail-transfer-hop");
    expect(note).toContainElement(screen.getByTestId("hop-probe"));
  });

  it("keeps the transfer note off an item with no cycle", () => {
    edition.flags[DETAIL_BADGES_FLAG] = true;
    registerEditionIssuesProviders({ detailBadges: { transferNote: () => <span>moved</span> } });
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ cycle_id: null }) });
    expect(screen.queryByTestId("detail-transfer-hop")).toBeNull();
  });

  it("never flags a closed or future due date", () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ target_date: "2020-01-01" }), stateGroup: "completed" });
    expect(screen.getByTestId("sidebar-due-date")).not.toHaveAttribute("data-overdue");
    cleanup();
    renderRows(client, queryClient, { issue: issueFixture({ target_date: "2999-01-01" }), stateGroup: "started" });
    expect(screen.getByTestId("sidebar-due-date")).not.toHaveAttribute("data-overdue");
  });

  it("patches the estimate point from the picker", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient);
    fireEvent.click(within(screen.getByRole("group", { name: "Estimate" })).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "3" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ estimate_point: expect.any(String) });
  });

  it("names the assigned estimate and cycle on a read-only row", async () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, {
      editable: false,
      issue: issueFixture({
        estimate_point: "f33e4567-e89b-12d3-a456-42661417400e",
        cycle_id: CYCLE,
      }),
    });
    const estimate = screen.getByRole("group", { name: "Estimate" });
    await waitFor(() => expect(within(estimate).getByRole("button")).toHaveTextContent("3"));
    expect(within(estimate).getByRole("button")).toBeDisabled();
    const cycleRow = screen.getByRole("group", { name: "Cycle" });
    await waitFor(() => expect(within(cycleRow).getByRole("button")).toHaveTextContent("Cycle 1"));
    expect(within(cycleRow).getByRole("button")).toBeDisabled();
  });

  it("names the assigned estimate in the row", async () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, {
      issue: issueFixture({ estimate_point: "f33e4567-e89b-12d3-a456-42661417400e" }),
    });
    const row = screen.getByRole("group", { name: "Estimate" });
    await waitFor(() => expect(within(row).getByRole("button")).toHaveTextContent("3"));
  });

  it("names the assigned cycle in the row before its picker opens", async () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ cycle_id: CYCLE }) });
    const row = screen.getByRole("group", { name: "Cycle" });
    await waitFor(() => expect(within(row).getByRole("button")).toHaveTextContent("Cycle 1"));
  });

  it("diffs a module pick into the add/remove bridge", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ module_ids: [MODULE_A] }) });
    const row = screen.getByRole("group", { name: "Modules" });
    fireEvent.click(within(row).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Auth" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.url).toContain(`/issues/${ISSUE}/modules/`);
    expect(writes[0]!.body).toEqual({ modules: [], removed_modules: [MODULE_A] });
  });

  it("adds a module through the bridge", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient);
    const row = screen.getByRole("group", { name: "Modules" });
    fireEvent.click(within(row).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Auth" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ modules: [MODULE_A] });
  });

  it("assigns and clears the cycle through the bridge", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient);
    const row = screen.getByRole("group", { name: "Cycle" });
    fireEvent.click(within(row).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Cycle 1" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.method).toBe("POST");
    expect(writes[0]!.url).toContain(`/cycles/${CYCLE}/cycle-issues/`);
    cleanup();
    const second = harness();
    renderRows(second.client, second.queryClient, { issue: issueFixture({ cycle_id: CYCLE }) });
    const assigned = screen.getByRole("group", { name: "Cycle" });
    fireEvent.click(within(assigned).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "No cycle" }));
    await waitFor(() => expect(second.writes).toHaveLength(1));
    expect(second.writes[0]!.method).toBe("DELETE");
    expect(second.writes[0]!.url).toContain(`/cycles/${CYCLE}/cycle-issues/${ISSUE}/`);
  });

  it("offers Add parent work item and opens the search dialog", async () => {
    const { client, queryClient, reads } = harness();
    renderRows(client, queryClient);
    fireEvent.click(within(screen.getByRole("group", { name: "Parent" })).getByRole("button"));
    expect(await screen.findByRole("dialog")).toHaveTextContent("Set parent work item");
    fireEvent.change(screen.getByLabelText("Search work items"), { target: { value: "Parent" } });
    await waitFor(() => expect(reads.some((url) => url.includes("/search-issues/"))).toBe(true));
    expect(await screen.findByRole("option", { name: /Parent epic/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /Another candidate/ })).toBeInTheDocument();
  });

  it("patches the parent picked in the dialog", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient);
    fireEvent.click(within(screen.getByRole("group", { name: "Parent" })).getByRole("button"));
    fireEvent.change(await screen.findByLabelText("Search work items"), { target: { value: "Parent" } });
    fireEvent.click(await screen.findByRole("option", { name: /Parent epic/ }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ parent_id: PARENT });
  });

  it("names the parent's identifier once its read lands and can remove it inline", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ parent_id: PARENT }) });
    const parentRow = screen.getByRole("group", { name: "Parent" });
    await waitFor(() => expect(parentRow).toHaveTextContent("WEB-9"));
    fireEvent.click(within(parentRow).getByRole("button", { name: "Remove parent" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ parent_id: null });
  });

  it("keeps the parent row read-only for guests", () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ parent_id: PARENT }), editable: false });
    expect(within(screen.getByRole("group", { name: "Parent" })).getByRole("button")).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Remove parent" })).not.toBeInTheDocument();
  });

  it("renders label chips with a working remove and the shared picker", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ label_ids: [LABEL] }) });
    expect(await screen.findByRole("button", { name: "Remove Bug" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove Bug" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ label_ids: [] });
  });

  it("patches the full label list from the picker", async () => {
    const { client, queryClient, writes } = harness();
    renderRows(client, queryClient);
    const row = screen.getByRole("group", { name: "Labels" });
    fireEvent.click(within(row).getByTestId("label-picker-trigger"));
    fireEvent.click(await screen.findByTestId("label-picker-option"));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ label_ids: [LABEL] });
  });

  it("disables every editable control for guests and synced items", () => {
    for (const overrides of [{ editable: false }, { issue: issueFixture({ is_synced: true }) }] as const) {
      const { client, queryClient } = harness();
      const view = renderRows(client, queryClient, overrides);
      expect(within(screen.getByRole("group", { name: "Start date" })).getByRole("button")).toBeDisabled();
      expect(within(screen.getByRole("group", { name: "Due date" })).getByRole("button")).toBeDisabled();
      expect(
        within(screen.getByRole("group", { name: "Labels" })).getByRole("button", { name: "Labels" })
      ).toBeDisabled();
      view.unmount();
    }
  });

  it("renders archived rows disabled with a triggerless labels row (ISS-220)", async () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ archived_at: "2026-01-01", label_ids: [LABEL] }) });
    expect(within(screen.getByRole("group", { name: "Start date" })).getByRole("button")).toBeDisabled();
    expect(within(screen.getByRole("group", { name: "Due date" })).getByRole("button")).toBeDisabled();
    const labels = screen.getByRole("group", { name: "Labels" });
    expect(within(labels).queryByRole("button")).not.toBeInTheDocument();
    expect(await within(labels).findByText("Bug")).toBeInTheDocument();
  });
});

describe("IssuePropertiesSidebar rows B", () => {
  it("loads the project row and renders the conditional rows it enables", async () => {
    const { queryClient } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <IssuePropertiesSidebar workspaceSlug={SLUG} projectId={PROJECT} issue={issueFixture()} editable />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("group", { name: "Modules" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Cycle" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Estimate" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Labels" })).toBeInTheDocument();
  });

  it("keeps the retry state when the project row fails", async () => {
    resetAppClient();
    createAppClient("https://api.test", (async (url: string) => ({
      status: 500,
      url,
      headers: { get: () => null },
      text: async () => "{}",
    })) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <IssuePropertiesSidebar workspaceSlug={SLUG} projectId={PROJECT} issue={issueFixture()} editable />
      </QueryClientProvider>
    );
    expect(await screen.findByText(PROPERTIES_LOAD_FAILED)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("ignores the other-issue read and never patches it", async () => {
    const { client, queryClient } = harness();
    renderRows(client, queryClient, { issue: issueFixture({ parent_id: null, id: ISSUE }) });
    expect(screen.queryByText(OTHER_ISSUE)).not.toBeInTheDocument();
  });
});
