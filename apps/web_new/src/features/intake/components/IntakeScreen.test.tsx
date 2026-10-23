// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake screen (INT-001–005, 008, 012, 031). Reads are stubbed at the
// transport so the screen exercises its real queries: the gate, the tab
// set sent on the wire, the badge from the read's total, auto-selection,
// and the empty state for each situation.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn();
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => navigate,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { IntakeSearch, IntakeSearchWriteOptions } from "../lib/search.js";
import { IntakeScreen } from "./IntakeScreen.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_1 = "11111111-1111-4111-8111-111111111111";
const ISSUE_2 = "22222222-2222-4222-8222-222222222222";
const CLOSED_1 = "33333333-3333-4333-8333-333333333333";
const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const ME_ID = "99999999-9999-4999-8999-999999999999";
const MEMBER_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const LABEL_ID = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

interface FakeRow {
  /** Full work-item UUID; the list contract validates it. */
  issueId: string;
  name: string;
  status?: number;
  priority?: string;
  labelIds?: string[];
  createdBy?: string | null;
}

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function projectRow(inboxView = true, name = "Apollo") {
  return {
    id: PROJECT_ID,
    name,
    identifier: "APO",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: false,
    inbox_view: inboxView,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
  };
}

function intakeRow(row: FakeRow, index: number) {
  return {
    id: `aaaaaaaa-aaaa-4aaa-8aaa-${String(index).padStart(12, "0")}`,
    status: row.status ?? -2,
    duplicate_to: null,
    snoozed_till: null,
    source: null,
    created_by: null,
    issue: {
      id: row.issueId,
      name: row.name,
      priority: row.priority ?? "none",
      sequence_id: 1,
      project_id: PROJECT_ID,
      created_at: "2026-10-01T00:00:00Z",
      label_ids: row.labelIds ?? [],
      created_by: row.createdBy === undefined ? null : row.createdBy,
    },
  };
}

function listResponse(rows: FakeRow[], page: { nextCursor?: string; nextPage?: boolean } = {}) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: rows.length,
    next_cursor: page.nextCursor ?? "",
    prev_cursor: "",
    next_page_results: page.nextPage ?? false,
    prev_page_results: false,
    count: rows.length,
    total_results: rows.length,
    results: rows.map(intakeRow),
  };
}

function labelRow() {
  return {
    id: LABEL_ID,
    project_id: PROJECT_ID,
    workspace_id: WS_ID,
    name: "Alpha",
    color: "#ff0000",
    parent: null,
    sort_order: 0,
  };
}

function projectMemberRow(memberId: string, rowId: string) {
  return {
    id: rowId,
    role: 15,
    member: memberId,
    project: PROJECT_ID,
    original_role: 15,
    created_at: "2026-10-01T00:00:00Z",
  };
}

function directoryRow(memberId: string, rowId: string, displayName: string, email: string) {
  return {
    id: rowId,
    member: {
      id: memberId,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: displayName,
      is_bot: false,
      email,
    },
    role: 15,
    is_active: true,
  };
}

interface Server {
  inboxView: boolean;
  projectName: string;
  pending: FakeRow[];
  closed: FakeRow[];
  listRequests: string[];
  writes: unknown[];
  listFails?: boolean;
  detailStatus?: number;
  rolesFail?: boolean;
  roles?: Record<string, number>;
  /** Cursor-paged rows for the paging tests; empty disables paging. */
  paged: FakeRow[];
  /** Deferred list reads wait on this gate when set. */
  listGate?: Promise<void>;
}

function detailPayload(issueId: string) {
  const row = [...server.pending, ...server.closed].find((candidate) => candidate.issueId === issueId);
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
    status: row?.status ?? -2,
    duplicate_to: null,
    snoozed_till: null,
    duplicate_issue_detail: null,
    source: null,
    issue: {
      id: issueId,
      name: row?.name ?? "Unknown",
      state_id: null,
      sort_order: 1,
      completed_at: null,
      priority: "none",
      sequence_id: 7,
      project_id: PROJECT_ID,
      parent_id: null,
      label_ids: [],
      assignee_ids: [],
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-02T00:00:00Z",
      created_by: null,
      updated_by: null,
      is_draft: false,
      archived_at: null,
      complexity_score: 0,
      agent_executor: null,
      is_synced: false,
      description_html: "<p>Body</p>",
      agent_ticker: null,
      agent_status: null,
      relations_summary: { blocked_by: [], blocking: [] },
      has_open_blockers: false,
    },
  };
}

