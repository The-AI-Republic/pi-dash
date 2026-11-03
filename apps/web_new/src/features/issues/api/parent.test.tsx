// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import type { Issue, SubIssuesResponse, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "./keys.js";
import { parentIssueQueryOptions, parentSiblingsQueryOptions, siblingRows, useParentWrite } from "./parent.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const PARENT = "423e4567-e89b-12d3-a456-426614174003";
const OTHER = "523e4567-e89b-12d3-a456-426614174004";

function issue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Child item",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
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
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

interface Handle {
  setParent: ((parentId: string | null) => Promise<void>) | null;
}

function Probe({ onReady }: { onReady: (handle: Handle) => void }) {
  const write = useParentWrite({ workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE });
  onReady({ setParent: write.setParent });
  return <span data-saving={write.isSavingParent ? "true" : "false"} />;
}

function harness() {
  resetAppClient();
  const writes: Array<{ url: string; body: unknown }> = [];
  const conversation = async (url: string, init: TransportInit) => {
    if (init.method === "PATCH") {
      writes.push({ url, body: JSON.parse(init.body ?? "{}") });
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"c"}' };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  };
  const client = createAppClient("https://api.test", conversation as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(issueKeys.detail(SLUG, ISSUE), issue());
  queryClient.setQueryData([...issueKeys.all(SLUG), "parent-siblings", PARENT], { sub_issues: [] });
  queryClient.setQueryData(issueKeys.detail(SLUG, PARENT), issue({ id: PARENT, sequence_id: 4 }));
  const handle: Handle = { setParent: null };
  render(
    <QueryClientProvider client={queryClient}>
      <Probe onReady={(next) => (handle.setParent = next.setParent)} />
    </QueryClientProvider>
  );
  return { client, queryClient, writes, handle };
}

afterEach(() => {
  resetAppClient();
});

describe("useParentWrite", () => {
  it("patches the parent id and re-reads the sibling list and the new parent's detail", async () => {
    const { queryClient, writes, handle } = harness();
    await handle.setParent!(PARENT);
    expect(writes).toHaveLength(1);
    expect(writes[0]!.body).toEqual({ parent_id: PARENT });
    expect(queryClient.getQueryData(issueKeys.detail(SLUG, ISSUE))).toMatchObject({ parent_id: PARENT });
    const siblings = queryClient.getQueryState([...issueKeys.all(SLUG), "parent-siblings", PARENT]);
    const parent = queryClient.getQueryState(issueKeys.detail(SLUG, PARENT));
    await waitFor(() => expect(siblings?.isInvalidated).toBe(true));
    expect(parent?.isInvalidated).toBe(true);
  });

  it("clears the parent without re-reading a parent detail", async () => {
    const { queryClient, writes, handle } = harness();
    await handle.setParent!(null);
    expect(writes).toHaveLength(1);
    expect(writes[0]!.body).toEqual({ parent_id: null });
    const siblings = queryClient.getQueryState([...issueKeys.all(SLUG), "parent-siblings", PARENT]);
    await waitFor(() => expect(siblings?.isInvalidated).toBe(true));
    expect(queryClient.getQueryState(issueKeys.detail(SLUG, PARENT))?.isInvalidated).not.toBe(true);
  });
});

describe("parent reads", () => {
  it("disables the parent detail read until an id is known", () => {
    const { client } = harness();
    expect(parentIssueQueryOptions(client, SLUG, PROJECT, "").enabled).toBe(false);
    expect(parentIssueQueryOptions(client, SLUG, PROJECT, PARENT).enabled).toBe(true);
    expect(parentSiblingsQueryOptions(client, SLUG, PROJECT, PARENT).queryKey).toContain("parent-siblings");
  });

  it("removes the item itself from its parent's children and tolerates a missing page", () => {
    const page: SubIssuesResponse = {
      sub_issues: [
        { id: ISSUE, name: "Child item", sequence_id: 7, state_group: "started" } as never,
        { id: OTHER, name: "Sibling item", sequence_id: 8, state_group: "started" } as never,
      ],
      state_distribution: {},
    };
    expect(siblingRows(page.sub_issues, ISSUE).map((row) => row.id)).toEqual([OTHER]);
    expect(siblingRows(undefined, ISSUE)).toEqual([]);
  });
});
