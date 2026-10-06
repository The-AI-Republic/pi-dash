// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Self-contained API mocks for the perf specs (NEWFRONT-21). The payloads
// follow the @pidash/api-client contracts; the app parses every one with
// zod at runtime, so a malformed mock fails the spec loudly instead of
// silently rendering nothing. Anything outside the four endpoints the
// issue-list slice calls is aborted, keeping the specs hermetic: a new
// endpoint shows up as a failure that updates these mocks.
import type { Page } from "@playwright/test";
import { randomUUID } from "node:crypto";
import type { IssueListItem, IssueListResponse, Me, Project, Workspace } from "@pidash/api-client";

export const PERF_WORKSPACE_SLUG = "perf-ws";
export const PERF_PROJECT_ID = "123e4567-e89b-42d3-a456-426614174001";
export const PERF_PROJECT_IDENTIFIER = "PF";

const USER_ID = "123e4567-e89b-42d3-a456-426614174002";
const WORKSPACE_ID = "123e4567-e89b-42d3-a456-426614174003";
const NOW = "2026-10-01T00:00:00Z";

export function perfIssuesUrl(): string {
  return `/${PERF_WORKSPACE_SLUG}/projects/${PERF_PROJECT_ID}/issues`;
}

export function mockMe(): Me {
  return {
    id: USER_ID,
    email: "perf.tester@example.com",
    username: "perf.tester",
    first_name: "Perf",
    last_name: "Tester",
    display_name: "Perf Tester",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: NOW,
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "password",
    last_login_time: NOW,
  };
}

export function mockWorkspaces(): Workspace[] {
  return [
    {
      id: WORKSPACE_ID,
      name: "Perf Workspace",
      slug: PERF_WORKSPACE_SLUG,
      logo_url: null,
      total_members: 1,
      role: 15,
    },
  ];
}

export function mockProjects(): Project[] {
  return [
    {
      id: PERF_PROJECT_ID,
      name: "Perf Project",
      identifier: PERF_PROJECT_IDENTIFIER,
      sort_order: null,
      logo_props: {},
      member_role: 15,
      intake_count: 0,
      archived_at: null,
      workspace: WORKSPACE_ID,
      cycle_view: false,
      issue_views_view: true,
      module_view: false,
      page_view: false,
      inbox_view: false,
      is_default: false,
      guest_view_all_features: false,
      project_lead: null,
      network: 0,
      created_at: NOW,
      updated_at: NOW,
      created_by: null,
      updated_by: null,
    },
  ];
}

const PRIORITIES: IssueListItem["priority"][] = ["urgent", "high", "medium", "low", "none"];

export function mockIssue(index: number): IssueListItem {
  return {
    id: randomUUID(),
    name: `Perf issue ${index + 1}`,
    state_id: null,
    sort_order: index,
    completed_at: null,
    estimate_point: null,
    priority: PRIORITIES[index % PRIORITIES.length] ?? "none",
    start_date: null,
    target_date: null,
    sequence_id: index + 1,
    project_id: PERF_PROJECT_ID,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: NOW,
    updated_at: NOW,
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };
}

export function mockIssues(count: number): IssueListResponse {
  const results = Array.from({ length: count }, (_, index) => mockIssue(index));
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: count,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count,
    total_results: count,
    results,
  };
}

function jsonFulfill(payload: unknown) {
  return { status: 200, contentType: "application/json", body: JSON.stringify(payload) };
}

export async function installApiMocks(
  page: Page,
  options: { issueCount: number; blockIssues?: boolean }
): Promise<void> {
  const issuesPath = `/api/workspaces/${PERF_WORKSPACE_SLUG}/projects/${PERF_PROJECT_ID}/issues/`;
  const issuesPayload = mockIssues(options.issueCount);
  // Predicate matchers (not globs) so query strings can't slip a route.
  await page.route(
    (url) => url.pathname === "/api/users/me/",
    (route) => route.fulfill(jsonFulfill(mockMe()))
  );
  await page.route(
    (url) => url.pathname === "/api/users/me/workspaces/",
    (route) => route.fulfill(jsonFulfill(mockWorkspaces()))
  );
  await page.route(
    (url) => url.pathname === `/api/workspaces/${PERF_WORKSPACE_SLUG}/projects/`,
    (route) => route.fulfill(jsonFulfill(mockProjects()))
  );
  await page.route(
    (url) => url.pathname === issuesPath,
    (route) => (options.blockIssues ? route.abort("blockedbyclient") : route.fulfill(jsonFulfill(issuesPayload)))
  );
  const known = new Set([
    "/api/users/me/",
    "/api/users/me/workspaces/",
    `/api/workspaces/${PERF_WORKSPACE_SLUG}/projects/`,
    issuesPath,
  ]);
  await page.route(
    (url) =>
      (url.pathname === "/api" || url.pathname.startsWith("/api/") || url.pathname.startsWith("/auth/")) &&
      !known.has(url.pathname),
    (route) => route.abort("blockedbyclient")
  );
}
