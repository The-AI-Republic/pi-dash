// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project read contracts (GET /api/workspaces/<slug>/projects/ and the
// user-property and role reads the shell and gates need). Lifecycle
// writes, identifier checks, invitations, favorites and preference
// writes live in project-management.ts, out of the initial bundle
// (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** Small project projection embedded in members and activity. */
export const ProjectLite = z.object({
  id: uuid(),
  name: z.string(),
  identifier: z.string(),
});
export type ProjectLite = z.infer<typeof ProjectLite>;

/**
 * Project list row. The endpoint answers a fixed `.values()` projection —
 * not the full serializer — with the caller's membership role, the pending
 * intake count and the view flags annotated. Anything else the server adds
 * passes through untouched.
 */
export const Project = z.object({
  id: uuid(),
  name: z.string(),
  identifier: z.string(),
  sort_order: z.nullable(z.number()),
  logo_props: z.record(z.string(), z.unknown()),
  member_role: z.nullable(z.number()),
  intake_count: z.number(),
  archived_at: nullableDateTime(),
  workspace: uuid(),
  cycle_view: z.boolean(),
  issue_views_view: z.boolean(),
  module_view: z.boolean(),
  // The serializer is fields="__all__"; older list fixtures predate it.
  estimate: z.optional(nullableUuid()),
  page_view: z.boolean(),
  inbox_view: z.boolean(),
  is_default: z.boolean(),
  guest_view_all_features: z.boolean(),
  project_lead: nullableUuid(),
  network: z.number(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type Project = z.infer<typeof Project>;

export const ProjectList = z.array(Project);
export type ProjectList = z.infer<typeof ProjectList>;

export async function listProjects(client: ApiClient, slug: string): Promise<ProjectList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/`);
  return client.parse(ProjectList, body, `GET /api/workspaces/${slug}/projects/`);
}

/**
 * One execution target the project offers, with live availability for the
 * viewer. `kind` is a server executor kind (local_runner, cloud_agent,
 * managed_runner); `reason_code` is empty exactly when `available` is
 * true. Both stay open strings: readers compare one kind for equality and
 * surface unknown codes as-is.
 */
export const AgentExecutorOption = z.object({
  kind: z.string(),
  available: z.boolean(),
  reason_code: z.string(),
});
export type AgentExecutorOption = z.infer<typeof AgentExecutorOption>;

const { intake_count: _listOnly, ...projectDetailShape } = Project.shape;

/**
 * One project row answered by GET .../projects/<id>/ (the retrieve view
 * serializes the project row, not the list `.values()` projection, so the
 * list-only pending-intake count is absent). Extra serializer fields pass
 * through untouched. The denial screens read this for their fetch status:
 * unknown projects 404, private projects 403 for non-members, other
 * non-member projects 409. The agent readiness gate reads the executor
 * options and the default executor off this same row (DESK-002).
 */
export const ProjectDetail = z.object({
  ...projectDetailShape,
  agent_executor_options: z.array(AgentExecutorOption),
  default_agent_executor: z.string(),
});
export type ProjectDetail = z.infer<typeof ProjectDetail>;

/** GET /api/workspaces/<slug>/projects/<projectId>/ — one project row. */
export async function getProject(client: ApiClient, slug: string, projectId: string): Promise<ProjectDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectDetail, body, `GET ${path}`);
}

/**
 * Full project row answered by GET .../projects/details/ and by POST
 * .../projects/ on create: every Project model field plus the membership
 * annotations (favorite flag, sort order, role, deploy-board anchor, member
 * ids) the list serializer adds. The executor options and default travel
 * here too, for the agent readiness gate (DESK-002).
 */
export const ProjectDetails = z.object({
  id: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  workspace: uuid(),
  name: z.string(),
  description: z.string(),
  description_text: z.nullable(z.record(z.string(), z.unknown())),
  description_html: z.nullable(z.record(z.string(), z.unknown())),
  network: z.number(),
  identifier: z.string(),
  default_assignee: nullableUuid(),
  project_lead: nullableUuid(),
  emoji: z.nullable(z.string()),
  icon_prop: z.nullable(z.record(z.string(), z.unknown())),
  module_view: z.boolean(),
  cycle_view: z.boolean(),
  issue_views_view: z.boolean(),
  page_view: z.boolean(),
  intake_view: z.boolean(),
  inbox_view: z.boolean(),
  is_time_tracking_enabled: z.boolean(),
  is_issue_type_enabled: z.boolean(),
  is_default: z.boolean(),
  guest_view_all_features: z.boolean(),
  members_can_edit_states: z.boolean(),
  cover_image: z.nullable(z.string()),
  cover_image_asset: nullableUuid(),
  cover_image_url: z.nullable(z.string()),
  estimate: nullableUuid(),
  archive_in: z.number(),
  close_in: z.number(),
  logo_props: z.record(z.string(), z.unknown()),
  default_state: nullableUuid(),
  archived_at: nullableDateTime(),
  timezone: z.string(),
  external_source: z.nullable(z.string()),
  external_id: z.nullable(z.string()),
  repo_url: z.string(),
  base_branch: z.string(),
  is_favorite: z.boolean(),
  sort_order: z.nullable(z.number()),
  member_role: z.nullable(z.number()),
  anchor: z.nullable(z.string()),
  members: z.array(uuid()),
  next_work_item_sequence: z.number(),
  agent_executor_options: z.array(AgentExecutorOption),
  default_agent_executor: z.string(),
});
export type ProjectDetails = z.infer<typeof ProjectDetails>;

export const ProjectDetailsList = z.array(ProjectDetails);
export type ProjectDetailsList = z.infer<typeof ProjectDetailsList>;

/** GET /api/workspaces/<slug>/projects/details/ — the rich project rows. */
export async function listProjectDetails(client: ApiClient, slug: string): Promise<ProjectDetailsList> {
  const path = `/api/workspaces/${slug}/projects/details/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectDetailsList, body, `GET ${path}`);
}

/**
 * Workspace-level display properties for the caller (filters, layout,
 * navigation prefs). GET and PATCH
 * /api/workspaces/<slug>/user-properties/; PATCH merges the given keys.
 */
export const WorkspaceUserProperties = z.object({
  id: uuid(),
  workspace: uuid(),
  user: uuid(),
  filters: z.record(z.string(), z.unknown()),
  display_filters: z.record(z.string(), z.unknown()),
  display_properties: z.record(z.string(), z.unknown()),
  rich_filters: z.record(z.string(), z.unknown()),
  navigation_project_limit: z.number(),
  navigation_control_preference: z.string(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type WorkspaceUserProperties = z.infer<typeof WorkspaceUserProperties>;

export async function getWorkspaceUserProperties(client: ApiClient, slug: string): Promise<WorkspaceUserProperties> {
  const path = `/api/workspaces/${slug}/user-properties/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceUserProperties, body, `GET ${path}`);
}

/**
 * Project-level display properties for the caller (issue filters, layout,
 * navigation preferences, sort order). GET and PATCH
 * /api/workspaces/<slug>/projects/<id>/user-properties/. PATCH merges the
 * given keys; preference writes carry the full preferences object so
 * sibling branches survive.
 */
export const ProjectUserProperties = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  user: uuid(),
  filters: z.record(z.string(), z.unknown()),
  display_filters: z.record(z.string(), z.unknown()),
  display_properties: z.record(z.string(), z.unknown()),
  rich_filters: z.record(z.string(), z.unknown()),
  preferences: z.record(z.string(), z.unknown()),
  sort_order: z.number(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type ProjectUserProperties = z.infer<typeof ProjectUserProperties>;

export async function getProjectUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/user-properties/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectUserProperties, body, `GET ${path}`);
}

/**
 * The caller's role in each project they belong to, keyed by project id.
 * The denial screens read this to decide join versus no-access.
 */
export const ProjectRoleMap = z.record(z.string(), z.number());
export type ProjectRoleMap = z.infer<typeof ProjectRoleMap>;

/** GET /api/users/me/workspaces/<slug>/project-roles/ — my project roles. */
export async function getMyProjectRoles(client: ApiClient, slug: string): Promise<ProjectRoleMap> {
  const path = `/api/users/me/workspaces/${slug}/project-roles/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectRoleMap, body, `GET ${path}`);
}
