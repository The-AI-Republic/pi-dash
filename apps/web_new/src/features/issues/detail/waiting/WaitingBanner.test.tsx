// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Issue } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { setSessionClient } from "../../../../core/session/queries.js";
import { CANCEL_RUN_FAILED_TITLE, RELEASE_RUN_FAILED_TITLE } from "./waitingRuns.js";
import { WaitingBanner } from "./WaitingBanner.js";

vi.mock("../../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { notifyError } from "../../../../shared/shell/index.js";

const SLUG = "acme";
const WORKSPACE_ID = "023e4567-e89b-12d3-a456-426614174009";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const RUN = "323e4567-e89b-12d3-a456-426614174002";
const POD = "423e4567-e89b-12d3-a456-426614174003";
const RUNNER = "523e4567-e89b-12d3-a456-426614174004";
const OWNER = "623e4567-e89b-12d3-a456-426614174005";

const WORKSPACES = [{ id: WORKSPACE_ID, name: "Acme", slug: SLUG, logo_url: null, total_members: 1, role: 20 }];

const PODS = [
  {
    id: POD,
    name: "WEB_pod",
    description: "",
    is_default: true,
    workspace: WORKSPACE_ID,
    project: PROJECT,
    project_identifier: "WEB",
    created_by: null,
    runner_count: 1,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  },
];

const RUNNERS = [
  {
    id: RUNNER,
    name: "gpu-box",
    status: "offline",
    host_label: "ana-desktop",
    provisioning: "manual",
    os: "linux",
    arch: "x86_64",
    runner_version: "1.2.3",
    dev_metadata: {},
    protocol_version: 4,
    capabilities: [],
    last_heartbeat_at: "2026-10-22T02:00:00Z",
    owner: OWNER,
    dev_machine: null,
    dev_machine_detail: null,
    visibility: 0,
    pod: POD,
    pod_detail: { id: POD, name: "WEB_pod", is_default: true, project: PROJECT, project_identifier: "WEB" },
    live_state: null,
    enrolled_at: "2024-01-02T03:04:05Z",
    revoked_at: null,
    revoked_reason: "",
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    assignable: true,
    not_assignable_code: "",
    is_busy: false,
    queue_depth: 0,
  },
];

const DIRECTORY = [
  {
    id: "723e4567-e89b-12d3-a456-426614174006",
    member: {
      id: OWNER,
      first_name: "Ana",
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: "Ana",
      is_bot: false,
    },
    role: 15,
    is_active: true,
  },
];

const PROJECT_ROW = {
  id: PROJECT,
  name: "Web",
  identifier: "WEB",
  sort_order: 1,
  logo_props: {},
  member_role: 20,
  archived_at: null,
  workspace: WORKSPACE_ID,
  cycle_view: false,
  issue_views_view: true,
  module_view: false,
  page_view: true,
  inbox_view: true,
  is_default: true,
  guest_view_all_features: false,
  project_lead: null,
  network: 2,
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-02-03T04:05:06Z",
  created_by: null,
  updated_by: null,
  agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
  default_agent_executor: "local_runner",
};

function agentRunPayload() {
  return {
    id: RUN,
    status: "queued",
    executor_kind: "local_runner",
    dispatch_attempts: 0,
    cancel_requested_at: null,
    cancel_reason: "",
    prompt: "",
    thread_id: "",
    agent_metadata: {},
    runner: null,
    work_item: ISSUE,
    scheduler_binding: null,
    scheduler_binding_detail: null,
    pod: POD,
    pod_detail: { id: POD, name: "WEB_pod", is_default: true, project: PROJECT, project_identifier: "WEB" },
    created_by: OWNER,
    owner: OWNER,
    created_at: "2026-10-11T20:14:03Z",
    assigned_at: null,
    queue_position: null,
    started_at: null,
    ended_at: null,
    done_payload: null,
    error: "",
    error_code: "",
    error_diagnostic: null,
    refusal_category: "",
    llm_model: "",
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    usage: {},
    tool_plan: {},
    tool_calls: [],
  };
}

function waitingIssue(overrides: Record<string, unknown> = {}): Issue {
  const wait = {
    reason: "runner_offline",
    pin_kind: "dedicated",
    runner: RUNNER,
    runner_name: "gpu-box",
    queued_at: "2026-10-11T20:14:03Z",
    runner_last_seen_at: "2026-10-22T02:00:00Z",
    runs_ahead: 0,
    releasable: true,
    ...(overrides.wait as Record<string, unknown> | undefined),
  };
  const activeRun = {
    id: RUN,
    status: "queued",
    executor_kind: "local_runner",
    queue_position: null,
    runner: null,
    runner_name: null,
    pinned_runner: RUNNER,
    pinned_runner_name: "gpu-box",
    pin_kind: "dedicated",
    dispatch_wait: wait,
    created_at: "2026-10-11T20:14:03Z",
    assigned_at: null,
    started_at: null,
    ended_at: null,
    done_payload: null,
    error: "",
    error_code: "",
    error_diagnostic: null,
    llm_model: "",
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    live_state: null,
    ...(overrides.activeRun as Record<string, unknown> | undefined),
  };
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "high",
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
    agent_executor: "local_runner",
    assigned_pod_id: POD,
    assigned_runner_id: RUNNER,
    is_synced: false,
    description_html: "<p>words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: { ticker: null, active_run: activeRun, latest_run: activeRun, run_count: 1 },
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

interface StubOptions {
  releaseStatus?: number;
  cancelStatus?: number;
  patchStatus?: number;
  patchBody?: unknown;
}

function harness(options: StubOptions = {}) {
  const calls: Array<{ url: string; method: string; body: string }> = [];
  const client = createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ url, method, body: init.body ?? "" });
    const json = (status: number, payload: unknown) => ({
      status,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(payload),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "cs" });
    const path = new URL(url).pathname;
    if (path === "/api/runners/pods/") return json(200, PODS);
    if (path === "/api/runners/") return json(200, RUNNERS);
    if (path === `/api/workspaces/${SLUG}/members/`) return json(200, DIRECTORY);
    if (path === `/api/workspaces/${SLUG}/projects/${PROJECT}/`) return json(200, PROJECT_ROW);
    if (path === `/api/runners/runs/${RUN}/release-pin/` && method === "POST") {
      const status = options.releaseStatus ?? 200;
      return json(status, status === 200 ? { ...agentRunPayload(), pinned_runner: null } : {});
    }
    if (path === `/api/runners/runs/${RUN}/cancel/` && method === "POST") {
      const status = options.cancelStatus ?? 200;
      return json(status, status === 200 ? { ...agentRunPayload(), status: "cancelled" } : {});
    }
    if (path.endsWith(`/issues/${ISSUE}/`) && method === "PATCH") {
      const status = options.patchStatus ?? 204;
      return json(status, status === 204 ? "" : (options.patchBody ?? {}));
    }
    return json(404, {});
  }) as never);
  setSessionClient(client);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(["session", "workspaces"], WORKSPACES);
  return { calls, queryClient };
}