const ME_PAYLOAD = {
  id: "023e4567-e89b-12d3-a456-426614174009",
  email: "me@example.com",
  username: "me",
  first_name: "Me",
  last_name: "M",
  display_name: "Me M",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00.000Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

function installServer(server: Server): void {
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const parsed = new URL(url);
    const path = parsed.pathname;
    if (path.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/`) {
      return jsonResponse(url, 200, projectRow(server.inboxView, server.projectName));
    }
    if (path.endsWith("/project-roles/")) {
      if (server.rolesFail) return jsonResponse(url, 500, { error: "boom" });
      return jsonResponse(url, 200, server.roles ?? { [PROJECT_ID]: 20 });
    }
    if (path.endsWith("/api/users/me/workspaces/")) {
      return jsonResponse(url, 200, [
        { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
      ]);
    }
    if (path.endsWith("/api/users/me/")) return jsonResponse(url, 200, ME_PAYLOAD);
    if (path.endsWith("/issue-labels/")) return jsonResponse(url, 200, [labelRow()]);
    if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/members/`) {
      return jsonResponse(url, 200, [
        projectMemberRow(ME_ID, "cccccccc-cccc-4ccc-8ccc-000000000001"),
        projectMemberRow(MEMBER_ID, "cccccccc-cccc-4ccc-8ccc-000000000002"),
      ]);
    }
    if (path === "/api/workspaces/acme/members/") {
      return jsonResponse(url, 200, [
        directoryRow(ME_ID, "dddddddd-dddd-4ddd-8ddd-000000000001", "Me Mine", "me@example.com"),
        directoryRow(MEMBER_ID, "dddddddd-dddd-4ddd-8ddd-000000000002", "Rin", "rin@example.com"),
      ]);
    }
    if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/intake-issues/`) {
      server.listRequests.push(parsed.search);
      if (server.listGate !== undefined) await server.listGate;
      if (server.listFails) return jsonResponse(url, 500, { error: "boom" });
      if (server.paged.length > 0) {
        const cursor = parsed.searchParams.get("cursor");
        if (cursor === null) {
          return jsonResponse(
            url,
            200,
            listResponse(server.paged.slice(0, 10), { nextCursor: "cursor-1", nextPage: true })
          );
        }
        return jsonResponse(url, 200, listResponse(server.paged.slice(10), {}));
      }
      const status = parsed.searchParams.get("status") ?? "";
      let rows = status.includes("1") ? server.closed : server.pending;
      const priorities = parsed.searchParams.get("priority");
      if (priorities !== null) {
        const wanted = priorities.split(",");
        rows = rows.filter((row) => wanted.includes(row.priority ?? "none"));
      }
      if (parsed.searchParams.get("order_by") === "issue__sequence_id") rows = [...rows].reverse();
      return jsonResponse(url, 200, listResponse(rows));
    }
    const detailPrefix = `/api/workspaces/acme/projects/${PROJECT_ID}/intake-issues/`;
    if (path.startsWith(detailPrefix) && path !== detailPrefix) {
      const issueId = path.slice(detailPrefix.length, -1);
      if ((init?.method ?? "GET") === "PATCH") {
        const body = JSON.parse(init?.body ?? "{}") as { status?: number };
        server.writes.push(body);
        // A triage move takes the row out of its tab; the echo carries
        // the new status while the row still resolves by id.
        server.pending = server.pending.filter((row) => row.issueId !== issueId);
        const echo = detailPayload(issueId);
        return jsonResponse(url, 200, { ...echo, status: body.status ?? echo.status });
      }
      const status = server.detailStatus ?? 200;
      if (status >= 400) return jsonResponse(url, status, {});
      return jsonResponse(url, 200, detailPayload(issueId));
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
}

type SearchWriter = (next: IntakeSearch, options?: IntakeSearchWriteOptions) => void;

function renderScreen(search: IntakeSearch, onSearchChange: SearchWriter = vi.fn(), onCreate?: () => void) {
  return renderScreenWithRerender(search, onSearchChange, onCreate).onSearchChange;
}

function renderScreenWithRerender(search: IntakeSearch, onSearchChange: SearchWriter = vi.fn(), onCreate?: () => void) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  // The session reads resolve from the cache so no transport stub is needed.
  client.setQueryData(["session", "me"], { id: ME_ID });
  client.setQueryData(["session", "profile"], { start_of_the_week: 0 });
  const tree = (next: IntakeSearch) => (
    <QueryClientProvider client={client}>
      <IntakeScreen
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        search={next}
        onSearchChange={onSearchChange}
        onCreate={onCreate}
      />
    </QueryClientProvider>
  );
  const view = render(tree(search));
  return {
    onSearchChange,
    rerender: (next: IntakeSearch) => view.rerender(tree(next)),
  };
}

let server: Server;

beforeEach(() => {
  navigate.mockReset();
  document.title = "";
  server = { inboxView: true, projectName: "Apollo", pending: [], closed: [], listRequests: [], writes: [], paged: [] };
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("IntakeScreen shell (INT-001/INT-003)", () => {
  it("renders the two-pane shell under the trail with the pick placeholder", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open" });
    expect(await screen.findByTestId("intake-shell")).toBeInTheDocument();
    expect(screen.getByRole("tabpanel")).toBeInTheDocument();
    expect(screen.getByTestId("intake-placeholder")).toBeInTheDocument();
    expect(screen.getByTestId("intake-breadcrumb")).toHaveTextContent("Intake");
  });

  it("replaces the pick placeholder with the request detail once a request is selected", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    expect(await screen.findByTestId("intake-detail")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-placeholder")).toBeNull();
  });

  it("names the document after the project plus Intake", async () => {
    installServer(server);
    renderScreen({ currentTab: "open" });
    await screen.findByTestId("intake-shell");
    await waitFor(() => expect(document.title).toBe("Apollo - Intake"));
  });

  it("falls back to the product name plus Intake while the project loads", () => {
    installServer(server);
    renderScreen({ currentTab: "open" });
    expect(document.title).toBe("Pi Dash - Intake");
  });

  it("reports a project read failure instead of a shell", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      const path = new URL(url).pathname;
      if (path.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/`) return jsonResponse(url, 404, { error: "gone" });
      if (path.endsWith("/api/users/me/workspaces/")) return jsonResponse(url, 200, []);
      return jsonResponse(url, 404, { error: "missing" });
    }) as never);
    renderScreen({ currentTab: "open" });
    expect(await screen.findByText("The project could not be loaded.")).toBeInTheDocument();
  });
});

