// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-contained API mocks for the perf specs (NEWFRONT-21). The payloads
// follow the @pidash/api-client contracts; the app parses every one with
// zod at runtime, so a malformed mock fails the spec loudly instead of
// silently rendering nothing. Anything outside the eleven endpoints the
// issue-list slice calls is aborted, keeping the specs hermetic: a new
// endpoint shows up as a failure that updates these mocks.
//
// The root startup gate (WSET-034/035) sits above every route and reads
// GET /api/instances/ before the issues list can render, so the running
// instance answer is part of the known set. It is a full InstanceBootstrap
// envelope with setup finished, so the gate admits the app exactly as a
// provisioned server would.
import type { Page } from "@playwright/test";
import { randomUUID } from "node:crypto";
import type {
  InstanceBootstrap,
  IssueListItem,
  IssueListResponse,
  Me,
  OwnWorkspaceMembership,
  Profile,
  Project,
  ProjectDetail,
  ProjectRoleMap,
  ProjectUserProperties,
  SidebarPreferences,
  UnreadNotifications,
  Workspace,
} from "@pidash/api-client";

export const PERF_WORKSPACE_SLUG = "perf-ws";
export const PERF_PROJECT_ID = "123e4567-e89b-42d3-a456-426614174001";
export const PERF_PROJECT_IDENTIFIER = "PF";

const USER_ID = "123e4567-e89b-42d3-a456-426614174002";
const WORKSPACE_ID = "123e4567-e89b-42d3-a456-426614174003";
const NOW = "2026-10-01T00:00:00Z";

export function perfIssuesUrl(): string {
  return `/${PERF_WORKSPACE_SLUG}/projects/${PERF_PROJECT_ID}/issues`;
}

