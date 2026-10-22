// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI Workers tab (hybrid assignment, design §6.6): the skeleton, the
// admin's rule change with its write, the inert member form, the
// default rule on rows without the setting, and the failed save.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN, ROLE_MEMBER } from "../../../core/session/permissions.js";

const harness = vi.hoisted(() => ({ role: 20 as number }));
const notify = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifySuccess: notify.success,
  notifyError: notify.error,
  getToastManager: () => ({ add: () => undefined }),
}));

vi.mock("../components/ProjectSettingsLayout.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useProjectSettings: () => ({ role: harness.role }),
}));

import { generalKeys } from "../general/api/queries.js";
import {
  LOST_RUNNER_RULE_SAVE_FAILED_MESSAGE,
  LOST_RUNNER_RULE_SAVE_FAILED_TITLE,
  LOST_RUNNER_RULE_SAVED_MESSAGE,
  LOST_RUNNER_RULE_SAVED_TITLE,
} from "./api/mutations.js";
import { ProjectSettingsAiWorkersTab } from "./AiWorkersTabScreen.js";

const UUID = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_PATH = `/api/workspaces/acme/projects/${UUID}/`;

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function projectRow(overrides: Record<string, unknown> = {}) {
  return {
    id: UUID,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    name: "Website relaunch",
    description: "Public site work",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "WEB",
    default_assignee: null,
    project_lead: null,
    default_state: null,
    timezone: "UTC",
    estimate: null,
    archive_in: 30,
    close_in: 30,
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    emoji: null,
    icon_prop: null,
    intake_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_favorite: false,
    anchor: null,
    members: [],
    next_work_item_sequence: 4,
    sort_order: 1,
    logo_props: {},
    member_role: ROLE_ADMIN,
    archived_at: null,
    cycle_view: false,
    issue_views_view: true,
    module_view: false,
    page_view: true,
    inbox_view: true,
    is_default: true,
    guest_view_all_features: false,
    members_can_edit_states: true,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    agent_ticking_enabled: true,
    agent_default_max_ticks: 10,
    agent_default_interval_seconds: 60,
    agent_review_default_interval_seconds: 60,
    agent_test_default_interval_seconds: 60,
    agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function setup(detail: Record<string, unknown>, patchStatus = 200, patchHang = false) {
  const calls: Array<{ method: string; body: string }> = [];
  let current = detail;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "cs" });
    if (url.endsWith(PROJECT_PATH) && method === "GET") return jsonResponse(url, 200, current);
    if (url.endsWith(PROJECT_PATH) && method === "PATCH") {
      calls.push({ method, body: init.body ?? "" });
      if (patchHang) await new Promise(() => {});
      const payload = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      if (patchStatus === 200) current = { ...current, ...payload };
      return jsonResponse(url, patchStatus, patchStatus === 200 ? current : {});
    }
    return jsonResponse(url, 404, {});
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectSettingsAiWorkersTab workspaceSlug="acme" projectId={UUID} />
    </QueryClientProvider>
  );
  return { calls, queryClient };
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
  harness.role = ROLE_ADMIN;
});

describe("ProjectSettingsAiWorkersTab", () => {
  it("shows the stored rule behind a skeleton", async () => {
    setup(projectRow({ dedicated_runner_lost_action: "fall_back" }));
    expect(screen.getByTestId("project-settings-ai-workers-skeleton")).toBeInTheDocument();
    expect(await screen.findByText("Fall back to the pod")).toBeInTheDocument();
    expect(screen.getByText("When a dedicated runner is revoked or deleted")).toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-ai-workers-skeleton")).not.toBeInTheDocument();
  });

  it("reads the default rule on rows without the setting", async () => {
    setup(projectRow());
    expect(await screen.findByText("Fail the run and move the issue to Backlog")).toBeInTheDocument();
  });

  it("saves an admin's rule change with one PATCH", async () => {
    const user = userEvent.setup();
    const { calls, queryClient } = setup(projectRow({ dedicated_runner_lost_action: "fail_to_backlog" }));
    await screen.findByText("Fail the run and move the issue to Backlog");
    await user.click(screen.getByRole("button", { name: /Fail the run and move the issue to Backlog/ }));
    await user.click(await screen.findByRole("option", { name: "Fall back to the pod" }));
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(JSON.parse(calls[0]?.body ?? "{}")).toEqual({ dedicated_runner_lost_action: "fall_back" });
    await waitFor(() =>
      expect(notify.success).toHaveBeenCalledWith(LOST_RUNNER_RULE_SAVED_TITLE, LOST_RUNNER_RULE_SAVED_MESSAGE)
    );
    await waitFor(() =>
      expect(
        queryClient.getQueryData<Record<string, unknown>>(generalKeys.detail("acme", UUID))
          ?.dedicated_runner_lost_action
      ).toBe("fall_back")
    );
  });

  it("renders the member form inert without a save control", async () => {
    harness.role = ROLE_MEMBER;
    const { calls } = setup(projectRow({ dedicated_runner_lost_action: "fall_back" }));
    await screen.findByText("Fall back to the pod");
    expect(screen.getByRole("button", { name: /Fall back to the pod/ })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Save changes" })).not.toBeInTheDocument();
    fireEvent.submit(screen.getByRole("form", { name: "AI Workers settings" }));
    await waitFor(() => expect(calls).toHaveLength(0));
  });

  it("shows the saving state and ignores resubmits while pending", async () => {
    const user = userEvent.setup();
    const { calls } = setup(projectRow({ dedicated_runner_lost_action: "fail_to_backlog" }), 200, true);
    await screen.findByText("Fail the run and move the issue to Backlog");
    await user.click(screen.getByRole("button", { name: /Fail the run and move the issue to Backlog/ }));
    await user.click(await screen.findByRole("option", { name: "Fall back to the pod" }));
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await screen.findByRole("button", { name: "Saving" });
    fireEvent.submit(screen.getByRole("form", { name: "AI Workers settings" }));
    await waitFor(() => expect(calls).toHaveLength(1));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(calls).toHaveLength(1);
  });

  it("toasts when the save fails", async () => {
    const user = userEvent.setup();
    setup(projectRow({ dedicated_runner_lost_action: "fail_to_backlog" }), 500);
    await screen.findByText("Fail the run and move the issue to Backlog");
    await user.click(screen.getByRole("button", { name: /Fail the run and move the issue to Backlog/ }));
    await user.click(await screen.findByRole("option", { name: "Fall back to the pod" }));
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() =>
      expect(notify.error).toHaveBeenCalledWith(
        LOST_RUNNER_RULE_SAVE_FAILED_TITLE,
        LOST_RUNNER_RULE_SAVE_FAILED_MESSAGE
      )
    );
  });
});
