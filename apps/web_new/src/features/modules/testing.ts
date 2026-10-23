// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules test fixtures. Tests only; never imported by production code.

import type { ModuleRow as ModuleRowData, ProjectDetails, TransportResponse } from "@pidash/api-client";

export const MODULES_WS_ID = "123e4567-e89b-12d3-a456-426614174000";
export const MODULES_PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
export const MODULES_MODULE_ID = "323e4567-e89b-12d3-a456-426614174002";
export const MODULES_LEAD_ID = "423e4567-e89b-12d3-a456-426614174003";

export function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

/** One project-list module row with every count present, as the server sends it. */
export function makeModuleRow(overrides: Partial<ModuleRowData> = {}): ModuleRowData {
  return {
    id: MODULES_MODULE_ID,
    workspace_id: MODULES_WS_ID,
    project_id: MODULES_PROJECT_ID,
    name: "Delivery",
    description: "",
    start_date: null,
    target_date: null,
    status: "backlog",
    lead_id: null,
    member_ids: [],
    sort_order: 0,
    is_favorite: false,
    total_issues: 0,
    completed_issues: 0,
    cancelled_issues: 0,
    started_issues: 0,
    unstarted_issues: 0,
    backlog_issues: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    ...overrides,
  };
}

/** The thin project row the modules screen reads for its name and flags. */
export function makeProjectRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: MODULES_PROJECT_ID,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    archived_at: null,
    workspace: MODULES_WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: false,
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

/** The rich project row (GET .../projects/details/) with its timezone. */
export function makeProjectDetails(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: MODULES_PROJECT_ID,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: MODULES_WS_ID,
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
    page_view: false,
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
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}
