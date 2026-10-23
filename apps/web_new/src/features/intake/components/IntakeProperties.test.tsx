// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request properties panel (INT-019/026): the read-only state row, the
// four editable rows writing through the intake update, the labels
// add-control rule, and the disabled panel for viewers who may not edit.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { IntakeIssueDetail, Issue, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { setSessionClient } from "../../../core/session/queries.js";
import { notifyError } from "../../../shared/shell/index.js";
import { formatDayLong } from "../../issues/index.js";
import { intakeKeys } from "../api/keys.js";
import { INTAKE_PROPERTY_SAVE_FAILED_TITLE, IntakeProperties, ignoreIntakeStateSelection } from "./IntakeProperties.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "323e4567-e89b-12d3-a456-426614174002";
const ME = "023e4567-e89b-12d3-a456-426614174009";
const TRIAGE = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const LABEL = "b23e4567-e89b-12d3-a456-42661417400a";

const ME_PAYLOAD = {
  id: ME,
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

function stateRow(id: string, name: string, group: string) {
  return {
    id,
    project_id: PROJECT,
    workspace_id: "b23e4567-e89b-12d3-a456-42661417400e",
    name,
    color: "#5e6ad2",
    group,
    default: false,
    description: "",
    sequence: 1,
  };
}

const DIRECTORY = [
  {
    id: "b23e4567-e89b-12d3-a456-42661417400b",
    member: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    },
    role: 15,
    is_active: true,
  },
  {
    id: "c23e4567-e89b-12d3-a456-42661417400c",
    member: {
      id: ME,
      first_name: "Me",
      last_name: "M",
      avatar: "",
      avatar_url: null,
      display_name: "Me M",
      is_bot: false,
    },
    role: 20,
    is_active: true,
  },
];

const PROJECT_MEMBERS = [
  {
    id: "d23e4567-e89b-12d3-a456-42661417400d",
    role: 15,
    member: ADA,
    project: PROJECT,
    original_role: 15,
    created_at: "2024-01-02T03:04:05Z",
  },
  {
    id: "e23e4567-e89b-12d3-a456-42661417400e",
    role: 20,
    member: ME,
    project: PROJECT,
    original_role: 20,
    created_at: "2024-01-02T03:04:05Z",
  },
];

const LABELS = [
  {
    id: LABEL,
    project_id: PROJECT,
    workspace_id: "b23e4567-e89b-12d3-a456-42661417400e",
    name: "Bug",
    color: "#ff0000",
    parent: null,
    sort_order: 1,
  },
];

function issueFixture(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "A request",
    state_id: TRIAGE,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
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
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function seedDetail(overrides: Partial<Issue> = {}): IntakeIssueDetail {
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
    status: -2,
    duplicate_to: null,
    snoozed_till: null,
    duplicate_issue_detail: null,
    source: null,
    issue: issueFixture(overrides) as IntakeIssueDetail["issue"],
  };
}

interface HarnessOptions {
  patchStatus?: number;
}

