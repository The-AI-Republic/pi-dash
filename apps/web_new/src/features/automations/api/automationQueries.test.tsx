// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The automation write folds the answered row back into the projects
// collection; an absent collection stays absent. The state read keeps a
// per-project key under the automations subtree.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ProjectDetails } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectKeys } from "../../projects/index.js";
import { automationKeys, automationStatesQueryOptions, useUpdateAutomation } from "./automationQueries.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function projectRow(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: PROJECT,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "323e4567-e89b-12d3-a456-426614174002",
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
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
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
    base_branch: "",
    is_favorite: false,
    sort_order: 10000,
    member_role: 20,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function stub(answer: ProjectDetails) {
  const calls: Array<{ method: string; url: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string } = {}) => {
    calls.push({ method: init.method ?? "GET", url, body: init.body ?? "" });
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(answer) };
  }) as never);
  return calls;
}

function wrap(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

afterEach(() => {
  resetAppClient();
});

describe("useUpdateAutomation", () => {
  it("PATCHes only the automation fields and replaces the matching project row", async () => {
    const updated = projectRow({ archive_in: 6 });
    const calls = stub(updated);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const other = projectRow({ id: "223e4567-e89b-12d3-a456-426614174009" });
    queryClient.setQueryData(projectKeys.details(SLUG), [projectRow(), other]);

    const { result } = renderHook(() => useUpdateAutomation(SLUG), { wrapper: wrap(queryClient) });
    await result.current.mutateAsync({ projectId: PROJECT, patch: { archive_in: 6 } });

    expect(calls.filter((call) => call.method === "PATCH")).toEqual([
      {
        method: "PATCH",
        url: `https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/`,
        body: '{"archive_in":6}',
      },
    ]);
    await waitFor(() => expect(queryClient.getQueryData(projectKeys.details(SLUG))).toEqual([updated, other]));
  });

  it("leaves an absent collection absent", async () => {
    stub(projectRow());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useUpdateAutomation(SLUG), { wrapper: wrap(queryClient) });
    await result.current.mutateAsync({ projectId: PROJECT, patch: { close_in: 1, default_state: null } });
    expect(queryClient.getQueryData(projectKeys.details(SLUG))).toBeUndefined();
  });
});

describe("automationStatesQueryOptions", () => {
  it("keys the state read by workspace and project", () => {
    createAppClient("https://api.test", (() => {
      throw new Error("unexpected fetch");
    }) as never);
    const options = automationStatesQueryOptions(getAppClient(), SLUG, PROJECT);
    expect(options.queryKey).toEqual(automationKeys.states(SLUG, PROJECT));
    expect(options.queryKey).toEqual(["ws", SLUG, "automations", "states", PROJECT]);
  });
});
