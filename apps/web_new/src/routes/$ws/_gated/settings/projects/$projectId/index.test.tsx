// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General leaf (PSET-001/PSET-004): the landing tab owns the slash-form
// address, draws the shell around the identity body and never refuses a
// role, so its page gate is its own screen rather than a denial. The tab's
// own settings read is stubbed so the leaf renders without a server.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../../core/api/client.js";

const harness = vi.hoisted(() => ({
  params: { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" },
  role: 20,
  workspaceRole: 20,
}));

vi.mock("../../../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNarrowViewport: () => false,
}));

vi.mock("../../../../../../features/auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useProjectRow: () => ({
    status: "success",
    data: { id: harness.params.projectId, name: "Website relaunch", identifier: "WEB", logo_props: {} },
    isPending: false,
    isSuccess: true,
  }),
  useProjectRoles: () => ({
    status: "success",
    data: { [harness.params.projectId]: harness.role },
    isPending: false,
    isSuccess: true,
  }),
  useWorkspaceMembership: () => ({ status: "success", data: { role: harness.workspaceRole }, isPending: false }),
  useJoinProject: () => ({ isPending: false, mutate: vi.fn() }),
}));

import { renderInRouter } from "../../../../../../features/project-settings/testing.js";
import { Route } from "./index.js";

const GENERAL_ADDRESS = `/acme/settings/projects/${harness.params.projectId}/`;

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function projectRow() {
  return {
    id: harness.params.projectId,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    name: "Website relaunch",
    description: "",
    description_text: null,
    description_html: null,
    network: 0,
    identifier: "WEB",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: true,
    cycle_view: false,
    issue_views_view: true,
    page_view: false,
    intake_view: true,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "main",
    is_favorite: false,
    sort_order: null,
    member_role: null,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
    default_agent_executor: "local_runner",
    agent_ticking_enabled: true,
    agent_default_max_ticks: 5,
    agent_default_interval_seconds: 10_800,
    agent_review_default_interval_seconds: 10_800,
    agent_test_default_interval_seconds: 10_800,
  };
}

beforeEach(() => {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith("/repository/")) return jsonResponse(url, 200, { bound: false });
    if (url.endsWith("/api/timezones/")) return jsonResponse(url, 200, { timezones: [] });
    return jsonResponse(url, 200, projectRow());
  }) as never);
});

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue(harness.params);
  const Component = Route.options.component;
  if (!Component) throw new Error("general route has no component");
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      {renderInRouter(GENERAL_ADDRESS, <Component />)}
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  harness.role = 20;
  harness.workspaceRole = 20;
  vi.restoreAllMocks();
});

describe("general settings route", () => {
  it("carries no loader prefetch", () => {
    expect(Route.options.beforeLoad).toBeUndefined();
  });

  it("mounts the identity body under the shell at the general address", async () => {
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument());
    expect(screen.getByRole("heading", { level: 1, name: "General" })).toBeInTheDocument();
    expect(await screen.findByTestId("project-settings-general")).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-tab-general")).toHaveAttribute("data-active", "true");
  });

  it("keeps the page open for a guest while the rail narrows to general and members", async () => {
    harness.role = 5;
    harness.workspaceRole = 5;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-general")).toBeInTheDocument());
    expect(screen.queryByTestId("project-settings-denied")).not.toBeInTheDocument();
    expect(screen.getByTestId("project-settings-tab-members")).toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-tab-states")).not.toBeInTheDocument();
  });
});