describe("IntakeScreen gate (INT-002)", () => {
  it("shows the gate and reads no list while the feature is off", async () => {
    server.inboxView = false;
    installServer(server);
    renderScreen({ currentTab: "open" });
    expect(await screen.findByRole("heading", { name: /^Intake is paused for this project/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Manage features" })).toBeEnabled();
    expect(server.listRequests).toEqual([]);
    expect(screen.queryByTestId("intake-shell")).not.toBeInTheDocument();
  });

  it("sends an admin to the project's intake feature leaf", async () => {
    server.inboxView = false;
    installServer(server);
    renderScreen({ currentTab: "open" });
    (await screen.findByRole("button", { name: "Manage features" })).click();
    expect(navigate).toHaveBeenCalledWith({ to: `/acme/settings/projects/${PROJECT_ID}/features/intake` });
  });

  it("keeps the shortcut disabled for a non-admin member", async () => {
    server.inboxView = false;
    installServer(server);
    // The role read answers member standing for this case.
    createAppClient("https://api.test", (async (url: string) => {
      const path = new URL(url).pathname;
      if (path.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/`) return jsonResponse(url, 200, projectRow(false));
      if (path.endsWith("/project-roles/")) return jsonResponse(url, 200, { [PROJECT_ID]: 15 });
      if (path.endsWith("/api/users/me/workspaces/")) {
        return jsonResponse(url, 200, [
          { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 2, role: 15 },
        ]);
      }
      return jsonResponse(url, 404, { error: "missing" });
    }) as never);
    renderScreen({ currentTab: "open" });
    expect(await screen.findByRole("button", { name: "Manage features" })).toBeDisabled();
  });

  it("keeps the shortcut disabled when the roles read names no stand for the project", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      const path = new URL(url).pathname;
      if (path.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/`) return jsonResponse(url, 200, projectRow(false));
      if (path.endsWith("/project-roles/")) return jsonResponse(url, 200, {});
      if (path.endsWith("/api/users/me/workspaces/")) {
        return jsonResponse(url, 200, [
          { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 2, role: 20 },
        ]);
      }
      return jsonResponse(url, 404, { error: "missing" });
    }) as never);
    renderScreen({ currentTab: "open" });
    expect(await screen.findByRole("button", { name: "Manage features" })).toBeDisabled();
  });

  it("falls back to the slug in the trail while the workspace name is unknown", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    createAppClient("https://api.test", (async (url: string) => {
      const path = new URL(url).pathname;
      if (path.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/`) return jsonResponse(url, 200, projectRow(true));
      if (path.endsWith("/project-roles/")) return jsonResponse(url, 200, { [PROJECT_ID]: 20 });
      if (path.endsWith("/api/users/me/workspaces/")) return jsonResponse(url, 200, []);
      if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/intake-issues/`) {
        return jsonResponse(url, 200, listResponse(server.pending));
      }
      return jsonResponse(url, 404, { error: "missing" });
    }) as never);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    expect(screen.getByTestId("intake-breadcrumb")).toHaveTextContent("acme");
  });
});

describe("IntakeScreen tabs and reads (INT-004/INT-012)", () => {
  it("reads Pending only on Open and badges the tab with the read total", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    installServer(server);
    renderScreen({ currentTab: "open" }, () => {});
    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    expect(screen.getByTestId("intake-open-count")).toHaveTextContent("2");
    expect(server.listRequests).toHaveLength(1);
    expect(new URLSearchParams(server.listRequests[0] ?? "").get("status")).toBe("-2");
  });

  it("leaves the address alone when the active tab is clicked again", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    screen.getByRole("tab", { name: /Open/ }).click();
    expect(onSearchChange).not.toHaveBeenCalled();
  });

  it("writes the tab switch to the address and drops the selection", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    server.closed = [{ issueId: CLOSED_1, name: "Closed one", status: 1 }];
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    screen.getByRole("tab", { name: /Closed/ }).click();
    expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "closed" });
  });

  it("re-reads Open when the screen switches back to it", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    server.closed = [{ issueId: CLOSED_1, name: "Closed one", status: 1 }];
    installServer(server);
    const { rerender } = renderScreenWithRerender({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    screen.getByRole("tab", { name: /Closed/ }).click();
    rerender({ currentTab: "closed" });
    await screen.findByText("Closed one");
    const before = server.listRequests.length;
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    screen.getByRole("tab", { name: /Open/ }).click();
    rerender({ currentTab: "open" });
    await waitFor(() => expect(screen.getByTestId("intake-open-count")).toHaveTextContent("2"));
    expect(server.listRequests.length).toBeGreaterThan(before);
    expect(new URLSearchParams(server.listRequests.at(-1) ?? "").get("status")).toBe("-2");
  });

  it("reads the three resolved statuses on Closed with no badge", async () => {
    server.closed = [{ issueId: CLOSED_1, name: "Closed one", status: 1 }];
    installServer(server);
    renderScreen({ currentTab: "closed", inboxIssueId: CLOSED_1 }, () => {});
    expect(await screen.findByText("Closed one")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-open-count")).not.toBeInTheDocument();
    expect(new URLSearchParams(server.listRequests[0] ?? "").get("status")).toBe("1,-1,2");
  });
});