function harness(options: HarnessOptions = {}) {
  const writes: Array<{ url: string; body: unknown }> = [];
  const conversation = async (url: string, init: TransportInit) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"cs"}' };
    }
    if (init.method === "PATCH" && url.includes(`/intake-issues/${ISSUE}/`)) {
      const body = JSON.parse(init.body ?? "{}") as { issue?: Record<string, unknown> };
      writes.push({ url, body });
      const status = options.patchStatus ?? 200;
      if (status >= 400) {
        return { status, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "nope" }) };
      }
      const echo = seedDetail({ ...(body.issue ?? {}) } as Partial<Issue>);
      return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(echo) };
    }
    if (url.endsWith(`/projects/${PROJECT}/intake-state/`)) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(stateRow(TRIAGE, "Triage", "triage")),
      };
    }
    if (url.endsWith(`/projects/${PROJECT}/states/`)) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify([stateRow(TRIAGE, "Triage", "triage")]),
      };
    }
    if (url.includes(`/projects/${PROJECT}/members/`)) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(PROJECT_MEMBERS) };
    }
    if (url.endsWith(`/workspaces/${SLUG}/members/`)) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(DIRECTORY) };
    }
    if (url.endsWith(`/projects/${PROJECT}/issue-labels/`)) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(LABELS) };
    }
    if (url.endsWith("/api/users/me/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(ME_PAYLOAD) };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  };
  const client = createAppClient("https://api.test", conversation as never);
  setSessionClient(client);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(["session", "me"], ME_PAYLOAD);
  queryClient.setQueryData(["session", "profile"], { start_of_the_week: 0 });
  queryClient.setQueryData(intakeKeys.detail(SLUG, PROJECT, ISSUE), seedDetail());
  return { client, queryClient, writes };
}

function renderPanel(queryClient: QueryClient, props: Partial<React.ComponentProps<typeof IntakeProperties>> = {}) {
  render(
    <QueryClientProvider client={queryClient}>
      <IntakeProperties workspaceSlug={SLUG} projectId={PROJECT} issue={issueFixture()} canEdit {...props} />
    </QueryClientProvider>
  );
}

/** An in-month Saturday a week out, immune to month boundaries (the grid renders six weeks). */
function targetDay(): { iso: string; label: string } {
  const target = new Date();
  target.setDate(target.getDate() + 7);
  const iso = `${target.getFullYear()}-${String(target.getMonth() + 1).padStart(2, "0")}-${String(target.getDate()).padStart(2, "0")}`;
  return { iso, label: formatDayLong(iso) };
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

beforeEach(() => {
  createAppClient("https://api.test");
});

describe("IntakeProperties (INT-019)", () => {
  it("shows the triage state read-only with the editable rows beneath", async () => {
    const { queryClient } = harness();
    renderPanel(queryClient);
    const state = screen.getByRole("group", { name: "State" });
    expect(await within(state).findByRole("button", { name: "Triage" })).toBeDisabled();
    expect(screen.getByRole("group", { name: "Assignees" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Priority" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Due date" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Labels" })).toBeInTheDocument();
  });

  it("ignores state selections", () => {
    expect(ignoreIntakeStateSelection(TRIAGE)).toBeUndefined();
  });

  it("patches the priority through the intake update", async () => {
    const { queryClient, writes } = harness();
    renderPanel(queryClient);
    fireEvent.click(within(screen.getByRole("group", { name: "Priority" })).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "High" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ issue: { priority: "high" } });
  });

  it("patches assignees through the intake update", async () => {
    const { queryClient, writes } = harness();
    renderPanel(queryClient);
    fireEvent.click(within(screen.getByRole("group", { name: "Assignees" })).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Ada L" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ issue: { assignee_ids: [ADA] } });
  });

  it("patches the due date with the picked day", async () => {
    const { queryClient, writes } = harness();
    renderPanel(queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Due date" }));
    const { iso, label } = targetDay();
    fireEvent.click(await screen.findByRole("button", { name: label }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ issue: { target_date: iso } });
  });

  it("clears the due date with null", async () => {
    const { queryClient, writes } = harness();
    renderPanel(queryClient, { issue: issueFixture({ target_date: "2026-10-10" }) });
    fireEvent.click(await screen.findByRole("button", { name: "Clear Due date" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ issue: { target_date: null } });
  });

  it("adds and removes labels through the intake update", async () => {
    const { queryClient, writes } = harness();
    renderPanel(queryClient);
    fireEvent.click(screen.getByTestId("label-picker-trigger"));
    const options = await screen.findAllByTestId("label-picker-option");
    fireEvent.click(options.find((option) => option.textContent?.includes("Bug"))!);
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ issue: { label_ids: [LABEL] } });
  });

  it("removes a label from its chip", async () => {
    const { queryClient, writes } = harness();
    renderPanel(queryClient, { issue: issueFixture({ label_ids: [LABEL] }) });
    fireEvent.click(await screen.findByRole("button", { name: "Remove Bug" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ issue: { label_ids: [] } });
  });

  it("flags an overdue due date while the request is open", async () => {
    const { queryClient } = harness();
    renderPanel(queryClient, { issue: issueFixture({ target_date: "2020-01-01" }) });
    const row = screen.getByRole("group", { name: "Due date" });
    await waitFor(() => expect(within(row).getByTestId("intake-due-date")).toHaveAttribute("data-overdue", "true"));
  });

  it("reverts to the server value and notices when a write is refused", async () => {
    const { queryClient, writes } = harness({ patchStatus: 500 });
    renderPanel(queryClient);
    fireEvent.click(within(screen.getByRole("group", { name: "Priority" })).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "High" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(INTAKE_PROPERTY_SAVE_FAILED_TITLE, expect.any(String))
    );
    expect(queryClient.getQueryData<IntakeIssueDetail>(intakeKeys.detail(SLUG, PROJECT, ISSUE))?.issue.priority).toBe(
      "none"
    );
  });
});

describe("IntakeProperties read-only (INT-026)", () => {
  it("disables every row and omits the labels add control", async () => {
    const { queryClient } = harness();
    renderPanel(queryClient, { canEdit: false, issue: issueFixture({ label_ids: [LABEL] }) });
    const state = screen.getByRole("group", { name: "State" });
    expect(await within(state).findByRole("button", { name: "Triage" })).toBeDisabled();
    expect(within(screen.getByRole("group", { name: "Assignees" })).getByRole("button")).toBeDisabled();
    expect(within(screen.getByRole("group", { name: "Priority" })).getByRole("button")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Due date" })).toBeDisabled();
    expect(screen.queryByTestId("label-picker-trigger")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove Bug" })).not.toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Selected labels" })).toHaveTextContent("Bug");
  });
});
