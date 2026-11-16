// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail header row (ISS-160–163): identifier, copy link, subscribe,
// and the quick-action menu in one place the peek reuses.
import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import type { Issue } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { createQueryClient } from "../../../core/query/client.js";
import type { QuickActionsState } from "../menus/useQuickActions.js";
import { DetailHeaderActions } from "./DetailHeaderActions.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function issueFixture(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 42,
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
    description_html: "<p>Words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function menuFixture(): QuickActionsState {
  return { entries: [], handlers: { run: vi.fn() }, overlays: null };
}

function stubTransport() {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify({ subscribed: false }),
  })) as never);
}

function renderRow(props: Partial<React.ComponentProps<typeof DetailHeaderActions>> = {}) {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <DetailHeaderActions
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        issueKey="WEB-42"
        projectIdentifier="WEB"
        editable
        menu={menuFixture()}
        {...props}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  resetAppClient();
  stubTransport();
});

afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("DetailHeaderActions", () => {
  it("carries the identifier, copy link, subscribe, and menu", async () => {
    renderRow();
    expect(screen.getByRole("button", { name: "WEB-42" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy work item link" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Subscribe" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Work item actions" })).toBeInTheDocument();
  });

  it("drops the menu where the surface has none", async () => {
    renderRow({ menu: null });
    expect(screen.getByRole("button", { name: "WEB-42" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Subscribe" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Work item actions" })).toBeNull();
  });

  it("drops the subscribe toggle on an archived issue", () => {
    renderRow({ issue: issueFixture({ archived_at: "2024-03-04T05:06:07Z" }) });
    expect(screen.getByRole("button", { name: "WEB-42" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^(Subscribe|Unsubscribe)$/ })).toBeNull();
  });
});