describe("IntakeScreen selection (INT-005)", () => {
  it("auto-selects the first row when the address names none", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open" });
    await screen.findByText("Alpha");
    await waitFor(() =>
      expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "open", inboxIssueId: ISSUE_1 }, { replace: true })
    );
  });

  it("steps between requests from the detail chevrons, pushing the address", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByTestId("intake-detail-header");
    fireEvent.click(screen.getByRole("button", { name: "Next request" }));
    expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "open", inboxIssueId: ISSUE_2 });
    fireEvent.click(screen.getByRole("button", { name: "Previous request" }));
    expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "open", inboxIssueId: ISSUE_2 });
  });

  it("drops a stale selection from the address and re-reads the tab", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    server.detailStatus = 404;
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    await waitFor(() => expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "open" }, { replace: true }));
    await waitFor(() => expect(server.listRequests.length).toBeGreaterThan(1));
  });

  it("steps nowhere when the tab holds no rows", async () => {
    server.pending = [];
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByTestId("intake-detail-header");
    vi.mocked(onSearchChange).mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Next request" }));
    fireEvent.click(screen.getByRole("button", { name: "Previous request" }));
    expect(onSearchChange).not.toHaveBeenCalled();
  });

  it("reads the detail without a role while the roles read fails", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    server.rolesFail = true;
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByTestId("intake-detail-header");
    const pane = screen.getByTestId("intake-content-pane");
    expect(within(pane).getByRole("heading", { name: "Alpha" })).toBeInTheDocument();
    expect(screen.queryByTestId("intake-detail-menu")).not.toBeInTheDocument();
  });

  it("writes the picked row into the address under the active tab", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    fireEvent.click(screen.getByText("Beta"));
    expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "open", inboxIssueId: ISSUE_2 });
  });

  it("keeps an id the list contains without rewriting", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    expect(onSearchChange).not.toHaveBeenCalled();
  });

  it("selects nothing for an empty tab", async () => {
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open" });
    expect(await screen.findByTestId("intake-empty-open")).toBeInTheDocument();
    expect(onSearchChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("intake-placeholder")).toBeInTheDocument();
  });
});