function renderBanner(queryClient: QueryClient, issue: Issue = waitingIssue(), disabled = false): void {
  render(
    <QueryClientProvider client={queryClient}>
      <WaitingBanner workspaceSlug={SLUG} projectId={PROJECT} issue={issue} disabled={disabled} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("WaitingBanner", () => {
  it("renders nothing while nothing waits", () => {
    const { queryClient } = harness();
    const quiet = waitingIssue();
    quiet.agent_status = null;
    renderBanner(queryClient, quiet);
    expect(screen.queryByRole("region", { name: "Waiting for runner" })).not.toBeInTheDocument();
    expect(screen.queryByText(/Waiting for runner/)).not.toBeInTheDocument();
  });

  it("names the runner, the reason and the three actions", async () => {
    const { queryClient } = harness();
    renderBanner(queryClient);
    const banner = await screen.findByText(/Waiting for runner gpu-box/);
    expect(banner.textContent).toContain("offline, last seen");
    expect(await screen.findByRole("button", { name: /Run on any runner in WEB_pod once/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change runner" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel run" })).toBeInTheDocument();
  });

  it("releases the run to the pod with one POST", async () => {
    const { calls, queryClient } = harness();
    renderBanner(queryClient);
    fireEvent.click(await screen.findByRole("button", { name: /Run on any runner in/ }));
    await waitFor(() =>
      expect(calls.some((call) => call.url.endsWith(`/api/runners/runs/${RUN}/release-pin/`))).toBe(true)
    );
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts when the release fails", async () => {
    const { queryClient } = harness({ releaseStatus: 500 });
    renderBanner(queryClient);
    fireEvent.click(await screen.findByRole("button", { name: /Run on any runner in/ }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(RELEASE_RUN_FAILED_TITLE, expect.any(String)));
  });

  it("disables the release while the run is not releasable", async () => {
    const { queryClient } = harness();
    renderBanner(queryClient, waitingIssue({ wait: { releasable: false } }));
    expect(await screen.findByRole("button", { name: /Run on any runner in/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Change runner" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Cancel run" })).toBeEnabled();
  });

  it("cancels the run with one POST", async () => {
    const { calls, queryClient } = harness();
    renderBanner(queryClient);
    fireEvent.click(await screen.findByRole("button", { name: "Cancel run" }));
    await waitFor(() => expect(calls.some((call) => call.url.endsWith(`/api/runners/runs/${RUN}/cancel/`))).toBe(true));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts when the cancel fails", async () => {
    const { queryClient } = harness({ cancelStatus: 500 });
    renderBanner(queryClient);
    fireEvent.click(await screen.findByRole("button", { name: "Cancel run" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(CANCEL_RUN_FAILED_TITLE, expect.any(String)));
  });

  it("changes the runner through the dialog picker", async () => {
    const user = userEvent.setup();
    const { calls, queryClient } = harness();
    renderBanner(queryClient);
    await user.click(await screen.findByRole("button", { name: "Change runner" }));
    await user.click(await screen.findByRole("option", { name: /gpu-box/ }));
    await waitFor(() =>
      expect(calls.some((call) => call.method === "PATCH" && call.url.includes(`/issues/${ISSUE}/`))).toBe(true)
    );
    const patch = calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({
      assigned_pod_id: POD,
      assigned_runner_id: RUNNER,
      agent_executor: "local_runner",
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("keeps the change dialog open on a validation refusal", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness({
      patchStatus: 400,
      patchBody: { assigned_runner_id: ["That runner belongs to another project."] },
    });
    renderBanner(queryClient);
    await user.click(await screen.findByRole("button", { name: "Change runner" }));
    await user.click(await screen.findByRole("option", { name: /gpu-box/ }));
    const dialog = await screen.findByRole("dialog");
    await waitFor(() =>
      expect(within(dialog).getByRole("alert")).toHaveTextContent("That runner belongs to another project.")
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("disables every action for read-only viewers", async () => {
    const { queryClient } = harness();
    renderBanner(queryClient, waitingIssue(), true);
    await screen.findByText(/Waiting for runner gpu-box/);
    expect(screen.getByRole("button", { name: /Run on any runner in/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Change runner" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel run" })).toBeDisabled();
  });

  it("falls back to the pod wording without a pod name", async () => {
    const { queryClient } = harness();
    const issue = waitingIssue();
    issue.assigned_pod_id = "99999999-9999-4999-8999-999999999999";
    renderBanner(queryClient, issue);
    expect(await screen.findByRole("button", { name: "Run on any runner in the pod once" })).toBeInTheDocument();
  });

  it("closes the change dialog without saving", async () => {
    const user = userEvent.setup();
    const { calls, queryClient } = harness();
    renderBanner(queryClient);
    await user.click(await screen.findByRole("button", { name: "Change runner" }));
    await screen.findByRole("option", { name: /gpu-box/ });
    await user.click(screen.getByRole("heading", { name: "Change runner" }));
    await waitFor(() => expect(screen.queryByRole("option", { name: /gpu-box/ })).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Change runner" })).not.toBeInTheDocument());
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });
});
