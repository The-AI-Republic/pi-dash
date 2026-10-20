// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One-boolean writes fold into both flag caches: the retrieve row the
// tabs read and the collection the project navigation reads.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ProjectDetails } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { authKeys } from "../../auth/index.js";
import { projectKeys } from "../../projects/index.js";
import { useUpdateProjectFeature } from "./featureQueries.js";

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

describe("useUpdateProjectFeature", () => {
  it("PATCHes exactly one flag and folds the answer into both caches", async () => {
    const updated = projectRow({ issue_views_view: true });
    const calls = stub(updated);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const other = projectRow({ id: "223e4567-e89b-12d3-a456-426614174009" });
    queryClient.setQueryData(projectKeys.details(SLUG), [projectRow(), other]);
    queryClient.setQueryData(authKeys.project(SLUG, PROJECT), {
      ...projectRow(),
      agent_executor_options: [],
      default_agent_executor: "local_runner",
    });

    const { result } = renderHook(() => useUpdateProjectFeature(SLUG), { wrapper: wrap(queryClient) });
    await result.current.mutateAsync({ projectId: PROJECT, patch: { issue_views_view: true } });

    expect(calls.filter((call) => call.method === "PATCH")).toEqual([
      {
        method: "PATCH",
        url: `https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/`,
        body: '{"issue_views_view":true}',
      },
    ]);
    await waitFor(() => expect(queryClient.getQueryData(projectKeys.details(SLUG))).toEqual([updated, other]));
    const detail = queryClient.getQueryData(authKeys.project(SLUG, PROJECT)) as Record<string, unknown>;
    expect(detail.issue_views_view).toBe(true);
    // Retrieve-only fields survive the fold.
    expect(detail.default_agent_executor).toBe("local_runner");
  });

  it("leaves an absent collection and an absent retrieve row absent", async () => {
    stub(projectRow());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useUpdateProjectFeature(SLUG), { wrapper: wrap(queryClient) });
    await result.current.mutateAsync({ projectId: PROJECT, patch: { page_view: true } });
    expect(queryClient.getQueryData(projectKeys.details(SLUG))).toBeUndefined();
    expect(queryClient.getQueryData(authKeys.project(SLUG, PROJECT))).toBeUndefined();
  });
});