describe("IntakeScreen empty states (INT-008) and errors", () => {
  it("guides an empty Open tab and informs an empty Closed tab", async () => {
    installServer(server);
    renderScreen({ currentTab: "open" });
    expect(await screen.findByTestId("intake-empty-open")).toBeInTheDocument();
    resetAppClient();
    installServer(server);
    renderScreen({ currentTab: "closed" });
    expect(await screen.findByTestId("intake-empty-closed")).toBeInTheDocument();
  });

  it("offers a retry when the list read fails", async () => {
    server.listFails = true;
    installServer(server);
    renderScreen({ currentTab: "open" });
    expect(await screen.findByTestId("intake-list-error")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("re-reads the list on retry", async () => {
    server.listFails = true;
    installServer(server);
    renderScreen({ currentTab: "open" });
    await screen.findByTestId("intake-list-error");
    expect(server.listRequests).toHaveLength(1);
    screen.getByRole("button", { name: "Retry" }).click();
    await waitFor(() => expect(server.listRequests.length).toBe(2));
  });

  it("never refetches on its own while the screen sits idle (INT-031)", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(server.listRequests).toHaveLength(1);
  });
});

describe("IntakeScreen header create control (INT-013)", () => {
  it("offers creation to a non-admin member while intake is on", async () => {
    server.roles = { [PROJECT_ID]: 15 };
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    const onCreate = vi.fn();
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 }, vi.fn(), onCreate);
    const control = await screen.findByTestId("intake-header-create");
    fireEvent.click(control);
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("hides the control when the viewer holds no project standing", async () => {
    server.roles = {};
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 }, vi.fn(), vi.fn());
    await screen.findByText("Alpha");
    await waitFor(() => expect(screen.queryByTestId("intake-header-create")).toBeNull());
  });

  it("hides the control without a create handler", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    expect(screen.queryByTestId("intake-header-create")).toBeNull();
  });
});

