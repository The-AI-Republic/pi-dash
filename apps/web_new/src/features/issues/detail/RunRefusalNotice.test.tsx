// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { setSessionClient } from "../../../core/session/queries.js";
import { RunRefusalNotice } from "./RunRefusalNotice.js";
import { NO_OWN_RUNNER_TEXT, RUN_ON_MY_ENGINE_LABEL, type RunRefusal } from "./runRefusal.js";

const SLUG = "acme";
const WORKSPACE_ID = "023e4567-e89b-12d3-a456-426614174009";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const POD = "423e4567-e89b-12d3-a456-426614174003";
const ME = "623e4567-e89b-12d3-a456-426614174005";
const OTHER = "723e4567-e89b-12d3-a456-426614174006";
const MY_ENGINE = "823e4567-e89b-12d3-a456-426614174007";
const THEIR_ENGINE = "923e4567-e89b-12d3-a456-426614174008";

function runnerRow(id: string, owner: string) {
  return {
    id,
    name: `builtin-${id.slice(0, 4)}`,
    status: "online",
    host_label: "box",
    provisioning: "desktop_bundled",
    os: "linux",
    arch: "x86_64",
    runner_version: "1.2.3",
    dev_metadata: {},
    protocol_version: 4,
    capabilities: [],
    last_heartbeat_at: null,
    owner,
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
  };
}

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
    runner_count: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  },
];

function harness(runners: unknown[]) {
  const calls: Array<{ url: string; method: string; body: string }> = [];
  const client = createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ url, method, body: init.body ?? "" });
    const json = (status: number, payload: unknown) => ({
      status,
      url,
      headers: { get: () => "application/json" },
      text: async () => (status === 204 ? "" : JSON.stringify(payload)),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "cs" });
    const path = new URL(url).pathname;
    if (path === "/api/runners/") return json(200, runners);
    if (path === "/api/runners/pods/") return json(200, PODS);
    if (path === `/api/workspaces/${SLUG}/members/`) return json(200, []);
    if (path.endsWith(`/issues/${ISSUE}/`) && method === "PATCH") return json(204, "");
    return json(404, {});
  }) as never);
  setSessionClient(client);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(
    ["session", "workspaces"],
    [{ id: WORKSPACE_ID, name: "Acme", slug: SLUG, logo_url: null, total_members: 2, role: 15 }]
  );
  queryClient.setQueryData(["session", "me"], { id: ME, display_name: "Me" });
  return { calls, queryClient };
}

function renderNotice(queryClient: QueryClient, refusal: RunRefusal, onRetry = vi.fn(), onResolved = vi.fn()) {
  render(
    <QueryClientProvider client={queryClient}>
      <RunRefusalNotice
        workspaceSlug={SLUG}
        workspaceId={WORKSPACE_ID}
        projectId={PROJECT}
        issue={{ id: ISSUE }}
        refusal={refusal}
        onRetry={onRetry}
        onResolved={onResolved}
      />
    </QueryClientProvider>
  );
  return { onRetry, onResolved };
}

afterEach(() => {
  resetAppClient();
});

const NOT_OWNER: RunRefusal = {
  code: "assigned_runner_not_owner",
  message: "This issue is assigned to Ana's laptop, which only runs its owner's work.",
  assignedRunnerId: THEIR_ENGINE,
};

describe("RunRefusalNotice", () => {
  it("re-pins the issue to my engine and retries the run", async () => {
    const user = userEvent.setup();
    const { calls, queryClient } = harness([runnerRow(THEIR_ENGINE, OTHER), runnerRow(MY_ENGINE, ME)]);
    const { onRetry, onResolved } = renderNotice(queryClient, NOT_OWNER);
    expect(screen.getByRole("alert")).toHaveTextContent("Ana's laptop");
    const action = await screen.findByRole("button", { name: RUN_ON_MY_ENGINE_LABEL });
    await waitFor(() => expect(action).toBeEnabled());
    await user.click(action);
    await waitFor(() => expect(onRetry).toHaveBeenCalledTimes(1));
    expect(onResolved).toHaveBeenCalledTimes(1);
    const patch = calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({
      assigned_pod_id: POD,
      assigned_runner_id: MY_ENGINE,
      agent_executor: "local_runner",
    });
  });

  it("says so when I have no runner in the project", async () => {
    const { queryClient } = harness([runnerRow(THEIR_ENGINE, OTHER)]);
    renderNotice(queryClient, NOT_OWNER);
    expect(await screen.findByText(NO_OWN_RUNNER_TEXT)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: RUN_ON_MY_ENGINE_LABEL })).not.toBeInTheDocument();
  });

  it("offers another runner or the pod when the assigned runner is offline", async () => {
    const user = userEvent.setup();
    const { calls, queryClient } = harness([runnerRow(MY_ENGINE, ME)]);
    const { onRetry, onResolved } = renderNotice(queryClient, {
      code: "assigned_runner_offline",
      message: "Work laptop is offline",
      assignedRunnerId: MY_ENGINE,
    });
    expect(screen.getByRole("alert")).toHaveTextContent("Work laptop is offline");
    await user.click(screen.getByRole("button", { name: /another runner or the pod/ }));
    await user.click(await screen.findByRole("option", { name: "Any runner in WEB_pod" }));
    await waitFor(() => expect(onResolved).toHaveBeenCalledTimes(1));
    expect(onRetry).not.toHaveBeenCalled();
    const patch = calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({
      assigned_pod_id: POD,
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
  });
});
