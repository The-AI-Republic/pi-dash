// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One-boolean writes fold into every flag cache: the retrieve row the
// tabs read, the rich collection, and the project-list collection the
// sidebar and the tab strip read. The last one is what makes an area
// disappear from navigation without a reload.
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { render, renderHook, screen, waitFor } from "@testing-library/react";
import type { ProjectDetails } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { authKeys } from "../../auth/index.js";
import { projectKeys, projectsQueryOptions, selectProjectTabs } from "../../projects/index.js";
import { foldFeatureIntoList, useUpdateProjectFeature } from "./featureQueries.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "223e4567-e89b-12d3-a456-426614174009";

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

/** The list projection the sidebar and the tab strip read. */
function listRow(overrides: Record<string, unknown> = {}) {
  return {
    id: PROJECT,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 7,
    archived_at: null,
    workspace: "323e4567-e89b-12d3-a456-426614174002",
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
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
  it("PATCHes exactly one flag and folds the answer into every cache", async () => {
    const updated = projectRow({ issue_views_view: true });
    const calls = stub(updated);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const other = projectRow({ id: OTHER });
    queryClient.setQueryData(projectKeys.details(SLUG), [projectRow(), other]);
    queryClient.setQueryData(projectKeys.list(SLUG), [listRow({ issue_views_view: false }), listRow({ id: OTHER })]);
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

    const list = queryClient.getQueryData(projectKeys.list(SLUG)) as Array<Record<string, unknown>>;
    expect(list[0]).toEqual(listRow({ issue_views_view: true }));
    // List-only annotations (the intake badge, the role) are untouched.
    expect(list[0]?.intake_count).toBe(7);
    expect(list[0]?.member_role).toBe(20);
    expect(list[1]?.id).toBe(OTHER);
  });

  it("leaves an absent collection, an absent retrieve row and an absent list absent", async () => {
    stub(projectRow());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useUpdateProjectFeature(SLUG), { wrapper: wrap(queryClient) });
    await result.current.mutateAsync({ projectId: PROJECT, patch: { page_view: true } });
    expect(queryClient.getQueryData(projectKeys.details(SLUG))).toBeUndefined();
    expect(queryClient.getQueryData(projectKeys.list(SLUG))).toBeUndefined();
    expect(queryClient.getQueryData(authKeys.project(SLUG, PROJECT))).toBeUndefined();
  });

  it("hides and restores the area in the list the navigation reads, without a refetch", async () => {
    const calls = stub(projectRow({ issue_views_view: true }));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 5 * 60_000 } } });
    queryClient.setQueryData(projectKeys.list(SLUG), [listRow({ issue_views_view: true })]);

    // The probe reads the list the way the sidebar and the tab strip do.
    function ListProbe(): React.ReactElement {
      const { data } = useQuery(projectsQueryOptions(getAppClient(), SLUG));
      const row = data?.[0];
      if (row === undefined) return <p>loading</p>;
      const tabs = selectProjectTabs({ workspaceSlug: SLUG, projectId: PROJECT, flags: row, role: row.member_role });
      return (
        <ul>
          {tabs.map((tab) => (
            <li key={tab.key}>{tab.name}</li>
          ))}
        </ul>
      );
    }

    const { result } = renderHook(() => useUpdateProjectFeature(SLUG), { wrapper: wrap(queryClient) });
    render(
      <QueryClientProvider client={queryClient}>
        <ListProbe />
      </QueryClientProvider>
    );
    expect(screen.getByText("Views")).toBeInTheDocument();

    await result.current.mutateAsync({ projectId: PROJECT, patch: { issue_views_view: false } });
    await waitFor(() => expect(screen.queryByText("Views")).not.toBeInTheDocument());
    expect(screen.getByText("Pages")).toBeInTheDocument();
    expect(projectReads(calls)).toEqual([]);

    await result.current.mutateAsync({ projectId: PROJECT, patch: { issue_views_view: true } });
    await waitFor(() => expect(screen.getByText("Views")).toBeInTheDocument());
    expect(projectReads(calls)).toEqual([]);
  });
});

/** Project reads the probe would have needed; the csrf prime is not one. */
function projectReads(calls: Array<{ method: string; url: string }>): Array<{ method: string; url: string }> {
  return calls.filter((call) => call.method === "GET" && call.url.includes("/projects/"));
}

describe("foldFeatureIntoList", () => {
  it("returns an absent list unchanged", () => {
    expect(foldFeatureIntoList(undefined, PROJECT, { page_view: true })).toBeUndefined();
  });

  it("leaves every other row alone", () => {
    const other = listRow({ id: OTHER });
    expect(foldFeatureIntoList([other], PROJECT, { page_view: true })).toEqual([other]);
    expect(foldFeatureIntoList([other], PROJECT, { page_view: true })?.[0]).toBe(other);
  });
});
