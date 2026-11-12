// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request writes (INT-019/029): the intake PATCH envelope, optimistic
// detail paint with rollback and notice, settle invalidation, and the
// top-level seam the triage moves reuse.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { IntakeIssueDetail, IntakeIssueDetailItem, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys } from "../../issues/index.js";
import { intakeKeys } from "./keys.js";
import { paintIntakeDetailPatch, useUpdateIntakeIssue } from "./mutations.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "11111111-1111-4111-8111-111111111111";
const DETAIL_PATH = `/api/workspaces/${WS}/projects/${PROJECT}/intake-issues/${ISSUE}/`;

function detailItem(overrides: Partial<IntakeIssueDetailItem> = {}): IntakeIssueDetailItem {
  return {
    id: ISSUE,
    name: "A request",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: null,
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Body</p>",
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function seedDetail(item: IntakeIssueDetailItem): IntakeIssueDetail {
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
    status: -2,
    duplicate_to: null,
    snoozed_till: null,
    duplicate_issue_detail: null,
    source: null,
    issue: item,
  };
}

function response(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => (body === undefined ? "" : JSON.stringify(body)),
  } as TransportResponse;
}

interface PatchServer {
  bodies: unknown[];
  status: number;
  refuse: unknown;
}

function stubTransport(server: PatchServer, echo: (body: string | undefined) => unknown): void {
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return response(url, 200, { csrf_token: "csrf" });
    if (url.includes(DETAIL_PATH) && method === "PATCH") {
      server.bodies.push(init?.body === undefined ? null : JSON.parse(init.body));
      if (server.status >= 400) return response(url, server.status, server.refuse);
      return response(url, server.status, echo(init?.body));
    }
    throw new Error(`unexpected ${method} ${url}`);
  }) as never);
}

function renderUpdate(queryClient: QueryClient) {
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return renderHook(
    () =>
      useUpdateIntakeIssue({ workspaceSlug: WS, projectId: PROJECT, issueId: ISSUE, failureTitle: "Couldn't save" }),
    {
      wrapper,
    }
  );
}

function freshClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("useUpdateIntakeIssue (INT-019)", () => {
  it("paints the detail optimistically and sends the issue envelope", async () => {
    const queryClient = freshClient();
    const seeded = seedDetail(detailItem());
    queryClient.setQueryData(intakeKeys.detail(WS, PROJECT, ISSUE), seeded);
    const server: PatchServer = { bodies: [], status: 200, refuse: { error: "refused" } };
    stubTransport(server, () => seedDetail(detailItem({ priority: "high", updated_at: "2026-10-03T00:00:00Z" })));
    const { result } = renderUpdate(queryClient);

    let settled!: IntakeIssueDetail;
    await React.act(async () => {
      settled = await result.current.mutateAsync({ issue: { priority: "high" } });
    });

    expect(server.bodies).toEqual([{ issue: { priority: "high" } }]);
    expect(settled.issue.priority).toBe("high");
    expect(queryClient.getQueryState(intakeKeys.detail(WS, PROJECT, ISSUE))?.isInvalidated).toBe(true);
  });

  it("shows the painted value while the write is in flight", async () => {
    const queryClient = freshClient();
    queryClient.setQueryData(intakeKeys.detail(WS, PROJECT, ISSUE), seedDetail(detailItem()));
    let release!: (body: unknown) => void;
    const gate = new Promise<unknown>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) return response(url, 200, { csrf_token: "csrf" });
      if (url.includes(DETAIL_PATH) && (init?.method ?? "GET") === "PATCH") {
        return response(url, 200, await gate);
      }
      throw new Error(`unexpected ${url}`);
    }) as never);
    const { result } = renderUpdate(queryClient);

    let pending!: Promise<IntakeIssueDetail>;
    await React.act(async () => {
      pending = result.current.mutateAsync({ issue: { name: "Painted" } });
      await Promise.resolve();
    });
    expect(queryClient.getQueryData<IntakeIssueDetail>(intakeKeys.detail(WS, PROJECT, ISSUE))?.issue.name).toBe(
      "Painted"
    );

    await React.act(async () => {
      release(seedDetail(detailItem({ name: "Painted" })));
      await pending;
    });
  });

  it("rolls back and notices when the server refuses", async () => {
    const queryClient = freshClient();
    const seeded = seedDetail(detailItem());
    queryClient.setQueryData(intakeKeys.detail(WS, PROJECT, ISSUE), seeded);
    const server: PatchServer = { bodies: [], status: 403, refuse: { error: "nope" } };
    stubTransport(server, () => seeded);
    const { result } = renderUpdate(queryClient);

    await React.act(async () => {
      await expect(result.current.mutateAsync({ issue: { priority: "urgent" } })).rejects.toThrow();
    });

    expect(queryClient.getQueryData(intakeKeys.detail(WS, PROJECT, ISSUE))).toEqual(seeded);
    expect(notifyError).toHaveBeenCalledWith("Couldn't save", expect.any(String));
  });

  it("sends top-level triage fields without painting the work item", async () => {
    const queryClient = freshClient();
    const seeded = seedDetail(detailItem());
    queryClient.setQueryData(intakeKeys.detail(WS, PROJECT, ISSUE), seeded);
    const server: PatchServer = { bodies: [], status: 200, refuse: { error: "refused" } };
    stubTransport(server, () => ({ ...seeded, status: 1 }));
    const { result } = renderUpdate(queryClient);

    await React.act(async () => {
      await result.current.mutateAsync({ status: 1 });
    });

    expect(server.bodies).toEqual([{ status: 1 }]);
    expect(queryClient.getQueryData(intakeKeys.detail(WS, PROJECT, ISSUE))).toEqual(seeded);
    expect(queryClient.getQueryState(intakeKeys.detail(WS, PROJECT, ISSUE))?.isInvalidated).toBe(true);
  });

  it("revalidates the versions list on description writes only", async () => {
    const queryClient = freshClient();
    queryClient.setQueryData(intakeKeys.detail(WS, PROJECT, ISSUE), seedDetail(detailItem()));
    const versionsKey = issueKeys.descriptionVersions(WS, PROJECT, ISSUE);
    queryClient.setQueryData(versionsKey, { results: [] });
    const server: PatchServer = { bodies: [], status: 200, refuse: { error: "refused" } };
    stubTransport(server, (body) => {
      const parsed = (body === undefined ? {} : JSON.parse(body)) as { issue?: Partial<IntakeIssueDetailItem> };
      return seedDetail(detailItem({ ...(parsed.issue ?? {}) }));
    });
    const { result } = renderUpdate(queryClient);

    await React.act(async () => {
      await result.current.mutateAsync({ issue: { priority: "low" } });
    });
    expect(queryClient.getQueryState(versionsKey)?.isInvalidated).toBe(false);

    await React.act(async () => {
      await result.current.mutateAsync({ issue: { description_html: "<p>New</p>" } });
    });
    expect(queryClient.getQueryState(versionsKey)?.isInvalidated).toBe(true);
  });
});

describe("paintIntakeDetailPatch", () => {
  it("paints nothing when the detail is not cached", () => {
    const queryClient = freshClient();
    const snapshots = paintIntakeDetailPatch(queryClient, WS, PROJECT, ISSUE, { priority: "high" });
    expect(snapshots).toEqual([]);
    expect(queryClient.getQueryData(intakeKeys.detail(WS, PROJECT, ISSUE))).toBeUndefined();
  });
});
