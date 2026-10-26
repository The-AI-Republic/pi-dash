// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived peek (parity/specs/archives.md §2.5, ARCH-007). It reads
// the archive-scoped detail, fills the live detail's absent annotation
// fields with empty values, and mounts the write surfaces disabled: the
// title is a heading and properties and activity are read-only.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { ArchivedIssuePeek, archivedIssueForDetail } from "./ArchivedIssuePeek.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "423e4567-e89b-12d3-a456-426614174003";
const USER = "123e4567-e89b-12d3-a456-426614174000";

function projectRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: PROJECT,
    name: "Website",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: USER,
    cycle_view: true,
    module_view: true,
    issue_views_view: true,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function archivedRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: ISSUE,
    name: "Old login form",
    state_id: null,
    sort_order: 1,
    completed_at: "2026-01-01T00:00:00Z",
    priority: "none",
    complexity_score: 0,
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: null,
    agent_executor: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: "2026-01-03",
    is_synced: false,
    description_html: "<p>Retired form</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

interface Answer {
  status: number;
  body: unknown;
}

function defaultRespond(url: string, init: { method: string }): Answer | null {
  if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: { csrf_token: "csrf" } };
  if (init.method !== "GET") return null;
  if (url.includes(`/issues/${ISSUE}/archive/`)) return { status: 200, body: archivedRow() };
  if (new RegExp(`/projects/${PROJECT}/$`).test(new URL(url).pathname)) return { status: 200, body: projectRow() };
  if (url.includes("/sub-issues/")) {
    return { status: 200, body: { sub_issues: [], state_distribution: {}, total_count: 0 } };
  }
  if (url.includes("/issue-relation/")) {
    return {
      status: 200,
      body: {
        blocking: [],
        blocked_by: [],
        duplicate: [],
        relates_to: [],
        start_after: [],
        start_before: [],
        finish_after: [],
        finish_before: [],
      },
    };
  }
  if (url.includes("description-versions")) {
    return {
      status: 200,
      body: {
        prev_cursor: "",
        cursor: "",
        next_cursor: null,
        prev_page_results: false,
        next_page_results: false,
        page_count: 0,
        total_results: 0,
        total_pages: 0,
        results: [],
      },
    };
  }
  if (
    url.includes("/issue-links/") ||
    url.includes("/attachments/") ||
    url.includes("/code-reviews/") ||
    url.includes("/history/") ||
    url.includes("/reactions/") ||
    url.endsWith("/comments/") ||
    url.includes("/states/")
  ) {
    return { status: 200, body: [] };
  }
  return null;
}

function renderPeek(overrides: { detail?: Answer; respond?: typeof defaultRespond } = {}) {
  const onClose = vi.fn();
  const reads: string[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method: string }) => {
    reads.push(url);
    const reply = (overrides.detail !== undefined && url.includes(`/issues/${ISSUE}/archive/`)
      ? overrides.detail
      : null) ??
      overrides.respond?.(url, init) ??
      defaultRespond(url, init) ?? { status: 404, body: {} };
    return { status: reply.status, url, headers: { get: () => null }, text: async () => JSON.stringify(reply.body) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  render(
    <QueryClientProvider client={queryClient}>
      <ArchivedIssuePeek
        workspaceSlug={SLUG}
        projectId={PROJECT}
        projectIdentifier="WEB"
        issueId={ISSUE}
        onClose={onClose}
      />
    </QueryClientProvider>
  );
  return { onClose, reads };
}

afterEach(() => {
  resetAppClient();
});

describe("archivedIssueForDetail", () => {
  it("supplies empty annotation-backed fields when no list row is in hand", () => {
    const issue = archivedIssueForDetail(archivedRow() as never);
    expect(issue.cycle_id).toBeNull();
    expect(issue.module_ids).toEqual([]);
    expect(issue.label_ids).toEqual([]);
    expect(issue.assignee_ids).toEqual([]);
    expect(issue.sub_issues_count).toBeNull();
    expect(issue.attachment_count).toBeNull();
    expect(issue.link_count).toBeNull();
    expect(issue.is_epic).toBe(false);
  });

  it("keeps the memberships and counts only the archived list row carries", () => {
    const cycle = "323e4567-e89b-12d3-a456-426614174002";
    const module = "523e4567-e89b-12d3-a456-426614174004";
    const label = "623e4567-e89b-12d3-a456-426614174005";
    const assignee = USER;
    const issue = archivedIssueForDetail(
      archivedRow() as never,
      {
        ...(archivedRow() as never as Record<string, unknown>),
        cycle_id: cycle,
        module_ids: [module],
        label_ids: [label],
        assignee_ids: [assignee],
        sub_issues_count: 2,
        attachment_count: 1,
        link_count: 3,
        is_epic: true,
      } as never
    );
    expect(issue.cycle_id).toBe(cycle);
    expect(issue.module_ids).toEqual([module]);
    expect(issue.label_ids).toEqual([label]);
    expect(issue.assignee_ids).toEqual([assignee]);
    expect(issue.sub_issues_count).toBe(2);
    expect(issue.attachment_count).toBe(1);
    expect(issue.link_count).toBe(3);
    expect(issue.is_epic).toBe(true);
  });
});

describe("ArchivedIssuePeek", () => {
  it("reads the archive-scoped detail and shows the title as a heading", async () => {
    const { reads } = renderPeek();
    expect(await screen.findByRole("heading", { name: "Old login form" })).toBeInTheDocument();
    expect(reads.some((url) => url.includes(`/projects/${PROJECT}/issues/${ISSUE}/archive/`))).toBe(true);
    expect(reads.some((url) => url.endsWith("/archived-issues/"))).toBe(false);
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
  });

  it("shows the loading panel until the detail arrives", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    createAppClient("https://api.test", (async (url: string, init: { method: string }) => {
      if (url.includes(`/issues/${ISSUE}/archive/`)) await gate;
      const reply = defaultRespond(url, init) ?? { status: 404, body: {} };
      return { status: reply.status, url, headers: { get: () => null }, text: async () => JSON.stringify(reply.body) };
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
    render(
      <QueryClientProvider client={queryClient}>
        <ArchivedIssuePeek
          workspaceSlug={SLUG}
          projectId={PROJECT}
          projectIdentifier="WEB"
          issueId={ISSUE}
          onClose={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("status", { name: "Loading work item" })).toBeInTheDocument();
    release();
    expect(await screen.findByRole("heading", { name: "Old login form" })).toBeInTheDocument();
  });

  it("collapses a missing archive detail into the error panel", async () => {
    renderPeek({ detail: { status: 404, body: {} } });
    expect(await screen.findByText("Work item does not exist")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Old login form" })).not.toBeInTheDocument();
  });

  it("collapses a malformed archive detail into the error panel", async () => {
    renderPeek({ detail: { status: 200, body: archivedRow({ archived_at: "not-a-date" }) } });
    expect(await screen.findByText("Work item does not exist")).toBeInTheDocument();
  });

  it("closes from the chrome and writes nothing", async () => {
    const user = userEvent.setup();
    const { onClose, reads } = renderPeek();
    await screen.findByRole("heading", { name: "Old login form" });
    const before = reads.length;
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(reads.length).toBe(before));
  });

  it("renders the title as a heading, never an editable field", async () => {
    renderPeek();
    await screen.findByRole("heading", { name: "Old login form" });
    expect(screen.queryByRole("textbox", { name: /title/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });
});