export function mockInstanceBootstrap(): InstanceBootstrap {
  return {
    config: {
      enable_signup: true,
      is_workspace_creation_disabled: false,
      is_google_enabled: false,
      is_github_enabled: false,
      is_gitlab_enabled: false,
      is_gitea_enabled: false,
      is_magic_login_enabled: false,
      is_email_password_enabled: true,
      is_smtp_configured: false,
      is_self_managed: true,
      instance_changelog_url: "",
    },
    instance: {
      id: "123e4567-e89b-42d3-a456-426614174005",
      created_at: NOW,
      updated_at: NOW,
      created_by: null,
      updated_by: null,
      deleted_at: null,
      instance_name: "Perf Instance",
      whitelist_emails: null,
      instance_id: "perf-instance",
      current_version: "1.0.0",
      latest_version: null,
      edition: "oss",
      domain: "perf.example.test",
      last_checked_at: NOW,
      namespace: null,
      is_telemetry_enabled: false,
      is_support_required: false,
      is_setup_done: true,
      is_signup_screen_visited: true,
      is_verified: true,
      is_test: false,
      is_current_version_deprecated: false,
      workspaces_exist: true,
    },
  };
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

export function mockProfile(): Profile {
  return {
    id: USER_ID,
    user: USER_ID,
    theme: { mode: "dark" },
    is_app_rail_docked: true,
    is_tour_completed: false,
    onboarding_step: {},
    use_case: null,
    role: null,
    is_onboarded: true,
    last_workspace_id: WORKSPACE_ID,
    billing_address_country: "INDIA",
    billing_address: null,
    has_billing_address: false,
    company_name: "",
    notification_view_mode: "full",
    is_smooth_cursor_enabled: false,
    is_mobile_onboarded: false,
    mobile_onboarding_step: {},
    mobile_timezone_auto_set: false,
    language: "en",
    start_of_the_week: 0,
    goals: {},
    background_color: "#fff",
    is_navigation_tour_completed: false,
    has_marketing_email_consent: false,
    is_subscribed_to_changelog: false,
    product_tour: {},
    settings: {},
    created_at: NOW,
    updated_at: NOW,
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

/** One project row without the list-only intake count (the retrieve view shape). */
export function mockProjectDetail(): ProjectDetail {
  const { intake_count: _listOnly, ...detail } = mockProjects()[0] ?? {};
  // The retrieve contract requires the executor options and default (the
  // readiness gate reads them); the app zod-parses this payload, so the
  // fields must travel here, not just in the type.
  return {
    ...detail,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
  } as ProjectDetail;
}

/** The perf user belongs to the perf workspace as a member. */
export function mockMembership(): OwnWorkspaceMembership {
  return {
    id: "123e4567-e89b-42d3-a456-426614174004",
    workspace: WORKSPACE_ID,
    member: USER_ID,
    role: 15,
    company_role: null,
    view_props: {},
    default_props: {},
    issue_props: {},
    is_active: true,
    getting_started_checklist: {},
    tips: {},
    explored_features: {},
    draft_issue_count: 0,
    created_by: null,
    updated_by: null,
    created_at: NOW,
    updated_at: NOW,
  };
}

/** The perf user's per-project roles: member of the perf project. */
export function mockProjectRoles(): ProjectRoleMap {
  return { [PERF_PROJECT_ID]: 15 };
}

/** An empty inbox: the top-bar dot stays off while the specs measure rows. */
export function mockUnreadNotifications(): UnreadNotifications {
  return { total_unread_notifications_count: 0, mention_unread_notifications_count: 0 };
}

/** Default pins: the sidebar's personal group renders while rows measure. */
export function mockSidebarPreferences(): SidebarPreferences {
  return { drafts: { is_pinned: true, sort_order: 0 } };
}

/** List layout with default grouping: the issues route prefetches these prefs. */
export function mockProjectUserProperties(): ProjectUserProperties {
  return {
    id: "123e4567-e89b-42d3-a456-426614174004",
    workspace: WORKSPACE_ID,
    project: PERF_PROJECT_ID,
    user: USER_ID,
    filters: {},
    display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
    display_properties: {},
    rich_filters: {},
    preferences: {},
    sort_order: 0,
    created_by: null,
    updated_by: null,
    created_at: NOW,
    updated_at: NOW,
  };
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
    (url) => url.pathname === "/api/instances/",
    (route) => route.fulfill(jsonFulfill(mockInstanceBootstrap()))
  );
  await page.route(
    (url) => url.pathname === "/api/users/me/",
    (route) => route.fulfill(jsonFulfill(mockMe()))
  );
  await page.route(
    (url) => url.pathname === "/api/users/me/profile/",
    (route) => route.fulfill(jsonFulfill(mockProfile()))
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
  // The project access gate (AUTH-024) reads the project row plus the
  // permission-store feeds before the list renders.
  const projectDetailPath = `/api/workspaces/${PERF_WORKSPACE_SLUG}/projects/${PERF_PROJECT_ID}/`;
  const membershipPath = `/api/workspaces/${PERF_WORKSPACE_SLUG}/workspace-members/me/`;
  const rolesPath = `/api/users/me/workspaces/${PERF_WORKSPACE_SLUG}/project-roles/`;
  const unreadPath = `/api/workspaces/${PERF_WORKSPACE_SLUG}/users/notifications/unread/`;
  const sidebarPrefsPath = `/api/workspaces/${PERF_WORKSPACE_SLUG}/sidebar-preferences/`;
  await page.route(
    (url) => url.pathname === projectDetailPath,
    (route) => route.fulfill(jsonFulfill(mockProjectDetail()))
  );
  await page.route(
    (url) => url.pathname === membershipPath,
    (route) => route.fulfill(jsonFulfill(mockMembership()))
  );
  await page.route(
    (url) => url.pathname === rolesPath,
    (route) => route.fulfill(jsonFulfill(mockProjectRoles()))
  );
  await page.route(
    (url) => url.pathname === unreadPath,
    (route) => route.fulfill(jsonFulfill(mockUnreadNotifications()))
  );
  await page.route(
    (url) => url.pathname === sidebarPrefsPath,
    (route) => route.fulfill(jsonFulfill(mockSidebarPreferences()))
  );
  // The issues route prefetches the caller's display prefs alongside the
  // list (ISS-002); without this the prefetch aborts and the route errors.
  const projectPrefsPath = `/api/workspaces/${PERF_WORKSPACE_SLUG}/projects/${PERF_PROJECT_ID}/user-properties/`;
  await page.route(
    (url) => url.pathname === projectPrefsPath,
    (route) => route.fulfill(jsonFulfill(mockProjectUserProperties()))
  );
  const known = new Set([
    "/api/instances/",
    "/api/users/me/",
    "/api/users/me/profile/",
    "/api/users/me/workspaces/",
    `/api/workspaces/${PERF_WORKSPACE_SLUG}/projects/`,
    issuesPath,
    projectDetailPath,
    membershipPath,
    rolesPath,
    unreadPath,
    sidebarPrefsPath,
    projectPrefsPath,
  ]);
  await page.route(
    (url) =>
      (url.pathname === "/api" || url.pathname.startsWith("/api/") || url.pathname.startsWith("/auth/")) &&
      !known.has(url.pathname),
    (route) => route.abort("blockedbyclient")
  );
}