describe("IntakeScreen triage advance (INT-020–025)", () => {
  it("moves the address to the neighbour after a triage move", async () => {
    const user = userEvent.setup();
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    installServer(server);
    const onSearchChange = vi.fn();
    const { rerender } = renderScreenWithRerender({ currentTab: "open", inboxIssueId: ISSUE_1 }, onSearchChange);
    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Decline" }));
    const dialog = await screen.findByRole("dialog", { name: "Decline work item" });
    await user.click(within(dialog).getByRole("button", { name: "Decline" }));
    await waitFor(() => expect(server.writes).toEqual([{ status: -1 }]));
    // A user move pushes, so back/forward walks it.
    await waitFor(() => expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "open", inboxIssueId: ISSUE_2 }));

    // The router follows the write: the neighbour's detail takes over.
    rerender({ currentTab: "open", inboxIssueId: ISSUE_2 });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Beta"));
  });

  it("clears the selection when a triage move empties the tab", async () => {
    const user = userEvent.setup();
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    const onSearchChange = vi.fn();
    const { rerender } = renderScreenWithRerender({ currentTab: "open", inboxIssueId: ISSUE_1 }, onSearchChange);
    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Decline" }));
    const dialog = await screen.findByRole("dialog", { name: "Decline work item" });
    await user.click(within(dialog).getByRole("button", { name: "Decline" }));
    await waitFor(() => expect(server.writes).toEqual([{ status: -1 }]));
    await waitFor(() => expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "open" }));

    rerender({ currentTab: "open" });
    await waitFor(() => expect(screen.getByTestId("intake-placeholder")).toBeInTheDocument());
  });
});

function openFilters(): void {
  fireEvent.click(screen.getByTestId("intake-filters-trigger"));
}

function lastListParams(): URLSearchParams {
  return new URLSearchParams(server.listRequests.at(-1) ?? "");
}

function scrollPaneToEnd(): void {
  const panel = screen.getByTestId("intake-list-pane");
  for (const [key, value] of [
    ["scrollHeight", 1000],
    ["scrollTop", 800],
    ["clientHeight", 200],
  ] as const) {
    Object.defineProperty(panel, key, { value, configurable: true });
  }
  fireEvent.scroll(panel);
}

function pagedRows(count: number): FakeRow[] {
  return Array.from({ length: count }, (_, index) => ({
    issueId: `10000000-0000-4000-8000-${String(index).padStart(12, "0")}`,
    name: `Row ${String(index).padStart(2, "0")}`,
  }));
}

describe("IntakeScreen rows (INT-006)", () => {
  it("reads the id, priority, labels and author at a glance", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Tagged request", priority: "urgent", labelIds: [LABEL_ID], createdBy: MEMBER_ID },
    ];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Tagged request");
    expect(screen.getByTestId("intake-row-id")).toHaveTextContent("APO-1");
    expect(screen.getByTestId("intake-row-priority")).toHaveAttribute("data-priority-key", "urgent");
    expect(screen.getByTestId("intake-row-label")).toHaveTextContent("Alpha");
    expect(screen.getByTestId("intake-row-avatar")).toHaveAttribute("data-avatar-kind", "member");
    expect(screen.queryByTestId("intake-row-status")).not.toBeInTheDocument();
  });
});

describe("IntakeScreen paging (INT-007)", () => {
  it("pages ten at a time on scroll without duplicates", async () => {
    server.paged = pagedRows(12);
    installServer(server);
    renderScreen({ currentTab: "open" });
    await waitFor(() => expect(screen.getAllByTestId("intake-row")).toHaveLength(10));
    expect(screen.getAllByTestId("intake-page-skeleton").length).toBeGreaterThan(0);
    scrollPaneToEnd();
    await waitFor(() => expect(screen.getAllByTestId("intake-row")).toHaveLength(12));
    const ids = screen.getAllByTestId("intake-row").map((node) => node.getAttribute("data-issue-id"));
    expect(new Set(ids).size).toBe(12);
    expect(screen.queryByTestId("intake-page-skeleton")).not.toBeInTheDocument();
    expect(new URLSearchParams(server.listRequests[0] ?? "").get("per_page")).toBe("10");
    expect(lastListParams().get("cursor")).toBe("cursor-1");
  });

  it("hints at syncing while a read is in flight, then clears", async () => {
    let release!: () => void;
    server.listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    expect(await screen.findByTestId("intake-syncing")).toBeInTheDocument();
    release();
    await screen.findByText("Alpha");
    await waitFor(() => expect(screen.queryByTestId("intake-syncing")).not.toBeInTheDocument());
  });
});

