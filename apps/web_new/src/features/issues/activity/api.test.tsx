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
  });

  it("surfaces a refused read", async () => {
    const { client } = stubClient(() => ({ status: 500, body: { error: "boom" } }));
    const options = activitiesQueryOptions(client, SCOPE);
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
});
