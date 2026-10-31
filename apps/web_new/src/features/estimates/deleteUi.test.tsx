// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete flow (PSET-049, EST-016): the confirmation cannot be dismissed
// while the delete is in flight, and a failure keeps the dialog open.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { authKeys } from "../auth/index.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import { EstimatesScreen } from "./EstimatesScreen.js";

const WORKSPACE = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ACTIVE_ID = "44444444-4444-4444-8444-444444444444";

vi.mock("../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
}));

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function projectRow(): Record<string, unknown> {
  return { id: PROJECT, name: "Website", identifier: "WEB", logo_props: {}, estimate: ACTIVE_ID };
}

function stubApi(options: { holdDelete?: Promise<void>; deleteStatus?: number } = {}) {
  const systems = [
    {
      id: ACTIVE_ID,
      workspace: "11111111-1111-4111-8111-111111111111",
      project: PROJECT,
      name: "Active system",
      description: "",
      type: "points",
      last_used: true,
      points: [],
      created_at: "2024-01-01T00:00:00Z",
      updated_at: "2024-01-01T00:00:00Z",
      created_by: null,
      updated_by: null,
    },
  ];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/projects/${PROJECT}/estimates/`) && method === "GET") return jsonResponse(url, 200, systems);
    if (url.includes("/estimates/") && method === "DELETE") {
      if (options.holdDelete !== undefined) await options.holdDelete;
      if ((options.deleteStatus ?? 204) !== 204) return jsonResponse(url, options.deleteStatus ?? 500, {});
      return { ...jsonResponse(url, 204, {}), text: async () => "" } as TransportResponse;
    }
    if (url.endsWith(`/projects/${PROJECT}/`) && method === "GET") return jsonResponse(url, 200, projectRow());
    if (url.endsWith(`/projects/${PROJECT}/`) && method === "PATCH") {
      return jsonResponse(url, 200, {
        ...projectRow(),
        estimate: null,
        created_at: "2024-01-01T00:00:00Z",
        updated_at: "2024-01-01T00:00:00Z",
        created_by: null,
        updated_by: null,
        deleted_at: null,
        workspace: "11111111-1111-4111-8111-111111111111",
        description: "",
        description_text: null,
        description_html: null,
        network: 2,
        default_assignee: null,
        project_lead: null,
        emoji: null,
        icon_prop: null,
        module_view: false,
        cycle_view: false,
        issue_views_view: false,
        page_view: false,
        intake_view: false,
        inbox_view: false,
        is_time_tracking_enabled: false,
        is_issue_type_enabled: true,
        is_default: false,
        guest_view_all_features: false,
        members_can_edit_states: true,
        cover_image: null,
        cover_image_asset: null,
        cover_image_url: null,
        archive_in: 0,
        close_in: 0,
        default_state: null,
        archived_at: null,
        timezone: "UTC",
        external_source: null,
        external_id: null,
        repo_url: "",
        base_branch: "",
        is_favorite: false,
        sort_order: null,
        member_role: 20,
        anchor: null,
        members: [],
        next_work_item_sequence: 1,
        agent_executor_options: [],
        default_agent_executor: "",
      });
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
}

function renderScreen(): void {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(authKeys.project(WORKSPACE, PROJECT), projectRow());
  render(
    <QueryClientProvider client={client}>
      <EstimatesScreen workspaceSlug={WORKSPACE} projectId={PROJECT} canManage />
    </QueryClientProvider>
  );
}

async function openDelete(user: ReturnType<typeof userEvent.setup>): Promise<HTMLElement> {
  await screen.findByText("Active system");
  await user.click(screen.getByRole("button", { name: "Delete Active system" }));
  return screen.findByRole("dialog");
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("estimate delete dialog", () => {
  it("blocks dismissal while the delete is in flight", async () => {
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubApi({ holdDelete: held });
    const user = userEvent.setup();
    renderScreen();
    const dialog = await openDelete(user);
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    release();
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(notifySuccess).toHaveBeenCalled();
  });

  it("keeps the dialog with the error notice when the delete fails", async () => {
    stubApi({ deleteStatus: 500 });
    const user = userEvent.setup();
    renderScreen();
    const dialog = await openDelete(user);
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        "Couldn’t delete the estimate",
        expect.stringContaining("unable to delete")
      )
    );
    // The failure title names the deletion, never the old creation-failure
    // title (NEWFRONT-344); the oracle pins the body, not the title.
    expect(notifyError).not.toHaveBeenCalledWith("Estimate creation failed", expect.anything());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
