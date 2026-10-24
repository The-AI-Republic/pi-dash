// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages test fixtures. Tests only; never imported by production code.

import type { PageRow, ProjectDetails, TransportResponse } from "@pidash/api-client";

export const PAGES_WS_ID = "123e4567-e89b-12d3-a456-426614174000";
export const PAGES_PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
export const PAGES_PAGE_ID = "323e4567-e89b-12d3-a456-426614174002";
export const PAGES_USER_ID = "423e4567-e89b-12d3-a456-426614174003";

export function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

export function makePageRow(overrides: Partial<PageRow> = {}): PageRow {
  return {
    id: PAGES_PAGE_ID,
    name: "Launch notes",
    owned_by: PAGES_USER_ID,
    access: 0,
    color: "#000000",
    parent: null,
    is_favorite: false,
    is_locked: false,
    archived_at: null,
    workspace: PAGES_WS_ID,
    created_at: "2026-09-01T12:00:00Z",
    updated_at: "2026-09-02T12:00:00Z",
    created_by: PAGES_USER_ID,
    updated_by: PAGES_USER_ID,
    view_props: {},
    logo_props: {},
    label_ids: [],
    project_ids: [PAGES_PROJECT_ID],
    ...overrides,
  };
}

export function makeProjectRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: PAGES_PROJECT_ID,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 20,
    archived_at: null,
    workspace: PAGES_WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: false,
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
    ...overrides,
  };
}

export function makeProjectDetails(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: PAGES_PROJECT_ID,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: PAGES_WS_ID,
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
    module_view: true,
    cycle_view: true,
    issue_views_view: true,
    page_view: true,
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
    sort_order: null,
    member_role: 20,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

export function makeWorkspaceMember(id: string = PAGES_USER_ID, name: string = "Ada Byron"): Record<string, unknown> {
  return {
    id,
    // GET /members/ nests the member projection; this fixture id is the
    // membership id while `member.id` is the owning user.
    member: {
      id,
      first_name: name.split(" ")[0] as string,
      last_name: name.split(" ").slice(1).join(" "),
      avatar: "",
      avatar_url: null,
      display_name: name,
      is_bot: false,
    },
    role: 15,
    is_active: true,
  };
}
