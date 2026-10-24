// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles test fixtures. Tests only; never imported by production code.

import type {
  CycleDistribution,
  CycleProgress,
  CycleRow,
  CycleUserProperties,
  Estimate,
  EstimateList,
  ProjectDetails,
  TransportResponse,
  WorkspaceMemberList,
} from "@pidash/api-client";

export const CYCLES_WS_ID = "123e4567-e89b-12d3-a456-426614174000";
export const CYCLES_PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
export const CYCLES_CYCLE_ID = "323e4567-e89b-12d3-a456-426614174002";

export function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

export function makeCycleRow(overrides: Partial<CycleRow> = {}): CycleRow {
  return {
    id: CYCLES_CYCLE_ID,
    workspace_id: CYCLES_WS_ID,
    project_id: CYCLES_PROJECT_ID,
    name: "Cycle 7",
    start_date: "2026-09-01T12:00:00Z",
    end_date: "2026-12-31T12:00:00Z",
    is_favorite: false,
    ...overrides,
  };
}

/** Rich project row (GET .../projects/details/), the dialog's flag/timezone read. */
export function makeProjectDetails(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: CYCLES_PROJECT_ID,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: CYCLES_WS_ID,
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

/** Progress snapshot (GET .../cycles/<id>/progress/), the sidebar counts read. */
export function makeCycleProgress(overrides: Partial<CycleProgress> = {}): CycleProgress {
  return {
    total_issues: 3,
    backlog_issues: 1,
    unstarted_issues: 1,
    started_issues: 0,
    completed_issues: 1,
    cancelled_issues: 0,
    backlog_estimate_points: 2,
    unstarted_estimate_points: 0,
    started_estimate_points: 0,
    completed_estimate_points: 0,
    cancelled_estimate_points: 0,
    total_estimate_points: 2,
    ...overrides,
  };
}

/** Breakdown distributions (GET .../analytics?type=), the entries read. */
export function makeCycleDistribution(overrides: Partial<CycleDistribution> = {}): CycleDistribution {
  return {
    assignees: [
      {
        display_name: "Ada Lovelace",
        assignee_id: "aaaaaaaa-0000-4000-8000-000000000001",
        avatar_url: null,
        total_issues: 2,
        total_estimates: 4,
      },
    ],
    labels: [
      {
        label_name: "Bug",
        label_id: "bbbbbbbb-0000-4000-8000-000000000002",
        color: "#e5484d",
        total_issues: 1,
        total_estimates: 2,
      },
    ],
    completion_chart: {
      "2026-10-01": 3,
      "2026-10-02": 2,
      "2026-10-03": 1,
      "2026-10-20": null,
    },
    ...overrides,
  };
}

/** Cycle user-properties record (GET .../user-properties/), the prefs read. */
export function makeCycleUserProperties(overrides: Partial<CycleUserProperties> = {}): CycleUserProperties {
  return {
    id: "623e4567-e89b-12d3-a456-426614174006",
    workspace: CYCLES_WS_ID,
    project: CYCLES_PROJECT_ID,
    cycle: CYCLES_CYCLE_ID,
    user: "723e4567-e89b-12d3-a456-426614174007",
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    created_by: null,
    updated_by: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    ...overrides,
  };
}

/** Estimate system row (GET .../estimates/), the Points-gate read. */
export function makeEstimate(overrides: Partial<Estimate> = {}): Estimate {
  return {
    id: "823e4567-e89b-12d3-a456-426614174008",
    workspace: CYCLES_WS_ID,
    project: CYCLES_PROJECT_ID,
    name: "Points",
    description: "",
    type: "points",
    last_used: true,
    points: [],
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

/** Estimate list helper; the active system is the last-used row. */
export function makeEstimateList(estimates: Estimate[]): EstimateList {
  return estimates;
}

/** Workspace member directory (GET .../members/), the avatar read. */
export function makeWorkspaceMembers(): WorkspaceMemberList {
  return [
    {
      id: "923e4567-e89b-12d3-a456-426614174009",
      member: {
        id: "aaaaaaaa-0000-4000-8000-000000000001",
        first_name: "Ada",
        last_name: "Lovelace",
        avatar: "",
        avatar_url: null,
        display_name: "Ada Lovelace",
        is_bot: false,
      },
      role: 15,
    },
    {
      id: "a23e4567-e89b-12d3-a456-42661417400a",
      member: {
        id: "aaaaaaaa-0000-4000-8000-000000000003",
        first_name: "Grace",
        last_name: "Hopper",
        avatar: "",
        avatar_url: "https://cdn.test/grace.png",
        display_name: "Grace Hopper",
        is_bot: false,
      },
      role: 15,
    },
  ];
}

/** Project retrieve row (GET .../projects/<id>/), the feature-gate read. */
export function makeProjectRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: CYCLES_PROJECT_ID,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    archived_at: null,
    workspace: CYCLES_WS_ID,
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
