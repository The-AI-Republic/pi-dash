// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity data (ISS-194): the property history read, its query key and
// the hook the feed uses.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { IssueActivity, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import { activitiesQueryOptions, useIssueActivities } from "./api.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const SCOPE = { workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE };

function activity(overrides: Partial<IssueActivity> = {}): IssueActivity {
  return {
    id: "a13e4567-e89b-12d3-a456-426614174001",
    issue: ISSUE,
    project: PROJECT,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    actor: "523e4567-e89b-12d3-a456-426614174004",
    actor_detail: null,
    verb: "created",
    field: null,
    old_value: null,
    new_value: null,
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    issue_detail: null,
    project_detail: null,
    workspace_detail: null,
    created_at: "2026-10-16T10:00:00Z",
    updated_at: "2026-10-16T10:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

interface Recorded {
  method: string;
  url: string;
}

function stubClient(handler: (call: Recorded) => { status: number; body: unknown }) {
  const calls: Recorded[] = [];
  const client = createAppClient("https://api.test", async (url: string, init: TransportInit) => {
    const call = { method: init.method, url };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    const answer = handler(call);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  });
  return { client, calls };
}

afterEach(() => {
  resetAppClient();
});

describe("activitiesQueryOptions", () => {
  it("reads the property split of the issue history", async () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: [activity()] }));
    const options = activitiesQueryOptions(client, SCOPE);
    expect(options.queryKey).toEqual(issueKeys.activity(SLUG, PROJECT, ISSUE));
    const rows = await (options.queryFn as () => Promise<IssueActivity[]>)();
    expect(rows).toHaveLength(1);
    expect(calls[0]?.url).toContain(`/issues/${ISSUE}/history/`);
    expect(calls[0]?.url).toContain("activity_type=issue-property");
    expect(calls[0]?.url).not.toContain("created_at__gt");
  });

  it("surfaces a refused read", async () => {
    const { client } = stubClient(() => ({ status: 500, body: { error: "boom" } }));
    const options = activitiesQueryOptions(client, SCOPE);
    await expect((options.queryFn as () => Promise<IssueActivity[]>)()).rejects.toBeTruthy();
  });

  it("refetches only the delta once rows are cached", async () => {
    const known = activity({ created_at: "2026-10-16T10:00:00Z" });
    const fresh = activity({
      id: "a23e4567-e89b-12d3-a456-426614174002",
      field: "name",
      verb: "updated",
      new_value: "Renamed",
      created_at: "2026-10-16T11:00:00Z",
    });
    const { client, calls } = stubClient(() => ({ status: 200, body: [fresh] }));
    const queryClient = new QueryClient();
    queryClient.setQueryData(issueKeys.activity(SLUG, PROJECT, ISSUE), [known]);
    const options = activitiesQueryOptions(client, SCOPE, queryClient);
    const rows = await (options.queryFn as () => Promise<IssueActivity[]>)();
    expect(new URL(calls[0]?.url ?? "").searchParams.get("created_at__gt")).toBe("2026-10-16T10:00:00Z");
    expect(rows.map((row) => row.id)).toEqual([known.id, fresh.id]);
  });

  it("reads the whole list when the cache is empty or unreadable", async () => {
    const { client, calls } = stubClient(() => ({ status: 200, body: [activity()] }));
    const queryClient = new QueryClient();
    queryClient.setQueryData(issueKeys.activity(SLUG, PROJECT, ISSUE), []);
    const empty = activitiesQueryOptions(client, SCOPE, queryClient);
    await (empty.queryFn as () => Promise<IssueActivity[]>)();
    expect(calls[0]?.url).not.toContain("created_at__gt");
    queryClient.setQueryData(issueKeys.activity(SLUG, PROJECT, ISSUE), [activity({ created_at: "not a date" })]);
    const unreadable = activitiesQueryOptions(client, SCOPE, queryClient);
    await (unreadable.queryFn as () => Promise<IssueActivity[]>)();
    expect(calls[1]?.url).not.toContain("created_at__gt");
  });

  it("surfaces a refused delta read", async () => {
    const { client } = stubClient(() => ({ status: 500, body: { error: "boom" } }));
    const queryClient = new QueryClient();
    queryClient.setQueryData(issueKeys.activity(SLUG, PROJECT, ISSUE), [activity()]);
    const options = activitiesQueryOptions(client, SCOPE, queryClient);
    await expect((options.queryFn as () => Promise<IssueActivity[]>)()).rejects.toBeTruthy();
  });
});

describe("useIssueActivities", () => {
  it("caches the rows under the activity key", async () => {
    stubClient(() => ({
      status: 200,
      body: [activity(), activity({ id: "a23e4567-e89b-12d3-a456-426614174002", field: "state" })],
    }));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = function Wrapper({ children }: { children?: unknown }): React.ReactElement {
      return (
        <QueryClientProvider client={queryClient}>
          <React.Fragment>{children as React.ReactNode}</React.Fragment>
        </QueryClientProvider>
      );
    };
    const { result } = renderHook(() => useIssueActivities(SCOPE), { wrapper });
    await waitFor(() => expect(result.current.data).toHaveLength(2));
    expect(queryClient.getQueryData(issueKeys.activity(SLUG, PROJECT, ISSUE))).toHaveLength(2);
  });

  it("reports the failure state the feed renders", async () => {
    stubClient(() => ({ status: 500, body: { error: "boom" } }));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = function Wrapper({ children }: { children?: unknown }): React.ReactElement {
      return (
        <QueryClientProvider client={queryClient}>
          <React.Fragment>{children as React.ReactNode}</React.Fragment>
        </QueryClientProvider>
      );
    };
    const { result } = renderHook(() => useIssueActivities(SCOPE), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });

  it("picks up new rows in place when the read is invalidated", async () => {
    const known = activity({ created_at: "2026-10-16T10:00:00Z" });
    const fresh = activity({
      id: "a23e4567-e89b-12d3-a456-426614174002",
      field: "name",
      verb: "updated",
      new_value: "Renamed",
      created_at: "2026-10-16T11:00:00Z",
    });
    const { calls } = stubClient((call) => ({
      status: 200,
      body: call.url.includes("created_at__gt=") ? [fresh] : [known],
    }));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = function Wrapper({ children }: { children?: unknown }): React.ReactElement {
      return (
        <QueryClientProvider client={queryClient}>
          <React.Fragment>{children as React.ReactNode}</React.Fragment>
        </QueryClientProvider>
      );
    };
    const { result } = renderHook(() => useIssueActivities(SCOPE), { wrapper });
    await waitFor(() => expect(result.current.data).toHaveLength(1));
    await queryClient.invalidateQueries({ queryKey: issueKeys.activity(SLUG, PROJECT, ISSUE) });
    await waitFor(() => expect(result.current.data).toHaveLength(2));
    expect(calls).toHaveLength(2);
    expect(new URL(calls[1]?.url ?? "").searchParams.get("created_at__gt")).toBe("2026-10-16T10:00:00Z");
    expect(result.current.data?.map((row) => row.id)).toEqual([known.id, fresh.id]);
  });
});
