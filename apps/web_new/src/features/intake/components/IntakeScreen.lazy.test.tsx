// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake screen lazy detail (NEWFRONT-870, perf budgets). The detail rides
// its own chunk behind a Suspense boundary: a selection first paints the
// detail skeleton, and the resolved chunk mounts the request detail with
// the screen's props. The chunk itself is latched so the test observes the
// skeleton; the stub stands in for the real detail pane.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn();
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => navigate,
}));

let releaseDetailChunk: (() => void) | undefined;
vi.mock("./IntakeDetail.js", async () => {
  await new Promise<void>((resolve) => {
    releaseDetailChunk = resolve;
  });
  return {
    IntakeDetail: ({ issueId }: { issueId: string }) => (
      <div data-testid="intake-detail-stub" data-issue-id={issueId} />
    ),
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { IntakeSearch } from "../lib/search.js";
import { IntakeScreen } from "./IntakeScreen.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_1 = "11111111-1111-4111-8111-111111111111";
const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function releaseDetail(): void {
  const release = releaseDetailChunk;
  if (release === undefined) throw new Error("the detail chunk was never requested");
  releaseDetailChunk = undefined;
  release();
}

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function installServer(): void {
  createAppClient("https://api.test", (async (url: string) => {
    const parsed = new URL(url);
    const path = parsed.pathname;
    if (path.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/`) {
      return jsonResponse(url, 200, {
        id: PROJECT_ID,
        name: "Apollo",
        identifier: "APO",
        sort_order: null,
        logo_props: {},
        member_role: 15,
        archived_at: null,
        workspace: WS_ID,
        cycle_view: true,
        issue_views_view: true,
        module_view: true,
        page_view: false,
        inbox_view: true,
        is_default: false,
        guest_view_all_features: false,
        project_lead: null,
        network: 0,
        created_at: "2026-01-02T03:04:05Z",
        updated_at: "2026-02-03T04:05:06Z",
        created_by: null,
        updated_by: null,
        agent_executor_options: [],
        default_agent_executor: "local_runner",
      });
    }
    if (path.endsWith("/project-roles/")) return jsonResponse(url, 200, { [PROJECT_ID]: 20 });
    if (path.endsWith("/api/users/me/workspaces/")) {
      return jsonResponse(url, 200, [
        { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
      ]);
    }
    if (path === `/api/workspaces/acme/projects/${PROJECT_ID}/intake-issues/`) {
      return jsonResponse(url, 200, {
        grouped_by: null,
        sub_grouped_by: null,
        total_count: 1,
        next_cursor: "",
        prev_cursor: "",
        next_page_results: false,
        prev_page_results: false,
        count: 1,
        total_results: 1,
        results: [
          {
            id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
            status: -2,
            duplicate_to: null,
            snoozed_till: null,
            source: null,
            created_by: null,
            issue: {
              id: ISSUE_1,
              name: "Alpha",
              priority: "none",
              sequence_id: 1,
              project_id: PROJECT_ID,
              created_at: "2026-10-01T00:00:00Z",
              label_ids: [],
              created_by: null,
            },
          },
        ],
      });
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
}

function renderScreen(search: IntakeSearch) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <IntakeScreen workspaceSlug="acme" projectId={PROJECT_ID} search={search} onSearchChange={vi.fn()} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("IntakeScreen lazy detail", () => {
  it("paints the detail skeleton while the detail chunk loads, then the detail", async () => {
    installServer();
    renderScreen({ currentTab: "open", inboxIssueId: ISSUE_1 });
    expect(await screen.findByRole("status", { name: "Loading work item" })).toBeInTheDocument();
    await vi.waitFor(() => expect(releaseDetailChunk).toBeTypeOf("function"));
    releaseDetail();
    const stub = await screen.findByTestId("intake-detail-stub");
    expect(stub).toHaveAttribute("data-issue-id", ISSUE_1);
    expect(screen.queryByRole("status", { name: "Loading work item" })).not.toBeInTheDocument();
  });
});