describe("IntakeScreen filters (INT-009)", () => {
  it("picks a priority and refetches narrowed with chips", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha", priority: "urgent" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Beta");
    openFilters();
    fireEvent.click(screen.getByRole("checkbox", { name: "Urgent" }));
    await waitFor(() =>
      expect(screen.getAllByTestId("intake-row-title").map((node) => node.textContent)).toEqual(["Alpha"])
    );
    expect(lastListParams().get("status")).toBe("-2");
    expect(lastListParams().get("priority")).toBe("urgent");
    expect(screen.getByTestId("intake-filters-count")).toHaveTextContent("2");
  });

  it("clears a filter through its chip", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha", priority: "urgent" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Beta");
    openFilters();
    fireEvent.click(screen.getByRole("checkbox", { name: "Urgent" }));
    await waitFor(() => expect(screen.getAllByTestId("intake-row")).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Remove Urgent Priority filter" }));
    await waitFor(() => expect(screen.getAllByTestId("intake-row")).toHaveLength(2));
    expect(lastListParams().get("priority")).toBe(null);
  });

  it("restores the tab defaults when tabs switch", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha", priority: "urgent" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    server.closed = [{ issueId: CLOSED_1, name: "Closed one", status: 1 }];
    installServer(server);
    const { rerender } = renderScreenWithRerender({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Beta");
    openFilters();
    fireEvent.click(screen.getByRole("checkbox", { name: "Urgent" }));
    await waitFor(() => expect(screen.getAllByTestId("intake-row")).toHaveLength(1));
    screen.getByRole("tab", { name: /Closed/ }).click();
    rerender({ currentTab: "closed" });
    await screen.findByText("Closed one");
    expect(lastListParams().get("status")).toBe("1,-1,2");
    expect(lastListParams().get("priority")).toBe(null);
  });

  it("shows the no-results state when filters match nothing", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open" });
    await screen.findByText("Alpha");
    openFilters();
    fireEvent.click(screen.getByRole("checkbox", { name: "Urgent" }));
    expect(await screen.findByTestId("intake-empty-filtered")).toBeInTheDocument();
  });

  it("drops a selection the narrowed list no longer contains", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha" },
      { issueId: ISSUE_2, name: "Beta", priority: "urgent" },
    ];
    installServer(server);
    const onSearchChange = renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Beta");
    openFilters();
    fireEvent.click(screen.getByRole("checkbox", { name: "Urgent" }));
    await waitFor(() =>
      expect(onSearchChange).toHaveBeenCalledWith({ currentTab: "open", inboxIssueId: ISSUE_2 }, { replace: true })
    );
  });
});

describe("IntakeScreen dates (INT-010)", () => {
  it("picks a preset and sends its encoded range", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    openFilters();
    const section = screen.getByTestId("intake-filter-createdAt");
    fireEvent.click(within(section).getByRole("radio", { name: "Today" }));
    const today = new Date().toISOString().slice(0, 10);
    await waitFor(() => expect(lastListParams().get("created_at")).toBe(`${today};after,${today};before`));
  });

  it("applies an explicit range from the dialog", async () => {
    server.pending = [{ issueId: ISSUE_1, name: "Alpha" }];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Alpha");
    openFilters();
    const section = screen.getByTestId("intake-filter-updatedAt");
    fireEvent.click(within(section).getByRole("radio", { name: "Custom" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    const today = new Date().toISOString().slice(0, 10);
    const [year, month, day] = today.split("-").map(Number) as [number, number, number];
    const nextMonth = new Date(Date.UTC(year, month, day)).toISOString().slice(0, 10);
    await waitFor(() => expect(lastListParams().get("updated_at")).toBe(`${today};after,${nextMonth};before`));
  });
});

describe("IntakeScreen order (INT-011)", () => {
  it("re-sorts server-side and stays marked", async () => {
    server.pending = [
      { issueId: ISSUE_1, name: "Alpha" },
      { issueId: ISSUE_2, name: "Beta" },
    ];
    installServer(server);
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    await screen.findByText("Beta");
    expect(new URLSearchParams(server.listRequests[0] ?? "").get("order_by")).toBe("-issue__created_at");
    fireEvent.click(screen.getByTestId("intake-order-trigger"));
    expect(screen.getByTestId("intake-order-field-created")).toHaveAttribute("data-order-checked", "true");
    fireEvent.click(screen.getByRole("menuitem", { name: "ID" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    await waitFor(() => expect(lastListParams().get("order_by")).toBe("issue__sequence_id"));
    expect(screen.getAllByTestId("intake-row-title").map((node) => node.textContent)).toEqual(["Beta", "Alpha"]);
  });
});
